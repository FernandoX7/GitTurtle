"""The XDG file-chooser portal on GNOME: read its dialog, type into it, record its D-Bus traffic.

The Import/Export dialogs are drawn by xdg-desktop-portal-gnome (or, on
GNOME 50, by Nautilus as `org.gnome.Nautilus`) as Wayland clients of the
session compositor, not X11 clients of the QA display. XTest
never reaches them (a whole file name typed into the Save dialog left its
Name entry unchanged) and org.gnome.Shell.Screenshot refuses this caller. So
the dialog's evidence is its AT-SPI tree, and input goes through
org.gnome.Mutter.RemoteDesktop; every keystroke sent is read back from the
dialog's entry before the next, so input that did not land is visible.
Mutter delivers those keys to whatever surface has compositor focus, so a key
is sent only while a widget inside the matched dialog holds focus and X focus is
off the app.

Start from an empty run directory: a stale export in the run's HOME makes
GTK raise its overwrite prompt, Return never completes the dialog, no portal
`Response` arrives and the app rightly keeps the transfer pending.
"""

from __future__ import annotations

import subprocess
import time
from pathlib import Path

PORTALS = ("xdg-desktop-portal-gnome", "xdg-desktop-portal-gtk")
STATES = ("focused", "focusable", "showing", "visible", "modal", "editable", "selected", "enabled")
# Nautilus hosts the FileChooser on GNOME 50; only its top-levels titled exactly like the app's requests
# are dialogs, so its browser windows are never read.
NAUTILUS = "org.gnome.Nautilus"
TITLES = ("Open File", "Save File")
# Nautilus nests the dialog's entries and focus far deeper than the portal backend does.
DIALOG_DEPTH = 40
# A session bus with no service directory: org.freedesktop.portal.Desktop is absent, so the app's
# picker call fails the way it does on a desktop without a FileChooser backend.
PRIVATE_BUS_CONFIG = """<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"""


# ---------- AT-SPI ----------
def _atspi():
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi

    return Atspi


def init() -> None:
    atspi = _atspi()
    atspi.set_timeout(800, 3000)
    atspi.init()


def dialog_windows() -> list[tuple[str, object]]:
    """Every top-level of the portal backends, and Nautilus's FileChooser by exact title, with its owner.

    Other applications are skipped by name, and no other Nautilus window is returned.
    """
    desktop = _atspi().get_desktop(0)
    found = []
    for i in range(desktop.get_child_count()):
        try:
            app = desktop.get_child_at_index(i)
            owner = app.get_name() if app else None
            if owner not in PORTALS and owner != NAUTILUS:
                continue
            for j in range(app.get_child_count()):
                window = app.get_child_at_index(j)
                if window is not None and (owner in PORTALS or window.get_name() in TITLES):
                    found.append((owner, window))
        except Exception:
            continue
    return found


def wait_for_dialog(timeout: float = 20.0, poll: float = 0.4):
    """(backend, node) of the first portal dialog to appear and the seconds it took, or (None, None)."""
    start = time.time()
    while time.time() - start < timeout:
        found = dialog_windows()
        if found:
            return found[0], round(time.time() - start, 2)
        time.sleep(poll)
    return None, None


def wait_until_gone(timeout: float = 20.0, poll: float = 0.4) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if not dialog_windows():
            return True
        time.sleep(poll)
    return False


def states_of(node, names=STATES) -> list[str]:
    """The `names` (AT-SPI StateType names in lower case) that `node` reports, in that order."""
    atspi = _atspi()
    try:
        state_set = node.get_state_set()
    except Exception:
        return []
    return [name for name in names
            if hasattr(atspi.StateType, name.upper()) and state_set.contains(getattr(atspi.StateType, name.upper()))]


def text_of(node) -> str | None:
    atspi = _atspi()
    try:
        count = atspi.Text.get_character_count(node)
        return None if count is None else atspi.Text.get_text(node, 0, count)
    except Exception:
        return None


def dump(node, depth: int = 0, limit: int = 7) -> dict:
    """The accessible tree as plain data: role, name, states, text and screen extents."""
    atspi = _atspi()
    try:
        entry = {"role": node.get_role_name(), "name": node.get_name(), "states": states_of(node)}
    except Exception:
        return {"error": "unreadable"}
    value = text_of(node)
    if value is not None:
        entry["text"] = value
    try:
        extents = node.get_extents(atspi.CoordType.SCREEN)
        entry["extents"] = [extents.x, extents.y, extents.width, extents.height]
    except Exception:
        pass
    if depth < limit:
        try:
            children = [dump(node.get_child_at_index(i), depth + 1, limit) for i in range(node.get_child_count())]
        except Exception:
            children = []
        if children:
            entry["children"] = children
    return entry


def find_nodes(node, role: str | None = None, name: str | None = None, depth: int = 0, limit: int = 8,
               out: list | None = None) -> list:
    """Live accessibles (not the dump) matching a role and/or name."""
    out = [] if out is None else out
    try:
        if (role is None or node.get_role_name() == role) and (name is None or node.get_name() == name):
            out.append(node)
        if depth < limit:
            for i in range(node.get_child_count()):
                child = node.get_child_at_index(i)
                if child is not None:
                    find_nodes(child, role, name, depth + 1, limit, out)
    except Exception:
        pass
    return out


