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
# The screen corners a large relative move pins the pointer in, by preference, as the move's (x, y) signs; never
# the top-left, where GNOME's hot corner opens the Activities overview.
ANCHORS = {(1, 1): "bottom-right", (1, -1): "top-right", (-1, 1): "bottom-left"}
ANCHOR_PUSH = 20000.0  # px of relative motion, more than any screen, so the pointer stops in the corner


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


def anchor_corner(rect, root) -> tuple[tuple[int, int], tuple[int, int]]:
    """The signs of the relative move that anchors the pointer and the root corner it stops in: the first of
    `ANCHORS` off the window at `rect` on the `root` (width, height) screen, else the bottom-right."""
    width, height = root
    corners = [(signs, (width - 1 if signs[0] > 0 else 0, height - 1 if signs[1] > 0 else 0)) for signs in ANCHORS]
    return next((corner for corner in corners if not x11.inside(rect, *corner[1])), corners[0])


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

    The RemoteDesktop session has no screencast stream, so it has no absolute pointer coordinates: every motion
    is relative. XWayland learns the pointer position only while the pointer is over an X window, so the position
    is tracked here: anchored by a large relative move into a screen corner off the app's window
    (`anchor_corner`; monitor scale 1.0, so logical coordinates are X root coordinates), then moved by exact
    relative steps. A target on the app window is verified against XWayland; a mismatch re-anchors once, then
    aborts. Mutter applies a relative motion as one jump to its end, with no pointer position between, so a
    park (`x11.plan_park`) moves straight from a tracked position to its point off the window and the app never
    sees the pointer on its window; its old move through the window's corner was the X11 hover clearing, not an
    addressing need. A park that finds the position unknown anchors first, in a corner off the window.
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
    def where(self) -> tuple[int, int] | None:
        """The tracked root position, None until the first anchor (XWayland's own is stale off its windows)."""
        return self.known

    def locate(self, rect, root) -> tuple[int, int]:
        self._anchor()
        return self.known

    def _anchor(self, rect=None, root=None) -> tuple[int, int]:
        """Pin the pointer in `anchor_corner` by one large relative move; returns that corner."""
        rect, root = rect or self.rect(), root or self.root_size()
        signs, corner = anchor_corner(rect, root)
        self.remote.relative(ANCHOR_PUSH * signs[0], ANCHOR_PUSH * signs[1])
        time.sleep(0.08)
        self.known = corner
        where = "off the window" if not x11.inside(rect, *corner) else "every allowed corner is on the window"
        self.note_motion(corner, f"anchor: a large relative move pins the pointer in the screen's {ANCHORS[signs]} "
                                 f"corner, {where}", clears=False)
        return corner

    def _to(self, tx: int, ty: int, why: str = "move") -> bool:
        """Move the pointer to root (tx, ty), kept on the screen as Mutter keeps it (one monitor), by one relative
        motion from the tracked position, or none when it is already there; during a park each motion is
        recorded. True when a motion was sent that ended on the app's window, which puts the app in mouse mode.
        On the window XWayland must report the pointer there (a mismatch re-anchors once, then aborts)."""
        rect, root = self.rect(), self.root_size()
        tx, ty = min(max(tx, 0), root[0] - 1), min(max(ty, 0), root[1] - 1)
        entered = False
        for attempt in range(2):
            if self.known is None or attempt:
                entered |= x11.inside(rect, *self._anchor(rect, root))
            if self.known != (tx, ty):
                kx, ky = self.known
                self.remote.relative(float(tx - kx), float(ty - ky))
                self.known = (tx, ty)
                self.note_motion((tx, ty), why)
                entered |= x11.inside(rect, tx, ty)
                time.sleep(0.06)
            if not x11.inside(rect, tx, ty):
                return entered  # off the app window: XWayland cannot confirm it
            if self.pointer() == (tx, ty):
                return entered
        raise InputRefused(f"pointer at root {self.pointer()}, wanted ({tx},{ty}); aborting")

    def park_motion(self, point: tuple[int, int], why: str) -> None:
        self._to(*point, why=why)

    def move(self, x: int, y: int, note: str | None = None) -> None:
        ox, oy = self.origin()
        if self._to(ox + x, oy + y):
            self.pointer_input()
        self.record(f"move ({x},{y})", note)

    def button(self, down: bool, number: int = 1, note: str | None = None) -> None:
        if number not in BUTTONS:
            raise InputRefused(f"button {number} has no evdev code here; nothing sent")
        self.remote.button(BUTTONS[number], down)
        if down and self.known is not None and x11.inside(self.rect(), *self.known):
            self.pointer_input()
        self.record(f"{'press' if down else 'release'} button {number}", note)

    def aim(self, x: int, y: int, note: str | None = None, settle: float = 0.8) -> None:
        """The glide to (x, y), after which XWayland must report the pointer there on the app window, or no
        button is sent (a click, or a probe's press or release)."""
        self.glide(x, y, note, settle=settle)
        if not self.on_window(x, y):
            raise InputRefused(f"pointer is not on the app window at ({x},{y}); no button sent")

    def click(self, x: int, y: int, note: str | None = None) -> None:
        self.aim(x, y, note, settle=0.2)
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
        self.keys_sent(sym)
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
            self.keys_sent(sym)
            time.sleep(per)
        self.record(f"type {text!r}", note)
