"""The launched app's accessibility tree over AT-SPI: turn it on for a launch, then read its focused node.

AccessKit registers the app on the accessibility bus only while
`org.a11y.Status IsEnabled` is true, and GNOME keeps that property in the
user's settings (`org.gnome.desktop.interface toolkit-accessibility`), so
setting it outlives the run unless it is put back. `enabled` sets it for one
launch and always restores the value it found and reads it back: after a
failure, an exception or a SIGTERM to the tool too, which meanwhile only
marks the launch to stop, so no signal can skip the restore.

`read_focus` finds the app's AT-SPI application by the launched PID, never by
name, and returns the node that holds focus, with its role and states. It
reads through `portal`'s AT-SPI access (PyGObject's Atspi); the status goes
through dbus-python on the session bus, as `mutter` does.
"""

from __future__ import annotations

import contextlib
import signal
import sys
import threading
import time

from . import portal, runenv

BUS_NAME = "org.a11y.Bus"
OBJECT_PATH = "/org/a11y/bus"
STATUS = "org.a11y.Status"
PROPERTIES = "org.freedesktop.DBus.Properties"
# The states a focused-node reading reports, AT-SPI's StateType names in lower case. AccessKit reports a
# Switch as a "toggle button" that is `pressed` while on, a Checkbox `checked`, and focus as `focused`.
STATES = ("focused", "focusable", "enabled", "sensitive", "pressed", "checked", "checkable", "indeterminate",
          "selected", "selectable", "expanded", "editable", "read_only", "showing", "visible", "active", "modal")
DEPTH = 64       # levels searched below the application
NODES = 5000     # nodes read per search at most, so a huge tree cannot stall a launch
WITHIN = 5.0     # seconds an `atspi_focus` step waits for the application and a focused node
POLL = 0.25


# ---------- org.a11y.Status IsEnabled ----------
class Status:
    """`org.a11y.Status` on the session bus; `types` supplies dbus-python's argument types."""

    def __init__(self, proxy, types) -> None:
        self.proxy, self.types = proxy, types

    @classmethod
    def connect(cls) -> "Status":
        import dbus

        return cls(dbus.SessionBus().get_object(BUS_NAME, OBJECT_PATH), dbus)

    def read(self) -> bool:
        return bool(self.proxy.Get(STATUS, "IsEnabled", dbus_interface=PROPERTIES))

    def write(self, value: bool) -> None:
        self.proxy.Set(STATUS, "IsEnabled", self.types.Boolean(value, variant_level=1), dbus_interface=PROPERTIES)


def require_modules() -> None:
    """Refuse, before IsEnabled is touched or anything is launched, when PyGObject's Atspi or dbus-python is
    missing: a tool that cannot read AT-SPI must not grade a build."""
    missing = []
    try:
        portal._atspi()
    except (ImportError, ValueError) as error:  # gi.require_version raises ValueError without the Atspi typelib
        missing.append(f"gi with Atspi 2.0 (PyGObject; {error})")
    try:
        import dbus  # noqa: F401
    except ImportError as error:
        missing.append(f"dbus (dbus-python; {error})")
    if missing:
        raise runenv.Refusal(f"\"atspi\": true needs {' and '.join(missing)}; nothing launched and "
                             "org.a11y.Status IsEnabled unchanged")


def note_sigterm(record: dict):
    """Make SIGTERM only set `record["sigterm"]` (main thread only); the previous handler, or None."""
    if threading.current_thread() is not threading.main_thread():
        return None

    def note(signum, frame):
        record["sigterm"] = True

    return signal.signal(signal.SIGTERM, note)


