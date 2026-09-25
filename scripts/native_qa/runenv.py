"""One isolated launch environment: run directory, HOME, XDG directories and Git identity.

GitTurtle ignores a relative XDG_CONFIG_HOME (`absolute_environment_path` in
crates/app/src/preferences.rs) and falls back to the operator's real
~/.config/gitturtle; a QA launch with a relative path rewrote the owner's
preferences on 2026-09-18. Every path here is therefore absolute, fresh for
each launch and refused when it is the operator's own directory.
"""

from __future__ import annotations

import os
import pwd
import time
from dataclasses import dataclass
from pathlib import Path

QA_NAME = "GitTurtle QA"
QA_EMAIL = "qa@example.invalid"
GITCONFIG = f"[user]\n\tname = {QA_NAME}\n\temail = {QA_EMAIL}\n"
EVIDENCE_ROOT = Path("/tmp/gitturtle-evidence")
HOME_ROOTS = (Path("/home"), Path("/Users"))
# Subdirectories of the run directory; each becomes one variable of the launch.
LAYOUT = {
    "HOME": "home",
    "XDG_CONFIG_HOME": "config",
    "XDG_DATA_HOME": "data",
    "XDG_CACHE_HOME": "cache",
    "XDG_STATE_HOME": "state",
}
DEFAULT_DISPLAY = ":1"
DEFAULT_SCALE = "1"


class Refusal(SystemExit):
    """A launch precondition failed; the message names the path and the reason."""

    def __init__(self, message: str) -> None:
        super().__init__(f"refusing: {message}")


@dataclass(frozen=True)
class RunDirs:
    root: Path
    captures: Path
    paths: dict[str, Path]

    @property
    def preferences(self) -> Path:
        return self.paths["XDG_CONFIG_HOME"] / "gitturtle" / "preferences.json"


def operator_home() -> Path:
    """The account's real home directory, independent of an inherited HOME override."""
    return Path(pwd.getpwuid(os.getuid()).pw_dir).resolve()


def operator_directories(home: Path | None = None) -> dict[str, Path]:
    home = home or operator_home()
    return {
        "HOME": home,
        "XDG_CONFIG_HOME": home / ".config",
        "XDG_DATA_HOME": home / ".local" / "share",
        "XDG_CACHE_HOME": home / ".cache",
        "XDG_STATE_HOME": home / ".local" / "state",
    }


def absolute(path: Path | str, what: str) -> Path:
    """Refuse a relative path before anything resolves it against the working directory."""
    if not Path(path).is_absolute():
        raise Refusal(f"{what} {path} must be an absolute path")
    return Path(os.path.normpath(path))


def within(path: Path, parent: Path) -> bool:
    return path == parent or parent in path.parents


def check_run_dir(run_dir: Path | str, home: Path | None = None) -> Path:
    run_dir = absolute(run_dir, "run directory")
    resolved = run_dir.resolve()
    for name, real in operator_directories(home).items():
        if name == "HOME":
            # A run directory may live under HOME, but never be HOME itself.
            if resolved == real:
                raise Refusal(f"run directory {run_dir} is the operator's HOME")
        elif within(resolved, real.resolve()):
            raise Refusal(f"run directory {run_dir} is inside the operator's {name} ({real})")
    if run_dir.exists():
        if not run_dir.is_dir() or run_dir.is_symlink():
            raise Refusal(f"run directory {run_dir} is not a plain directory")
        if any(run_dir.iterdir()):
            raise Refusal(f"run directory {run_dir} is not empty; preserve its contents elsewhere first")
    return run_dir


def check_fixture(fixture: Path | str, for_commit: bool) -> tuple[Path, list[str]]:
    """Refuse (for commit) or warn about a fixture whose path could put private text in a frame."""
    fixture = absolute(fixture, "fixture")
    warnings = []
    if not within(fixture.resolve(), EVIDENCE_ROOT.resolve()):  # /tmp is /private/tmp on macOS
        message = f"fixture {fixture} is outside {EVIDENCE_ROOT}/, so its path may show private details"
        if for_commit:
            raise Refusal(message)
        warnings.append(message)
    if not (fixture / ".git").exists():
        warnings.append(f"fixture {fixture} has no .git entry")
    return fixture, warnings


