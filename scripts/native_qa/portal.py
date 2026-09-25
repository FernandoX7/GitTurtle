"""The XDG file-chooser portal on GNOME: read its dialog, type into it, record its D-Bus traffic.

The Import/Export dialogs are drawn by xdg-desktop-portal-gnome as Wayland
clients of the session compositor, not X11 clients of the QA display. XTest
never reaches them (a whole file name typed into the Save dialog left its
Name entry unchanged) and org.gnome.Shell.Screenshot refuses this caller. So
the dialog's evidence is its AT-SPI tree, and input goes through
org.gnome.Mutter.RemoteDesktop; every keystroke sent is read back from the
dialog's entry before the next, so input that did not land is visible.

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
REMOTE_DESKTOP = "org.gnome.Mutter.RemoteDesktop"
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
CHAR_KEYSYMS = {"/": "slash", ".": "period", "-": "minus", "_": "underscore", " ": "space",
                "(": "parenleft", ")": "parenright", "#": "numbersign", ",": "comma", "~": "asciitilde",
                ":": "colon", "=": "equal", "+": "plus", "{": "braceleft", "}": "braceright",
                '"': "quotedbl", "'": "apostrophe", "*": "asterisk", "?": "question", "!": "exclam"}


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
    """Every top-level of the portal backends, with the backend that owns it."""
    desktop = _atspi().get_desktop(0)
    found = []
    for i in range(desktop.get_child_count()):
        try:
            app = desktop.get_child_at_index(i)
            if not app or app.get_name() not in PORTALS:
                continue
            found += [(app.get_name(), app.get_child_at_index(j)) for j in range(app.get_child_count())]
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


def states_of(node) -> list[str]:
    atspi = _atspi()
    try:
        state_set = node.get_state_set()
    except Exception:
        return []
    return [name for name in STATES if state_set.contains(getattr(atspi.StateType, name.upper()))]


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
            for node in find_nodes(window, role="text") + find_nodes(window, role="entry")]


def focused_text(window) -> str | None:
    entries = entry_texts(window)
    for entry in entries:
        if "focused" in entry["states"]:
            return entry["text"]
    return entries[0]["text"] if entries else None


# ---------- compositor keyboard ----------
class Keyboard:
    """Keysyms injected through a Mutter RemoteDesktop session, which reach the portal dialog."""

    def __init__(self) -> None:
        import dbus

        self.dbus = dbus
        bus = dbus.SessionBus()
        top = dbus.Interface(bus.get_object(REMOTE_DESKTOP, "/org/gnome/Mutter/RemoteDesktop"), REMOTE_DESKTOP)
        self.path = str(top.CreateSession())
        self.session = dbus.Interface(bus.get_object(REMOTE_DESKTOP, self.path), f"{REMOTE_DESKTOP}.Session")
        self.session.Start()
        self.log: list[str] = []

    def stop(self) -> None:
        try:
            self.session.Stop()
        except Exception:
            pass

    def _sym(self, keysym: int, down: bool) -> None:
        self.session.NotifyKeyboardKeysym(self.dbus.UInt32(keysym), self.dbus.Boolean(down))

    def key(self, name: str, mods=(), wait: float = 0.25) -> None:
        from Xlib import XK

        syms = [XK.string_to_keysym(mod) for mod in mods]
        sym = XK.string_to_keysym(name)
        if sym == 0:
            raise ValueError(f"unknown keysym name {name!r}")
        for mod in syms:
            self._sym(mod, True)
        self._sym(sym, True)
        self._sym(sym, False)
        for mod in reversed(syms):
            self._sym(mod, False)
        self.log.append("+".join([*(m.replace("_L", "") for m in mods), name]))
        time.sleep(wait)

    def type(self, text: str, per: float = 0.045) -> None:
        from Xlib import XK

        for char in text:
            name = CHAR_KEYSYMS.get(char, char if char.isalnum() else None)
            if name is None:
                raise ValueError(f"no keysym mapping for {char!r}")
            sym = XK.string_to_keysym(name) or (ord(char) if char.isupper() else 0)
            self._sym(sym, True)
            self._sym(sym, False)
            time.sleep(per)
        self.log.append(f"type {text!r}")
        time.sleep(0.2)

    def type_checked(self, window, text: str, select_all: bool = True) -> tuple[bool, str | None]:
        """Type into the dialog's focused entry and read it back."""
        if select_all:
            self.key("a", ["Control_L"])
        self.type(text)
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
