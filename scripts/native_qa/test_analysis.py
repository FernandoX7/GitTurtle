from __future__ import annotations

import importlib.util
import unittest

from native_qa import analysis, frames

HAVE_PIL = importlib.util.find_spec("PIL") is not None
SURFACE = (16, 21, 31)    # Midnight's list surface
RING = (117, 224, 187)    # Midnight's focus ring
BORDER = (60, 66, 80)     # a neutral row border
RECT = (40, 30, 160, 70)  # the ring's outer box in every synthetic frame


def ring_frame(width=2, skip=(), clip_top=0, double=False):
    """A 200x100 surface with a `width` px ring drawn inside RECT, minus the `skip` sides, its top `clip_top`
    rows painted over (a clip), and with `double` a second outline 1 px inside the ring, like a kit border."""
    from PIL import Image, ImageDraw

    image = Image.new("RGB", (200, 100), SURFACE)
    draw = ImageDraw.Draw(image)
    x0, y0, x1, y1 = RECT
    bands = dict(top=(x0, y0, x1 - 1, y0 + width - 1), bottom=(x0, y1 - width, x1 - 1, y1 - 1),
                 left=(x0, y0, x0 + width - 1, y1 - 1), right=(x1 - width, y0, x1 - 1, y1 - 1))
    for side, box in bands.items():
        if side not in skip:
            draw.rectangle(box, fill=RING)
    if double:
        draw.rectangle((x0 + width + 1, y0 + width + 1, x1 - width - 2, y1 - width - 2), outline=RING)
    if clip_top:
        draw.rectangle((0, 0, 199, y0 + clip_top - 1), fill=SURFACE)
    return image


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class RingTest(unittest.TestCase):
    def frame(self, *args, **kwargs):
        return ring_frame(*args, **kwargs)

    def test_a_whole_ring_passes_with_its_width_box_and_contrast(self) -> None:
        result = analysis.ring_sides(self.frame(), RECT, RING, min_contrast=3.0)
        self.assertTrue(result["passed"], result["reasons"])
        self.assertEqual(result["box"], list(RECT))
        for side in analysis.SIDES:
            entry = result["sides"][side]
            self.assertTrue(entry["continuous"], side)
            self.assertEqual((entry["width"], entry["width_max"], entry["outlines"]), (2, 2, 1), side)
            self.assertEqual(entry["surface"], list(SURFACE))
            self.assertEqual(entry["contrast"], frames.contrast(RING, SURFACE))
        self.assertGreater(result["sides"]["top"]["contrast"], 11)

    def test_the_ring_colour_is_detected_as_the_most_saturated_accent(self) -> None:
        self.assertEqual(analysis.detect_ring_colour(self.frame(), RECT), RING)
        result = analysis.ring_sides(self.frame(), RECT)
        self.assertTrue(result["detected"])
        self.assertTrue(result["passed"], result["reasons"])
        from PIL import Image

        plain = Image.new("RGB", (200, 100), SURFACE)
        self.assertIsNone(analysis.detect_ring_colour(plain, RECT))
        self.assertFalse(analysis.ring_sides(plain, RECT)["passed"])

    def test_a_ring_missing_one_side_fails_on_that_side(self) -> None:
        result = analysis.ring_sides(self.frame(skip=("right",)), RECT, RING)
        self.assertFalse(result["passed"])
        self.assertFalse(result["sides"]["right"]["continuous"])
        self.assertTrue(all(result["sides"][s]["continuous"] for s in ("top", "bottom", "left")))
        self.assertEqual(len(result["reasons"]), 1)
        self.assertTrue(result["reasons"][0].startswith("right: no ring"))
        self.assertIsNone(result["box"])
        # Checking only the other sides passes.
        self.assertTrue(analysis.ring_sides(self.frame(skip=("right",)), RECT, RING,
                                            sides=("top", "bottom", "left"))["passed"])

    def test_a_ring_clipped_by_one_pixel_fails_on_its_width(self) -> None:
        result = analysis.ring_sides(self.frame(clip_top=1), RECT, RING)
        self.assertFalse(result["passed"])
        self.assertEqual(result["sides"]["top"]["width"], 1)
        self.assertEqual(result["sides"]["top"]["outer"], RECT[1] + 1)
        self.assertIn("differs between sides", result["reasons"][0])
        self.assertTrue(analysis.ring_sides(self.frame(clip_top=1), RECT, RING, uniform_width=False)["passed"])
        self.assertFalse(analysis.ring_sides(self.frame(clip_top=1), RECT, RING, uniform_width=False,
                                             min_width=2)["passed"])

    def test_blended_pixels_where_a_side_meets_its_corner_keep_the_usual_width(self) -> None:
        # The 2026-10-02 re-check: with 8 px of corner left out, each side's end positions crossed the rounded
        # corner, whose outer pixel blends into the surface; a minimum width then read 1 on every side.
        image = ring_frame()
        x0, y0, x1, y1 = RECT
        for x in (x0 + 8, x1 - 9):
            image.putpixel((x, y0), (105, 200, 169))
            image.putpixel((x, y1 - 1), (105, 200, 169))
        for y in (y0 + 8, y1 - 9):
            image.putpixel((x0, y), (105, 200, 169))
            image.putpixel((x1 - 1, y), (105, 200, 169))
        result = analysis.ring_sides(image, RECT, RING, corner=8)
        self.assertTrue(result["passed"], result["reasons"])
        self.assertEqual({(e["width"], e["width_min"], e["width_max"]) for e in result["sides"].values()}, {(2, 1, 2)})
        self.assertTrue(analysis.ring_sides(image, RECT, RING)["passed"])

    def test_a_second_outline_in_the_ring_colour_is_counted(self) -> None:
        result = analysis.ring_sides(self.frame(double=True), RECT, RING)
        self.assertTrue(result["passed"], result["reasons"])
        self.assertEqual(result["sides"]["left"]["outlines"], 2)
        self.assertEqual(result["sides"]["left"]["width"], 2)  # the ring's own run, not both outlines
        self.assertFalse(analysis.ring_sides(self.frame(double=True), RECT, RING, max_outlines=1)["passed"])

    def test_low_contrast_fails_the_threshold(self) -> None:
        result = analysis.ring_sides(self.frame(), RECT, RING, min_contrast=12.0)
        self.assertFalse(result["passed"])
        self.assertEqual(len(result["reasons"]), 4)


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ClearanceTest(unittest.TestCase):
    def frame(self, gap: int):
        """RECT's 2 px ring, and a neighbour's 1 px border `gap` px below the ring's outer edge."""
        from PIL import ImageDraw

        image = ring_frame(2)
        ImageDraw.Draw(image).line((20, RECT[3] + gap, 180, RECT[3] + gap), fill=BORDER)
        return image

    def test_clearance_zero_and_two(self) -> None:
        touching = analysis.clearance(self.frame(0), RECT, "bottom", SURFACE, ring=RING, at=[60, 140], min_px=2)
        self.assertEqual(touching["clearance"], 0)
        self.assertFalse(touching["passed"])
        self.assertEqual(touching["lines"][0]["border"], RECT[3])
        self.assertEqual(touching["lines"][0]["border_colour"], list(BORDER))
        clear = analysis.clearance(self.frame(2), RECT, "bottom", SURFACE, ring=RING, at=[60, 140], min_px=2)
        self.assertEqual(clear["clearance"], 2)
        self.assertTrue(clear["passed"], clear["reasons"])
        self.assertEqual(clear["lines"][1]["start"], RECT[3] - 1)  # the ring's outermost row

    def test_clearance_from_the_controls_edge_and_a_missing_ring(self) -> None:
        from PIL import Image, ImageDraw

        image = Image.new("RGB", (200, 100), SURFACE)
        draw = ImageDraw.Draw(image)
        draw.rectangle((40, 30, 159, 69), outline=BORDER)
        draw.line((20, 25, 180, 25), fill=BORDER)  # the neighbour above, 4 px clear of the control's top edge
        result = analysis.clearance(image, RECT, "top", SURFACE, max_px=4)
        self.assertEqual(result["clearance"], 4)
        self.assertTrue(result["passed"])
        self.assertFalse(analysis.clearance(image, RECT, "top", SURFACE, max_px=3)["passed"])
        missing = analysis.clearance(image, RECT, "top", SURFACE, ring=RING, min_px=0)
        self.assertFalse(missing["passed"])
        self.assertIsNone(missing["clearance"])

    def test_no_neighbour_within_the_limit(self) -> None:
        from PIL import Image

        image = Image.new("RGB", (200, 100), SURFACE)
        self.assertTrue(analysis.clearance(image, RECT, "right", SURFACE, limit=10, min_px=2)["passed"])
        result = analysis.clearance(image, RECT, "right", SURFACE, limit=10, max_px=20)
        self.assertFalse(result["passed"])
        self.assertIn("no neighbour", result["reasons"][0])


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class FillAndCompareTest(unittest.TestCase):
    def test_fill_contrast_known_values(self) -> None:
        from PIL import Image, ImageDraw

        image = Image.new("RGB", (100, 50), (255, 255, 255))
        ImageDraw.Draw(image).rectangle((10, 10, 59, 39), fill=(0, 0, 0))
        result = analysis.fill_contrast(image, (10, 10, 60, 40), (255, 255, 255), min_contrast=20)
        self.assertEqual((result["fill"], result["share"], result["contrast"]), ([0, 0, 0], 1.0, 21.0))
        self.assertTrue(result["passed"])
        mixed = analysis.fill_contrast(image, (0, 0, 60, 40), (255, 255, 255), max_contrast=1.5)
        self.assertEqual(mixed["fill"], [0, 0, 0])  # 1500 black of 2400 pixels
        self.assertEqual(mixed["share"], 0.625)
        self.assertFalse(mixed["passed"])
        self.assertEqual(analysis.fill_contrast(image, (0, 0, 5, 5), SURFACE)["contrast"],
                         frames.contrast((255, 255, 255), SURFACE))

    def test_masked_compare(self) -> None:
        from PIL import Image, ImageDraw

        base = Image.new("RGB", (200, 120), SURFACE)
        candidate = base.copy()
        ImageDraw.Draw(candidate).rectangle((10, 20, 19, 29), fill=RING)
        same = analysis.masked_compare(base, base.copy())
        self.assertEqual((same["result"], same["differing_pixels"], same["passed"]), ("identical", 0, True))
        changed = analysis.masked_compare(base, candidate)
        self.assertEqual((changed["differing_pixels"], changed["passed"]), (100, False))
        self.assertEqual(changed["regions"], [[10, 20, 20, 30]])
        self.assertEqual(changed["row_bands"], [[20, 29]])
        masked = analysis.masked_compare(base, candidate, masks=[(0, 15, 50, 35)])
        self.assertEqual((masked["differing_pixels"], masked["masked_pixels"], masked["passed"]), (0, 100, True))
        # A region crops both frames; masks and regions stay in window coordinates.
        inside = analysis.masked_compare(base, candidate, region=(5, 10, 100, 60), masks=[(15, 0, 40, 120)])
        self.assertEqual((inside["differing_pixels"], inside["masked_pixels"]), (50, 50))
        self.assertEqual(inside["regions"], [[10, 20, 15, 30]])
        self.assertEqual(inside["bbox"], [10, 20, 15, 30])
        self.assertTrue(analysis.masked_compare(base, candidate, region=(100, 0, 200, 120))["passed"])
        bands = analysis.masked_compare(base, candidate, bands=[[20, 29]], band_min=10)
        self.assertTrue(bands["passed"], bands["reasons"])
        self.assertFalse(analysis.masked_compare(base, candidate, bands=[[20, 29]], band_min=11)["passed"])
        self.assertTrue(analysis.masked_compare(base, candidate, min_pixels=100)["passed"])
        self.assertFalse(analysis.masked_compare(base, candidate, min_pixels=101)["passed"])
        self.assertEqual(analysis.masked_compare(base, Image.new("RGB", (10, 10)))["result"], "size-mismatch")

    def test_evaluate_runs_entries_and_reports_missing_frames(self) -> None:
        image = ring_frame()
        frames_ = {"focus": image, "rest": ring_frame(skip=analysis.SIDES)}
        entry = dict(kind="ring", frame="focus", rect=RECT, colour="detect", sides=analysis.SIDES, reach=4, corner=8,
                     tolerance=6, min_width=1, uniform_width=True, min_contrast=3.0)
        self.assertTrue(analysis.evaluate(entry, frames_.get)["passed"])
        self.assertFalse(analysis.evaluate(dict(entry, frame="rest"), frames_.get)["passed"])
        missing = analysis.evaluate(dict(entry, frame="gone"), frames_.get)
        self.assertEqual(missing["missing"], ["gone"])
        compare = dict(kind="compare", a="rest", b="focus", masks=["status-timing"], band_min=1, max_pixels=0)
        self.assertFalse(analysis.evaluate(compare, frames_.get)["passed"])
        clear = dict(kind="clearance", frame="focus", rect=RECT, side="bottom", surface={"at": (5, 5)}, ring="detect",
                     reach=4, tolerance=6, limit=40, min_px=20)
        self.assertEqual(analysis.evaluate(clear, frames_.get)["clearance"], 30)  # to the frame's bottom edge
        fill = dict(kind="fill", frame="focus", region=(45, 35, 155, 65), reference={"region": (0, 0, 10, 10),
                                                                                    "frame": None}, min_contrast=1.0)
        self.assertEqual(analysis.evaluate(fill, frames_.get)["contrast"], 1.0)


if __name__ == "__main__":
    unittest.main()
