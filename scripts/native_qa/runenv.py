"""One isolated launch environment: run directory, HOME, XDG directories and Git identity.

GitTurtle ignores a relative XDG_CONFIG_HOME (`absolute_environment_path` in
crates/app/src/preferences.rs) and falls back to the operator's real
~/.config/gitturtle; a QA launch with a relative path rewrote the owner's
preferences on 2026-09-18. Every path here is therefore absolute, fresh for
each launch and refused when it is the operator's own directory.
"""

from __future__ import annotations

import hashlib
import os
import pwd
import stat
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
# The run directory's XDG homes, where a `read_only` step may remove write bits; home/ and captures/ stay writable.
READ_ONLY_ROOTS = tuple(sub for name, sub in LAYOUT.items() if name != "HOME")
# Files a scenario seeds into each launch's HOME (`home`): at most this many, each at most this many bytes.
HOME_FILES = 64
HOME_FILE_BYTES = 1_000_000
HOME_OWN = (".gitconfig",)  # written by `prepare` itself: the run's Git identity
# The app's configuration directory under the run's XDG_CONFIG_HOME (`settings_path` in crates/app/src/preferences.rs),
# where it keeps the preference store and the stores beside it, such as recovery-drafts.json.
APP_CONFIG = "gitturtle"
STORE = "preferences.json"  # written by `prepare` itself, generated or supplied: never an `app_config` file
# Files a scenario seeds beside the store (`app_config`): plain names, with the caps of HOME files.
APP_CONFIG_FILES = HOME_FILES
APP_CONFIG_FILE_BYTES = HOME_FILE_BYTES
NAME_MAX = 255  # bytes Linux allows one name in a path (a component), however many characters encode to them


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
    def app_config(self) -> Path:
        """The app's configuration directory, which holds the store and the files `app_config` seeds."""
        return self.paths["XDG_CONFIG_HOME"] / APP_CONFIG

    @property
    def preferences(self) -> Path:
        return self.app_config / STORE


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


def prepare(run_dir: Path | str, preferences: bytes, home: Path | None = None,
            home_files: dict[str, bytes] | None = None,
            app_config_files: dict[str, bytes] | None = None) -> RunDirs:
    """Create the empty run directory, its HOME/XDG tree, the Git identity, the seeded HOME files, the store and
    the files seeded beside it."""
    check_home_files(home_files or {})
    check_app_config_files(app_config_files or {})
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
    seed_home(paths["HOME"], home_files or {})
    dirs = RunDirs(run_dir, run_dir / "captures", paths)
    dirs.app_config.mkdir()
    dirs.preferences.write_bytes(preferences)
    seed_app_config(dirs.app_config, app_config_files or {})  # after the store, which O_EXCL then keeps
    dirs.captures.mkdir()
    return dirs


# ---------- names of seeded files ----------
def name_problem(part: str) -> str | None:
    """Why the file system cannot take `part` as one name in a path, or None: it must encode as strict UTF-8 (a lone
    surrogate from a JSON escape names no file, and `os.fsencode` would fail on most or write a stray byte for the
    rest), and its encoding must fit the NAME_MAX bytes Linux allows a name, which characters do not measure."""
    try:
        part.encode("utf-8")
        encoded = os.fsencode(part)
    except UnicodeEncodeError:
        return f"{part!r} cannot be encoded as a file name; give the name in valid UTF-8"
    if len(encoded) > NAME_MAX:
        return f"{part!r} is {len(encoded)} bytes as a file name; at most {NAME_MAX}"
    return None


# ---------- files seeded into HOME ----------
def home_file_problem(value) -> str | None:
    """Why `value` cannot name a file seeded under a launch's HOME, or None: a relative POSIX path that stays
    under HOME and is not the run's own Git identity, each of whose names the file system can take."""
    if not isinstance(value, str) or not value or len(value) > 300 or "\0" in value:
        return f"expected a non-empty path of at most 300 characters, got {value!r}"
    parts = value.split("/")
    if value.startswith("/") or "\\" in value or any(part in ("", ".", "..") for part in parts):
        return f"{value!r} must be a relative POSIX path under HOME without empty, . or .. parts"
    problem = next(filter(None, map(name_problem, parts)), None)
    if problem is not None:
        return f"{value!r}: {problem}"
    if parts[0] in HOME_OWN:
        return f"{value!r} is the run's own Git identity, which every launch writes itself"
    return None


