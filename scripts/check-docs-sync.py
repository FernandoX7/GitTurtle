#!/usr/bin/env python3
"""Verify factual synchronization between code/manifests and maintained documentation.

Reads `docs/doc-manifest.json` using only the Python standard library and checks:
- Pinned Rust toolchain version (`rust-toolchain.toml`) across listed docs & website guides
- Workspace MSRV (`Cargo.toml` `rust-version`) across listed docs
- Pinned crate versions in `Cargo.lock` cited in living documentation
- Rust source constants (themes, tabs, history window) against user-facing docs
- GitHub Actions workflow filenames listed in contributor documentation (bidirectional)
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

_UNITS = (
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
    "seventeen", "eighteen", "nineteen",
)
_TENS = ("", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety")

RUST_VERSION_RE = re.compile(r"\bRust\s+(?:\*{1,2}|`)?(\d+\.\d+\.\d+)(?:\*{1,2}|`)?")
RUST_MSRV_RE = re.compile(r"\bRust\s+(?:\*{1,2}|`)?(\d+\.\d+)(?!\d)(?!\.\d)(?:\*{1,2}|`)?")
WORKFLOW_TICK_RE = re.compile(r"`([A-Za-z0-9_.-]+\.ya?ml)`")
INCLUDE_STR_RE = re.compile(r'include_str!\s*\(\s*"([^"]+)"\s*\)')


@dataclass(frozen=True)
class SyncIssue:
    path: str
    message: str

    def __str__(self) -> str:
        return f"{self.path}: {self.message}"


def _number_word(value: int) -> str:
    if 0 <= value < len(_UNITS):
        return _UNITS[value]
    if 20 <= value < 100:
        tens, rem = divmod(value, 10)
        return _TENS[tens] if rem == 0 else f"{_TENS[tens]}-{_UNITS[rem]}"
    return str(value)


def _resolve_repo_file(root: Path, rel: str) -> tuple[Path | None, str | None]:
    raw = Path(rel)
    if raw.is_absolute() or ".." in raw.parts:
        return None, f"path escapes repository: {rel!r}"
    candidate = root / raw
    if candidate.is_symlink():
        return None, f"refusing symlinked path: {rel!r}"
    resolved = candidate.resolve()
    if not resolved.is_relative_to(root):
        return None, f"path resolves outside repository: {rel!r}"
    if not resolved.is_file():
        return None, f"missing file: {rel!r}"
    return resolved, None


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _toolchain_channel(root: Path) -> str:
    path, err = _resolve_repo_file(root, "rust-toolchain.toml")
    if err or path is None:
        raise ValueError(err or "missing rust-toolchain.toml")
    data = tomllib.loads(_read_text(path))
    channel = data.get("toolchain", {}).get("channel")
    if not isinstance(channel, str) or not channel.strip():
        raise ValueError("rust-toolchain.toml missing toolchain.channel")
    return channel.strip()


def _workspace_rust_version(root: Path) -> str:
    path, err = _resolve_repo_file(root, "Cargo.toml")
    if err or path is None:
        raise ValueError(err or "missing Cargo.toml")
    data = tomllib.loads(_read_text(path))
    version = data.get("workspace", {}).get("package", {}).get("rust-version")
    if not isinstance(version, str) or not version.strip():
        raise ValueError("Cargo.toml missing workspace.package.rust-version")
    return version.strip()


def _lockfile_versions(root: Path) -> dict[str, set[str]]:
    path, err = _resolve_repo_file(root, "Cargo.lock")
    if err or path is None:
        return {}
    data = tomllib.loads(_read_text(path))
    versions: dict[str, set[str]] = {}
    for pkg in data.get("package", []):
        name = pkg.get("name")
        version = pkg.get("version")
        if isinstance(name, str) and isinstance(version, str):
            versions.setdefault(name, set()).add(version)
    return versions


def _extract_constant(root: Path, spec: dict) -> int:
    source_rel = spec["source"]
    source, err = _resolve_repo_file(root, source_rel)
    if err or source is None:
        raise ValueError(err or f"missing source {source_rel!r}")
    text = _read_text(source)
    kind = spec["kind"]
    if kind == "regex_int":
        match = re.search(spec["pattern"], text)
        if not match:
            raise ValueError(f"pattern {spec['pattern']!r} not found in {source_rel}")
        return int(match.group(1).replace("_", ""))
    symbol = re.escape(spec["symbol"])
    if kind == "rust_const_int":
        match = re.search(
            rf"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+{symbol}\s*:\s*[A-Za-z0-9_]+\s*=\s*([0-9][0-9_]*)(?:[iu](?:8|16|32|64|128|size))?\s*;",
            text,
        )
        if not match:
            raise ValueError(f"integer const {spec['symbol']!r} not found in {source_rel}")
        return int(match.group(1).replace("_", ""))
    if kind == "rust_const_mib":
        match = re.search(
            rf"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+{symbol}\s*:\s*[A-Za-z0-9_]+\s*=\s*([0-9][0-9_]*)\s*\*\s*1_?024\s*\*\s*1_?024\s*;",
            text,
        )
        if not match:
            raise ValueError(f"MiB const {spec['symbol']!r} not found in {source_rel}")
        return int(match.group(1).replace("_", ""))
    raise ValueError(f"unsupported constant kind: {kind!r}")


def _check_rust_toolchain(root: Path, docs: list[str]) -> list[SyncIssue]:
    if not docs:
        return []
    issues: list[SyncIssue] = []
    try:
        channel = _toolchain_channel(root)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        return [SyncIssue("rust-toolchain.toml", str(error))]
    for rel in docs:
        path, err = _resolve_repo_file(root, rel)
        if err or path is None:
            issues.append(SyncIssue(rel, f"invalid toolchain-referenced doc ({err})"))
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


def _check_rust_version(root: Path, docs: list[str]) -> list[SyncIssue]:
    if not docs:
        return []
    issues: list[SyncIssue] = []
    try:
        msrv = _workspace_rust_version(root)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        return [SyncIssue("Cargo.toml", str(error))]
    for rel in docs:
        path, err = _resolve_repo_file(root, rel)
        if err or path is None:
            issues.append(SyncIssue(rel, f"invalid MSRV-referenced doc ({err})"))
            continue
        text = _read_text(path)
        found = RUST_MSRV_RE.findall(text)
        if not found:
            issues.append(SyncIssue(rel, f"expected Rust {msrv} MSRV reference, found no 'Rust X.Y' mention"))
            continue
        stale = sorted({ver for ver in found if ver != msrv})
        if stale:
            issues.append(SyncIssue(rel, f"stale Rust MSRV mention(s) {', '.join(stale)} (Cargo.toml pins {msrv})"))
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
        path, err = _resolve_repo_file(root, rel)
        if err or path is None:
            issues.append(SyncIssue(rel, f"invalid doc referencing crate {crate!r} ({err})"))
            continue
        try:
            compiled = re.compile(pattern)
        except re.error as error:
            issues.append(SyncIssue("docs/doc-manifest.json", f"invalid regex for crate {crate!r}: {error}"))
            continue
        if compiled.groups != 1:
            issues.append(SyncIssue("docs/doc-manifest.json", f"regex for crate {crate!r} must have exactly 1 capture group"))
            continue
        matches = compiled.findall(_read_text(path))
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
        except (OSError, ValueError, re.error) as error:
            issues.append(SyncIssue(spec.get("source", "docs/doc-manifest.json"), str(error)))
            continue
        word_value = _number_word(value)
        comma_value = f"{value:,}"
        for target in spec.get("docs", []):
            rel = target["path"]
            path, err = _resolve_repo_file(root, rel)
            if err or path is None:
                issues.append(SyncIssue(rel, f"invalid doc for constant {spec['name']} ({err})"))
                continue
            try:
                rendered = target["pattern"].format(
                    value=re.escape(str(value)),
                    comma_value=re.escape(comma_value),
                    word_value=re.escape(word_value),
                )
                flags = re.IGNORECASE if target.get("ignore_case") else 0
                matched = re.search(rendered, _read_text(path), flags)
            except (KeyError, ValueError, re.error) as error:
                issues.append(SyncIssue("docs/doc-manifest.json", f"invalid pattern for {spec['name']} in {rel}: {error}"))
                continue
            if not matched:
                issues.append(
                    SyncIssue(
                        rel,
                        f"constant {spec['name']}={value} (from {spec['source']}) not matched by {rendered!r}",
                    )
                )
    return issues


def _check_workflows(root: Path, docs: list[str]) -> list[SyncIssue]:
    if not docs:
        return []
    workflows_dir = root / ".github" / "workflows"
    if not workflows_dir.is_dir() or workflows_dir.is_symlink():
        return [SyncIssue(".github/workflows", "expected workflows directory is missing or symlinked")]
    actual = sorted(
        p.name for p in workflows_dir.iterdir()
        if p.is_file() and not p.is_symlink() and p.suffix in {".yml", ".yaml"}
    )
    actual_set = set(actual)
    issues: list[SyncIssue] = []
    for rel in docs:
        path, err = _resolve_repo_file(root, rel)
        if err or path is None:
            issues.append(SyncIssue(rel, f"invalid workflow-listing doc ({err})"))
            continue
        text = _read_text(path)
        for name in actual:
            if not re.search(rf"(?<![A-Za-z0-9_.-]){re.escape(name)}(?![A-Za-z0-9_.-])", text):
                issues.append(SyncIssue(rel, f"workflow {name!r} exists in .github/workflows/ but is not mentioned"))
        for cited in sorted(set(WORKFLOW_TICK_RE.findall(text))):
            if cited not in actual_set:
                issues.append(SyncIssue(rel, f"cited workflow {cited!r} does not exist in .github/workflows/"))
    return issues


def _check_include_str(root: Path, roots: list[str]) -> list[SyncIssue]:
    issues: list[SyncIssue] = []
    for rel_root in roots:
        raw_root = Path(rel_root)
        if raw_root.is_absolute() or ".." in raw_root.parts:
            issues.append(SyncIssue(rel_root, "include_str_roots entry escapes repository"))
            continue
        base = root / raw_root
        if not base.is_dir() or base.is_symlink():
            issues.append(SyncIssue(rel_root, "declared include_str_roots directory is missing or symlinked"))
            continue
        for rs_file in sorted(base.rglob("*.rs")):
            if not rs_file.is_file() or rs_file.is_symlink():
                continue
            resolved_rs = rs_file.resolve()
            if not resolved_rs.is_relative_to(root):
                issues.append(SyncIssue(rel_root, f"Rust source resolves outside repository: {rs_file}"))
                continue
            text = resolved_rs.read_text(encoding="utf-8", errors="replace")
            rel_rs = resolved_rs.relative_to(root).as_posix()
            for match in INCLUDE_STR_RE.finditer(text):
                raw_target = match.group(1)
                if "{" in raw_target:
                    continue
                target_path = Path(raw_target)
                if target_path.is_absolute():
                    issues.append(SyncIssue(rel_rs, f"include_str! uses absolute path outside repo: {raw_target!r}"))
                    continue
                candidate = resolved_rs.parent / target_path
                resolved = candidate.resolve()
                if not resolved.is_relative_to(root):
                    issues.append(SyncIssue(rel_rs, f"include_str! escapes repo: {raw_target!r}"))
                elif candidate.is_symlink():
                    issues.append(SyncIssue(rel_rs, f"include_str! target is a symlink: {raw_target!r}"))
                elif not resolved.is_file():
                    issues.append(SyncIssue(rel_rs, f"include_str! target missing: {raw_target!r}"))
    return issues


def _check_website_guides(root: Path) -> list[SyncIssue]:
    script, err = _resolve_repo_file(root, "website/build_guides.py")
    if err or script is None:
        return [SyncIssue("website/build_guides.py", f"cannot verify website guides ({err})")]
    try:
        completed = subprocess.run(
            [sys.executable, "-B", str(script), "--check"],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return [SyncIssue("website/build_guides.py", "website guide verification timed out after 60s")]
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip().splitlines()
        summary = "; ".join(detail[:3]) if detail else f"exit {completed.returncode}"
        return [SyncIssue("website/build_guides.py", f"generated website guides/notices out of date: {summary}")]
    return []


def _load_manifest(root: Path, manifest_path: Path | None = None) -> tuple[dict | None, list[SyncIssue]]:
    if manifest_path is None:
        manifest_file, err = _resolve_repo_file(root, "docs/doc-manifest.json")
        if err or manifest_file is None:
            return None, [SyncIssue("docs/doc-manifest.json", f"cannot load doc manifest: {err}")]
    else:
        manifest_file = manifest_path
    try:
        manifest = json.loads(_read_text(manifest_file))
    except (OSError, ValueError) as error:
        return None, [SyncIssue(str(manifest_file), f"cannot load doc manifest: {error}")]
    if not isinstance(manifest, dict) or manifest.get("version") != 1:
        return None, [SyncIssue(str(manifest_file), "doc manifest must be a JSON object with version == 1")]
    return manifest, []


def validate(root: Path, manifest_path: Path | None = None) -> list[SyncIssue]:
    root = root.resolve()
    manifest, load_issues = _load_manifest(root, manifest_path)
    if load_issues or manifest is None:
        return load_issues

    issues: list[SyncIssue] = []
    issues.extend(_check_rust_toolchain(root, manifest.get("rust_toolchain_docs", [])))
    issues.extend(_check_rust_version(root, manifest.get("rust_version_docs", [])))
    issues.extend(_check_lockfile_mentions(root, manifest.get("lockfile_crate_mentions", [])))
    issues.extend(_check_constants(root, manifest.get("constants", [])))
    issues.extend(_check_workflows(root, manifest.get("workflows_listed_in", [])))
    issues.extend(_check_include_str(root, manifest.get("include_str_roots", [])))
    if manifest.get("check_website_guides"):
        issues.extend(_check_website_guides(root))
    return issues


def _matches_prefix(path: str, prefix: str) -> bool:
    if prefix.endswith("/"):
        return path.startswith(prefix)
    return path == prefix or path.startswith(prefix + "/")


def affected_surfaces(
    root: Path,
    ref: str,
    manifest_path: Path | None = None,
    manifest: dict | None = None,
) -> list[tuple[str, list[str]]]:
    root = root.resolve()
    if manifest is None:
        manifest, load_issues = _load_manifest(root, manifest_path)
        if load_issues or manifest is None:
            raise ValueError("; ".join(str(issue) for issue in load_issues))
    if not ref or ref.startswith("-"):
        raise ValueError(f"invalid git ref for --changed-since: {ref!r}")
    env = {
        **os.environ,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_NO_LAZY_FETCH": "1",
        "GIT_TERMINAL_PROMPT": "0",
    }
    git_base = ["git", "-C", str(root), "-c", "core.fsmonitor=false", "-c", "diff.external="]
    resolved = subprocess.run(
        [*git_base, "rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}"],
        capture_output=True, text=True, check=False, env=env, timeout=30,
    )
    if resolved.returncode != 0 or not resolved.stdout.strip():
        raise ValueError(f"cannot resolve commit ref {ref!r} for --changed-since")
    base_oid = resolved.stdout.strip()
    changed: set[str] = set()
    diff = subprocess.run(
        [*git_base, "diff", "--no-ext-diff", "--no-textconv", "--name-only", "-z", base_oid, "--"],
        capture_output=True, check=False, env=env, timeout=30,
    )
    if diff.returncode != 0:
        raise ValueError(f"git diff failed for {ref!r} (exit {diff.returncode})")
    changed.update(os.fsdecode(item) for item in diff.stdout.split(b"\0") if item)
    status = subprocess.run(
        [*git_base, "ls-files", "-z", "--others", "--exclude-standard"],
        capture_output=True, check=False, env=env, timeout=30,
    )
    if status.returncode != 0:
        raise ValueError(f"git ls-files failed (exit {status.returncode})")
    changed.update(os.fsdecode(item) for item in status.stdout.split(b"\0") if item)

    advisories: list[tuple[str, list[str]]] = []
    for entry in manifest.get("surface_map", []):
        prefixes = tuple(entry.get("code_prefixes", ()))
        matched = sorted(path for path in changed if any(_matches_prefix(path, p) for p in prefixes))
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
    manifest, load_issues = _load_manifest(root, args.manifest)
    issues = load_issues if (load_issues or manifest is None) else validate(root, args.manifest)
    if args.changed_since and manifest is not None:
        try:
            for trigger, docs in affected_surfaces(root, args.changed_since, args.manifest, manifest=manifest):
                print(f"advisory: changes in {trigger} -> review {', '.join(docs)}")
        except (ValueError, subprocess.SubprocessError) as error:
            issues = [*issues, SyncIssue("--changed-since", str(error))]
    if issues:
        for issue in issues:
            print(issue, file=sys.stderr)
        print(f"Docs sync check failed: {len(issues)} issue(s).", file=sys.stderr)
        return 1
    print("Docs sync check passed: toolchain, MSRV, lockfile mentions, constants, workflows, include_str! targets, and website guides.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
