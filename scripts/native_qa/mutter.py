"""App input through org.gnome.Mutter.RemoteDesktop, for hosts where XTest goes through the RemoteDesktop portal.

When XWayland runs with `-enable-ei-portal` (GNOME 50 on Ubuntu 26.04), XTest
input is routed through the RemoteDesktop portal, whose "Allow Remote
Interaction" prompt swallows keys. Mutter's own RemoteDesktop API needs no
prompt, so on such a host every key, pointer motion, button and wheel step is
sent through it, and XTest is never used. X still finds the window, reads focus
and geometry and grabs frames (`x11`).

Mutter delivers keys to whatever surface has compositor focus, so a key is sent
only while X input focus is verified on the app's window; dialog keys have their
own guard in `portal.Keyboard`.
"""

from __future__ import annotations

import os
import time
from pathlib import Path

from . import x11

BUS_NAME = "org.gnome.Mutter.RemoteDesktop"
OBJECT_PATH = "/org/gnome/Mutter/RemoteDesktop"
EI_PORTAL_FLAG = "-enable-ei-portal"
INPUTS = ("mutter", "xtest")
BUTTONS = {1: 0x110, 2: 0x112, 3: 0x111}  # evdev BTN_LEFT, BTN_MIDDLE, BTN_RIGHT
VERTICAL = 0  # NotifyPointerAxisDiscrete axis


class InputRefused(SystemExit):
    """An input was not sent because its guard failed; the message says which and why."""

    def __init__(self, message: str) -> None:
        super().__init__(f"refusing: {message}")


# ---------- backend selection ----------
def process_argvs(proc: Path = Path("/proc")) -> list[list[str]]:
    """The command line of every readable process (empty where /proc does not exist, as on macOS)."""
    argvs = []
    try:
        entries = list(proc.iterdir())
    except OSError:
        return argvs
    for entry in entries:
        if not entry.name.isdigit():
            continue
        try:
            raw = (entry / "cmdline").read_bytes()
        except OSError:
            continue
        if raw:
            argvs.append([part.decode(errors="replace") for part in raw.rstrip(b"\0").split(b"\0")])
    return argvs


def ei_portal_host(argvs: list[list[str]]) -> bool:
    """True when an XWayland on this host runs with -enable-ei-portal, so its XTest reaches the portal."""
    return any(argv and os.path.basename(argv[0]) == "Xwayland" and EI_PORTAL_FLAG in argv[1:] for argv in argvs)


def choose_input(requested: str | None, argvs: list[list[str]]) -> str:
    """`requested`, or the host's default; XTest is refused outright on an ei-portal host."""
    portal_host = ei_portal_host(argvs)
    if requested is not None and requested not in INPUTS:
        raise ValueError(f"unknown input backend {requested!r}; expected one of {', '.join(INPUTS)}")
    if requested == "xtest" and portal_host:
        raise InputRefused(f"XWayland runs with {EI_PORTAL_FLAG}: XTest would go through the RemoteDesktop portal, "
                           "whose prompt swallows keys; use --input mutter")
    return requested or ("mutter" if portal_host else "xtest")


# ---------- D-Bus ----------
def keysym(name: str) -> int:
    """The X keysym for a name such as `comma` or `Control_L`; 0 when unknown."""
    if x11.XK is None:
        raise SystemExit("keysym lookup needs python-xlib (import Xlib failed)")
    return x11.XK.string_to_keysym(name)


def char_keysym(char: str) -> int:
    sym = keysym(x11.CHAR_KEYSYMS.get(char, char)) or (ord(char) if char.isalnum() else 0)
    if not sym:
        raise InputRefused(f"no keysym for {char!r}; nothing typed")
    return sym


