"""Display ownership check: the time, running GitTurtle and QA-driver processes, GitTurtle windows and the session lock.

The coordinator verifies a handover with this output rather than on an
agent's word. Patterns are anchored on the command's first word, so a shell
or editor whose arguments merely mention GitTurtle does not match, and this
process and its ancestors are excluded.

The lock is read from two read-only sources, GNOME ScreenSaver's `GetActive`
on the session bus and logind's `LockedHint` for the user's graphical
session, because either alone misses a case. Nothing here locks, unlocks,
wakes or otherwise changes the session.
"""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

# A GitTurtle executable: argv[0]'s basename starts with gitturtle (gitturtle, gitturtle-base-ff06709, ...).
BINARY_PATTERN = r"^([^ ]*/)?[Gg]it[Tt]urtle[^/ ]*( |$)"
# A Python QA driver: this package's launchers (launch, scenario run, recheck), or the earlier bundle drivers by name.
DRIVER_PATTERN = (r"^([^ ]*/)?python3?[.0-9]*( -[^ ]+)* [^ ]*(native_qa/qa\.py (launch|run|scenario run|recheck)"
                  r"|(drive_|d3_|capture_|design_probe|portal_probe)[^ /]*\.py)( |$)")


def ancestors(pid: int | None = None) -> set[int]:
    """This process and its parents, read from /proc where available."""
    pid = os.getpid() if pid is None else pid
    found = set()
    while pid > 1 and pid not in found:
        found.add(pid)
        try:
            stat = Path(f"/proc/{pid}/stat").read_text()
            pid = int(stat.rsplit(") ", 1)[1].split()[1])
        except (OSError, IndexError, ValueError):
            if pid == os.getpid():
                pid = os.getppid()
                continue
            break
    return found


def parse_pgrep(text: str, exclude: set[int] = frozenset()) -> list[tuple[int, str]]:
    """`pgrep -af` lines as (pid, command line), without the excluded PIDs."""
    found = []
    for line in text.splitlines():
        pid, _, command = line.strip().partition(" ")
        if pid.isdigit() and int(pid) not in exclude:
            found.append((int(pid), command))
    return found


