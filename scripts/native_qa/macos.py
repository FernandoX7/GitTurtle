"""Opt-in macOS native-QA helpers (isolated HOME, PID-scoped JXA window query, window screencapture).

This module stays importable and unit-testable on Linux and macOS using only the
Python standard library. Live execution requires macOS (`darwin`), `/usr/bin/osascript`,
and `/usr/bin/screencapture`. All launches isolate `HOME` inside `--run-dir`, never
touch the operator's `~/Library/Application Support/GitTurtle`, target windows strictly
by the launched process's `unixId: pid`, and stop only the launched PID with SIGTERM.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time

from . import identity, runenv, session

BACKEND_LABEL = "macOS AppKit/Quartz (osascript + screencapture)"
# Matches `crates/prefs/src/preferences.rs` (`Library/Application Support/GitTurtle`).
MACOS_APP_CONFIG = "GitTurtle"
OSASCRIPT_BIN = "/usr/bin/osascript"
SCREENCAPTURE_BIN = "/usr/bin/screencapture"
COMMAND_TIMEOUT_SECONDS = 10.0


def _validate_positive_int(value: int, label: str) -> int:
    if type(value) is not int or value <= 0 or value > 2_147_483_647:
        raise runenv.Refusal(f"{label} must be a positive 32-bit integer, got {value!r}")
    return value


def validate_capture_name(name: str) -> str:
    """Require a plain filename stem for captures so it can never escape `captures/`."""
    if not isinstance(name, str) or not name or "\0" in name:
        raise runenv.Refusal(f"capture name must be a non-empty plain filename stem, got {name!r}")
    if "/" in name or "\\" in name or name in (".", "..") or name.startswith((".", "-")):
        raise runenv.Refusal(f"capture name {name!r} must be a plain filename stem without path separators or leading dots/dashes")
    problem = runenv.name_problem(f"{name}.png")
    if problem is not None:
        raise runenv.Refusal(f"capture name {name!r}: {problem}")
    return name


def probe_macos(
    *,
    sys_platform: str | None = None,
    which=shutil.which,
    run=subprocess.run,
) -> dict:
    """Read-only probe checking macOS native-QA tools without modifying TCC permissions."""
    plat = sys.platform if sys_platform is None else sys_platform
    issues: list[str] = []
    if plat != "darwin":
        issues.append(f"macOS native QA requires darwin (host is {plat})")
    osascript = OSASCRIPT_BIN if Path(OSASCRIPT_BIN).is_file() else which("osascript")
    screencapture = SCREENCAPTURE_BIN if Path(SCREENCAPTURE_BIN).is_file() else which("screencapture")
    if not osascript:
        issues.append("osascript not found")
    if not screencapture:
        issues.append("screencapture not found")

    system_events_ok: bool | None = None
    if not issues:
        try:
            probe = run(
                [osascript, "-l", "JavaScript", "-e", 'Application("System Events").name()'],
                capture_output=True,
                text=True,
                stdin=subprocess.DEVNULL,
                timeout=COMMAND_TIMEOUT_SECONDS,
                check=False,
            )
            system_events_ok = probe.returncode == 0
            if not system_events_ok:
                detail = (probe.stderr or probe.stdout or "").strip().splitlines()
                issues.append(
                    "System Events Accessibility probe failed"
                    + (f": {detail[0]}" if detail else "")
                )
        except (OSError, subprocess.TimeoutExpired) as error:
            system_events_ok = False
            issues.append(f"osascript probe failed: {error}")

    return {
        "backend": BACKEND_LABEL,
        "platform": plat,
        "osascript": osascript,
        "screencapture": screencapture,
        "system_events_accessible": system_events_ok,
        "ready": not issues,
        "issues": issues,
    }


def prepare_macos_run_dir(
    run_dir: Path | str,
    preferences: bytes,
    *,
    home: Path | None = None,
) -> dict[str, Path]:
    """Create an isolated macOS run directory with `home/Library/Application Support/GitTurtle/`."""
    real_home = (home or runenv.operator_home()).resolve()
    checked = runenv.check_run_dir(run_dir, home=real_home)
    resolved_run = checked.resolve()
    if runenv.within(resolved_run, (real_home / "Library").resolve()):
        raise runenv.Refusal(f"run directory {run_dir} is inside operator Library ({real_home / 'Library'})")
    checked.mkdir(parents=True, exist_ok=True)
    started = time.time()
    (checked / "RUN-STARTED").write_text(
        f"{int(started)}\n{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(started))}\n",
        encoding="utf-8",
    )
    home_dir = (checked / "home").resolve()
    app_support = home_dir / "Library" / "Application Support" / MACOS_APP_CONFIG
    captures = (checked / "captures").resolve()
    app_support.mkdir(parents=True)
    captures.mkdir()
    (home_dir / ".gitconfig").write_text(runenv.GITCONFIG, encoding="utf-8")
    prefs_path = app_support / runenv.STORE
    prefs_path.write_bytes(preferences)
    return {
        "root": checked,
        "home": home_dir,
        "app_support": app_support,
        "preferences": prefs_path,
        "captures": captures,
    }


def macos_launch_env(
    paths: dict[str, Path],
    *,
    base: dict[str, str] | None = None,
    extra: dict[str, str] | None = None,
    home: Path | None = None,
) -> dict[str, str]:
    """Construct an isolated macOS launch environment without touching the operator's HOME."""
    real_home = (home or runenv.operator_home()).resolve()
    isolated_home = paths["home"].resolve()
    root_resolved = paths["root"].resolve()
    if (
        isolated_home == real_home
        or runenv.within(real_home, isolated_home)
        or runenv.within(isolated_home, (real_home / "Library").resolve())
        or not runenv.within(isolated_home, root_resolved)
    ):
        raise runenv.Refusal(f"isolated HOME {isolated_home} is outside run dir or overlaps operator HOME/Library")
    if not paths["preferences"].is_file():
        raise runenv.Refusal(f"seeded preferences missing at {paths['preferences']}")
    env = {
        key: value
        for key, value in (os.environ if base is None else base).items()
        if key != "ZED_HEADLESS" and not key.startswith("GIT_") and not key.startswith("XDG_")
    }
    if extra:
        runenv.check_extra(extra)
        env.update(extra)
    env["HOME"] = str(isolated_home)
    return env


