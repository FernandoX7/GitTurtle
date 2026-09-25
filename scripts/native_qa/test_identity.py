from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from native_qa import identity

INFO = {"application": "GitTurtle", "version": "0.1.0", "source_revision": "a" * 40, "source_tree": "clean",
        "target": "x86_64-unknown-linux-gnu", "profile": "debug", "rustc": "rustc 1.98.0",
        "build_unix_seconds": "1790261796"}


def entry(path: str, sha: str, **info) -> dict:
    return dict(path=path, sha256=sha, build_info={**INFO, **info})


class IdentityTest(unittest.TestCase):
    def test_parse_build_info(self) -> None:
        self.assertEqual(identity.parse_build_info(json.dumps(INFO, indent=2)), INFO)
        with self.assertRaisesRegex(ValueError, "did not print JSON"):
            identity.parse_build_info("GitTurtle 0.1.0 (abc; clean)")
        with self.assertRaisesRegex(ValueError, "source_tree"):
            identity.parse_build_info(json.dumps({k: v for k, v in INFO.items() if k != "source_tree"}))

    def test_single_binary_flags_a_dirty_or_unknown_tree(self) -> None:
        self.assertEqual(identity.problems([entry("b", "1")], pair=False), [])
        found = identity.problems([entry("b", "1", source_tree="dirty")], pair=False)
        self.assertEqual(len(found), 1)
        self.assertIn("not 'clean'", found[0])
        self.assertEqual(identity.problems([entry("b", "1", source_tree="dirty")], pair=False, allow_dirty=True), [])
        self.assertTrue(identity.problems([entry("b", "1", source_revision="unknown")], pair=False))

    def test_pair_refuses_a_no_op_candidate(self) -> None:
        base = entry("base", "1" * 64, source_revision="a" * 40)
        good = entry("cand", "2" * 64, source_revision="b" * 40)
        self.assertEqual(identity.problems([base, good], pair=True), [])
        same_bytes = entry("cand", "1" * 64, source_revision="b" * 40)
        self.assertTrue(any("same sha256" in p for p in identity.problems([base, same_bytes], pair=True)))
        same_source = entry("cand", "2" * 64, source_revision="a" * 40)
        self.assertTrue(any("same source_revision" in p for p in identity.problems([base, same_source], pair=True)))

    def test_describe_runs_build_info_without_a_display_or_real_store(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            binary = Path(scratch) / "gitturtle"
            binary.write_text("#!/bin/sh\n"
                              "[ \"$1\" = --build-info ] || exit 3\n"
                              f"printf '%s' '{json.dumps(INFO)[:-1]}'\n"
                              "printf ', \"seen\": \"%s|%s|%s\"}' \"$DISPLAY\" \"$WAYLAND_DISPLAY\" \"$XDG_CONFIG_HOME\"\n")
            binary.chmod(0o755)
            with mock.patch.dict(os.environ, {"DISPLAY": ":1", "WAYLAND_DISPLAY": "wayland-0",
                                              "XDG_CONFIG_HOME": "/nowhere"}):
                described = identity.describe(binary)
            self.assertEqual(described["sha256"], identity.sha256_file(binary))
            display, wayland, config = described["build_info"]["seen"].split("|")
            self.assertEqual((display, wayland), ("", ""))
            self.assertNotEqual(config, "/nowhere")
            with self.assertRaises(SystemExit):
                identity.describe(Path(scratch) / "missing")


if __name__ == "__main__":
    unittest.main()
