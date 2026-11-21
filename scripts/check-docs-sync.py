#!/usr/bin/env python3
"""Verify factual synchronization between code/manifests and maintained documentation.

Reads `docs/doc-manifest.json` using only the Python standard library and checks:
- Pinned Rust toolchain version (`rust-toolchain.toml`) across listed docs & website guides
- Pinned crate versions in `Cargo.lock` cited in living documentation
- Rust source constants (themes, tabs, history window) against user-facing docs
- GitHub Actions workflow filenames listed in contributor documentation
- `include_str!(...)` paths in Rust sources
- Rendered website guides & legal notices (`website/build_guides.py --check`)

With `--changed-since <ref>`, prints an advisory map of documentation surfaces
associated with the changed files.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

NUMBER_WORDS = {
    0: "zero", 1: "one", 2: "two", 3: "three", 4: "four",
    5: "five", 6: "six", 7: "seven", 8: "eight", 9: "nine",
    10: "ten", 11: "eleven", 12: "twelve", 13: "thirteen", 14: "fourteen",
    15: "fifteen", 16: "sixteen", 17: "seventeen", 18: "eighteen", 19: "nineteen",
    20: "twenty", 24: "twenty-four", 32: "thirty-two",
}

RUST_VERSION_RE = re.compile(r"\bRust\s+\*{0,2}(\d+\.\d+\.\d+)\*{0,2}")
INCLUDE_STR_RE = re.compile(r'include_str!\s*\(\s*"([^"]+)"\s*\)')


@dataclass(frozen=True)
class SyncIssue:
    path: str
    message: str

    def __str__(self) -> str:
        return f"{self.path}: {self.message}"


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _toolchain_channel(root: Path) -> str:
    data = tomllib.loads(_read_text(root / "rust-toolchain.toml"))
    channel = data.get("toolchain", {}).get("channel")
    if not isinstance(channel, str) or not channel.strip():
        raise ValueError("rust-toolchain.toml missing toolchain.channel")
    return channel.strip()


def _lockfile_versions(root: Path) -> dict[str, set[str]]:
    lock_path = root / "Cargo.lock"
    if not lock_path.is_file():
        return {}
    data = tomllib.loads(_read_text(lock_path))
    versions: dict[str, set[str]] = {}
    for pkg in data.get("package", []):
        name = pkg.get("name")
        version = pkg.get("version")
        if isinstance(name, str) and isinstance(version, str):
            versions.setdefault(name, set()).add(version)
    return versions


def _extract_constant(root: Path, spec: dict) -> int:
    source = root / spec["source"]
    text = _read_text(source)
    kind = spec["kind"]
    if kind == "regex_int":
        match = re.search(spec["pattern"], text)
        if not match:
            raise ValueError(f"pattern {spec['pattern']!r} not found in {spec['source']}")
        return int(match.group(1).replace("_", ""))
    symbol = re.escape(spec["symbol"])
    if kind == "rust_const_int":
        match = re.search(rf"\bconst\s+{symbol}\s*:\s*[A-Za-z0-9_]+\s*=\s*([0-9_]+)\s*;", text)
        if not match:
            raise ValueError(f"integer const {spec['symbol']!r} not found in {spec['source']}")
        return int(match.group(1).replace("_", ""))
    if kind == "rust_const_mib":
        match = re.search(
            rf"\bconst\s+{symbol}\s*:\s*[A-Za-z0-9_]+\s*=\s*([0-9_]+)\s*\*\s*1024\s*\*\s*1024\s*;",
            text,
        )
        if not match:
            raise ValueError(f"MiB const {spec['symbol']!r} not found in {spec['source']}")
        return int(match.group(1).replace("_", ""))
    raise ValueError(f"unsupported constant kind: {kind!r}")


def _check_rust_toolchain(root: Path, docs: list[str]) -> list[SyncIssue]:
    issues: list[SyncIssue] = []
    try:
        channel = _toolchain_channel(root)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        return [SyncIssue("rust-toolchain.toml", str(error))]
    for rel in docs:
        path = root / rel
        if not path.is_file():
            issues.append(SyncIssue(rel, "missing toolchain-referenced doc"))
            continue
        text = _read_text(path)
        found = RUST_VERSION_RE.findall(text)
        if not found:
            issues.append(SyncIssue(rel, f"expected Rust {channel} reference, found no 'Rust X.Y.Z' mention"))
            continue
        stale = sorted({ver for ver in found if ver != channel})
        if stale:
            issues.append(SyncIssue(rel, f"stale Rust version(s) {', '.join(stale)} (rust-toolchain.toml pins {channel})"))
    return issues


def _check_lockfile_mentions(root: Path, mentions: list[dict]) -> list[SyncIssue]:
    if not mentions:
        return []
    versions = _lockfile_versions(root)
    issues: list[SyncIssue] = []
    for entry in mentions:
        crate = entry["crate"]
        rel = entry["doc"]
        pattern = entry["pattern"]
        pinned = versions.get(crate, set())
        if not pinned:
            issues.append(SyncIssue("Cargo.lock", f"crate {crate!r} not found in Cargo.lock"))
            continue
        path = root / rel
        if not path.is_file():
            issues.append(SyncIssue(rel, f"missing doc referencing crate {crate!r}"))
            continue
        matches = re.findall(pattern, _read_text(path))
        if not matches:
            issues.append(SyncIssue(rel, f"expected {crate} version mention matching {pattern!r}"))
            continue
        stale = sorted({ver for ver in matches if ver not in pinned})
        if stale:
            issues.append(SyncIssue(rel, f"stale {crate} version(s) {', '.join(stale)} (Cargo.lock has {', '.join(sorted(pinned))})"))
    return issues


def _check_constants(root: Path, constants: list[dict]) -> list[SyncIssue]:
    issues: list[SyncIssue] = []
    for spec in constants:
        try:
            value = _extract_constant(root, spec)
        except (OSError, ValueError) as error:
            issues.append(SyncIssue(spec.get("source", "docs/doc-manifest.json"), str(error)))
            continue
        word_value = NUMBER_WORDS.get(value, str(value))
        comma_value = f"{value:,}"
        for target in spec.get("docs", []):
            rel = target["path"]
            path = root / rel
            if not path.is_file():
                issues.append(SyncIssue(rel, f"missing doc for constant {spec['name']}"))
                continue
            rendered = target["pattern"].format(
                value=re.escape(str(value)),
                comma_value=re.escape(comma_value),
                word_value=re.escape(word_value),
            )
            flags = re.IGNORECASE if target.get("ignore_case") else 0
            if not re.search(rendered, _read_text(path), flags):
                issues.append(
                    SyncIssue(
                        rel,
                        f"constant {spec['name']}={value} (from {spec['source']}) not matched by {rendered!r}",
                    )
                )
    return issues


def _check_workflows(root: Path, docs: list[str]) -> list[SyncIssue]:
    workflows_dir = root / ".github" / "workflows"
    if not workflows_dir.is_dir():
        return []
    actual = sorted(p.name for p in workflows_dir.iterdir() if p.is_file() and p.suffix in {".yml", ".yaml"})
    issues: list[SyncIssue] = []
    for rel in docs:
        path = root / rel
        if not path.is_file():
            issues.append(SyncIssue(rel, "missing workflow-listing doc"))
            continue
        text = _read_text(path)
        for name in actual:
            if name not in text:
                issues.append(SyncIssue(rel, f"workflow {name!r} exists in .github/workflows/ but is not mentioned"))
    return issues


def _check_include_str(root: Path, roots: list[str]) -> list[SyncIssue]:
    issues: list[SyncIssue] = []
    for rel_root in roots:
        base = root / rel_root
        if not base.is_dir():
            continue
        for rs_file in sorted(base.rglob("*.rs")):
            text = rs_file.read_text(encoding="utf-8", errors="replace")
            for match in INCLUDE_STR_RE.finditer(text):
                raw_target = match.group(1)
                if raw_target.startswith("/") or "{" in raw_target:
                    continue
                resolved = (rs_file.parent / raw_target).resolve()
                if not resolved.is_relative_to(root):
                    issues.append(SyncIssue(rs_file.relative_to(root).as_posix(), f"include_str! escapes repo: {raw_target!r}"))
                elif not resolved.is_file():
                    issues.append(SyncIssue(rs_file.relative_to(root).as_posix(), f"include_str! target missing: {raw_target!r}"))
    return issues


def _check_website_guides(root: Path) -> list[SyncIssue]:
    script = root / "website" / "build_guides.py"
    if not script.is_file():
        return []
    completed = subprocess.run(
        [sys.executable, "-B", str(script), "--check"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip().splitlines()
        summary = "; ".join(detail[:3]) if detail else f"exit {completed.returncode}"
        return [SyncIssue("website/build_guides.py", f"generated website guides/notices out of date: {summary}")]
    return []


def validate(root: Path, manifest_path: Path | None = None) -> list[SyncIssue]:
    root = root.resolve()
    manifest_file = manifest_path or (root / "docs" / "doc-manifest.json")
    try:
        manifest = json.loads(_read_text(manifest_file))
    except (OSError, ValueError) as error:
        return [SyncIssue(str(manifest_file), f"cannot load doc manifest: {error}")]
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        return [SyncIssue(str(manifest_file), "doc manifest must be a JSON object with version == 1")]

    issues: list[SyncIssue] = []
    issues.extend(_check_rust_toolchain(root, manifest.get("rust_toolchain_docs", [])))
    issues.extend(_check_lockfile_mentions(root, manifest.get("lockfile_crate_mentions", [])))
    issues.extend(_check_constants(root, manifest.get("constants", [])))
    issues.extend(_check_workflows(root, manifest.get("workflows_listed_in", [])))
    issues.extend(_check_include_str(root, manifest.get("include_str_roots", [])))
    if manifest.get("check_website_guides"):
        issues.extend(_check_website_guides(root))
    return issues


def affected_surfaces(root: Path, ref: str, manifest_path: Path | None = None) -> list[tuple[str, list[str]]]:
    root = root.resolve()
    manifest_file = manifest_path or (root / "docs" / "doc-manifest.json")
    manifest = json.loads(_read_text(manifest_file))
    env = {**os.environ, "GIT_OPTIONAL_LOCKS": "0", "GIT_NO_LAZY_FETCH": "1"}
    changed: set[str] = set()
    diff = subprocess.run(
        ["git", "-C", str(root), "diff", "--no-ext-diff", "--name-only", ref],
        capture_output=True, text=True, check=False, env=env,
    )
    if diff.returncode == 0:
        changed.update(line.strip() for line in diff.stdout.splitlines() if line.strip())
    status = subprocess.run(
        ["git", "-C", str(root), "ls-files", "--others", "--exclude-standard"],
        capture_output=True, text=True, check=False, env=env,
    )
    if status.returncode == 0:
        changed.update(line.strip() for line in status.stdout.splitlines() if line.strip())

    advisories: list[tuple[str, list[str]]] = []
    for entry in manifest.get("surface_map", []):
        prefixes = tuple(entry.get("code_prefixes", ()))
        matched = sorted(path for path in changed if any(path == p or path.startswith(p) for p in prefixes))
        if matched:
            advisories.append((", ".join(matched[:3]) + (f" (+{len(matched) - 3} more)" if len(matched) > 3 else ""), list(entry.get("docs", []))))
    return advisories


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent,
                        help="repository root (defaults to this script's repository)")
    parser.add_argument("--manifest", type=Path, default=None,
                        help="path to doc-manifest.json")
    parser.add_argument("--changed-since", metavar="REF", default=None,
                        help="also print advisory documentation surfaces affected since REF")
    args = parser.parse_args(argv)

    root = args.root.resolve()
    issues = validate(root, args.manifest)
    if args.changed_since:
        for trigger, docs in affected_surfaces(root, args.changed_since, args.manifest):
            print(f"advisory: changes in {trigger} -> review {', '.join(docs)}")
    if issues:
        for issue in issues:
            print(issue, file=sys.stderr)
        print(f"Docs sync check failed: {len(issues)} issue(s).", file=sys.stderr)
        return 1
    print("Docs sync check passed: toolchain, lockfile mentions, constants, workflows, include_str! targets, and website guides.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
