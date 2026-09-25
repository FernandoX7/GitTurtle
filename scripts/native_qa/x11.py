"""python-xlib primitives: find GitTurtle's window by process, drive it with XTest and grab it.

computer-use cannot see GPUI windows, so everything goes through the X
display. The operator may run their own GitTurtle: every lookup is scoped to
the process this session launched, and nothing here kills by name.
"""

from __future__ import annotations

import time

from Xlib import X, XK, display as xdisplay
from Xlib.ext import xtest

WM_CLASS = "gitturtle"
# Keysym names for characters XK.string_to_keysym does not accept as-is.
CHAR_KEYSYMS = {
    " ": "space", "#": "numbersign", "/": "slash", ".": "period", "-": "minus", "_": "underscore",
    "(": "parenleft", ")": "parenright", ",": "comma", "~": "asciitilde", ":": "colon", "=": "equal",
    "+": "plus", "{": "braceleft", "}": "braceright", '"': "quotedbl", "'": "apostrophe",
    "*": "asterisk", "?": "question", "!": "exclam", "@": "at",
}


def connect(name: str):
    return xdisplay.Display(name)


def walk(window):
    try:
        children = window.query_tree().children
    except Exception:
        return
    for child in children:
        yield child
        yield from walk(child)


def gitturtle_windows(dsp) -> list[dict]:
    """Every window whose WM_CLASS names GitTurtle, with its _NET_WM_PID and size."""
    atom = dsp.intern_atom("_NET_WM_PID")
    found = []
    for window in walk(dsp.screen().root):
        try:
            cls = window.get_wm_class()
            if not cls or not any(WM_CLASS in item.lower() for item in cls):
                continue
            owner = window.get_full_property(atom, 0)
            geometry = window.get_geometry()
        except Exception:
            continue
        found.append(dict(id=window.id, wm_class=list(cls), pid=owner.value[0] if owner else None,
                          width=geometry.width, height=geometry.height, window=window))
    return found


