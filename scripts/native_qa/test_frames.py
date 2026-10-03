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
        # Differences between launches of one build at 1000x680 (IE-D3 round 3, native-QA phase 2).
        x0, y0, x1, y1 = frames.status_timing(1000, 680)
        for box in ((128, 663, 133, 672), (136, 663, 141, 672), (156, 663, 161, 671), (164, 663, 169, 672),
                    (127, 663, 141, 672)):
            self.assertTrue(x0 <= box[0] and y0 <= box[1] and box[2] <= x1 and box[3] <= y1, box)
        self.assertLess(y1, 680)
        self.assertGreater(y0, 680 - 26)  # stays inside the 26 px status bar
        self.assertLessEqual((x1 - x0) * (y1 - y0), 1500)  # the text line's timing, not the whole bar
        self.assertEqual(frames.status_timing(1440, 900), (96, 882, 208, 893))


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


SURFACE = (16, 21, 31)
NEAREST = 0  # PIL.Image.Resampling.NEAREST, without importing Pillow where it is missing
LEFT, RIGHT = (1, 2, 9, 10), (19, 4, 25, 9)  # 8x8 and 6x5 boxes of glyph-like noise on the surface


def noisy_frame():
    """A 40x30 surface whose two boxes hold seeded noise inside a 1 px margin, so a misplaced pixel shows."""
    import random

    from PIL import Image

    rng = random.Random(5)
    image = Image.new("RGB", (40, 30), SURFACE)
    for x0, y0, x1, y1 in (LEFT, RIGHT):
        for y in range(y0 + 1, y1 - 1):
            for x in range(x0 + 1, x1 - 1):
                image.putpixel((x, y), (rng.randrange(256), rng.randrange(256), rng.randrange(256)))
    return image


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ComposeTest(unittest.TestCase):
    def expected_pixel(self, image, x, y, gap, fill):
        """The 1x composite's pixel at (x, y): the left box, the gap, the right box, or fill below the shorter box."""
        width = LEFT[2] - LEFT[0]
        if x < width:
            return image.getpixel((LEFT[0] + x, LEFT[1] + y))
        if x < width + gap or y >= RIGHT[3] - RIGHT[1]:
            return fill
        return image.getpixel((RIGHT[0] + x - width - gap, RIGHT[1] + y))

    def test_a_4x_composition_is_exact_blocks_side_by_side_top_aligned(self) -> None:
        image = noisy_frame()
        composed, fill = frames.compose(image, [LEFT, RIGHT], scale=4, gap=3)
        self.assertEqual(fill, SURFACE)  # the boxes' most frequent colour
        self.assertEqual(composed.size, frames.composed_size([LEFT, RIGHT], 4, 3))
        self.assertEqual(composed.size, ((8 + 3 + 6) * 4, 8 * 4))
        for y in range(composed.height):
            for x in range(composed.width):
                self.assertEqual(composed.getpixel((x, y)), self.expected_pixel(image, x // 4, y // 4, 3, SURFACE),
                                 (x, y))

    def test_the_fill_is_declared_sampled_or_the_boxes_surface(self) -> None:
        image = noisy_frame()
        image.putpixel((39, 29), (90, 90, 90))
        for fill, colour in (((1, 2, 3), (1, 2, 3)), ({"at": [39, 29]}, (90, 90, 90)), (None, SURFACE)):
            with self.subTest(fill=fill):
                composed, used = frames.compose(image, [LEFT, RIGHT], gap=2, fill=fill)
                self.assertEqual(used, colour)
                self.assertEqual(composed.size, (16, 8))
                self.assertEqual({composed.getpixel((x, y)) for x in (8, 9) for y in range(8)}, {colour})  # the gap
                self.assertEqual(composed.getpixel((12, 6)), colour)  # under the shorter box
                self.assertEqual(composed.crop((10, 0, 16, 5)).tobytes(), image.crop(RIGHT).tobytes())
        single, used = frames.compose(image, [LEFT], scale=3)
        self.assertIsNone(used)
        self.assertEqual(single.size, (24, 24))
        self.assertEqual(single.resize((8, 8), NEAREST).tobytes(), image.crop(LEFT).tobytes())

    def test_magnifications_recover_what_a_scaled_crop_magnifies_at_every_exact_factor(self) -> None:
        image = noisy_frame()
        plain, _ = frames.compose(image, [LEFT, RIGHT])  # 14x8
        for scale, exact in ((2, [2]), (3, [3]), (4, [2, 4]), (8, [2, 4, 8])):
            with self.subTest(scale=scale):
                magnified, _ = frames.compose(image, [LEFT, RIGHT], scale=scale)
                found = dict(frames.magnifications(magnified))
                self.assertEqual(sorted(found), exact)
                self.assertEqual(found[scale].tobytes(), plain.tobytes())
                self.assertEqual([f for f, _ in frames.magnifications(magnified.convert("L"))], exact)
        # A 4x crop of content that is itself 2x2-blocky is an exact 8x magnification as well; its 1/4 reading is
        # the content at its own size, which the largest factor alone would miss.
        blocky = image.resize((80, 60), NEAREST)
        content, _ = frames.compose(blocky, [(2, 4, 18, 20)])
        crop, _ = frames.compose(blocky, [(2, 4, 18, 20)], scale=4)
        found = dict(frames.magnifications(crop))
        self.assertEqual(sorted(found), [2, 4, 8])
        self.assertEqual(found[4].tobytes(), content.tobytes())
        self.assertEqual(frames.magnifications(image), [])
        self.assertEqual(frames.magnifications(image.crop((0, 0, 20, 20)).resize((20, 40), NEAREST)), [])


if __name__ == "__main__":
    unittest.main()
