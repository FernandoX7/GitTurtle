#!/usr/bin/env python3
"""Fast offline audit of vendored crates, patch notes, Cargo patches and third-party notices.

Checks in <0.1 s with zero network or Cargo invocation:
  - 1:1 parity across vendor/*/, Cargo.toml [patch.crates-io], [workspace].exclude,
    vendor/AGENTS.md table rows, and THIRD_PARTY_NOTICES.md rows (including exact version).
  - Standard GITTURTLE-PATCH.md naming (rejects README.gitturtle.md which is not exempt in
    .github/actions/setup-rust/cache.py's VENDOR_DOC_NAMES), preserved non-empty LICENSE*
    files, exact crate version (token-bounded), 64-hex registry archive SHA-256, and
    explicit Removal section.

Optional `--diff-cache` compares vendored sources against any matching `.crate` archives
already present in `~/.cargo/registry/cache` (never accesses the network).
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SHA256_RE = re.compile(r"\b[0-9a-f]{64}\b")
REMOVAL_HEADING_RE = re.compile(r"^##\s+(?:Removal|Removing the patch)\b", re.M)
IGNORED_ARCHIVE_FILES = frozenset({".cargo-ok", ".cargo-checksum.json", "Cargo.lock"})
PATCH_META_FILES = frozenset({"GITTURTLE-PATCH.md", "README.gitturtle.md"})


@dataclass(frozen=True)
class Violation:
    rule: str
    path: str
    message: str

    def format(self) -> str:
        return f"[{self.rule}] {self.path}: {self.message}"


def _extract_table_lines(text: str, header_prefix: str) -> list[str]:
    """Return data rows from the first Markdown table whose header starts with `header_prefix`."""
    lines = text.splitlines()
    for idx, line in enumerate(lines):
        if line.strip().startswith(header_prefix):
            rows: list[str] = []
            for row in lines[idx + 2:]:
                stripped = row.strip()
                if not stripped.startswith("|"):
                    break
                rows.append(stripped)
            return rows
    return []


def _parse_vendor_agents_rows(text: str) -> set[str]:
    rows = _extract_table_lines(text, "| Dependency |")
    found: set[str] = set()
    for row in rows:
        match = re.match(r"^\|\s*`([^`]+)`\s*\|", row)
        if match:
            found.add(match.group(1))
    return found


def _parse_third_party_rows(text: str) -> tuple[dict[str, str], list[str]]:
    rows = _extract_table_lines(text, "| Vendored package |")
    found: dict[str, str] = {}
    duplicates: list[str] = []
    for row in rows:
        match = re.match(r"^\|\s*`([^`]+)`\s+([0-9][0-9A-Za-z.+-]*)\s*\|", row)
        if not match:
            continue
        name, version = match.group(1), match.group(2)
        if name in found:
            duplicates.append(name)
        else:
            found[name] = version
    return found, duplicates


def _find_cached_crate(cache_roots: list[Path], name: str, version: str) -> Path | None:
    filename = f"{name}-{version}.crate"
    for root in cache_roots:
        if not root.is_dir():
            continue
        direct = root / filename
        if direct.is_file():
            return direct
        for child in sorted(root.iterdir()):
            candidate = child / filename
            if candidate.is_file():
                return candidate
    return None


def _diff_against_archive(crate_dir: Path, archive_path: Path, name: str, version: str) -> dict:
    raw = archive_path.read_bytes()
    archive_sha256 = hashlib.sha256(raw).hexdigest()
    prefix = f"{name}-{version}/"
    pristine: dict[str, bytes] = {}
    with tarfile.open(archive_path, mode="r:gz") as tf:
        for member in tf.getmembers():
            if not member.isfile():
                continue
            rel = member.name[len(prefix):] if member.name.startswith(prefix) else member.name
            if rel in IGNORED_ARCHIVE_FILES:
                continue
            extracted = tf.extractfile(member)
            if extracted is not None:
                pristine[rel] = extracted.read()

    vendored: dict[str, bytes] = {}
    for file_path in sorted(crate_dir.rglob("*")):
        if not file_path.is_file() or file_path.is_symlink():
            continue
        rel = file_path.relative_to(crate_dir).as_posix()
        if rel in IGNORED_ARCHIVE_FILES or rel in PATCH_META_FILES or rel.startswith("upstream-files/"):
            continue
        vendored[rel] = file_path.read_bytes()

    modified = added = deleted = lines_added = lines_removed = 0
    for rel in sorted(set(pristine) | set(vendored)):
        if rel.startswith("LICENSE") and rel not in pristine:
            continue
        before = pristine.get(rel)
        after = vendored.get(rel)
        if before == after:
            continue
        if before is None:
            added += 1
            lines_added += len(after.decode("utf-8", errors="replace").splitlines()) if after else 0
        elif after is None:
            deleted += 1
            lines_removed += len(before.decode("utf-8", errors="replace").splitlines())
        else:
            modified += 1
            diff = difflib.unified_diff(
                before.decode("utf-8", errors="replace").splitlines(),
                after.decode("utf-8", errors="replace").splitlines(),
                lineterm="",
                n=0,
            )
            for line in diff:
                if line.startswith("+") and not line.startswith("+++"):
                    lines_added += 1
                elif line.startswith("-") and not line.startswith("---"):
                    lines_removed += 1

    return {
        "archive": str(archive_path),
        "archive_sha256": archive_sha256,
        "changed_files": modified + added + deleted,
        "modified_files": modified,
        "added_files": added,
        "deleted_files": deleted,
        "lines_added": lines_added,
        "lines_removed": lines_removed,
    }


def _version_mentioned(note_text: str, version: str) -> bool:
    pattern = re.compile(rf"(?<![0-9A-Za-z._-]){re.escape(version)}(?![0-9A-Za-z._-])")
    return bool(pattern.search(note_text))


def audit_repo(
    root: Path = ROOT,
    *,
    diff_cache: bool = False,
    cargo_cache_roots: list[Path] | None = None,
) -> tuple[list[Violation], list[dict]]:
    violations: list[Violation] = []
    summaries: list[dict] = []

    vendor_dir = root / "vendor"
    cargo_toml_path = root / "Cargo.toml"
    if vendor_dir.is_symlink() or not vendor_dir.is_dir():
        return [Violation("vendor-dir", "vendor/", "vendor directory is missing or is a symlink")], summaries
    if cargo_toml_path.is_symlink() or not cargo_toml_path.is_file():
        return [Violation("manifest", "Cargo.toml", "root Cargo.toml is missing or is a symlink")], summaries

    manifest = tomllib.loads(cargo_toml_path.read_text(encoding="utf-8"))
    workspace_exclude = set(manifest.get("workspace", {}).get("exclude", []))
    vendor_excludes = {item.removeprefix("vendor/") for item in workspace_exclude if item.startswith("vendor/")}
    patches = manifest.get("patch", {}).get("crates-io", {})

    vendor_dirs: list[str] = []
    for child in sorted(vendor_dir.iterdir()):
        if child.is_symlink():
            violations.append(Violation("vendor-symlink", f"vendor/{child.name}", "symlinks are not permitted under vendor/"))
            continue
        if child.is_dir():
            vendor_dirs.append(child.name)
    vendor_set = set(vendor_dirs)

    for missing in sorted(vendor_set - set(patches.keys())):
        violations.append(Violation("patch-table", "Cargo.toml", f"vendor/{missing} missing from [patch.crates-io]"))
    for extra in sorted(set(patches.keys()) - vendor_set):
        violations.append(Violation("patch-table", "Cargo.toml", f"[patch.crates-io].{extra} has no vendor/{extra} directory"))
    for name, entry in sorted(patches.items()):
        expected = f"vendor/{name}"
        if not isinstance(entry, dict) or entry.get("path") != expected:
            violations.append(
                Violation("patch-table", "Cargo.toml", f"[patch.crates-io].{name} must point to {expected!r}")
            )

    for missing in sorted(vendor_set - vendor_excludes):
        violations.append(
            Violation("workspace-exclude", "Cargo.toml", f"vendor/{missing} missing from [workspace].exclude")
        )
    for extra in sorted(vendor_excludes - vendor_set):
        violations.append(
            Violation("workspace-exclude", "Cargo.toml", f"vendor/{extra} listed in [workspace].exclude but absent")
        )

    vendor_agents = vendor_dir / "AGENTS.md"
    if vendor_agents.is_file() and not vendor_agents.is_symlink():
        agent_rows = _parse_vendor_agents_rows(vendor_agents.read_text(encoding="utf-8"))
        for missing in sorted(vendor_set - agent_rows):
            violations.append(Violation("vendor-agents", "vendor/AGENTS.md", f"missing table row for `{missing}`"))
        for extra in sorted(agent_rows - vendor_set):
            violations.append(Violation("vendor-agents", "vendor/AGENTS.md", f"stale table row for absent `{extra}`"))
    else:
        violations.append(Violation("vendor-agents", "vendor/AGENTS.md", "vendor/AGENTS.md is missing"))

    third_party_path = root / "THIRD_PARTY_NOTICES.md"
    third_party_rows: dict[str, str] = {}
    if third_party_path.is_file() and not third_party_path.is_symlink():
        third_party_rows, duplicates = _parse_third_party_rows(third_party_path.read_text(encoding="utf-8"))
        for dup in sorted(set(duplicates)):
            violations.append(
                Violation("third-party-notices", "THIRD_PARTY_NOTICES.md", f"duplicate vendored package row for `{dup}`")
            )
        for extra in sorted(set(third_party_rows) - vendor_set):
            violations.append(
                Violation("third-party-notices", "THIRD_PARTY_NOTICES.md", f"stale vendored row for absent `{extra}`")
            )
    else:
        violations.append(Violation("third-party-notices", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md is missing"))

    cache_roots = cargo_cache_roots
    if cache_roots is None:
        cargo_home = Path(os.environ.get("CARGO_HOME") or (Path.home() / ".cargo"))
        cache_roots = [cargo_home / "registry" / "cache"]

    for dirname in vendor_dirs:
        crate_dir = vendor_dir / dirname
        crate_manifest_path = crate_dir / "Cargo.toml"
        if crate_manifest_path.is_symlink() or not crate_manifest_path.is_file():
            violations.append(Violation("crate-manifest", f"vendor/{dirname}", "missing or symlinked Cargo.toml"))
            continue
        crate_meta = tomllib.loads(crate_manifest_path.read_text(encoding="utf-8"))
        pkg = crate_meta.get("package", {})
        raw_name = pkg.get("name")
        raw_version = pkg.get("version")
        pkg_name = raw_name if isinstance(raw_name, str) else ""
        pkg_version = raw_version if isinstance(raw_version, str) else ""

        if pkg_name != dirname:
            violations.append(
                Violation("crate-name", f"vendor/{dirname}/Cargo.toml", f"package name {pkg_name!r} != dir {dirname!r}")
            )
        if not pkg_version:
            violations.append(
                Violation("crate-manifest", f"vendor/{dirname}/Cargo.toml", "missing literal string [package].version")
            )

        if third_party_path.is_file() and not third_party_path.is_symlink() and pkg_version:
            listed_version = third_party_rows.get(dirname)
            if listed_version is None:
                violations.append(
                    Violation(
                        "third-party-notices",
                        "THIRD_PARTY_NOTICES.md",
                        f"missing vendored package row for `{dirname}` {pkg_version}",
                    )
                )
            elif listed_version != pkg_version:
                violations.append(
                    Violation(
                        "third-party-notices",
                        "THIRD_PARTY_NOTICES.md",
                        f"`{dirname}` listed as {listed_version}, but vendor/{dirname}/Cargo.toml is {pkg_version}",
                    )
                )

        legacy_note = crate_dir / "README.gitturtle.md"
        if legacy_note.is_symlink() or legacy_note.exists():
            violations.append(
                Violation(
                    "patch-note-name",
                    f"vendor/{dirname}/README.gitturtle.md",
                    "rename to GITTURTLE-PATCH.md so .github/actions/setup-rust/cache.py exempts doc edits",
                )
            )

        licenses = [
            p.name
            for p in crate_dir.iterdir()
            if not p.is_symlink() and p.is_file() and p.name.startswith("LICENSE") and p.stat().st_size > 0
        ]
        if not licenses:
            violations.append(Violation("license", f"vendor/{dirname}", "missing preserved non-empty LICENSE* file"))

        patch_note = crate_dir / "GITTURTLE-PATCH.md"
        note_text = ""
        if patch_note.is_symlink() or not patch_note.is_file():
            violations.append(Violation("patch-note", f"vendor/{dirname}", "missing or symlinked GITTURTLE-PATCH.md"))
        else:
            note_text = patch_note.read_text(encoding="utf-8")
            if pkg_version and not _version_mentioned(note_text, pkg_version):
                violations.append(
                    Violation(
                        "patch-note-version",
                        f"vendor/{dirname}/GITTURTLE-PATCH.md",
                        f"does not mention exact crate version {pkg_version}",
                    )
                )
            if not SHA256_RE.search(note_text):
                violations.append(
                    Violation(
                        "patch-note-checksum",
                        f"vendor/{dirname}/GITTURTLE-PATCH.md",
                        "missing 64-hex crates.io archive SHA-256 checksum",
                    )
                )
            if not REMOVAL_HEADING_RE.search(note_text):
                violations.append(
                    Violation(
                        "patch-note-removal",
                        f"vendor/{dirname}/GITTURTLE-PATCH.md",
                        "missing `## Removal` or `## Removing the patch` retirement section",
                    )
                )

        entry: dict = {
            "crate": dirname,
            "version": pkg_version,
            "licenses": sorted(licenses),
        }
        if diff_cache and pkg_version:
            archive = _find_cached_crate(cache_roots, pkg_name, pkg_version)
            if archive is None:
                entry["cache"] = "absent"
            else:
                stats = _diff_against_archive(crate_dir, archive, pkg_name, pkg_version)
                entry["cache"] = stats
                if note_text and stats["archive_sha256"] not in note_text:
                    violations.append(
                        Violation(
                            "archive-checksum-mismatch",
                            f"vendor/{dirname}/GITTURTLE-PATCH.md",
                            f"recorded checksum does not match cached archive SHA-256 {stats['archive_sha256']}",
                        )
                    )
        summaries.append(entry)

    return violations, summaries


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT, help="repository root directory")
    parser.add_argument(
        "--diff-cache",
        action="store_true",
        help="also diff against any matching .crate archives in ~/.cargo/registry/cache (offline)",
    )
    parser.add_argument("--json", type=Path, help="write JSON audit summary to this path")
    args = parser.parse_args(argv)

    violations, summaries = audit_repo(args.root.resolve(), diff_cache=args.diff_cache)
    payload = {
        "crates": summaries,
        "violations": [asdict(v) for v in violations],
    }
    if args.json is not None:
        args.json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    if violations:
        for violation in violations:
            print(violation.format(), file=sys.stderr)
        return 1

    if args.diff_cache:
        for item in summaries:
            cache = item.get("cache")
            if isinstance(cache, dict):
                print(
                    f"  {item['crate']} {item['version']}: {cache['changed_files']} changed files "
                    f"(+{cache['lines_added']} / -{cache['lines_removed']})"
                )
            else:
                print(f"  {item['crate']} {item['version']}: local .crate cache absent")
    print(f"vendor-patch-audit: ok ({len(summaries)} vendored crates verified).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