class RemoteDesktop:
    """One started org.gnome.Mutter.RemoteDesktop session; `types` supplies the D-Bus argument types."""

    def __init__(self, session, types) -> None:
        self.session, self.types = session, types

    @classmethod
    def connect(cls) -> "RemoteDesktop":
        import dbus

        bus = dbus.SessionBus()
        top = dbus.Interface(bus.get_object(BUS_NAME, OBJECT_PATH), BUS_NAME)
        path = str(top.CreateSession())
        session = dbus.Interface(bus.get_object(BUS_NAME, path), f"{BUS_NAME}.Session")
        session.Start()
        return cls(session, dbus)

    def stop(self) -> None:
        try:
            self.session.Stop()
        except Exception:
            pass

    def keysym(self, sym: int, down: bool) -> None:
        self.session.NotifyKeyboardKeysym(self.types.UInt32(sym), self.types.Boolean(down))

    def chord(self, syms: list[int], sym: int) -> None:
        for mod in syms:
            self.keysym(mod, True)
        self.keysym(sym, True)
        self.keysym(sym, False)
        for mod in reversed(syms):
            self.keysym(mod, False)

    def relative(self, dx: float, dy: float) -> None:
        self.session.NotifyPointerMotionRelative(self.types.Double(dx), self.types.Double(dy))

    def button(self, code: int, down: bool) -> None:
        self.session.NotifyPointerButton(self.types.Int32(code), self.types.Boolean(down))

    def wheel(self, steps: int) -> None:
        self.session.NotifyPointerAxisDiscrete(self.types.UInt32(VERTICAL), self.types.Int32(steps))


def screen_locked(bus=None) -> bool | None:
    """org.gnome.ScreenSaver.GetActive on the session bus: True while locked, None when it cannot be read."""
    try:
        if bus is None:
            import dbus

            bus = dbus.SessionBus()
        saver = bus.get_object("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver")
        return bool(saver.GetActive(dbus_interface="org.gnome.ScreenSaver"))
    except Exception:
        return None