@contextlib.contextmanager
def enabled(record: dict, status: Status | None = None):
    """`IsEnabled` true for the block; then the value it had, read back. `record` gets original, set and restored.

    Refuses, with nothing changed, when the property cannot be read, and
    refuses the block when setting it does not read back true. From just
    before the set until the restored value has been read back, SIGTERM to
    the tool raises nothing: it only sets `record["sigterm"]`, which the
    caller checks to stop the launch (`play.launch` does, between steps). So
    no signal can land between the end of the block and the restore, and the
    restore always runs to the end. A restore that fails or reads back another
    value is printed and kept in `record["restore_error"]`.
    """
    try:
        status = status or Status.connect()
        original = status.read()
    except Exception as error:
        raise runenv.Refusal(f"org.a11y.Status IsEnabled cannot be read ({error}); nothing changed") from None
    record.update(original=original, set=None, restored=None)
    previous = note_sigterm(record)  # before anything changes, so the default action can never skip the restore
    try:
        try:
            status.write(True)
            record["set"] = status.read()
        except Exception as error:
            raise runenv.Refusal(f"org.a11y.Status IsEnabled could not be set ({error})") from None
        if record["set"] is not True:
            raise runenv.Refusal(f"org.a11y.Status IsEnabled read back {record['set']} after setting it true")
        print(f"org.a11y.Status IsEnabled: {original} -> true for the launch", flush=True)
        yield record
    finally:
        problem = None
        try:
            status.write(original)
        except Exception as error:
            problem = f"could not write it back ({error})"
        try:
            record["restored"] = status.read()
        except Exception as error:
            problem = problem or f"could not read it back ({error})"
        if problem is None and record["restored"] != original:
            problem = f"read back {record['restored']} after restoring {original}"
        if problem is not None:
            record["restore_error"] = (
                f"org.a11y.Status IsEnabled {problem}; restore it by hand: gdbus call --session --dest org.a11y.Bus "
                f"--object-path /org/a11y/bus --method org.freedesktop.DBus.Properties.Set org.a11y.Status IsEnabled "
                f"'<{'true' if original else 'false'}>'")
            print(f"error: {record['restore_error']}", file=sys.stderr, flush=True)
        else:
            print(f"org.a11y.Status IsEnabled restored to {original} and read back", flush=True)
        if previous is not None:
            signal.signal(signal.SIGTERM, previous)


# ---------- the focused node ----------
def application_for(pid: int):
    """The AT-SPI application whose bus connection belongs to `pid`, or None."""
    desktop = portal._atspi().get_desktop(0)
    for index in range(desktop.get_child_count()):
        try:
            app = desktop.get_child_at_index(index)
            if app is not None and app.get_process_id() == pid:
                return app
        except Exception:
            continue
    return None


def describe(node, depth: int) -> dict:
    return dict(name=node.get_name(), role=node.get_role_name(), states=portal.states_of(node, STATES),
                depth=depth)


def focused_nodes(app) -> tuple[list[dict], int, bool]:
    """Every node holding `focused`, in tree order; the nodes read; and whether the search stopped at NODES."""
    found, count, stack = [], 0, [(app, 0)]
    while stack:
        node, depth = stack.pop()
        if count >= NODES:
            return found, count, True
        count += 1
        try:
            if "focused" in portal.states_of(node, ("focused",)):
                found.append(describe(node, depth))
            children = [node.get_child_at_index(i) for i in range(node.get_child_count())] if depth < DEPTH else []
        except Exception:
            continue
        stack.extend((child, depth + 1) for child in reversed(children) if child is not None)
    return found, count, False


def read_focus(pid: int, within: float = WITHIN, poll: float = POLL, clock=time.monotonic, sleep=time.sleep) -> dict:
    """The focused node of `pid`'s application: the deepest node holding `focused` (AccessKit also reports the
    window when nothing in it has focus), with every focused node, waiting up to `within` seconds for the
    application to register and report one. A reading whose application never registered has an `error`."""
    portal.init()
    start = clock()
    app, found, count, truncated = None, [], 0, False
    while True:
        app = application_for(pid)
        if app is not None:
            with contextlib.suppress(Exception):
                app.clear_cache()  # every read asks the app again, even if libatspi cached an earlier answer
            found, count, truncated = focused_nodes(app)
        if found or clock() - start >= within:
            break
        sleep(poll)
    reading = dict(pid=pid, seconds=round(clock() - start, 3), application=None, nodes=count, truncated=truncated,
                   focused=max(found, key=lambda node: node["depth"]) if found else None, all_focused=found)
    if app is None:
        reading["error"] = (f"no AT-SPI application with pid {pid} within {within} s; is org.a11y.Status "
                            "IsEnabled true for the launch?")
    else:
        try:
            reading["application"] = app.get_name()
        except Exception:
            reading["application"] = ""
    return reading
