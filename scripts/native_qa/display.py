"""Display ownership check: the time, running GitTurtle and QA-driver processes, and GitTurtle windows.

The coordinator verifies a handover with this output rather than on an
agent's word. Patterns are anchored on the command's first word, so a shell
or editor whose arguments merely mention GitTurtle does not match, and this
process and its ancestors are excluded.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

# A GitTurtle executable: argv[0]'s basename starts with gitturtle (gitturtle, gitturtle-base-ff06709, ...).
BINARY_PATTERN = r"^([^ ]*/)?[Gg]it[Tt]urtle[^/ ]*( |$)"
# A Python QA driver: this package's launcher, or the earlier bundle drivers by name.
DRIVER_PATTERN = (r"^([^ ]*/)?python3?[.0-9]*( -[^ ]+)* [^ ]*(native_qa/qa\.py (launch|run)"
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


def windows(name: str) -> list[dict]:
    from . import x11

    dsp = x11.connect(name)
    try:
        return [{key: value for key, value in entry.items() if key != "window"}
                for entry in x11.gitturtle_windows(dsp)]
    finally:
        dsp.close()


def check(display: str, allow: set[int] = frozenset()) -> tuple[int, list[str]]:
    """0 when nothing foreign runs, 1 when something does, 2 when the display could not be read."""
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