# ---------- driver ----------
class MutterDriver(x11.Driver):
    """x11.Driver with every input sent through Mutter; window lookup, geometry, focus and grabs are unchanged.

    XWayland learns the pointer position only while the pointer is over an X window, so the position is
    tracked here: anchored by a large relative move into the bottom-right screen corner (monitor scale 1.0,
    so logical coordinates are X root coordinates; there is no hot corner there), then moved by exact
    relative steps. A target on the app window is verified against XWayland; a mismatch re-anchors once,
    then aborts.
    """

    def __init__(self, dsp, window, log: list[str] | None, remote: RemoteDesktop) -> None:
        super().__init__(dsp, window, log)
        self.remote = remote
        self.known: tuple[int, int] | None = None

    # ---------- focus ----------
    def app_focused(self) -> bool:
        return x11.focus_within(self.d, self.w)

    def require_focus(self, why: str) -> None:
        """Keys reach the compositor's focus, so they go only to a verified X focus on the app."""
        if self.app_focused():
            return
        self.activate()
        time.sleep(0.4)
        ok = self.app_focused()
        self.record(f"guard x-focus before {why}: re-activated, {'focused' if ok else 'still not focused'}")
        if not ok:
            raise InputRefused(f"X focus is not on the app's window before {why}; no key sent")

    def dialog_keyboard(self):
        """A portal.Keyboard on this session that also refuses while X focus is on the app."""
        from . import portal

        return portal.Keyboard(self.remote, app_focused=self.app_focused)

    # ---------- pointer ----------
    def pointer(self) -> tuple[int, int]:
        q = self.d.screen().root.query_pointer()
        return q.root_x, q.root_y

    def _anchor(self) -> None:
        root = self.d.screen().root.get_geometry()
        self.remote.relative(20000.0, 20000.0)
        time.sleep(0.08)
        self.known = (root.width - 1, root.height - 1)

    def _to(self, tx: int, ty: int) -> None:
        for attempt in range(2):
            if self.known is None or attempt:
                self._anchor()
            kx, ky = self.known
            self.remote.relative(float(tx - kx), float(ty - ky))
            self.known = (tx, ty)
            time.sleep(0.06)
            ox, oy = self.origin()
            width, height = self.size()
            if not (ox <= tx < ox + width and oy <= ty < oy + height):
                return  # off the app window: XWayland cannot confirm it
            if self.pointer() == (tx, ty):
                return
        raise InputRefused(f"pointer at root {self.pointer()}, wanted ({tx},{ty}); aborting")

    def on_window(self, x: int, y: int) -> bool:
        q = self.w.query_pointer()
        width, height = self.size()
        return bool(q.same_screen) and (q.win_x, q.win_y) == (x, y) and 0 <= x < width and 0 <= y < height

    def move(self, x: int, y: int, note: str | None = None) -> None:
        ox, oy = self.origin()
        self._to(ox + x, oy + y)
        self.record(f"move ({x},{y})", note)

    def park(self, settle: float = 0.8) -> None:
        width, height = self.size()
        ox, oy = self.origin()
        self._to(ox + 4, oy + height - 4)
        time.sleep(0.2)
        root = self.d.screen().root.get_geometry()
        x, y = min(ox + width + 120, root.width - 2), min(oy + height + 120, root.height - 2)
        if x < ox + width or y < oy + height:
            x, y = max(ox - 120, 1), max(oy - 120, 1)
        self._to(x, y)
        self.record(f"park off-window at root ({x},{y})")
        time.sleep(settle)

    def button(self, down: bool, number: int = 1, note: str | None = None) -> None:
        if number not in BUTTONS:
            raise InputRefused(f"button {number} has no evdev code here; nothing sent")
        self.remote.button(BUTTONS[number], down)
        self.record(f"{'press' if down else 'release'} button {number}", note)

    def click(self, x: int, y: int, note: str | None = None) -> None:
        self.glide(x, y, note, settle=0.2)
        if not self.on_window(x, y):
            raise InputRefused(f"pointer is not on the app window at ({x},{y}); no click sent")
        self.button(True)
        time.sleep(0.05)
        self.button(False)
        time.sleep(0.4)

    def wheel(self, x: int, y: int, steps: int) -> None:
        """`steps` wheel steps; Mutter drops the first discrete click of each batch, so one extra is sent first."""
        self.glide(x, y, note="wheel position", settle=0.3)
        if not self.on_window(x, y):
            raise InputRefused(f"pointer is not on the app window at ({x},{y}); no wheel sent")
        if steps == 0:
            return
        one = 1 if steps > 0 else -1
        for _ in range(abs(steps) + 1):
            self.remote.wheel(one)
            time.sleep(0.12)
        self.record(f"wheel {steps} at ({x},{y}) ({abs(steps) + 1} clicks sent; Mutter drops the first)")
        time.sleep(0.8)

    # ---------- keyboard ----------
    def key(self, name: str, mods: tuple[str, ...] | list[str] = (), note: str | None = None,
            wait: float = 0.4) -> None:
        syms = [keysym(mod) for mod in mods]
        sym = keysym(name)
        if sym == 0 or 0 in syms:
            raise InputRefused(f"unknown keysym in {list(mods)} {name!r}; nothing sent")
        self.require_focus(f"key {name}")
        self.remote.chord(syms, sym)
        self.record("key " + "+".join([*(m.replace("_L", "") for m in mods), name]), note)
        time.sleep(wait)

    def type(self, text: str, note: str | None = None, per: float = 0.05) -> None:
        syms = [char_keysym(char) for char in text]
        self.require_focus(f"type {text!r}")
        for index, sym in enumerate(syms):
            if index and not self.app_focused():
                raise InputRefused(f"X focus left the app's window after {text[:index]!r}; the rest not typed")
            self.remote.keysym(sym, True)
            self.remote.keysym(sym, False)
            time.sleep(per)
        self.record(f"type {text!r}", note)
