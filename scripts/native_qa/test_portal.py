from __future__ import annotations

import unittest
from types import SimpleNamespace
from unittest import mock

from native_qa import mutter, portal
from native_qa.test_mutter import TYPES, FakeSession, keysym


class Node:
    """An AT-SPI accessible: name, role, states and children."""

    def __init__(self, name: str = "", role: str = "panel", states=(), children=(), text: str | None = None) -> None:
        self.name, self.role, self.states, self.children, self.text = name, role, set(states), list(children), text

    def get_name(self) -> str:
        return self.name

    def get_role_name(self) -> str:
        return self.role

    def get_child_count(self) -> int:
        return len(self.children)

    def get_child_at_index(self, index: int):
        return self.children[index]

    def get_state_set(self):
        return SimpleNamespace(contains=lambda state: state in self.states)


class FakeText:
    @staticmethod
    def get_character_count(node):
        if node.text is None:
            raise RuntimeError("not a text")
        return len(node.text)

    @staticmethod
    def get_text(node, start, end):
        return node.text[start:end]


def fake_atspi(desktop: Node):
    return SimpleNamespace(get_desktop=lambda index: desktop, Text=FakeText,
                           StateType=SimpleNamespace(**{name.upper(): name for name in portal.STATES}))


def nested(leaf: Node, depth: int) -> Node:
    """`leaf` under `depth` panels, as Nautilus nests its dialog widgets."""
    for _ in range(depth):
        leaf = Node(children=[leaf])
    return leaf


class PortalTest(unittest.TestCase):
    def setUp(self) -> None:
        self.entry = Node("", "text", states=("focusable", "editable"), text="")
        self.open_file = Node("Open File", "dialog", children=[nested(self.entry, 20)])
        self.browser = Node("Home", "frame", children=[Node("", "text", states=("focused", "editable"), text="x")])
        self.nautilus = Node(portal.NAUTILUS, "application",
                             children=[self.browser, self.open_file, Node("Open File (copy)", "frame"),
                                       Node("open file", "frame")])
        self.other = Node("Google Chrome", "application", children=[Node("Open File", "dialog")])
        self.desktop = Node(children=[self.other, self.nautilus])
        patches = [mock.patch.object(portal, "_atspi", lambda: fake_atspi(self.desktop)),
                   mock.patch.object(mutter, "keysym", keysym), mock.patch("time.sleep")]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def test_nautilus_file_chooser_is_found_by_exact_title_only(self) -> None:
        self.assertEqual(portal.dialog_windows(), [(portal.NAUTILUS, self.open_file)])
        save = Node("Save File", "dialog")
        self.nautilus.children.append(save)
        self.assertEqual([w for _, w in portal.dialog_windows()], [self.open_file, save])
        (owner, window), _ = portal.wait_for_dialog(timeout=1)
        self.assertEqual((owner, window), (portal.NAUTILUS, self.open_file))

    def test_other_nautilus_windows_and_other_apps_are_ignored(self) -> None:
        self.nautilus.children.remove(self.open_file)
        self.assertEqual(portal.dialog_windows(), [])
        self.assertTrue(portal.wait_until_gone(timeout=1))

    def test_portal_backend_top_levels_are_still_found(self) -> None:
        dialog = Node("Export theme", "dialog")
        self.desktop.children.append(Node("xdg-desktop-portal-gnome", "application", children=[dialog]))
        self.assertIn(("xdg-desktop-portal-gnome", dialog), portal.dialog_windows())

    def test_dialog_keys_need_focus_inside_the_dialog(self) -> None:
        session = FakeSession()
        app_focused = mock.Mock(return_value=False)
        keyboard = portal.Keyboard(mutter.RemoteDesktop(session, TYPES), app_focused=app_focused)
        self.assertFalse(portal.dialog_has_focus(self.open_file))
        with self.assertRaises(portal.DialogFocusRefused):
            keyboard.key(self.open_file, "a", ["Control_L"])
        with self.assertRaises(portal.DialogFocusRefused):
            keyboard.type(self.open_file, "ab")
        self.assertEqual(session.calls, [])  # the focused entry of another Nautilus window does not count

        self.entry.states.add("focused")  # 21 levels deep: beyond find_nodes' default depth of 8
        self.assertTrue(portal.dialog_has_focus(self.open_file))
        keyboard.key(self.open_file, "a", ["Control_L"])
        self.assertEqual(len(session.calls), 4)

        app_focused.return_value = True  # X focus back on the app: Mutter keys would reach it
        with self.assertRaises(portal.DialogFocusRefused):
            keyboard.type(self.open_file, "b")
        self.assertEqual(len(session.calls), 4)

    def test_typed_text_is_read_back_from_the_deep_focused_entry(self) -> None:
        session = FakeSession()
        keyboard = portal.Keyboard(mutter.RemoteDesktop(session, TYPES))
        self.entry.states.add("focused")
        self.entry.text = "ab"
        self.assertEqual(keyboard.type_checked(self.open_file, "ab"), (True, "ab"))
        keyboard.stop()  # a borrowed session is left to its owner
        self.assertNotIn("Stop", [call[0] for call in session.calls])


if __name__ == "__main__":
    unittest.main()
