#!/usr/bin/env python3
"""Generate a deterministic CycloneDX 1.5 JSON SBOM from Cargo.lock and license metadata.

Pure Python standard library (zero external binaries, zero network calls).
Produces reproducible output for a given repository commit so assembled release
digests and SHA256SUMS remain deterministic.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
import tomllib

SPEC_VERSION = "1.5"
BOM_FORMAT = "CycloneDX"


def _license_map(repo_root: Path) -> dict[tuple[str, str], str]:
    """Load optional SPDX license strings keyed by (name, version) from docs/licenses/dependencies.json."""
    path = repo_root / "docs" / "licenses" / "dependencies.json"
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    mapping: dict[tuple[str, str], str] = {}
    entries = data.get("packages", data) if isinstance(data, dict) else data
    if isinstance(entries, dict):
        for key, value in entries.items():
            if isinstance(value, dict):
                spdx = value.get("license") or value.get("spdx")
                if isinstance(spdx, str) and "@" in key:
                    name, version = key.rsplit("@", 1)
                    mapping[(name, version)] = spdx
    elif isinstance(entries, list):
        for item in entries:
            if not isinstance(item, dict):
                continue
            name, version = item.get("name"), item.get("version")
            spdx = item.get("license") or item.get("spdx")
            if isinstance(name, str) and isinstance(version, str) and isinstance(spdx, str):
                mapping[(name, version)] = spdx
    return mapping


def build_sbom(
    repo_root: Path,
    *,
    version: str | None = None,
    commit: str | None = None,
) -> dict[str, object]:
    """Build a deterministic CycloneDX 1.5 JSON document from `Cargo.lock`."""
    repo_root = repo_root.resolve()
    lock_path = repo_root / "Cargo.lock"
    if not lock_path.is_file():
        raise FileNotFoundError(f"Cargo.lock not found at {lock_path}")
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))

    if version is None:
        cargo_toml = repo_root / "Cargo.toml"
        if cargo_toml.is_file():
            manifest = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
            version = manifest.get("workspace", {}).get("package", {}).get("version")
        version = version or "0.0.0"

    licenses = _license_map(repo_root)
    components: list[dict[str, object]] = []
    seen: set[tuple[str, str]] = set()
    for pkg in sorted(
        lock.get("package", []),
        key=lambda item: (str(item.get("name", "")), str(item.get("version", ""))),
    ):
        name = pkg.get("name")
        pkg_version = pkg.get("version")
        if not isinstance(name, str) or not isinstance(pkg_version, str):
            continue
        if (name, pkg_version) in seen:
            continue
        seen.add((name, pkg_version))
        comp: dict[str, object] = {
            "type": "library",
            "bom-ref": f"pkg:cargo/{name}@{pkg_version}",
            "name": name,
            "version": pkg_version,
            "purl": f"pkg:cargo/{name}@{pkg_version}",
        }
        checksum = pkg.get("checksum")
        if isinstance(checksum, str) and checksum:
            comp["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        spdx = licenses.get((name, pkg_version))
        if spdx:
            # Normalize legacy Cargo slash syntax (`MIT/Apache-2.0`) to SPDX 2.x `OR`.
            normalized = " OR ".join(part.strip() for part in spdx.split("/") if part.strip())
            comp["licenses"] = [{"expression": normalized}]
        components.append(comp)

    metadata: dict[str, object] = {
        "component": {
            "type": "application",
            "name": "gitturtle",
            "version": version,
            "purl": f"pkg:cargo/gitturtle@{version}",
        },
        "tools": [{"name": "scripts/generate-sbom.py", "version": "1"}],
    }
    if commit:
        metadata["properties"] = [{"name": "gitturtle:commit", "value": commit}]

    return {
        "bomFormat": BOM_FORMAT,
        "specVersion": SPEC_VERSION,
        "version": 1,
        "metadata": metadata,
        "components": components,
    }


def write_sbom(
    output: Path,
    repo_root: Path,
    *,
    version: str | None = None,
    commit: str | None = None,
) -> dict[str, object]:
    sbom = build_sbom(repo_root, version=version, commit=commit)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(sbom, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return sbom


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Generate a deterministic CycloneDX 1.5 JSON SBOM from Cargo.lock.")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent, help="Repository root")
    parser.add_argument("--version", default=None, help="Application version override")
    parser.add_argument("--commit", default=None, help="Source commit SHA metadata")
    parser.add_argument("output", type=Path, help="Output CycloneDX JSON path")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    sbom = write_sbom(args.output, args.root, version=args.version, commit=args.commit)
    print(f"Wrote CycloneDX {SPEC_VERSION} SBOM with {len(sbom['components'])} components to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