def pgrep(pattern: str, exclude: set[int]) -> list[tuple[int, str]]:
    result = subprocess.run(["pgrep", "-af", pattern], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise RuntimeError(f"pgrep exited {result.returncode}: {result.stderr.strip()}")
    return parse_pgrep(result.stdout, exclude)


# Every lock read is bounded: gdbus's own D-Bus timeout, and a process timeout above it.
LOCK_TIMEOUT_SECONDS = 2
PROCESS_TIMEOUT_SECONDS = 4
SCREENSAVER = "GNOME ScreenSaver"
LOGIND = "logind"
LOCKED_PREFIX = "LOCKED:"
SESSION_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")


class Unreadable(Exception):
    """A lock source that could not be read or parsed."""


def screensaver_argv() -> list[str]:
    return ["gdbus", "call", "--session", "--dest", "org.gnome.ScreenSaver",
            "--object-path", "/org/gnome/ScreenSaver", "--method", "org.gnome.ScreenSaver.GetActive",
            "--timeout", str(LOCK_TIMEOUT_SECONDS)]


def logind_user_argv(uid: int) -> list[str]:
    return ["loginctl", "show-user", str(uid), "-p", "Display"]


def logind_session_argv(session: str) -> list[str]:
    return ["loginctl", "show-session", session, "-p", "Type", "-p", "LockedHint"]


def session_env() -> dict[str, str]:
    """This environment, with the user's session bus socket when the caller has none (an agent or SSH shell)."""
    env = dict(os.environ)
    if not env.get("DBUS_SESSION_BUS_ADDRESS"):
        bus = Path(f"/run/user/{os.getuid()}/bus")
        if bus.exists():
            env["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={bus}"
    return env


def run_read(argv: list[str], env: dict[str, str] | None = None) -> str:
    """Stdout of one read-only command; Unreadable when it is missing, fails or times out."""
    try:
        result = subprocess.run(argv, capture_output=True, text=True, env=env, stdin=subprocess.DEVNULL,
                                timeout=PROCESS_TIMEOUT_SECONDS)
    except subprocess.TimeoutExpired:
        raise Unreadable(f"{argv[0]} timed out after {PROCESS_TIMEOUT_SECONDS}s") from None
    except OSError as error:
        raise Unreadable(f"{type(error).__name__}: {error}") from None
    if result.returncode != 0:
        detail = (result.stderr.strip() or result.stdout.strip()).splitlines()
        raise Unreadable(f"{argv[0]} exited {result.returncode}" + (f": {detail[0]}" if detail else ""))
    return result.stdout


def properties(text: str) -> dict[str, str]:
    """`loginctl show-*` Key=Value lines."""
    found = {}
    for line in text.splitlines():
        key, sep, value = line.strip().partition("=")
        if sep:
            found[key] = value
    return found


def screensaver_active() -> bool:
    out = run_read(screensaver_argv(), session_env()).strip()
    if out == "(true,)":
        return True
    if out == "(false,)":
        return False
    raise Unreadable(f"unparseable GetActive reply {out[:80]!r}")


def logind_locked() -> tuple[bool, str]:
    """LockedHint of the user's graphical (Display) session, and that session's id and type."""
    session = properties(run_read(logind_user_argv(os.getuid()))).get("Display", "")
    if not session:
        raise Unreadable("no graphical session for this user")
    if not SESSION_ID.match(session):
        raise Unreadable(f"unparseable session id {session[:40]!r}")
    values = properties(run_read(logind_session_argv(session)))
    hint = values.get("LockedHint")
    if hint not in ("yes", "no"):
        raise Unreadable(f"unparseable LockedHint {hint!r}")
    return hint == "yes", f"session {session} ({values.get('Type') or 'unknown type'})"


def lock_check() -> tuple[int, list[str]]:
    """0 when a readable source reports unlocked and none reports a lock, 1 on a lock, 2 when neither is readable."""
    lines = []
    readable = 0
    locked = False
    lines.append(f"$ {' '.join(screensaver_argv())}  # {SCREENSAVER}")
    try:
        active = screensaver_active()
    except Unreadable as error:
        lines.append(f"{SCREENSAVER}: unreadable ({error})")
    else:
        readable += 1
        locked |= active
        lines.append(f"{LOCKED_PREFIX} {SCREENSAVER} GetActive returned true" if active
                     else f"{SCREENSAVER}: GetActive false (unlocked)")
    lines.append(f"$ {' '.join(logind_user_argv(os.getuid()))}; loginctl show-session <Display> -p Type -p LockedHint"
                 f"  # {LOGIND}")
    try:
        hint, session = logind_locked()
    except Unreadable as error:
        lines.append(f"{LOGIND}: unreadable ({error})")
    else:
        readable += 1
        locked |= hint
        lines.append(f"{LOCKED_PREFIX} {LOGIND} LockedHint=yes for {session}" if hint
                     else f"{LOGIND}: LockedHint=no for {session} (unlocked)")
    if locked:
        return 1, lines
    if not readable:
        lines.append(f"session lock: INCONCLUSIVE (neither {SCREENSAVER} nor {LOGIND} could be read)")
        return 2, lines
    return 0, lines


def windows(name: str) -> list[dict]:
    from . import x11

    dsp = x11.connect(name)
    try:
        return [{key: value for key, value in entry.items() if key != "window"}
                for entry in x11.gitturtle_windows(dsp)]
    finally:
        dsp.close()


def check(display: str, allow: set[int] = frozenset()) -> tuple[int, list[str]]:
    """0 when nothing foreign runs and the session is unlocked, 1 when something foreign runs or a source
    reports a lock, 2 when the processes, the display or both lock sources could not be read.

    The last line is the verdict `qa.py display-check` prints.
    """
    status, lines = processes_and_windows(display, allow)
    foreign = status == 1
    lock_status, lock_lines = lock_check()
    lines += lock_lines
    if lock_status == 1:
        status = 1  # a lock fails the check even when something else was unreadable
    elif lock_status == 2:
        status = status or 2  # as an unreadable display: inconclusive unless already failing
    if status == 1:
        lines.append("; ".join((["FOREIGN processes or windows present"] if foreign else [])
                               + (["session LOCKED"] if lock_status == 1 else [])))
    else:
        lines.append({0: "display clear", 2: "INCONCLUSIVE"}[status])
    return status, lines


def processes_and_windows(display: str, allow: set[int]) -> tuple[int, list[str]]:
    """The time, foreign processes and GitTurtle windows: 0 none, 1 some, 2 unreadable."""
    lines = []
    date = subprocess.run(["date", "-u"], capture_output=True, text=True).stdout.strip()
    lines.append(f"$ date -u\n{date}")
    exclude = ancestors()
    foreign = False
    for label, pattern in (("GitTurtle executables", BINARY_PATTERN), ("QA drivers", DRIVER_PATTERN)):
        lines.append(f"$ pgrep -af '{pattern}'  # {label}")
        try:
            found = pgrep(pattern, exclude)
        except (OSError, RuntimeError) as error:
            lines.append(f"unreadable ({type(error).__name__}: {error})")
            return 2, lines
        lines += [f"{pid} {command}" + ("  (allowed)" if pid in allow else "") for pid, command in found]
        if not found:
            lines.append("(nothing)")
        foreign |= any(pid not in allow for pid, _ in found)
    status = 1 if foreign else 0
    try:
        entries = windows(display)
    except Exception as error:  # no python-xlib, no display, or no access to it
        lines.append(f"windows on {display}: unreadable ({type(error).__name__}: {error})")
        return (status or 2), lines
    lines.append(f"GitTurtle windows on {display} (_NET_WM_PID):")
    lines += [f"window {entry['id']:#x} pid {entry['pid']} {entry['width']}x{entry['height']} {entry['wm_class']}"
              + ("  (allowed)" if entry["pid"] in allow else "") for entry in entries]
    if not entries:
        lines.append("(none)")
    if any(entry["pid"] not in allow for entry in entries):
        status = 1
    return status, lines