def jxa_window_info_script(pid: int) -> str:
    """Build a strictly PID-scoped JXA read-only script for System Events windows."""
    pid = _validate_positive_int(pid, "pid")
    return (
        'const se = Application("System Events");\n'
        f"const procs = se.applicationProcesses.whose({{unixId: {pid}}});\n"
        "if (procs.length === 0) { JSON.stringify([]); } else {\n"
        "  const wins = procs[0].windows();\n"
        "  JSON.stringify(wins.map((w) => ({\n"
        '    title: (() => { try { return String(w.name()); } catch (_) { return ""; } })(),\n'
        "    position: (() => { try { return w.position(); } catch (_) { return null; } })(),\n"
        "    size: (() => { try { return w.size(); } catch (_) { return null; } })(),\n"
        '    role: (() => { try { return String(w.role()); } catch (_) { return ""; } })(),\n'
        "  })));\n"
        "}\n"
    )


def query_windows(
    pid: int,
    *,
    osascript_bin: str = OSASCRIPT_BIN,
    run=subprocess.run,
) -> list[dict]:
    """Query System Events for windows owned strictly by `pid` (read-only)."""
    script = jxa_window_info_script(pid)
    result = run(
        [osascript_bin, "-l", "JavaScript", "-e", script],
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
        timeout=COMMAND_TIMEOUT_SECONDS,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        raise runenv.Refusal(f"osascript window query failed (exit {result.returncode}): {detail}")
    try:
        parsed = json.loads(result.stdout or "[]")
    except ValueError as error:
        raise runenv.Refusal(f"osascript returned non-JSON output: {error}") from None
    if not isinstance(parsed, list):
        raise runenv.Refusal("osascript window query returned unexpected JSON shape")
    return parsed


def capture_window_argv(window_id: int, output: Path, *, screencapture_bin: str = SCREENCAPTURE_BIN) -> list[str]:
    """Build fixed-argv silent, shadowless, strictly window-scoped `screencapture` command."""
    wid = _validate_positive_int(window_id, "window_id")
    out = runenv.absolute(output, "capture output")
    if out.suffix.lower() != ".png":
        raise runenv.Refusal(f"capture output {out} must end with .png")
    if out.exists() or out.is_symlink():
        raise runenv.Refusal(f"capture output {out} already exists")
    return [screencapture_bin, "-x", "-o", "-l", str(wid), str(out)]


def capture_window(
    window_id: int,
    output: Path,
    *,
    screencapture_bin: str = SCREENCAPTURE_BIN,
    run=subprocess.run,
) -> Path:
    """Capture a single macOS CGWindowID to a new PNG path without taking focus or playing sound."""
    argv = capture_window_argv(window_id, output, screencapture_bin=screencapture_bin)
    result = run(
        argv,
        capture_output=True,
        text=True,
        stdin=subprocess.DEVNULL,
        timeout=COMMAND_TIMEOUT_SECONDS,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "").strip()
        raise runenv.Refusal(f"screencapture failed (exit {result.returncode}): {detail}")
    out = Path(argv[-1])
    if not out.is_file():
        raise runenv.Refusal(f"screencapture succeeded without writing {out}")
    return out


class MacosSession:
    """Isolated macOS launch wrapper with SIGTERM-only teardown and flow-log recording."""

    def __init__(
        self,
        binary: Path,
        fixture: Path,
        run_dir: Path,
        preferences: bytes,
        *,
        for_commit: bool = False,
        extra_env: dict[str, str] | None = None,
        settle: float = 3.0,
    ) -> None:
        if sys.platform != "darwin":
            raise runenv.Refusal(f"macos-launch requires macOS (host is {sys.platform})")
        self.binary = runenv.absolute(binary, "binary")
        self.fixture, warnings = runenv.check_fixture(fixture, for_commit)
        checked_run_dir = runenv.check_run_dir(run_dir)
        if for_commit:
            runenv.check_commit_run_dir(checked_run_dir)
        runenv.check_extra(extra_env or {})
        described = identity.describe(self.binary)
        self.paths = prepare_macos_run_dir(checked_run_dir, preferences)
        self.env = macos_launch_env(self.paths, extra=extra_env)
        self.settle = settle
        self.proc: subprocess.Popen | None = None
        self.applog = None
        self.log = {
            "header": {
                "binary": described,
                "fixture": str(self.fixture),
                "fixture_before": session.fixture_state(self.fixture),
                "store_sha256": identity.sha256_bytes(preferences),
                "backend": BACKEND_LABEL,
                "for_commit": for_commit,
                "warnings": warnings,
                "started_utc": session.utc(),
                "env_home": str(self.paths["home"]),
                "env_extra": sorted(extra_env or ()),
            },
            "windows": [],
            "captures": [],
        }

    def launch(self) -> list[dict]:
        argv = [str(self.binary), str(self.fixture)]
        self.applog = open(self.paths["root"] / "app.log", "wb")
        self.proc = subprocess.Popen(argv, env=self.env, stdout=self.applog, stderr=subprocess.STDOUT)
        self.log["launch"] = {"argv": argv, "pid": self.proc.pid, "at_utc": session.utc()}
        time.sleep(self.settle)
        windows = query_windows(self.proc.pid)
        self.log["windows"] = windows
        return windows

    def capture(self, name: str, window_id: int) -> Path:
        stem = validate_capture_name(name)
        captures_dir = self.paths["captures"]
        if captures_dir.is_symlink():
            raise runenv.Refusal(f"captures directory {captures_dir} is a symlink")
        out = captures_dir / f"{stem}.png"
        if out.resolve().parent != captures_dir.resolve():
            raise runenv.Refusal(f"capture path {out} escaped {captures_dir}")
        written = capture_window(window_id, out)
        self.log["captures"].append(
            {
                "capture": written.name,
                "window_id": window_id,
                "at_utc": session.utc(),
                "sha256": hashlib.sha256(written.read_bytes()).hexdigest(),
            }
        )
        return written

    def close(self, timeout: float = 15.0) -> int | None:
        try:
            if self.proc is not None:
                if self.proc.poll() is None:
                    try:
                        self.proc.send_signal(signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                    try:
                        self.proc.wait(timeout=timeout)
                    except subprocess.TimeoutExpired:
                        self.log["exit"] = f"pid {self.proc.pid} still running {timeout}s after SIGTERM; not killed"
                if "exit" not in self.log:
                    self.log["exit"] = self.proc.returncode
        finally:
            if self.applog is not None:
                try:
                    self.applog.close()
                finally:
                    self.applog = None
        header = self.log["header"]
        header["ended_utc"] = session.utc()
        header["fixture_after"] = session.fixture_state(self.fixture)
        self.log["fixture_unchanged"] = header["fixture_before"] == header["fixture_after"]
        (self.paths["root"] / "flow-log.json").write_text(
            json.dumps(self.log, indent=1, default=str),
            encoding="utf-8",
        )
        return self.log.get("exit") if isinstance(self.log.get("exit"), int) else None