def entry_texts(window) -> list[dict]:
    return [{"role": node.get_role_name(), "name": node.get_name(), "text": text_of(node), "states": states_of(node)}
            for node in find_nodes(window, role="text", limit=DIALOG_DEPTH)
            + find_nodes(window, role="entry", limit=DIALOG_DEPTH)]


def focused_text(window) -> str | None:
    entries = entry_texts(window)
    for entry in entries:
        if "focused" in entry["states"]:
            return entry["text"]
    return entries[0]["text"] if entries else None


def dialog_has_focus(window) -> bool:
    """A widget inside the dialog holds focus. GTK4 never reports ACTIVE on the frame, so FOCUSED is the signal."""
    return any("focused" in states_of(node) for node in find_nodes(window, limit=DIALOG_DEPTH))


# ---------- compositor keyboard ----------
class DialogFocusRefused(SystemExit):
    def __init__(self, message: str) -> None:
        super().__init__(f"refusing: {message}")


class Keyboard:
    """Keysyms for a portal dialog through a Mutter RemoteDesktop session, each batch behind the focus guard.

    `app_focused` (MutterDriver.app_focused) adds the second half of the guard: X focus must be off the app.
    """

    def __init__(self, remote=None, app_focused=None) -> None:
        from . import mutter

        self.mutter = mutter
        self.owned = remote is None
        self.remote = mutter.RemoteDesktop.connect() if remote is None else remote
        self.app_focused = app_focused
        self.log: list[str] = []

    def stop(self) -> None:
        if self.owned:
            self.remote.stop()

    def guard(self, window, why: str, attempts: int = 20, poll: float = 0.3) -> None:
        """Wait up to attempts x poll seconds for focus inside `window` with X focus off the app, else refuse."""
        for attempt in range(max(1, attempts)):
            inside = dialog_has_focus(window)
            off_app = self.app_focused is None or not self.app_focused()
            if inside and off_app:
                return
            if attempt + 1 < attempts:
                time.sleep(poll)
        self.log.append(f"guard refused {why}: focus inside dialog {inside}, X focus off the app {off_app}")
        raise DialogFocusRefused(f"no widget inside the dialog holds focus (or X focus is on the app) before {why}; "
                                 "no key sent")

    def key(self, window, name: str, mods=(), wait: float = 0.25) -> None:
        syms = [self.mutter.keysym(mod) for mod in mods]
        sym = self.mutter.keysym(name)
        if sym == 0 or 0 in syms:
            raise ValueError(f"unknown keysym in {list(mods)} {name!r}")
        self.guard(window, f"dialog key {name}")
        self.remote.chord(syms, sym)
        self.log.append("+".join([*(m.replace("_L", "") for m in mods), name]))
        time.sleep(wait)

    def type(self, window, text: str, per: float = 0.045) -> None:
        syms = [self.mutter.char_keysym(char) for char in text]
        self.guard(window, f"dialog type {text!r}")
        for sym in syms:
            self.remote.keysym(sym, True)
            self.remote.keysym(sym, False)
            time.sleep(per)
        self.log.append(f"type {text!r}")
        time.sleep(0.2)

    def type_checked(self, window, text: str, select_all: bool = True) -> tuple[bool, str | None]:
        """Type into the dialog's focused entry and read it back."""
        if select_all:
            self.key(window, "a", ["Control_L"])
        self.type(window, text)
        time.sleep(0.5)
        got = focused_text(window)
        return got == text, got


# ---------- D-Bus ----------
class FileChooserMonitor:
    """A dbus-monitor transcript of the FileChooser and Request interfaces, for the run directory.

    A `SaveFile` without a `Response` is the signature of a dialog the driver never completed.
    """

    def __init__(self, path: Path) -> None:
        self.handle = open(path, "w")
        self.proc = subprocess.Popen(
            ["dbus-monitor", "--session", "interface='org.freedesktop.portal.FileChooser'",
             "interface='org.freedesktop.portal.Request'"],
            stdout=self.handle, stderr=subprocess.STDOUT)
        time.sleep(0.8)

    def stop(self) -> None:
        time.sleep(0.5)
        self.proc.terminate()
        self.proc.wait(timeout=5)
        self.handle.close()


def start_private_bus(root: Path) -> tuple[subprocess.Popen, str]:
    """A session bus without the portal; pass its address as the launch's DBUS_SESSION_BUS_ADDRESS."""
    config = root / "private-bus.conf"
    config.write_text(PRIVATE_BUS_CONFIG)
    proc = subprocess.Popen(["dbus-daemon", "--config-file", str(config), "--print-address", "--nofork"],
                            stdout=subprocess.PIPE, text=True)
    address = proc.stdout.readline().strip()
    if not address:
        proc.terminate()
        raise SystemExit("dbus-daemon printed no address")
    return proc, address
