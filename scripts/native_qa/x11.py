"""python-xlib primitives: find GitTurtle's window by process, drive it with XTest and grab it.

computer-use cannot see GPUI windows, so everything goes through the X
display. The operator may run their own GitTurtle: every lookup is scoped to
the process this session launched, and nothing here kills by name. On a host
whose XWayland routes XTest through the RemoteDesktop portal, input goes
through `mutter.MutterDriver` instead; window lookup, focus and grabs stay here.
"""

from __future__ import annotations

import time

try:
    from Xlib import X, XK, display as xdisplay
    from Xlib.ext import xtest
except ImportError:  # the unit tests run without python-xlib; connect() names the missing module
    X = XK = xdisplay = xtest = None

WM_CLASS = "gitturtle"
PARK_MARGIN = 120  # px past the window's edge where a park leaves the pointer
CORNER_INSET = 4   # px into the window's bottom-left corner, where a park clears a hover the pointer left
# Keysym names for characters XK.string_to_keysym does not accept as-is.
CHAR_KEYSYMS = {
    " ": "space", "#": "numbersign", "/": "slash", ".": "period", "-": "minus", "_": "underscore",
    "(": "parenleft", ")": "parenright", ",": "comma", "~": "asciitilde", ":": "colon", "=": "equal",
    "+": "plus", "{": "braceleft", "}": "braceright", '"': "quotedbl", "'": "apostrophe",
    "*": "asterisk", "?": "question", "!": "exclam", "@": "at",
}


def image_from(data: bytes, size):
    """An RGB frame from a grab's BGRX bytes (`Driver.grab`)."""
    from PIL import Image

    return Image.frombytes("RGB", tuple(size), data, "raw", "BGRX")


def connect(name: str):
    if xdisplay is None:
        raise SystemExit("launch needs python-xlib (import Xlib failed)")
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


def focus_within(dsp, window, depth: int = 12) -> bool:
    """True when X input focus is on `window` or one of its descendants (PointerRoot and None are not)."""
    focus = dsp.get_input_focus().focus
    if focus is None or isinstance(focus, int):
        return False
    root = dsp.screen().root.id
    node = focus
    for _ in range(depth):
        if node.id == window.id:
            return True
        try:
            parent = node.query_tree().parent
        except Exception:
            return False
        if parent is None or parent.id in (0, root):
            return False
        node = parent
    return False


# ---------- parking ----------
def modifier_keysym(sym: int) -> bool:
    """True for a keysym GPUI's X11 client drops on its own, sending no KeyDown (xkeysym's `is_modifier_key`):
    Shift_L to Hyper_R (Shift, Control, the locks, Meta, Alt, Super, Hyper), ISO_Lock to ISO_Level5_Lock,
    Mode_switch and Num_Lock. Such a key alone leaves the app's input mode as it was."""
    return 0xFFE1 <= sym <= 0xFFEE or 0xFE01 <= sym <= 0xFE13 or sym in (0xFF7E, 0xFF7F)


def inside(rect, x: int, y: int) -> bool:
    """True when root point (x, y) is on the window at `rect`, its root (x0, y0, x1, y1) with exclusive ends."""
    x0, y0, x1, y1 = rect
    return x0 <= x < x1 and y0 <= y < y1


def park_target(rect, root) -> tuple[int, int] | None:
    """Where a park leaves the pointer: PARK_MARGIN px below and right of the window, else above and left, above
    and right or below and left, each kept within the screen (`root`, its width and height); None when all of
    them are on the window, as when it covers the screen."""
    x0, y0, x1, y1 = rect
    width, height = root
    right, below = min(x1 + PARK_MARGIN, width - 2), min(y1 + PARK_MARGIN, height - 2)
    left, above = max(x0 - PARK_MARGIN, 1), max(y0 - PARK_MARGIN, 1)
    return next((point for point in ((right, below), (left, above), (right, above), (left, below))
                 if not inside(rect, *point)), None)


def plan_park(rect, root, start, hover_pending: bool) -> list[tuple[tuple[int, int], str]]:
    """The root points a park moves the pointer to, in order, each with its reason.

    GPUI (`Window::dispatch_event`) puts a window in keyboard mode on a key
    press and back in mouse mode on a pointer motion or button press on it;
    the pointer leaving changes neither. Keyboard mode hides hover and draws
    focus-visible indicators, so a single motion onto the window after a key
    removes them from every later frame. When the pointer leaves, GPUI also
    keeps its last position on the window, so in mouse mode the hover there
    stays drawn.

    With no hover pending (the app's last input was a key, which hides hover,
    or an earlier park cleared it), the pointer goes straight to
    `park_target`: the app receives no motion, at most the pointer leaving,
    and keeps its mode. A pointer already off the window is not moved at
    all, since an off-window motion reaches none of the app's windows, and
    neither is one on a window that covers the screen. With a hover pending
    (the app's last input was the pointer, so it is in mouse mode already)
    the pointer first moves into the window's bottom-left margin, where
    nothing hovers, and then off: that motion clears the hover, and the app
    has no keyboard mode to lose.
    """
    target = park_target(rect, root)
    steps = []
    if hover_pending:
        steps.append(((rect[0] + CORNER_INSET, rect[3] - CORNER_INSET),
                      "into the window's bottom-left margin to clear the hover; the app's last input was the pointer, "
                      "so it is in mouse mode already"))
    elif start is not None and not inside(rect, *start):
        return []
    if target is not None:
        steps.append((target, f"off the window, {PARK_MARGIN} px past it"))
    return steps