def home_overlap(paths) -> str | None:
    """Why these files cannot all be written, one being another's directory, or None."""
    ordered = sorted(paths)
    for path in ordered:
        inside = next((other for other in ordered if other.startswith(path + "/")), None)
        if inside is not None:
            return f"{path!r} is a file, but {inside!r} needs it as a directory"
    return None


def home_files_problem(files: dict[str, bytes]) -> str | None:
    """Why `seed_home` would not write this whole set of HOME files, or None: too many, a bad path, a file too
    large, or one file another needs as a directory."""
    if len(files) > HOME_FILES:
        return f"{len(files)} files; at most {HOME_FILES}"
    for relative, data in files.items():
        problem = home_file_problem(relative)
        if problem is not None:
            return problem
        if not isinstance(data, bytes) or len(data) > HOME_FILE_BYTES:
            return f"{relative}: expected at most {HOME_FILE_BYTES} bytes"
    return home_overlap(files)


def check_home_files(files: dict[str, bytes]) -> dict[str, bytes]:
    """Refuse, before anything is created, HOME files `seed_home` would not write."""
    problem = home_files_problem(files)
    if problem is not None:
        raise Refusal(f"HOME files: {problem}")
    return files


def seed_home(home: Path, files: dict[str, bytes]) -> dict[str, str]:
    """Write each file under `home` without following a link; each one's sha256 by path (`seed_files`)."""
    check_home_files(files)
    return seed_files(home, files, "HOME file")


def seed_files(root: Path, files: dict[str, bytes], what: str) -> dict[str, str]:
    """Write each file under `root` without following a link; each one's sha256 by path.

    Every directory on the way is opened relative to its parent with
    O_NOFOLLOW and every file is created with O_EXCL, so a symbolic link or
    an existing file is refused rather than followed or replaced, and nothing
    lands outside `root`. The caller has checked the paths and sizes.
    """
    digests = {}
    for relative in sorted(files):
        *directories, name = relative.split("/")
        fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            for depth, part in enumerate(directories):
                try:
                    os.mkdir(part, 0o755, dir_fd=fd)
                except FileExistsError:
                    pass
                try:
                    child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                except OSError as error:
                    where = "/".join(directories[:depth + 1])
                    raise Refusal(f"{what} {relative}: {where} is not a plain directory ({error})") from None
                os.close(fd)
                fd = child
            try:
                out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644, dir_fd=fd)
            except OSError as error:
                raise Refusal(f"{what} {relative}: cannot create it in {root} ({error})") from None
            with os.fdopen(out, "wb") as handle:
                handle.write(files[relative])
        finally:
            os.close(fd)
        digests[relative] = hashlib.sha256(files[relative]).hexdigest()
    return digests


# ---------- files seeded beside the store ----------
def app_config_file_problem(value) -> str | None:
    """Why `value` cannot name a file seeded in a launch's app configuration directory, or None: a plain file name
    there that the file system can take (`name_problem`), other than the store, which every launch writes itself."""
    if not isinstance(value, str) or not value or "\0" in value:
        return f"expected a non-empty file name, got {value!r}"
    if "/" in value or "\\" in value or value in (".", ".."):
        return f"{value!r} must be a plain file name in the app's configuration directory, without / or \\"
    problem = name_problem(value)
    if problem is not None:
        return problem
    if value == STORE:
        return f"{value!r} is the preference store, which every launch writes itself from the variants and settings"
    return None


def app_config_files_problem(files: dict[str, bytes]) -> str | None:
    """Why `seed_app_config` would not write this whole set of files, or None: too many, a bad name or a file too
    large."""
    if len(files) > APP_CONFIG_FILES:
        return f"{len(files)} files; at most {APP_CONFIG_FILES}"
    for name, data in files.items():
        problem = app_config_file_problem(name)
        if problem is not None:
            return problem
        if not isinstance(data, bytes) or len(data) > APP_CONFIG_FILE_BYTES:
            return f"{name}: expected at most {APP_CONFIG_FILE_BYTES} bytes"
    return None


def check_app_config_files(files: dict[str, bytes]) -> dict[str, bytes]:
    """Refuse, before anything is created, app configuration files `seed_app_config` would not write."""
    problem = app_config_files_problem(files)
    if problem is not None:
        raise Refusal(f"app configuration files: {problem}")
    return files


