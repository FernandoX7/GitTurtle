#!/usr/bin/env python3
"""Collect notices for a local package; --require-complete gates public binaries."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile


LEGAL_NAME = re.compile(r"^(license|licence|notice|copying|copyright|ofl)(?:[._-]|$)", re.I)
MAX_NOTICE_BYTES = 4 * 1024 * 1024


def copy_notice(source, destination):
    if source.is_symlink() or source.stat().st_size > MAX_NOTICE_BYTES:
        raise ValueError(f"Unsafe or oversized notice: {source.name}")
    data = source.read_bytes()
    if b"\0" in data:
        raise ValueError(f"Notice is not text: {source.name}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)
    return hashlib.sha256(data).hexdigest()


def source_archive(crate, destination, package_name):
    # MPL source accompanies the binary instead of depending on a future offer.
    # Store published source with neutral ownership and reproducible timestamps.
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("wb") as output:
        with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w|") as archive:
                for path in sorted(crate.rglob("*")):
                    if not path.is_file() or path.is_symlink() or path.name == ".cargo-ok":
                        continue
                    data = path.read_bytes()
                    info = tarfile.TarInfo(f"{package_name}/{path.relative_to(crate).as_posix()}")
                    info.size = len(data)
                    info.mode = 0o644
                    archive.addfile(info, io.BytesIO(data))


def collect(args):
    root = Path(__file__).resolve().parent.parent
    destination = args.output.resolve()
    if destination.exists() and (not destination.is_dir() or any(destination.iterdir())):
        raise ValueError("Output must be a new or empty directory; existing notices were not replaced")
    result = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", args.target],
        cwd=root, check=True, stdout=subprocess.PIPE, text=True,
    )
    metadata = json.loads(result.stdout)
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [package["id"] for package in packages.values() if package["name"] == "gitturtle"]
    if len(pending) != 1:
        raise ValueError("Expected exactly one gitturtle application package")
    selected = set()
    while pending:
        identity = pending.pop()
        if identity in selected:
            continue
        selected.add(identity)
        pending.extend(
            dependency["pkg"] for dependency in nodes[identity]["deps"]
            if any(kind["kind"] != "dev" for kind in dependency["dep_kinds"])
        )
    supplements_root = root / "docs/licenses"
    supplements = {
        (entry["name"], entry["version"]): entry
        for entry in json.loads((supplements_root / "dependencies.json").read_text())
    }
    destination.mkdir(parents=True, exist_ok=True)
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        copy_notice(root / name, destination / name)
    # The source index accompanies a standalone package; its source-code links
    # refer back to the repository, while LICENSE remains a bundled local file.
    index_path = destination / "THIRD_PARTY_NOTICES.md"
    index_path.write_text(re.sub(
        r"\]\((?!https?://|LICENSE\))([^\n)]+)\)",
        r"](https://github.com/FernandoX7/GitTurtle/blob/main/\1)",
        index_path.read_text(),
    ))
    for notice in json.loads((supplements_root / "assets/sources.json").read_text()):
        source = supplements_root / "assets" / notice["file"]
        if (source.is_symlink()
                or not source.resolve().is_relative_to((supplements_root / "assets").resolve())
                or hashlib.sha256(source.read_bytes()).hexdigest() != notice["sha256"]):
            raise ValueError("Asset notice checksum or path changed")
    shutil.copytree(supplements_root / "assets", destination / "assets")
    inventory = []
    missing = []
    for identity in sorted(selected, key=lambda value: (packages[value]["name"], packages[value]["version"])):
        package = packages[identity]
        if identity in metadata["workspace_members"]:
            continue
        label = f"{package['name']}-{package['version']}"
        crate = Path(package["manifest_path"]).parent
        directory = destination / "notices" / label
        entry = {
            "name": package["name"], "version": package["version"],
            "declared_license": package["license"], "authors": package["authors"],
            "repository": package["repository"], "source": package["source"] or "vendored source in GitTurtle",
            "notices": [],
        }
        paths = {
            path for path in crate.rglob("*")
            if path.is_file() and not path.is_symlink() and LEGAL_NAME.match(path.name)
        }
        if package["license_file"]:
            declared = crate / package["license_file"]
            if declared.resolve().is_relative_to(crate.resolve()) and declared.is_file():
                paths.add(declared)
        for path in sorted(paths):
            target = directory / path.relative_to(crate)
            digest = copy_notice(path, target)
            entry["notices"].append({"path": str(target.relative_to(destination)), "sha256": digest})
        if package["name"] == "meshopt":
            # The Cargo archive retains meshoptimizer's complete MIT notice in
            # its C++ header, but omits a separate native-library license file.
            header = (crate / "vendor/src/meshoptimizer.h").read_text()
            offset = header.rfind("/**\n * Copyright (c)")
            notice = header[offset:] if offset >= 0 else ""
            if "Permission is hereby granted" not in notice or "THE SOFTWARE IS PROVIDED" not in notice:
                raise ValueError("The embedded meshoptimizer license needs review")
            target = directory / "meshoptimizer-LICENSE.txt"
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(notice)
            entry["notices"].append({
                "path": str(target.relative_to(destination)),
                "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
                "source_file": "vendor/src/meshoptimizer.h (complete final license comment)",
            })
        supplement = supplements.get((package["name"], package["version"]))
        if supplement:
            for number, notice in enumerate(supplement["files"]):
                source = supplements_root / notice["path"]
                if not source.resolve().is_relative_to(supplements_root.resolve()):
                    raise ValueError("Supplemental notice escapes its source directory")
                target = directory / f"upstream-{number + 1}.txt"
                digest = copy_notice(source, target)
                if digest != notice["sha256"]:
                    raise ValueError(f"Supplemental notice checksum changed: {label}")
                entry["notices"].append({"path": str(target.relative_to(destination)), "sha256": digest, "url": notice["url"]})
            if supplement.get("note"):
                entry["notice_provenance"] = supplement["note"]
        if crate.is_relative_to(root / "vendor"):
            for patch_name in ("GITTURTLE-PATCH.md", "README.gitturtle.md"):
                if (crate / patch_name).is_file():
                    copy_notice(crate / patch_name, directory / patch_name)
        if not entry["notices"] or not package["license"]:
            missing.append(label)
            entry["review_required"] = "Published crate and inspected upstream source omit a standalone license notice; verify required attribution before public binary distribution."
        if package["license"] == "MPL-2.0":
            archive = destination / "sources" / f"{label}.tar.gz"
            source_archive(crate, archive, label)
            entry["source_archive"] = str(archive.relative_to(destination))
        inventory.append(entry)
    report = {
        "target": args.target,
        "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
        "scope": "Cargo metadata runtime/build dependency edges for this target, excluding dev edges. Workspace feature unification may conservatively include additional packages; this is not a linker inventory. Includes supplied nested notices conservatively.",
        "review_required": missing,
        "dependencies": inventory,
    }
    (destination / "dependencies.json").write_text(json.dumps(report, indent=2) + "\n")
    if missing:
        (destination / "REVIEW_REQUIRED.md").write_text(
            "# Before distributing public binaries\n\n"
            "These packages declare licenses in Cargo metadata, but their published crates and inspected upstream sources omit standalone license notices. "
            "Do not treat this generated bundle as completed license clearance. Resolve their required attribution before a public binary release. "
            "The dependency inventory preserves the actual declared license, authors and source location.\n\n"
            + "".join(f"- `{name}`\n" for name in missing)
            + "\nThe native ufbx library notice is preserved in assets/ufbx-native-LICENSE; the Rust wrapper remains separately listed above.\n"
        )
        print(f"License review required for {len(missing)} packages; see {destination / 'REVIEW_REQUIRED.md'}", file=sys.stderr)
    print(f"Collected {len(inventory)} dependency records for {args.target}")
    return 2 if args.require_complete and missing else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, help="The packaged Rust target triple")
    parser.add_argument("--require-complete", action="store_true", help="Fail if required license notices remain unresolved")
    parser.add_argument("output", type=Path, help="New or empty output directory")
    args = parser.parse_args()
    try:
        return collect(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Could not collect package licenses: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