def check_commit_run_dir(run_dir: Path) -> None:
    """Settings can draw HOME-derived paths, so a committed frame's run directory avoids home roots."""
    # Compare literal and resolved forms: macOS resolves /home to /System/Volumes/Data/home.
    paths = {run_dir.absolute(), run_dir.resolve()}
    for root in (*HOME_ROOTS, operator_home()):
        if any(within(path, candidate) for path in paths for candidate in {root, root.resolve()}):
            raise Refusal(f"run directory {run_dir} is under {root}; use {EVIDENCE_ROOT}/runs/<name> for commit captures")


def prepare(run_dir: Path | str, preferences: bytes, home: Path | None = None) -> RunDirs:
    """Create the empty run directory, its HOME/XDG tree, the Git identity and the seeded store."""
    run_dir = check_run_dir(run_dir, home)
    run_dir.mkdir(parents=True, exist_ok=True)
    started = time.time()
    (run_dir / "RUN-STARTED").write_text(
        f"{int(started)}\n{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(started))}\n")
    paths = {}
    for name, sub in LAYOUT.items():
        path = (run_dir / sub).resolve()
        path.mkdir()
        paths[name] = path
    (paths["HOME"] / ".gitconfig").write_text(GITCONFIG)
    store = paths["XDG_CONFIG_HOME"] / "gitturtle" / "preferences.json"
    store.parent.mkdir()
    store.write_bytes(preferences)
    captures = run_dir / "captures"
    captures.mkdir()
    return RunDirs(run_dir, captures, paths)


def launch_env(dirs: RunDirs, display: str = DEFAULT_DISPLAY, scale: str = DEFAULT_SCALE,
               base: dict[str, str] | None = None, extra: dict[str, str] | None = None,
               home: Path | None = None) -> dict[str, str]:
    """The inherited session environment with Wayland removed and every store redirected.

    DBUS_SESSION_BUS_ADDRESS and XDG_RUNTIME_DIR stay, because the file-chooser
    portal needs them; GIT_* variables are dropped so an inherited GIT_DIR or
    GIT_CONFIG_* cannot redirect the fixture or the identity.
    """
    env = {key: value for key, value in (os.environ if base is None else base).items()
           if key != "WAYLAND_DISPLAY" and not key.startswith("GIT_")}
    env.update({name: str(path) for name, path in dirs.paths.items()})
    env.update(DISPLAY=display, GPUI_X11_SCALE_FACTOR=str(scale))
    env.update(check_extra(extra or {}))
    verify(env, dirs, home)
    return env


def check_extra(extra: dict[str, str]) -> dict[str, str]:
    for key in extra:
        if key in LAYOUT or key == "WAYLAND_DISPLAY" or key.startswith("GIT_"):
            raise Refusal(f"--env cannot set {key}")
    return extra


def verify(env: dict[str, str], dirs: RunDirs, home: Path | None = None) -> None:
    """The last check before exec: the app will read exactly the store that was seeded."""
    if "WAYLAND_DISPLAY" in env:
        raise Refusal("WAYLAND_DISPLAY is set; the app would not use XWayland")
    real = operator_directories(home)
    for name in LAYOUT:
        value = env.get(name, "")
        if not Path(value).is_absolute():
            raise Refusal(f"{name}={value!r} is not absolute")
        if Path(value).resolve() == real[name].resolve():
            raise Refusal(f"{name} is the operator's own {real[name]}")
        if Path(value) != dirs.paths[name]:
            raise Refusal(f"{name} is not this run's {dirs.paths[name]}")
    if not dirs.preferences.is_file():
        raise Refusal(f"seeded store missing at {dirs.preferences}")
