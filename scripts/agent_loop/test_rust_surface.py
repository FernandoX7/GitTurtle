"""The render-surface test that decides whether app Rust needs native evidence."""

from __future__ import annotations

import unittest

from agent_loop.rust_surface import renders, scrub, strip_test_items, surface


CONSTANTS = """\
//! Upstream palette values.

/// Solarized, read from altercation/solarized at 62f656a.
pub mod solarized {
    pub const BASE03: u32 = 0x002b36;
    pub const BASE02: u32 = 0x073642;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_constant_is_a_six_digit_value() {
        for value in [solarized::BASE03, solarized::BASE02] {
            assert!(value <= 0xffffff, "{value:#08x} is out of range");
        }
    }
}
"""

VIEW = """\
use gpui::{div, IntoElement};

pub fn header() -> impl IntoElement {
    div().child("History")
}
"""


class InertChanges(unittest.TestCase):
    def test_a_new_constant_module_adds_no_render_surface(self):
        self.assertFalse(renders("", CONSTANTS))

    def test_declaring_the_module_behind_a_lint_attribute_is_inert(self):
        before = "pub const DEFAULT: u8 = 13;\n"
        after = "#[cfg_attr(not(test), allow(dead_code))]\nmod sources;\n\npub const DEFAULT: u8 = 13;\n"
        self.assertFalse(renders(before, after))

    def test_comments_alone_never_require_evidence(self):
        after = VIEW.replace("pub fn header", "// The history header.\npub fn header")
        self.assertFalse(renders(VIEW, after))

    def test_adding_a_test_to_an_existing_test_module_is_inert(self):
        after = CONSTANTS.replace(
            "    #[test]\n    fn every_constant",
            "    #[test]\n    fn values_are_distinct() {\n"
            "        assert_ne!(solarized::BASE03, solarized::BASE02);\n    }\n\n"
            "    #[test]\n    fn every_constant",
        )
        self.assertFalse(renders(CONSTANTS, after))

    def test_a_cfg_test_statement_inside_a_function_is_inert(self):
        before = "pub fn save() {\n    write();\n}\n"
        after = "pub fn save() {\n    #[cfg(test)]\n    let directory = test_directory();\n    write();\n}\n"
        self.assertFalse(renders(before, after))

    def test_adding_a_grouped_import_is_inert(self):
        after = "use std::sync::{Arc, Mutex};\n" + CONSTANTS
        self.assertFalse(renders(CONSTANTS, after))


class RenderingChanges(unittest.TestCase):
    def test_changing_a_constant_keeps_the_native_profile(self):
        # The line looks inert, but retuning a value the UI reads repaints every
        # screen, and the edit removes the old line.
        after = CONSTANTS.replace("0x002b36", "0x002b37")
        self.assertTrue(renders(CONSTANTS, after))

    def test_adding_a_function_keeps_the_native_profile(self):
        after = CONSTANTS + "\npub fn canvas() -> u32 {\n    solarized::BASE03\n}\n"
        self.assertTrue(renders(CONSTANTS, after))

    def test_adding_a_configuration_attribute_keeps_the_native_profile(self):
        # Unlike a lint attribute, this one decides what is compiled in.
        after = VIEW.replace("pub fn header", '#[cfg(target_os = "macos")]\npub fn header')
        self.assertTrue(renders(VIEW, after))

    def test_a_glob_import_keeps_the_native_profile(self):
        self.assertTrue(renders(CONSTANTS, "use crate::theme::*;\n" + CONSTANTS))

    def test_deleting_production_code_keeps_the_native_profile(self):
        self.assertTrue(renders(VIEW, "use gpui::{div, IntoElement};\n"))

    def test_a_brace_in_a_test_string_cannot_hide_a_later_change(self):
        before = '#[cfg(test)]\nmod tests {\n    fn helper() {\n        let brace = "{";\n    }\n}\n' + VIEW
        after = before.replace('div().child("History")', 'div().child("Commits")')
        self.assertTrue(renders(before, after))

    def test_an_unterminated_test_item_keeps_its_lines(self):
        # The extent cannot be determined, so the text stays in the comparison.
        broken = "#[cfg(test)]\nmod tests {\n    fn helper() {\n"
        self.assertIn("mod tests {", strip_test_items(scrub(broken)))
        self.assertTrue(renders("", broken))


class Scrubbing(unittest.TestCase):
    def test_literals_and_comments_lose_their_contents_but_keep_their_lines(self):
        source = 'let a = "one\\ntwo";\n// note {\nlet b = r#"raw " }"#;\nlet c = \'}\';\n'
        scrubbed = scrub(source)
        self.assertEqual(len(scrubbed.splitlines()), len(source.splitlines()))
        self.assertNotIn("}", scrubbed)
        self.assertNotIn("note", scrubbed)
        self.assertIn("let a =", scrubbed)

    def test_a_lifetime_is_not_mistaken_for_a_character_literal(self):
        self.assertIn("&'a str", scrub("pub fn name<'a>(value: &'a str) -> &'a str { value }\n"))

    def test_the_surface_drops_test_items_and_keeps_declarations(self):
        lines = [line.strip() for line in surface(CONSTANTS) if line.strip()]
        self.assertIn("pub const BASE03: u32 = 0x002b36;", lines)
        self.assertNotIn("mod tests {", lines)
        self.assertFalse(any("assert" in line for line in lines))


if __name__ == "__main__":
    unittest.main()
