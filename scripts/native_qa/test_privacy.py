from __future__ import annotations

import importlib.util
import os
import random
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from native_qa import privacy

HAVE_PIL = importlib.util.find_spec("PIL") is not None
HAVE_CC = any(shutil.which(name) for name in ("cc", "gcc", "clang"))
REPO = Path(__file__).resolve().parents[2]
QA = Path(__file__).resolve().parent / "qa.py"


def synthetic(width: int = 90, height: int = 40, at: tuple[int, int] = (30, 12), invert: bool = False):
    """A flat frame with a seeded noise patch standing in for a rendered string, and that patch."""
    rng = random.Random(7)
    tw, th = 16, 8
    patch = bytes(rng.randrange(256) for _ in range(tw * th))
    frame = bytearray([40] * (width * height))
    for y in range(th):
        for x in range(tw):
            value = patch[y * tw + x]
            frame[(at[1] + y) * width + at[0] + x] = 255 - value if invert else value
    return bytes(frame), width, height, (patch, tw, th)


class EngineTest(unittest.TestCase):
    def test_python_engine_finds_the_patch(self) -> None:
        frame, width, height, template = synthetic()
        result = privacy.ncc_python(frame, width, height, *template)
        self.assertEqual((result["best"], result["at"], result["sign"], result["hits"]), (1.0, [30, 12], "+", 1))
        self.assertEqual(result["hit_list"], [[30, 12, 1.0]])

    def test_inverted_rendering_matches_with_a_negative_sign(self) -> None:
        frame, width, height, template = synthetic(invert=True)
        result = privacy.ncc_python(frame, width, height, *template)
        self.assertEqual((result["best"], result["at"], result["sign"], result["hits"]), (1.0, [30, 12], "-", 1))

    def test_flat_frame_and_flat_template_never_match(self) -> None:
        _, width, height, template = synthetic()
        flat = bytes([40] * (width * height))
        self.assertEqual(privacy.ncc_python(flat, width, height, *template)["hits"], 0)
        frame, *_ = synthetic()
        blank = (bytes([9] * 128), 16, 8)
        self.assertEqual(privacy.ncc_python(frame, width, height, *blank)["best"], 0.0)

    def test_parse_ncc_output(self) -> None:
        text = "tpl 0 best 0.9912 30 12 sign + hits 1\nhit 30 12 0.9912\ntpl 1 best 0.2100 4 5 sign - hits 0\n"
        parsed = privacy.parse_ncc_output(text, ["a", "b"])
        self.assertEqual(parsed["a"], dict(best=0.9912, at=[30, 12], sign="+", hits=1, hit_list=[[30, 12, 0.9912]]))
        self.assertEqual(parsed["b"]["hits"], 0)
        with self.assertRaises(RuntimeError):
            privacy.parse_ncc_output(text, ["a", "b", "c"])

    @unittest.skipUnless(HAVE_CC, "no C compiler")
    def test_c_helper_agrees_with_python_and_builds_outside_the_repository(self) -> None:
        with tempfile.TemporaryDirectory() as cache, mock.patch.dict(os.environ, {"XDG_CACHE_HOME": cache}):
            binary = privacy.helper()
            self.assertTrue(binary.is_relative_to(Path(cache)))
            self.assertEqual(privacy.helper(build=False), binary)  # cached per source digest
            for invert in (False, True):
                frame, width, height, template = synthetic(invert=invert)
                ours = privacy.ncc_python(frame, width, height, *template)
                theirs = privacy.ncc_c(binary, frame, width, height, {"t": template}, privacy.THRESHOLD)["t"]
                self.assertEqual((theirs["at"], theirs["sign"], theirs["hits"]), (ours["at"], ours["sign"], ours["hits"]))
                self.assertAlmostEqual(theirs["best"], ours["best"], places=3)


class TrackedPathTest(unittest.TestCase):
    def test_templates_inside_the_work_tree_must_be_ignored(self) -> None:
        if not (REPO / ".git").exists():
            self.skipTest("not a Git work tree")
        with self.assertRaises(SystemExit):
            privacy.refuse_tracked(REPO / "scripts" / "native_qa" / "templates", "template directory")
        privacy.refuse_tracked(REPO / ".local" / "privacy-templates", "template directory")  # ignored
        with tempfile.TemporaryDirectory() as scratch:
            privacy.refuse_tracked(Path(scratch) / "tpl", "template directory")  # outside any work tree


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ScanTest(unittest.TestCase):
    def test_scan_and_cli_on_synthetic_frames(self) -> None:
        from PIL import Image

        frame, width, height, (patch, tw, th) = synthetic()
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "tpl").mkdir()
            Image.frombytes("L", (tw, th), patch).save(root / "tpl" / "name.png")
            Image.frombytes("L", (width, height), frame).convert("RGB").save(root / "leaky.png")
            Image.new("RGB", (width, height), (40, 40, 40)).save(root / "clean.png")
            templates = privacy.load_templates(root / "tpl")
            report = privacy.scan([root / "leaky.png", root / "clean.png"], templates, engine="python")
            self.assertEqual(report[str(root / "leaky.png")]["templates"]["name"]["at"], [30, 12])
            self.assertEqual((report[str(root / "leaky.png")]["hits"], report[str(root / "clean.png")]["hits"]), (1, 0))

            run = [sys.executable, "-B", str(QA), "privacy", "scan", "--templates", str(root / "tpl"), "--engine", "python"]
            leaky = subprocess.run(run + [str(root / "leaky.png")], capture_output=True, text=True)
            self.assertEqual(leaky.returncode, 1, leaky.stderr)
            self.assertIn("HIT name at (30,12)", leaky.stdout)
            clean = subprocess.run(run + [str(root / "clean.png")], capture_output=True, text=True)
            self.assertEqual(clean.returncode, 0, clean.stderr)

            privacy.crop(root / "leaky.png", (30, 12, 30 + tw, 12 + th), root / "cut" / "again.png")
            with Image.open(root / "cut" / "again.png") as cut:
                self.assertEqual(cut.tobytes(), patch)


if __name__ == "__main__":
    unittest.main()
