#!/usr/bin/env python3
"""Tiered quality gate for GitTurtle.

    python3 scripts/gate.py <fast|full> [--base REF] [--report PATH] [--strict]
        [--quiet] [--known-failures FILE] [--changed-only | --workspace]

``fast`` is the per-change gate (formatting, spelling, unused dependencies, a
workspace check, and clippy plus tests scoped to the changed crates). ``full``
is the per-candidate gate at workspace scope with doctests, rustdoc, license
and advisory checks, snapshot hygiene, diff-scoped mutation testing, the
release build and the development-tooling checks.

Exit codes: 0 green; 1 a stage failed; 3 a required tool is missing or unusable
in ``--strict`` mode; 4 usage error. Without ``--strict`` a missing optional
tool prints a ``warn:`` line and its stage is skipped; advisory stages (unused
dependencies, rustdoc, license/advisory audits, mutation testing) also only
warn without ``--strict``.

Output: on success exactly one stdout line ``gate: ok (<tier>, <n> stages,
<secs>s)``. Stage progress goes to stderr unless ``--quiet``. On failure a
compact report for the first failing stage is printed to stdout and written to
``--report`` (default ``.local/gate/report.md``; ``.local/`` is git-ignored)
with a JSON twin beside it.

Scope: the base ref is ``--base``, else ``GITTURTLE_GATE_BASE``, else the
merge-base with ``main`` (``origin/main`` fallback), else ``HEAD``. Changed
paths are ``git diff --name-only <base>`` plus the working tree. Paths under a
workspace crate select that crate; Rust-affecting paths outside a crate
(``vendor/``, ``Cargo.toml``, ``Cargo.lock``, ``.cargo/``, stray ``.rs`` files
and the gate configuration) widen the scope to the whole workspace;
documentation, agent configuration and scripts do not widen it.

Known failures: ``--known-failures FILE`` or ``GITTURTLE_GATE_KNOWN_FAILURES``
(default ``.local/gate/known-failures.txt`` when it exists) lists test names,
one per line; a test stage whose only failures are listed passes with a warn.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

TIERS = ("fast", "full")
DEFAULT_REPORT = Path(".local/gate/report.md")
DEFAULT_KNOWN_FAILURES = Path(".local/gate/known-failures.txt")
FALLBACK_CRATES = {
    "crates/app": "gitturtle",
    "crates/git-core": "gitturtle-core",
    "crates/preview": "gitturtle-preview",
}
WIDEN_PREFIXES = ("vendor/", ".cargo/", ".config/")
WIDEN_FILES = {
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "clippy.toml",
    "deny.toml",
    "build.rs",
}
OPTIONAL_TOOLS = {
    "nextest": "cargo-nextest",
    "typos": "typos",
    "machete": "cargo-machete",
    "deny": "cargo-deny",
    "audit": "cargo-audit",
    "mutants": "cargo-mutants",
    "llvm-cov": "cargo-llvm-cov",
}
COVERAGE_CRATES = ("gitturtle-core", "gitturtle-preview")
MAX_REPORT_LINES = 200
MAX_TAIL_LINES = 20
MAX_DIAGNOSTICS = 5
MAX_STAGE_SECONDS = {"fast": 1800, "full": 5400}
MAX_KEPT_LINES = 20000
MAX_ITERATION_TESTS = 40
GPUI_ITERATIONS = "20"

RUST_SPAN = re.compile(r"--> ([^\s:]+):(\d+):(\d+)")
PANIC_AT = re.compile(r"panicked at ([^\s:]+):(\d+):(\d+)")
TYPOS_LINE = re.compile(r"^([^\s:]+):(\d+):(\d+): ")
# nextest 0.9.145 prints `FAIL [   0.014s] (1/1) gitturtle-core path::to::test`;
# older versions omit the `(n/m)` counter.
NEXTEST_STATUS = re.compile(r"^\s*([A-Z][A-Z0-9 /]*?)\s+\[\s*[0-9.]+s\]\s+(?:\(\d+/\d+\)\s+)?(\S+)\s+(\S+)\s*$")
NEXTEST_BLOCK = re.compile(r"^(?:--- (?:STDOUT|STDERR):|\s*(?:stdout|stderr) ─)")
NEXTEST_RULE = re.compile(r"^\s*(?:-{6,}|─{6,})\s*$")
CARGO_TEST_FAILED = re.compile(r"^test (\S+) \.\.\. FAILED$")
REMOVED_TEST = re.compile(r"^-\s*#\[(?:test\b|gpui::test\b|tokio::test\b)")
GPUI_TEST_FN = re.compile(r"#\[gpui::test[^\]]*\]\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")


class GateUsage(Exception):
    pass


@dataclass(frozen=True)
class Scope:
    crates: tuple[str, ...]
    widened: bool
    rust_changed: bool
    app_sources: tuple[str, ...]

    @property
    def workspace(self) -> bool:
        return self.widened

    @property
    def empty(self) -> bool:
        return not self.crates and not self.widened


@dataclass
class Stage:
    name: str
    argv: list[str] | None
    kind: str = "plain"
    reproduce: str = ""
    optional_tool: str | None = None
    advisory: bool = False
    strict_only: bool = False
    env: dict[str, str] = field(default_factory=dict)
    cwd: Path | None = None
    timeout: float | None = None
    run: Callable[[], tuple[int, str]] | None = None
    clear_target_dir: bool = False
    umask: int | None = None
    note: str = ""


@dataclass
class Outcome:
    stage: Stage
    returncode: int
    elapsed: float
    lines: list[str]
    filtered: list[str]
    first_error: str | None
    failed_tests: tuple[str, ...]
    status: str  # ok, warn, skipped, failed, missing
    message: str = ""


# --- repository and scope --------------------------------------------------


def find_root(start: Path | None = None) -> Path:
    start = (start or Path.cwd()).resolve()
    probe = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        cwd=start,
        capture_output=True,
        text=True,
        check=False,
    )
    if probe.returncode == 0 and probe.stdout.strip():
        return Path(probe.stdout.strip())
    here = Path(__file__).resolve().parent.parent
    if (here / "Cargo.toml").exists():
        return here
    raise GateUsage("not inside a Git checkout of GitTurtle")


def git(root: Path, *args: str) -> tuple[int, str]:
    result = subprocess.run(
        ["git", *args], cwd=root, capture_output=True, text=True, errors="replace", check=False
    )
    return result.returncode, result.stdout


def load_crates(root: Path) -> dict[str, str]:
    """Map workspace member directories to their package names."""
    manifest = root / "Cargo.toml"
    members: list[str] = []
    if manifest.exists():
        text = manifest.read_text(encoding="utf-8", errors="replace")
        match = re.search(r"^members\s*=\s*\[(.*?)\]", text, re.M | re.S)
        if match:
            members = re.findall(r'"([^"]+)"', match.group(1))
    mapping: dict[str, str] = {}
    for member in members:
        member_manifest = root / member / "Cargo.toml"
        if not member_manifest.exists():
            continue
        body = member_manifest.read_text(encoding="utf-8", errors="replace")
        package = re.search(r"^\[package\](.*?)(?=^\[|\Z)", body, re.M | re.S)
        section = package.group(1) if package else body
        name = re.search(r'^name\s*=\s*"([^"]+)"', section, re.M)
        if name:
            mapping[member.strip("/")] = name.group(1)
    return mapping or dict(FALLBACK_CRATES)


def resolve_base(root: Path, explicit: str | None) -> str | None:
    candidates = [explicit, os.environ.get("GITTURTLE_GATE_BASE")]
    for candidate in candidates:
        if candidate:
            code, _ = git(root, "rev-parse", "--verify", "--quiet", candidate + "^{commit}")
            if code != 0:
                raise GateUsage(f"base ref {candidate!r} does not resolve to a commit")
            return candidate
    for branch in ("main", "origin/main"):
        code, out = git(root, "merge-base", "HEAD", branch)
        if code == 0 and out.strip():
            return out.strip()
    code, out = git(root, "rev-parse", "--verify", "--quiet", "HEAD")
    return out.strip() if code == 0 and out.strip() else None


def porcelain_paths(text: str) -> set[str]:
    paths: set[str] = set()
    for line in text.splitlines():
        if len(line) < 4:
            continue
        entry = line[3:]
        if " -> " in entry:
            old, new = entry.split(" -> ", 1)
            paths.add(old.strip('"'))
            paths.add(new.strip('"'))
        else:
            paths.add(entry.strip('"'))
    return paths


def changed_paths(root: Path, base: str | None) -> set[str]:
    paths: set[str] = set()
    if base:
        code, out = git(root, "diff", "--name-only", base)
        if code == 0:
            paths.update(line.strip() for line in out.splitlines() if line.strip())
    code, out = git(root, "status", "--porcelain", "--untracked-files=all")
    if code == 0:
        paths.update(porcelain_paths(out))
    return {path.rstrip("/") for path in paths if path}


def classify(paths: set[str], crates: dict[str, str]) -> Scope:
    selected: set[str] = set()
    widened = False
    app_sources: list[str] = []
    for path in sorted(paths):
        crate_dir = next((d for d in crates if path == d or path.startswith(d + "/")), None)
        if crate_dir:
            selected.add(crates[crate_dir])
            if crate_dir == "crates/app" and path.endswith(".rs") and "/src/" in path:
                app_sources.append(path)
            continue
        name = path.rsplit("/", 1)[-1]
        if (
            path.endswith(".rs")
            or name in WIDEN_FILES
            or path.startswith(WIDEN_PREFIXES)
        ):
            widened = True
    rust_changed = widened or bool(selected)
    return Scope(tuple(sorted(selected)), widened, rust_changed, tuple(app_sources))


def removed_tests(root: Path, base: str | None) -> int:
    if not base:
        return 0
    code, out = git(root, "diff", base)
    if code != 0:
        return 0
    return sum(1 for line in out.splitlines() if REMOVED_TEST.match(line))


def gpui_test_names(root: Path, sources: tuple[str, ...]) -> list[str]:
    names: list[str] = []
    for relative in sources:
        path = root / relative
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        names.extend(GPUI_TEST_FN.findall(text))
    return sorted(set(names))


def read_known_failures(root: Path, explicit: str | None) -> tuple[set[str], str | None]:
    candidate = explicit or os.environ.get("GITTURTLE_GATE_KNOWN_FAILURES")
    path = Path(candidate) if candidate else root / DEFAULT_KNOWN_FAILURES
    if not path.is_absolute():
        path = root / path
    if not path.exists():
        if explicit:
            raise GateUsage(f"known-failures file {path} does not exist")
        return set(), None
    names = {
        line.strip()
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    }
    return names, str(path)


# --- output filtering -------------------------------------------------------


def first_rust_location(lines: list[str]) -> str | None:
    for line in lines:
        match = PANIC_AT.search(line) or RUST_SPAN.search(line)
        if match:
            return f"{match.group(1)}:{match.group(2)}:{match.group(3)}"
    return None


def filter_cargo_json(lines: list[str]) -> tuple[list[str], str | None]:
    """Reduce ``--message-format=json`` output to a few rendered diagnostics."""
    rendered: list[str] = []
    first: str | None = None
    first_warning: str | None = None
    total = 0
    other: list[str] = []
    for line in lines:
        if not line.startswith("{"):
            if line.strip():
                other.append(line)
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            other.append(line)
            continue
        if message.get("reason") != "compiler-message":
            continue
        diagnostic = message.get("message") or {}
        level = diagnostic.get("level")
        if level not in ("error", "warning", "error: internal compiler error"):
            continue
        total += 1
        location = None
        for span in diagnostic.get("spans") or []:
            if span.get("is_primary"):
                location = f"{span.get('file_name')}:{span.get('line_start')}:{span.get('column_start')}"
                break
        if location:
            if level == "warning":
                first_warning = first_warning or location
            else:
                first = first or location
        if len(rendered) < MAX_DIAGNOSTICS:
            rendered.append((diagnostic.get("rendered") or diagnostic.get("message") or "").rstrip())
    out: list[str] = []
    for block in rendered:
        out.extend(block.splitlines())
    if total > len(rendered):
        out.append(f"... {total - len(rendered)} more diagnostics")
    if first is None:
        first = first_warning or first_rust_location(out) or first_rust_location(other)
    out.extend(other[-MAX_TAIL_LINES:])
    return out, first


def filter_nextest(lines: list[str]) -> tuple[list[str], tuple[str, ...], str | None]:
    kept: list[str] = []
    failed: list[str] = []
    in_block = False
    block_lines = 0
    for line in lines:
        status = NEXTEST_STATUS.match(line)
        if status:
            verdict = status.group(1).strip()
            in_block = False
            if verdict.startswith("PASS") or verdict.startswith("SKIP") or verdict.startswith("SLOW"):
                continue
            if any(token in verdict for token in ("FAIL", "TIMEOUT", "LEAK", "SIG", "ABORT")):
                name = status.group(3)
                if name not in failed:
                    failed.append(name)
                kept.append(line.rstrip())
            continue
        if NEXTEST_BLOCK.match(line):
            in_block = True
            block_lines = 0
            kept.append(line.rstrip())
            continue
        if NEXTEST_RULE.match(line) or line.strip().startswith("Summary"):
            in_block = False
            kept.append(line.rstrip())
            continue
        if in_block:
            block_lines += 1
            if block_lines <= 60:
                kept.append(line.rstrip())
            continue
        if line.startswith("error") or "error[" in line or line.startswith("warning: unused"):
            kept.append(line.rstrip())
    if not kept:
        kept = [line.rstrip() for line in lines[-MAX_TAIL_LINES:]]
    return kept, tuple(failed), first_rust_location(kept) or first_rust_location(lines)


def filter_cargo_test(lines: list[str]) -> tuple[list[str], tuple[str, ...], str | None]:
    failed: list[str] = []
    kept: list[str] = []
    in_failures = False
    for line in lines:
        match = CARGO_TEST_FAILED.match(line.strip())
        if match:
            failed.append(match.group(1))
            kept.append(line.rstrip())
            continue
        if line.strip() == "failures:":
            in_failures = True
        if in_failures or line.startswith("error") or "error[" in line:
            kept.append(line.rstrip())
        if line.startswith("test result:"):
            in_failures = False
    if len(kept) > MAX_REPORT_LINES:
        kept = kept[: MAX_REPORT_LINES // 2] + ["..."] + kept[-MAX_REPORT_LINES // 2 :]
    if not kept:
        kept = [line.rstrip() for line in lines[-MAX_TAIL_LINES:]]
    return kept, tuple(dict.fromkeys(failed)), first_rust_location(kept) or first_rust_location(lines)


def filter_typos(lines: list[str]) -> tuple[list[str], str | None]:
    hits = [line.rstrip() for line in lines if TYPOS_LINE.match(line)]
    first = None
    if hits:
        match = TYPOS_LINE.match(hits[0])
        if match:
            first = f"{match.group(1)}:{match.group(2)}:{match.group(3)}"
    kept = hits[:MAX_DIAGNOSTICS]
    if len(hits) > MAX_DIAGNOSTICS:
        kept.append(f"... {len(hits) - MAX_DIAGNOSTICS} more typos")
    return kept or [line.rstrip() for line in lines[-MAX_TAIL_LINES:]], first


def filter_deny(lines: list[str]) -> tuple[list[str], str | None]:
    errors: list[str] = []
    warnings: list[str] = []
    counts: dict[str, int] = {}
    for line in lines:
        if not line.startswith("{"):
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        fields = entry.get("fields") or {}
        severity = fields.get("severity") or entry.get("type") or "diagnostic"
        counts[severity] = counts.get(severity, 0) + 1
        if severity not in ("error", "warning"):
            continue
        code = fields.get("code", "")
        message = fields.get("message", "")
        graphs = fields.get("graphs") or []
        crate = ""
        if graphs and isinstance(graphs, list) and isinstance(graphs[0], dict):
            krate = graphs[0].get("Krate")
            if isinstance(krate, dict):
                crate = f"{krate.get('name', '')} {krate.get('version', '')}".strip()
        labels = " ".join(str(label.get("span", "")).splitlines()[0] for label in fields.get("labels") or [] if label.get("span"))
        advisory = (fields.get("advisory") or {}).get("id", "")
        text = " ".join(part for part in (f"{severity}[{code}]", crate, message, labels, advisory) if part)
        (errors if severity == "error" else warnings).append(text)
    kept = errors[:MAX_DIAGNOSTICS] + warnings[: max(0, MAX_DIAGNOSTICS - len(errors))]
    summary = ", ".join(f"{k}={v}" for k, v in sorted(counts.items()))
    if summary:
        kept.append(f"cargo-deny diagnostics: {summary}")
    return kept or [line.rstrip() for line in lines[-MAX_TAIL_LINES:]], None


def filter_audit(lines: list[str]) -> tuple[list[str], str | None]:
    for line in lines:
        if not line.startswith("{"):
            continue
        try:
            report = json.loads(line)
        except json.JSONDecodeError:
            continue
        vulnerabilities = (report.get("vulnerabilities") or {}).get("list") or []
        warnings = report.get("warnings") or {}
        kept = [f"advisories: {len(vulnerabilities)} vulnerabilities, {sum(len(v) for v in warnings.values())} warnings"]
        for item in vulnerabilities[:MAX_DIAGNOSTICS]:
            advisory = item.get("advisory") or {}
            package = item.get("package") or {}
            kept.append(f"{advisory.get('id')} {package.get('name')} {package.get('version')}: {advisory.get('title')}")
        for kind, entries in warnings.items():
            for item in entries[:MAX_DIAGNOSTICS]:
                package = item.get("package") or {}
                advisory = item.get("advisory") or {}
                kept.append(f"{kind}: {package.get('name')} {package.get('version')} {advisory.get('id') or ''}".rstrip())
        return kept, None
    return [line.rstrip() for line in lines[-MAX_TAIL_LINES:]], None


def filter_mutants(lines: list[str]) -> tuple[list[str], str | None]:
    kept = [
        line.rstrip()
        for line in lines
        if line.startswith(("MISSED", "TIMEOUT", "UNVIABLE", "Found", "error", "warning"))
        or " mutants tested" in line
    ]
    return kept[:MAX_REPORT_LINES] or [line.rstrip() for line in lines[-MAX_TAIL_LINES:]], None


def filter_plain(lines: list[str]) -> tuple[list[str], str | None]:
    kept = [line.rstrip() for line in lines[-MAX_TAIL_LINES:]]
    return kept, first_rust_location(lines)


# --- stage execution ---------------------------------------------------------


def which_tools() -> dict[str, bool]:
    return {name: shutil.which(binary) is not None for name, binary in OPTIONAL_TOOLS.items()}


def base_env(clear_target_dir: bool = False) -> dict[str, str]:
    env = dict(os.environ)
    env["CARGO_TERM_COLOR"] = "never"
    env["INSTA_UPDATE"] = "no"
    env.setdefault("CARGO_INCREMENTAL", "1")
    if clear_target_dir:
        env.pop("CARGO_TARGET_DIR", None)
    return env


def run_stage(stage: Stage, root: Path, tier: str) -> Outcome:
    started = time.monotonic()
    if stage.run is not None:
        code, text = stage.run()
        lines = text.splitlines()
    else:
        assert stage.argv is not None
        env = base_env(stage.clear_target_dir)
        env.update(stage.env)
        umask = stage.umask
        try:
            completed = subprocess.run(
                stage.argv,
                cwd=stage.cwd or root,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                errors="replace",
                timeout=stage.timeout or MAX_STAGE_SECONDS[tier],
                check=False,
                preexec_fn=(lambda: os.umask(umask)) if umask is not None else None,
            )
            code, text = completed.returncode, completed.stdout or ""
        except subprocess.TimeoutExpired as error:
            partial = error.stdout or b""
            if isinstance(partial, bytes):
                partial = partial.decode("utf-8", "replace")
            code, text = 124, (partial or "") + f"\n[gate] stage timed out after {int(time.monotonic() - started)}s\n"
        except FileNotFoundError as error:
            code, text = 127, f"[gate] cannot launch {stage.argv[0]}: {error}\n"
        lines = text.splitlines()
    if len(lines) > MAX_KEPT_LINES:
        lines = lines[-MAX_KEPT_LINES:]
    elapsed = time.monotonic() - started
    failed_tests: tuple[str, ...] = ()
    if stage.kind == "cargo-json":
        filtered, first = filter_cargo_json(lines)
    elif stage.kind == "nextest":
        filtered, failed_tests, first = filter_nextest(lines)
    elif stage.kind == "cargo-test":
        filtered, failed_tests, first = filter_cargo_test(lines)
    elif stage.kind == "typos":
        filtered, first = filter_typos(lines)
    elif stage.kind == "deny":
        filtered, first = filter_deny(lines)
    elif stage.kind == "audit":
        filtered, first = filter_audit(lines)
    elif stage.kind == "mutants":
        filtered, first = filter_mutants(lines)
    else:
        filtered, first = filter_plain(lines)
    status = "ok" if code == 0 else "failed"
    return Outcome(stage, code, elapsed, lines, filtered, first, failed_tests, status)


def apply_known_failures(outcome: Outcome, known: set[str]) -> Outcome:
    if outcome.status != "failed" or outcome.stage.kind not in ("nextest", "cargo-test"):
        return outcome
    if not outcome.failed_tests or not known:
        return outcome
    if set(outcome.failed_tests) <= known:
        outcome.status = "warn"
        outcome.message = "only known host failures: " + ", ".join(outcome.failed_tests)
    return outcome


# --- stage construction ------------------------------------------------------


def cargo_json(argv: list[str]) -> list[str]:
    return argv + ["--message-format=json"]


def insta_check(root: Path) -> Callable[[], tuple[int, str]]:
    def run() -> tuple[int, str]:
        stale = sorted(
            str(path.relative_to(root))
            for path in root.glob("crates/**/*.snap.new")
            if "target" not in path.parts
        )
        if stale:
            return 1, "pending insta snapshots must be reviewed, not accepted blindly:\n" + "\n".join(stale) + "\n"
        return 0, "no pending .snap.new files\n"

    return run


def build_stages(
    tier: str,
    scope: Scope,
    root: Path,
    tools: dict[str, bool],
    base: str | None,
    strict: bool,
    crates: dict[str, str],
    scratch: Path,
) -> list[Stage]:
    stages: list[Stage] = []
    workspace = scope.workspace
    targets = list(scope.crates)
    app_crate = crates.get("crates/app", "gitturtle")

    stages.append(Stage("format", ["cargo", "fmt", "--all", "--", "--check"], reproduce="cargo fmt --all -- --check"))
    stages.append(
        Stage("typos", ["typos", "--format", "brief"], kind="typos", reproduce="typos --format brief", optional_tool="typos")
    )
    stages.append(
        Stage(
            "machete",
            ["cargo", "machete", "crates"],
            reproduce="cargo machete crates",
            optional_tool="machete",
            advisory=True,
            note="workspace crates only; vendored packages are upstream code",
        )
    )
    stages.append(
        Stage(
            "check",
            cargo_json(["cargo", "check", "--locked", "--workspace", "--all-targets"]),
            kind="cargo-json",
            reproduce="cargo check --locked --workspace --all-targets",
        )
    )

    if workspace:
        stages.append(
            Stage(
                "clippy",
                cargo_json(["cargo", "clippy", "--locked", "--workspace", "--all-targets"]) + ["--", "-D", "warnings"],
                kind="cargo-json",
                reproduce="cargo clippy --locked --workspace --all-targets -- -D warnings",
            )
        )
        stages.append(test_stage("tests", ["--workspace"], tools, "workspace"))
    else:
        for crate in targets:
            stages.append(
                Stage(
                    f"clippy:{crate}",
                    cargo_json(["cargo", "clippy", "--locked", "-p", crate, "--no-deps", "--all-targets"]) + ["--", "-D", "warnings"],
                    kind="cargo-json",
                    reproduce=f"cargo clippy --locked -p {crate} --no-deps --all-targets -- -D warnings",
                )
            )
        for crate in targets:
            stages.append(test_stage(f"tests:{crate}", ["-p", crate], tools, crate))

    if scope.app_sources and (workspace or app_crate in targets):
        names = gpui_test_names(root, scope.app_sources)
        if names and len(names) <= MAX_ITERATION_TESTS:
            if tools["nextest"]:
                filterset = " + ".join(f"test(/::{re.escape(name)}$/)" for name in names)
                argv = ["cargo", "nextest", "run", "--locked", "-p", app_crate, "-P", "ci", "--no-fail-fast", "--no-tests=warn", "-E", filterset]
                kind = "nextest"
            else:
                argv = ["cargo", "test", "--locked", "-p", app_crate, "--", *names]
                kind = "cargo-test"
            stages.append(
                Stage(
                    "gpui-iterations",
                    argv,
                    kind=kind,
                    reproduce=f"ITERATIONS={GPUI_ITERATIONS} " + " ".join(shell_quote(a) for a in argv),
                    env={"ITERATIONS": GPUI_ITERATIONS},
                    note=f"{len(names)} gpui tests from changed app sources",
                )
            )
        elif names:
            stages.append(Stage("gpui-iterations", None, note=f"skipped: {len(names)} gpui tests exceed the {MAX_ITERATION_TESTS} rerun cap"))

    if tier == "fast":
        return stages

    stages.append(
        Stage("doctests", ["cargo", "test", "--doc", "--workspace", "--locked"], reproduce="cargo test --doc --workspace --locked")
    )
    stages.append(
        Stage(
            "doc",
            ["cargo", "doc", "--no-deps", "--workspace", "--locked"],
            reproduce="RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --workspace --locked",
            env={"RUSTDOCFLAGS": "-D warnings"},
            advisory=True,
        )
    )
    stages.append(Stage("insta", None, reproduce="find crates -name '*.snap.new'", run=insta_check(root)))
    stages.append(
        Stage(
            "deny",
            ["cargo", "deny", "--format", "json", "check"],
            kind="deny",
            reproduce="cargo deny check",
            optional_tool="deny",
            advisory=True,
        )
    )
    stages.append(
        Stage(
            "audit",
            ["cargo", "audit", "--json", "--deny", "warnings"],
            kind="audit",
            reproduce="cargo audit --deny warnings",
            optional_tool="audit",
            advisory=True,
        )
    )
    if base and scope.rust_changed and tools["mutants"]:
        diff_path = scratch / "git.diff"
        code, diff_text = git(root, "diff", base, "--", "crates")
        if code == 0 and diff_text.strip():
            diff_path.write_text(diff_text, encoding="utf-8")
            argv = ["cargo", "mutants", "--in-diff", str(diff_path), "--test-tool", "nextest" if tools["nextest"] else "cargo", "--baseline=skip", "-j", "2", "--output", str(root / ".local/gate/mutants")]
            for crate in targets if not workspace else []:
                argv += ["-p", crate]
            stages.append(
                Stage(
                    "mutants",
                    argv,
                    kind="mutants",
                    reproduce=" ".join(shell_quote(a) for a in argv),
                    advisory=True,
                    clear_target_dir=True,
                    note="diff-scoped mutation testing; exit 2 means uncaught mutants",
                )
            )
    elif scope.rust_changed and not tools["mutants"]:
        stages.append(Stage("mutants", None, optional_tool="mutants", run=lambda: (0, "")))
    if strict and tools["llvm-cov"]:
        minimum = os.environ.get("GITTURTLE_GATE_COVERAGE_MIN")
        if minimum:
            argv = ["cargo", "llvm-cov", "nextest" if tools["nextest"] else "test", "--locked"]
            for crate in COVERAGE_CRATES:
                argv += ["-p", crate]
            argv += ["--fail-under-lines", minimum]
            stages.append(Stage("coverage", argv, reproduce=" ".join(argv), strict_only=True, note=f"minimum lines {minimum}%"))
    stages.append(
        Stage("release", ["cargo", "build", "--release", "--locked", "-p", app_crate], reproduce=f"cargo build --release --locked -p {app_crate}")
    )
    stages.append(
        Stage("guidance", [sys.executable, "scripts/check-agent-guidance.py"], reproduce="python3 scripts/check-agent-guidance.py")
    )
    stages.append(
        Stage(
            "controller-tests",
            [sys.executable, "-m", "unittest", "discover", "-s", "scripts/agent_loop", "-t", "scripts", "-p", "test_*.py"],
            reproduce="(umask 077 && python3 -m unittest discover -s scripts/agent_loop -t scripts -p 'test_*.py')",
            # The controller refuses group-writable records; a host umask of 002
            # would otherwise fail its suite without any code defect.
            umask=0o077,
            note="umask 077",
        )
    )
    return stages


def test_stage(name: str, selector: list[str], tools: dict[str, bool], label: str) -> Stage:
    if tools["nextest"]:
        argv = ["cargo", "nextest", "run", "--locked", *selector, "-P", "ci", "--no-fail-fast"]
        return Stage(name, argv, kind="nextest", reproduce=" ".join(argv))
    argv = ["cargo", "test", "--locked", *selector]
    return Stage(name, argv, kind="cargo-test", reproduce=" ".join(argv), note="cargo-nextest missing; using cargo test")


def shell_quote(value: str) -> str:
    if re.fullmatch(r"[A-Za-z0-9_./:=@%+,-]+", value):
        return value
    return "'" + value.replace("'", "'\\''") + "'"


def narrow_command(outcome: Outcome) -> str:
    stage = outcome.stage
    if outcome.failed_tests:
        crate = ""
        if stage.argv and "-p" in stage.argv:
            crate = stage.argv[stage.argv.index("-p") + 1]
        test = outcome.failed_tests[0]
        if stage.kind == "nextest":
            selector = f"-p {crate}" if crate else "--workspace"
            return f"cargo nextest run --locked {selector} -E 'test(={test})'"
        selector = f"-p {crate}" if crate else "--workspace"
        return f"cargo test --locked {selector} -- {test} --exact"
    return stage.reproduce or (" ".join(shell_quote(a) for a in stage.argv) if stage.argv else stage.name)


# --- reporting ---------------------------------------------------------------


def render_report(
    tier: str,
    failing: Outcome,
    outcomes: list[Outcome],
    tests_removed: int,
    tools: dict[str, bool],
    root: Path,
) -> str:
    stage = failing.stage
    lines = [
        f"# Gate report: {tier} ({time.strftime('%Y-%m-%d %H:%M:%S')})",
        "",
        f"Stage: {stage.name}",
        f"Command: {' '.join(shell_quote(a) for a in stage.argv) if stage.argv else stage.reproduce or stage.name}",
        f"Exit: {failing.returncode}",
        f"First error: {failing.first_error or 'n/a'}",
        f"Next command: {narrow_command(failing)}",
        "",
        "Last lines:",
        "```",
    ]
    tail = failing.filtered[-MAX_TAIL_LINES:] if failing.filtered else failing.lines[-MAX_TAIL_LINES:]
    lines.extend(tail)
    lines.append("```")
    extra = [line for line in failing.filtered if line not in tail]
    if extra:
        lines.append("")
        lines.append("Filtered diagnostics:")
        lines.append("```")
        lines.extend(extra[: MAX_REPORT_LINES - len(lines) - 12])
        lines.append("```")
    lines.append("")
    lines.append(f"Tests removed: {tests_removed}")
    lines.append("Tooling: " + " ".join(f"{name}={'yes' if present else 'no'}" for name, present in tools.items()))
    lines.append("Stages: " + ", ".join(f"{o.stage.name}={o.status}" for o in outcomes))
    lines.append(f"Root: {root}")
    return "\n".join(lines[:MAX_REPORT_LINES]) + "\n"


def write_report(path: Path, text: str, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.with_suffix(".json").write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


# --- entry point -------------------------------------------------------------


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="gate.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("tier", choices=TIERS)
    parser.add_argument("--base")
    parser.add_argument("--report")
    parser.add_argument("--strict", action="store_true")
    parser.add_argument("--quiet", action="store_true")
    parser.add_argument("--known-failures")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--changed-only", action="store_true")
    group.add_argument("--workspace", action="store_true")
    try:
        return parser.parse_args(argv)
    except SystemExit as error:
        raise GateUsage("invalid arguments; see --help") from error


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(sys.argv[1:] if argv is None else argv)
        root = find_root()
        known, known_path = read_known_failures(root, args.known_failures)
        base = resolve_base(root, args.base)
    except GateUsage as error:
        print(f"gate: usage error: {error}", file=sys.stderr)
        return 4

    def progress(message: str) -> None:
        if not args.quiet:
            print(message, file=sys.stderr, flush=True)

    crates = load_crates(root)
    paths = changed_paths(root, base)
    scope = classify(paths, crates)
    if args.workspace:
        scope = Scope(tuple(sorted(crates.values())), True, True, scope.app_sources)
    elif args.tier == "full" and not args.changed_only:
        scope = Scope(tuple(sorted(crates.values())), True, True, scope.app_sources)
    tools = which_tools()
    tests_removed = removed_tests(root, base)
    progress(
        f"gate {args.tier}: base={base[:12] if base else 'none'} crates={','.join(scope.crates) or 'none'}"
        f"{' (workspace)' if scope.workspace else ''} tools=" + ",".join(name for name, ok in tools.items() if ok)
    )
    if known_path:
        progress(f"gate: known failures from {known_path}: {len(known)}")
    if not scope.rust_changed and args.tier == "fast":
        progress("gate: no Rust changes; running formatting, spelling and workspace check only")

    started = time.monotonic()
    outcomes: list[Outcome] = []
    failing: Outcome | None = None
    with tempfile.TemporaryDirectory(prefix="gitturtle-gate-") as scratch_name:
        scratch = Path(scratch_name)
        stages = build_stages(args.tier, scope, root, tools, base, args.strict, crates, scratch)
        for stage in stages:
            if stage.optional_tool and not tools[stage.optional_tool]:
                binary = OPTIONAL_TOOLS[stage.optional_tool]
                if args.strict:
                    print(f"gate: required tool {binary} is missing for stage {stage.name}", file=sys.stderr)
                    return 3
                progress(f"warn: {stage.name} skipped; install {binary} (cargo install --locked {binary})")
                outcomes.append(Outcome(stage, 0, 0.0, [], [], None, (), "missing"))
                continue
            if stage.argv is None and stage.run is None:
                outcomes.append(Outcome(stage, 0, 0.0, [], [], None, (), "skipped", stage.note))
                progress(f"warn: {stage.name} {stage.note}")
                continue
            progress(f"stage {stage.name}{' (' + stage.note + ')' if stage.note else ''} ...")
            outcome = apply_known_failures(run_stage(stage, root, args.tier), known)
            if outcome.status == "failed" and stage.advisory and not args.strict:
                outcome.status = "warn"
                summary = outcome.first_error or (outcome.filtered[0] if outcome.filtered else f"exit {outcome.returncode}")
                outcome.message = f"advisory stage red (exit {outcome.returncode}): {summary}"
            if outcome.status == "failed" and stage.kind == "mutants" and outcome.returncode == 3:
                outcome.status = "warn"
                outcome.message = "mutants timed out; rerun with a longer budget"
            outcomes.append(outcome)
            if outcome.status == "warn":
                progress(f"warn: {stage.name} {outcome.message} ({outcome.elapsed:.1f}s)")
                for line in outcome.filtered[:MAX_DIAGNOSTICS]:
                    progress(f"      {line}")
                continue
            if outcome.status == "failed":
                progress(f"FAIL {stage.name} exit {outcome.returncode} ({outcome.elapsed:.1f}s)")
                failing = outcome
                break
            progress(f"ok   {stage.name} ({outcome.elapsed:.1f}s)")

    elapsed = time.monotonic() - started
    report_path = Path(args.report) if args.report else root / DEFAULT_REPORT
    if not report_path.is_absolute():
        report_path = root / report_path
    payload = {
        "tier": args.tier,
        "base": base,
        "crates": list(scope.crates),
        "workspace": scope.workspace,
        "tests_removed": tests_removed,
        "tools": tools,
        "elapsed_seconds": round(elapsed, 1),
        "stages": [
            {
                "name": o.stage.name,
                "status": o.status,
                "returncode": o.returncode,
                "elapsed_seconds": round(o.elapsed, 1),
                "first_error": o.first_error,
                "failed_tests": list(o.failed_tests),
                "message": o.message,
            }
            for o in outcomes
        ],
    }
    if failing is None:
        payload["ok"] = True
        try:
            write_report(report_path, f"# Gate report: {args.tier} ok\n\nTests removed: {tests_removed}\n", payload)
        except OSError:
            pass
        ran = sum(1 for o in outcomes if o.status in ("ok", "warn"))
        print(f"gate: ok ({args.tier}, {ran} stages, {elapsed:.0f}s)")
        return 0
    payload["ok"] = False
    payload["failed_stage"] = failing.stage.name
    payload["next_command"] = narrow_command(failing)
    text = render_report(args.tier, failing, outcomes, tests_removed, tools, root)
    try:
        write_report(report_path, text, payload)
    except OSError as error:
        print(f"gate: could not write report {report_path}: {error}", file=sys.stderr)
    sys.stdout.write(text)
    sys.stdout.flush()
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