class Driver:
    """XTest input and grabs for one window, logging every event it sends."""

    def __init__(self, dsp, window, log: list[str] | None = None) -> None:
        self.d, self.w = dsp, window
        self.log = [] if log is None else log
        # The app's GPUI input mode as this driver's own input last set it (`plan_park`): keyboard after a key
        # other than a modifier alone, mouse after a motion that moved the pointer onto or within the window or a
        # button press there, which also computes a hover that may still be drawn. At launch the app is in mouse
        # mode and its hover is unknown.
        self.keyboard_mode, self.hover_pending = False, True
        # A park had to move the pointer onto the window in keyboard mode, and no input to the app followed.
        self.keyboard_mode_lost = False
        self.park_motions: list[dict] | None = None  # while a park runs, every pointer motion it sends
        self.park_rect: tuple[int, int, int, int] | None = None

    def record(self, text: str, note: str | None = None) -> None:
        self.log.append(text + (f"  # {note}" if note else ""))

    # ---------- input mode ----------
    def keys_sent(self, sym: int) -> None:
        """The key `sym` reached the app: keyboard mode, which hides any hover, unless it is a modifier alone,
        for which GPUI sends no KeyDown (`modifier_keysym`) and nothing changes."""
        if modifier_keysym(sym):
            return
        self.keyboard_mode, self.hover_pending, self.keyboard_mode_lost = True, False, False

    def pointer_input(self) -> None:
        """A motion or button press reached the app's window: mouse mode, with a hover where the pointer is."""
        self.keyboard_mode, self.hover_pending, self.keyboard_mode_lost = False, True, False

    def note_motion(self, point: tuple[int, int], why: str, clears: bool = True) -> None:
        """Record a motion a park sent, and what it did to the app's mode; outside a park, nothing.

        A motion onto the window puts the app in mouse mode; with `clears` it
        lands where nothing hovers, so no hover is pending after it.
        """
        if self.park_motions is None:
            return
        x0, y0 = self.park_rect[:2]
        on = inside(self.park_rect, *point)
        self.park_motions.append(dict(root=list(point), window=[point[0] - x0, point[1] - y0], inside=on, why=why))
        self.record(f"park motion to root ({point[0]},{point[1]}), window ({point[0] - x0},{point[1] - y0}), "
                    f"{'INSIDE' if on else 'off'} the window", why)
        if on:
            if self.keyboard_mode:
                self.keyboard_mode_lost = True
                self.record("WARNING: that motion took the app out of keyboard mode")
            self.keyboard_mode, self.hover_pending = False, not clears

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

    def rect(self) -> tuple[int, int, int, int]:
        """The window's root rectangle, (x0, y0, x1, y1) with exclusive ends."""
        ox, oy = self.origin()
        width, height = self.size()
        return ox, oy, ox + width, oy + height

    def root_size(self) -> tuple[int, int]:
        root = self.d.screen().root.get_geometry()
        return root.width, root.height

    # ---------- pointer ----------
    def pointer(self) -> tuple[int, int]:
        """The pointer's root position as the X server reports it."""
        q = self.d.screen().root.query_pointer()
        return q.root_x, q.root_y

    def where(self) -> tuple[int, int] | None:
        """The pointer's root position, None when unknown; XTest moves the X server's own pointer, so it knows."""
        return self.pointer()

    def locate(self, rect, root) -> tuple[int, int]:
        """Find an unknown pointer position for a park; XTest's is always known (`where`)."""
        raise SystemExit("refusing: the pointer's position is unknown; nothing sent")

    def park_motion(self, point: tuple[int, int], why: str) -> None:
        """One motion of a park to root `point`, recorded by `note_motion`."""
        xtest.fake_input(self.d, X.MotionNotify, x=point[0], y=point[1])
        self.d.sync()
        self.note_motion(point, why)

    def move(self, x: int, y: int, note: str | None = None) -> None:
        """Warp the pointer to window point (x, y). The mode follows what the X server did, read back from it: a
        pointer already there gets nothing sent, and only a motion that moved it and ended on the window puts
        the app in mouse mode."""
        ox, oy = self.origin()
        before = self.pointer()
        if before == (ox + x, oy + y):
            self.record(f"move ({x},{y})", "; ".join(filter(None, [note, "already there: nothing sent"])))
            return
        xtest.fake_input(self.d, X.MotionNotify, x=ox + x, y=oy + y)
        self.d.sync()
        after = self.pointer()
        if after != before and inside(self.rect(), *after):
            self.pointer_input()
        self.record(f"move ({x},{y})", note)

    def glide(self, x: int, y: int, note: str | None = None, settle: float = 0.8) -> None:
        """Two-step approach: a move to where the pointer already is emits no motion event."""
        self.move(x - 40, y - 30)
        time.sleep(0.2)
        self.move(x, y, note)
        time.sleep(settle)

    def park(self, settle: float = 0.8) -> dict:
        """Take the pointer off the window, leaving no hover and the app's input mode as it was (`plan_park`).

        Returns what it did: the start (None when unknown), the mode and
        pending hover it found, every motion it sent with its root and window
        point and whether it was on the window, `entered_window`, where the
        pointer ended and whether that is off the window, and
        `keyboard_mode_lost`. Each motion is also an input-log line. A park
        that sends nothing still waits `settle`, since a capture without
        `stable_within` relies on that wait after the input before it.
        """
        rect, root = self.rect(), self.root_size()
        keyboard, hover = self.keyboard_mode, self.hover_pending
        self.park_motions, self.park_rect = [], rect
        try:
            start = known = self.where()
            if known is None:
                known = self.locate(rect, root)
            at = known
            for point, why in plan_park(rect, root, known, self.hover_pending):
                if point == at:  # a motion there would move nothing, so none is sent and the app sees none
                    self.record(f"park: the pointer already rests at root ({point[0]},{point[1]}); nothing sent", why)
                    if inside(rect, *point):
                        self.hover_pending = False  # its hover is the margin's, where nothing hovers
                    continue
                if self.park_motions:
                    time.sleep(0.2)  # the app handles the motion onto the window before the pointer leaves
                self.park_motion(point, why)
                at = point
            motions = self.park_motions
        finally:
            self.park_motions = None
        at = tuple(motions[-1]["root"]) if motions else known
        off = not inside(rect, *at)
        if not motions:
            self.record(f"park: nothing sent; the pointer is already off the window at root ({at[0]},{at[1]})" if off
                        else f"park: nothing sent; no point is off the window {list(rect)} on the {root[0]}x{root[1]} "
                             f"screen, so the pointer stays at root ({at[0]},{at[1]})")
        else:
            self.record(f"park {'off-window' if off else 'ON the window'} at root ({at[0]},{at[1]})")
        time.sleep(settle)
        return dict(start=None if start is None else list(start), keyboard_mode=keyboard, hover_pending=hover,
                    motions=motions, entered_window=any(m["inside"] for m in motions), at=list(at), off_window=off,
                    keyboard_mode_lost=self.keyboard_mode_lost)

    def button(self, down: bool, number: int = 1, note: str | None = None) -> None:
        xtest.fake_input(self.d, X.ButtonPress if down else X.ButtonRelease, number)
        self.d.sync()
        if down and inside(self.rect(), *self.pointer()):
            self.pointer_input()
        self.record(f"{'press' if down else 'release'} button {number}", note)

    def on_window(self, x: int, y: int) -> bool:
        """True while the X server reports the pointer at window point (x, y) on this window."""
        q = self.w.query_pointer()
        width, height = self.size()
        return bool(q.same_screen) and (q.win_x, q.win_y) == (x, y) and 0 <= x < width and 0 <= y < height

    def aim(self, x: int, y: int, note: str | None = None, settle: float = 0.8) -> None:
        """Bring the pointer to (x, y) for a button sent next; XTest delivers it wherever the pointer is."""
        self.glide(x, y, note, settle=settle)

    def click(self, x: int, y: int, note: str | None = None) -> None:
        self.aim(x, y, note, settle=0.2)
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
        self.keys_sent(XK.string_to_keysym(name))
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
            self.keys_sent(keysym)
            time.sleep(per)
        self.record(f"type {text!r}", note)

    # ---------- frames ----------
    def snap(self):
        width, height = self.size()
        return image_from(self.grab((0, 0, width, height)), (width, height))

    def grab(self, box) -> bytes:
        """The raw BGRX bytes of `box` (window pixels, exclusive ends): one GetImage and no geometry query, so a
        probe can grab back to back and compare bytes; `image_from` turns them into a frame."""
        x0, y0, x1, y1 = box
        return self.w.get_image(x0, y0, x1 - x0, y1 - y0, X.ZPixmap, 0xFFFFFFFF).data

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
