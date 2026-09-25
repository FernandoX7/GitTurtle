from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from native_qa import frames

HAVE_PIL = importlib.util.find_spec("PIL") is not None
QA = Path(__file__).resolve().parent / "qa.py"


class MaskTest(unittest.TestCase):
    def test_parse_mask(self) -> None:
        self.assertEqual(frames.parse_mask("status-timing"), "status-timing")
        self.assertEqual(frames.parse_mask("1,2,30,40"), (1, 2, 30, 40))
        for bad in ("1,2,3", "5,5,5,9", "0,9,4,2", "-1,0,4,4", "a,b,c,d", "timing"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                frames.parse_mask(bad)

    def test_status_timing_covers_the_observed_digits(self) -> None:
        # Round-3 IE-D3 differences between launches of one build at 1000x680.
        x0, y0, x1, y1 = frames.status_timing(1000, 680)
        for box in ((128, 663, 133, 672), (136, 663, 141, 672), (156, 663, 161, 671), (164, 663, 169, 672)):
            self.assertTrue(x0 <= box[0] and y0 <= box[1] and box[2] <= x1 and box[3] <= y1, box)
        self.assertLess(y1, 680)
        self.assertGreater(y0, 680 - 26)  # stays inside the 26 px status bar


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class CompareTest(unittest.TestCase):
    def setUp(self) -> None:
        from PIL import Image

        self.Image = Image
        self.base = Image.new("RGB", (200, 120), (19, 26, 37))

    def changed(self, *boxes, colour=(200, 200, 200)):
        image = self.base.copy()
        for box in boxes:
            image.paste(colour, box)
        return image

    def test_identical(self) -> None:
        report = frames.compare(self.base, self.base.copy())
        self.assertEqual((report["result"], report["differing_pixels"], report["masked_pixels"]), ("identical", 0, 0))

    def test_masked_difference_is_counted_but_not_a_failure(self) -> None:
        candidate = self.changed((10, 100, 20, 110))
        report = frames.compare(self.base, candidate, [(0, 95, 50, 115)])
        self.assertEqual(report["result"], "identical")
        self.assertEqual(report["masked_pixels"], 100)
        self.assertEqual(report["regions"], [])

    def test_difference_outside_masks_reports_regions(self) -> None:
        candidate = self.changed((10, 100, 20, 110), (150, 10, 153, 12), (157, 10, 160, 12), colour=(19, 26, 38))
        report = frames.compare(self.base, candidate, [(0, 95, 50, 115)])
        self.assertEqual(report["result"], "different")
        self.assertEqual(report["differing_pixels"], 12)  # a one-unit change in one channel still counts
        self.assertEqual(report["regions"], [[150, 10, 160, 12]])  # 4 px apart: one region
        self.assertEqual(report["bbox"], [150, 10, 160, 12])

    def test_named_mask_resolves_against_the_frame(self) -> None:
        base = self.Image.new("RGB", (1000, 680))
        candidate = base.copy()
        candidate.paste((255, 255, 255), (128, 663, 170, 672))
        self.assertEqual(frames.compare(base, candidate, ["status-timing"])["result"], "identical")
        self.assertEqual(frames.compare(base, candidate)["result"], "different")

    def test_size_mismatch(self) -> None:
        report = frames.compare(self.base, self.Image.new("RGB", (201, 120)))
        self.assertEqual(report["result"], "size-mismatch")

    def test_cli_exit_status_and_directories(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "base").mkdir()
            (root / "cand").mkdir()
            self.base.save(root / "base" / "a.png")
            self.base.save(root / "cand" / "a.png")
            self.changed((150, 10, 160, 20)).save(root / "base" / "b.png")
            self.base.save(root / "cand" / "b.png")
            run = [sys.executable, "-B", str(QA), "compare"]
            same = subprocess.run(run + [str(root / "base" / "a.png"), str(root / "cand" / "a.png")],
                                  capture_output=True, text=True)
            self.assertEqual(same.returncode, 0, same.stderr)
            both = subprocess.run(run + [str(root / "base"), str(root / "cand"), "--json", str(root / "r.json")],
                                  capture_output=True, text=True)
            self.assertEqual(both.returncode, 1, both.stderr)
            self.assertIn("b.png: different, 100 px outside masks", both.stdout)
            masked = subprocess.run(run + [str(root / "base"), str(root / "cand"), "--mask", "150,10,160,20"],
                                    capture_output=True, text=True)
            self.assertEqual(masked.returncode, 0, masked.stdout)
            self.assertEqual([r["result"] for r in json.loads((root / "r.json").read_text())], ["identical", "different"])
            bad = subprocess.run(run + [str(root / "base"), str(root / "cand"), "--mask", "1,2"],
                                 capture_output=True, text=True)
            self.assertEqual(bad.returncode, 2)


if __name__ == "__main__":
    unittest.main()