def seed_app_config(directory: Path, files: dict[str, bytes]) -> dict[str, str]:
    """Write each file into the app's configuration `directory`, never through a link or over an existing file (the
    store included); each one's sha256 by name."""
    check_app_config_files(files)
    return seed_files(directory, files, "app configuration file")


def read_only_problem(value) -> str | None:
    """Why `value` cannot name a `read_only` step's path, or None: a relative POSIX path in one of the XDG homes."""
    if not isinstance(value, str) or not value or len(value) > 300 or "\0" in value:
        return f"expected a non-empty path of at most 300 characters, got {value!r}"
    parts = value.split("/")
    if value.startswith("/") or "\\" in value or any(part in ("", ".", "..") for part in parts):
        return f"{value!r} must be a relative POSIX path without empty, . or .. parts"
    if parts[0] not in READ_ONLY_ROOTS:
        return f"{value!r} must start with one of {', '.join(READ_ONLY_ROOTS)}, the run directory's XDG homes"
    return None


@dataclass(frozen=True)
class Locked:
    """A path a `read_only` step changed, held open so the restore changes that same file or directory even if
    something else has since been put at its path."""
    path: str
    fd: int
    old_mode: int
    new_mode: int


def lock_read_only(root: Path, relative: str) -> Locked:
    """Remove every write bit of `root/relative` (a directory keeps at most 0o500) without following a link.

    Refuses, with nothing changed, a symbolic link in any component, a missing
    component, a final entry that is neither a directory nor a regular file,
    and a target that resolves outside `root`. Any exception after the mode
    changed puts the old mode back before it propagates.
    """
    problem = read_only_problem(relative)
    if problem is not None:
        raise Refusal(f"read_only: {problem}")
    parts = relative.split("/")
    fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
    old = None
    try:
        for depth, part in enumerate(parts):
            where = "/".join(parts[:depth + 1])
            try:
                info = os.stat(part, dir_fd=fd, follow_symlinks=False)
            except FileNotFoundError:
                raise Refusal(f"read_only {relative}: {where} does not exist in {root}") from None
            if stat.S_ISLNK(info.st_mode):
                raise Refusal(f"read_only {relative}: {where} is a symbolic link; nothing changed")
            last = depth == len(parts) - 1
            directory = stat.S_ISDIR(info.st_mode)
            if not directory and not (last and stat.S_ISREG(info.st_mode)):
                raise Refusal(f"read_only {relative}: {where} is not a directory{' or regular file' if last else ''}")
            try:
                child = os.open(part, os.O_RDONLY | os.O_NOFOLLOW | (os.O_DIRECTORY if directory else 0), dir_fd=fd)
            except OSError as error:
                raise Refusal(f"read_only {relative}: cannot open {where} without following a link ({error})") from None
            os.close(fd)
            fd = child
        resolved = Path(os.path.realpath(Path(root) / relative))
        if not within(resolved, Path(root).resolve()):
            raise Refusal(f"read_only {relative}: resolves to {resolved}, outside the run directory {root}")
        info = os.fstat(fd)
        old = stat.S_IMODE(info.st_mode)
        new = old & 0o500 if stat.S_ISDIR(info.st_mode) else old & ~0o222
        os.fchmod(fd, new)
        return Locked(relative, fd, old, new)
    except BaseException:
        try:
            if old is not None and stat.S_IMODE(os.fstat(fd).st_mode) != old:
                os.fchmod(fd, old)  # the mode changed before the failure: put it back before reporting it
        finally:
            os.close(fd)
        raise


def restore_mode(locked: Locked) -> None:
    """Put back the mode `lock_read_only` replaced, on the object it changed; the descriptor is closed either way."""
    try:
        os.fchmod(locked.fd, locked.old_mode)
    finally:
        os.close(locked.fd)


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


# Variables that decide where the app reads and draws, which the run records itself (display, scale) or isolates.
RESERVED = ("DISPLAY", "GPUI_X11_SCALE_FACTOR", "DBUS_SESSION_BUS_ADDRESS")
RESERVED_PREFIXES = ("GIT_", "WAYLAND_", "XDG_")


def reserved(key: str) -> bool:
    return key in LAYOUT or key in RESERVED or key.startswith(RESERVED_PREFIXES)


def check_extra(extra: dict[str, str]) -> dict[str, str]:
    for key in extra:
        if reserved(key):
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