def find_window(dsp, pid: int, timeout: float = 40.0):
    """The main window of `pid` alone, so another instance is never captured or driven."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for entry in gitturtle_windows(dsp):
            if entry["pid"] == pid and entry["width"] > 200 and entry["height"] > 200:
                return entry["window"]
        time.sleep(0.5)
    raise SystemExit(f"no GitTurtle window for pid {pid} on {dsp.get_display_name()}")


class Driver:
    """XTest input and grabs for one window, logging every event it sends."""

    def __init__(self, dsp, window, log: list[str] | None = None) -> None:
        self.d, self.w = dsp, window
        self.log = [] if log is None else log

    def record(self, text: str, note: str | None = None) -> None:
        self.log.append(text + (f"  # {note}" if note else ""))

    # ---------- window ----------
    def activate(self) -> None:
        self.w.set_input_focus(X.RevertToParent, X.CurrentTime)
        self.w.configure(stack_mode=X.Above)
        self.d.sync()
        time.sleep(0.3)

    def size(self) -> tuple[int, int]:
        geometry = self.w.get_geometry()
        return geometry.width, geometry.height

    def resize(self, width: int, height: int, timeout: float = 5.0) -> None:
        """Mutter honours a plain ConfigureWindow; the app's minimum is 1000x680."""
        self.w.configure(width=width, height=height)
        self.d.sync()
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.size() == (width, height):
                time.sleep(0.6)  # let the relayout settle before grabbing
                self.record(f"resize {width}x{height}")
                return
            time.sleep(0.2)
        raise SystemExit(f"window stayed {self.size()}, wanted {width}x{height}")

    def origin(self) -> tuple[int, int]:
        coords = self.w.translate_coords(self.d.screen().root, 0, 0)
        return -coords.x, -coords.y

    # ---------- pointer ----------
    def move(self, x: int, y: int, note: str | None = None) -> None:
        ox, oy = self.origin()
        xtest.fake_input(self.d, X.MotionNotify, x=ox + x, y=oy + y)
        self.d.sync()
        self.record(f"move ({x},{y})", note)

    def glide(self, x: int, y: int, note: str | None = None, settle: float = 0.8) -> None:
        """Two-step approach: a move to where the pointer already is emits no motion event."""
        self.move(x - 40, y - 30)
        time.sleep(0.2)
        self.move(x, y, note)
        time.sleep(settle)

    def park(self, settle: float = 0.8) -> None:
        """Leave no hover: a move into the window's bottom-left margin, then off the window.

        The last event the app sees is the pointer leaving. Moving straight off
        the window can leave a hover the app already computed standing.
        """
        width, height = self.size()
        ox, oy = self.origin()
        self.move(4, height - 4)
        time.sleep(0.2)
        root = self.d.screen().root.get_geometry()
        x, y = min(ox + width + 120, root.width - 2), min(oy + height + 120, root.height - 2)
        if x < ox + width or y < oy + height:
            x, y = max(ox - 120, 1), max(oy - 120, 1)
        xtest.fake_input(self.d, X.MotionNotify, x=x, y=y)
        self.d.sync()
        self.record(f"park off-window at root ({x},{y})")
        time.sleep(settle)

    def button(self, down: bool, number: int = 1, note: str | None = None) -> None:
        xtest.fake_input(self.d, X.ButtonPress if down else X.ButtonRelease, number)
        self.d.sync()
        self.record(f"{'press' if down else 'release'} button {number}", note)

    def click(self, x: int, y: int, note: str | None = None) -> None:
        self.glide(x, y, note, settle=0.2)
        self.button(True)
        time.sleep(0.05)
        self.button(False)
        time.sleep(0.4)

    def wheel(self, x: int, y: int, steps: int) -> None:
        self.glide(x, y, note="wheel position", settle=0.3)
        number = 5 if steps > 0 else 4
        for _ in range(abs(steps)):
            xtest.fake_input(self.d, X.ButtonPress, number)
            xtest.fake_input(self.d, X.ButtonRelease, number)
            self.d.sync()
            time.sleep(0.12)
        self.record(f"wheel {steps} at ({x},{y})")
        time.sleep(0.8)

    # ---------- keyboard ----------
    def keycode(self, name: str) -> int:
        keysym = XK.string_to_keysym(name)
        if keysym == 0:
            raise SystemExit(f"unknown keysym name {name!r}")
        return self.d.keysym_to_keycode(keysym)

    def key(self, name: str, mods: tuple[str, ...] | list[str] = (), note: str | None = None,
            wait: float = 0.4) -> None:
        codes = [self.keycode(mod) for mod in mods]
        code = self.keycode(name)
        for mod in codes:
            xtest.fake_input(self.d, X.KeyPress, mod)
        xtest.fake_input(self.d, X.KeyPress, code)
        xtest.fake_input(self.d, X.KeyRelease, code)
        for mod in reversed(codes):
            xtest.fake_input(self.d, X.KeyRelease, mod)
        self.d.sync()
        self.record("key " + "+".join([*(m.replace("_L", "") for m in mods), name]), note)
        time.sleep(wait)

    def type(self, text: str, note: str | None = None, per: float = 0.05) -> None:
        """Type into the app window; shifted characters hold Shift_L."""
        shift = self.keycode("Shift_L")
        for char in text:
            keysym = XK.string_to_keysym(CHAR_KEYSYMS.get(char, char))
            code = self.d.keysym_to_keycode(keysym)
            if not keysym or not code:
                raise SystemExit(f"no key for {char!r}")
            shifted = self.d.keycode_to_keysym(code, 0) != keysym
            if shifted:
                xtest.fake_input(self.d, X.KeyPress, shift)
            xtest.fake_input(self.d, X.KeyPress, code)
            xtest.fake_input(self.d, X.KeyRelease, code)
            if shifted:
                xtest.fake_input(self.d, X.KeyRelease, shift)
            self.d.sync()
            time.sleep(per)
        self.record(f"type {text!r}", note)

    # ---------- frames ----------
    def snap(self):
        from PIL import Image

        width, height = self.size()
        raw = self.w.get_image(0, 0, width, height, X.ZPixmap, 0xFFFFFFFF)
        return Image.frombytes("RGB", (width, height), raw.data, "raw", "BGRX")

    def stable(self, timeout: float = 4.0, quiet: float = 0.5) -> float | None:
        """Seconds until two grabs `quiet` apart are identical (animations and tooltips settled)."""
        from PIL import ImageChops

        start = time.perf_counter()
        previous = self.snap()
        while time.perf_counter() - start < timeout:
            time.sleep(quiet)
            current = self.snap()
            if ImageChops.difference(previous, current).getbbox() is None:
                return round(time.perf_counter() - start, 2)
            previous = current
        return None

    def wait_change(self, before, timeout: float = 3.0, region=None):
        """Seconds until the window differs from `before` (inside `region`), and that frame."""
        from PIL import ImageChops

        start = time.perf_counter()
        while time.perf_counter() - start < timeout:
            current = self.snap()
            a, b = (before, current) if region is None else (before.crop(region), current.crop(region))
            if ImageChops.difference(a, b).getbbox() is not None:
                return round(time.perf_counter() - start, 3), current
            time.sleep(0.01)
        return None, self.snap()
