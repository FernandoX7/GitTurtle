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


BUTTON = (40, 46, 58)        # a button surface
TINT = (200, 206, 218)       # the colour a glyph is drawn in
GLYPH_BOX = (10, 10, 30, 30)  # 20x20, with room around the glyph


def blend(coverage: float, tint=TINT, surface=BUTTON) -> tuple[int, int, int]:
    """The colour of a pixel a stroke of `tint` covers by `coverage`, anti-aliased onto `surface`."""
    return tuple(round(s + (t - s) * coverage) for t, s in zip(tint, surface))


def glyph(*strokes, size=(60, 40)):
    """A frame of the button surface with each stroke, `(box, colour)`, painted on it."""
    from PIL import Image

    image = Image.new("RGB", size, BUTTON)
    for box, colour in strokes:
        image.paste(colour, box)
    return image


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class GlyphContrastTest(unittest.TestCase):
    def test_a_solid_glyph_reports_its_tint_against_the_surface(self) -> None:
        image = glyph(((15, 13, 17, 27), TINT), ((15, 25, 25, 27), TINT))  # an L of 2 px strokes, 44 px
        result = analysis.glyph_contrast(image, GLYPH_BOX, min_contrast=3.0)
        self.assertTrue(result["passed"], result["reasons"])
        tint = frames.contrast(TINT, BUTTON)
        self.assertEqual((result["surface"], result["surface_from"], result["ink_pixels"], result["pixels"]),
                         (list(BUTTON), "box", 44, 400))
        self.assertEqual(result["peak"], dict(colour=list(TINT), contrast=tint, pixels=44, support=44))
        self.assertEqual((result["dominant"], result["strongest"], result["median_contrast"]),
                         (dict(colour=list(TINT), contrast=tint, pixels=44),) * 2 + (tint,))
        self.assertEqual((result["ink_box"], result["touches"]), ([15, 13, 25, 27], []))
        self.assertGreater(tint, 8)

    def test_an_antialiased_stroke_makes_the_peak_a_lower_bound(self) -> None:
        # A 1 px stroke centred on a pixel boundary covers two columns by half: its tint never shows unblended.
        half = blend(0.5)
        image = glyph(((15, 12, 17, 28), half))
        result = analysis.glyph_contrast(image, GLYPH_BOX, min_contrast=4.5)
        self.assertEqual(result["peak"], dict(colour=list(half), contrast=frames.contrast(half, BUTTON), pixels=32,
                                              support=32))
        self.assertLess(result["peak"]["contrast"], 4.5)
        self.assertGreater(frames.contrast(TINT, BUTTON), 4.5)  # the tint itself would pass
        self.assertFalse(result["passed"])
        self.assertEqual(result["reasons"], [f"peak ink contrast {result['peak']['contrast']} ({list(half)}, 32 px) "
                                             f"against the surface {list(BUTTON)}, under 4.5"])
        # A stroke with a full core row between quarter-covered edges: the peak is the tint, while the median and
        # the most frequent ink colour are the blended edges.
        edged = glyph(((12, 19, 28, 20), blend(0.25)), ((12, 20, 28, 21), TINT), ((12, 21, 28, 22), blend(0.25)))
        result = analysis.glyph_contrast(edged, GLYPH_BOX, min_contrast=4.5)
        self.assertTrue(result["passed"], result["reasons"])
        edge = frames.contrast(blend(0.25), BUTTON)
        self.assertEqual((result["ink_pixels"], result["peak"]["contrast"], result["peak"]["pixels"]),
                         (48, frames.contrast(TINT, BUTTON), 16))
        self.assertEqual((result["dominant"]["colour"], result["dominant"]["pixels"], result["median_contrast"]),
                         (list(blend(0.25)), 32, edge))

    def test_an_empty_box_fails_on_its_ink_count(self) -> None:
        result = analysis.glyph_contrast(glyph(), GLYPH_BOX, min_contrast=3.0)
        self.assertFalse(result["passed"])
        self.assertEqual((result["ink_pixels"], result["peak"], result["dominant"], result["median_contrast"],
                          result["ink_box"], result["touches"]), (0, None, None, None, None, []))
        self.assertEqual(result["reasons"], [f"0 ink pixels (more than 6 from the surface {list(BUTTON)} in a "
                                             "channel), under 8: no glyph in the box"])
        # Seven ink pixels are still under the minimum; a pixel within the tolerance is surface, not ink.
        few = glyph(((12, 12, 19, 13), TINT), ((20, 20, 21, 21), (46, 46, 58)))
        self.assertEqual(analysis.glyph_contrast(few, GLYPH_BOX, min_contrast=3.0)["ink_pixels"], 7)
        self.assertTrue(analysis.glyph_contrast(few, GLYPH_BOX, min_contrast=3.0, min_ink=7)["passed"])
        noise = glyph(((20, 20, 21, 21), (47, 46, 58)))
        self.assertEqual(analysis.glyph_contrast(noise, GLYPH_BOX, min_ink=1)["ink_pixels"], 1)

    def test_the_surface_is_named_sampled_or_the_most_frequent_in_the_box(self) -> None:
        image = glyph(((8, 8, 32, 32), TINT), ((0, 0, 60, 4), (30, 30, 30)))  # a glyph filling its box
        crowded = analysis.glyph_contrast(image, GLYPH_BOX, min_contrast=3.0)
        self.assertEqual((crowded["surface"], crowded["ink_pixels"]), (list(TINT), 0))  # the box held no surface
        for surface, source in ((BUTTON, "spec"), ({"at": [50, 30]}, "at"), ({"region": (40, 10, 60, 40)}, "region")):
            with self.subTest(surface=surface):
                result = analysis.glyph_contrast(image, (6, 6, 34, 34), surface, min_contrast=3.0)
                self.assertEqual((result["surface"], result["surface_from"]), (list(BUTTON), source))
                self.assertEqual((result["ink_pixels"], result["touches"]), (576, []))
        cut = analysis.glyph_contrast(image, (20, 6, 34, 34), BUTTON, min_contrast=3.0)
        self.assertEqual((cut["ink_box"], cut["touches"], cut["passed"]), ([20, 8, 32, 32], ["left"], False))

    def test_ink_on_a_side_of_the_box_fails_unless_allowed(self) -> None:
        # The box catches only a button's 1 px edge: high contrast, enough pixels, and no glyph at all.
        from PIL import Image

        image = Image.new("RGB", (60, 40), (40, 40, 40))
        image.paste((200, 200, 200), (10, 0, 11, 40))
        edge = analysis.glyph_contrast(image, (10, 5, 30, 25), min_contrast=3.0)
        self.assertEqual((edge["ink_pixels"], edge["touches"], edge["passed"]), (20, ["top", "bottom", "left"], False))
        self.assertGreater(edge["peak"]["contrast"], 8)
        self.assertEqual(edge["reasons"], ["ink reaches the box's top, bottom, left side(s): the box cuts the glyph or "
                                           "holds a border or a neighbour; give the glyph a clear margin of surface "
                                           "on every side"])
        allowed = analysis.glyph_contrast(image, (10, 5, 30, 25), min_contrast=3.0, allow_edge=True)
        self.assertEqual((allowed["allow_edge"], allowed["passed"]), (True, True))

    def test_a_stray_pixel_cannot_carry_the_peak(self) -> None:
        faint = blend(0.3)
        image = glyph(((14, 14, 24, 15), faint), ((18, 20, 19, 21), TINT))  # a faint 10 px stroke and one stray pixel
        result = analysis.glyph_contrast(image, GLYPH_BOX, min_contrast=3.0)
        self.assertEqual(result["strongest"], dict(colour=list(TINT), contrast=frames.contrast(TINT, BUTTON), pixels=1))
        self.assertEqual((result["peak"]["colour"], result["peak"]["support"]), (list(faint), 10))
        self.assertLess(result["peak"]["contrast"], 3.0)
        self.assertFalse(result["passed"])
        self.assertTrue(analysis.glyph_contrast(image, GLYPH_BOX, min_contrast=3.0, min_peak_pixels=1)["passed"])
        # Pixels of colours too far apart to back one another are no glyph, however many there are.
        scattered = glyph(*(((12 + 2 * i, 12, 13 + 2 * i, 13), (200, 60 + 20 * i, 60)) for i in range(8)))
        result = analysis.glyph_contrast(scattered, GLYPH_BOX, min_contrast=1.0)
        self.assertEqual((result["ink_pixels"], result["peak"], result["passed"]), (8, None, False))
        self.assertEqual(result["reasons"], ["no ink colour is backed by 2 ink pixels within 6 of it: stray pixels, "
                                             "not a glyph"])

    def test_evaluate_runs_a_glyph_contrast_entry(self) -> None:
        captured = {"toolbar": glyph(((15, 13, 17, 27), TINT))}
        entry = dict(kind="glyph_contrast", frame="toolbar", box=GLYPH_BOX, surface=None, min_contrast=3.0,
                     tolerance=6, min_ink=8, min_peak_pixels=2, allow_edge=False)
        self.assertTrue(analysis.evaluate(entry, captured.get)["passed"])
        self.assertEqual(analysis.frame_refs(entry), ["toolbar"])
        self.assertEqual(analysis.evaluate(dict(entry, frame="gone"), captured.get)["missing"], ["gone"])
        self.assertFalse(analysis.evaluate(dict(entry, min_contrast=21.0), captured.get)["passed"])

    def test_weighted_median(self) -> None:
        self.assertEqual(analysis.weighted_median([(3.0, 1), (1.0, 1)]), 2.0)
        self.assertEqual(analysis.weighted_median([(5.0, 3), (1.0, 2)]), 5.0)
        self.assertEqual(analysis.weighted_median([(5.0, 2), (1.0, 2)]), 3.0)
        self.assertIsNone(analysis.weighted_median([]))


SWITCH = dict(name="Follow system appearance", role="toggle button", states=["focused", "focusable", "enabled"],
              depth=5)


def focus_reading(label: str, focused=SWITCH, **extra) -> dict:
    return dict(label=label, kind="atspi_focus", application="GitTurtle", focused=focused,
                all_focused=[focused] if focused else [], **extra)


def snapshot(label: str, data: bytes | None, inode: int = 7, mtime: int = 1) -> dict:
    """A store_snapshot reading of `data` (None: the file is absent)."""
    import hashlib
    import json

    entry = dict(label=label, kind="store_snapshot", path="config/gitturtle/preferences.json", stable=True,
                 exists=data is not None, sha256=None, bytes=None, mtime_ns=None, inode=None, json=None)
    if data is not None:
        entry.update(sha256=hashlib.sha256(data).hexdigest(), bytes=len(data), mtime_ns=mtime, inode=inode)
        try:
            entry["json"] = json.loads(data)
        except ValueError as error:
            entry["json_error"] = str(error)
    return entry


class ReadingTest(unittest.TestCase):
    """The reading kinds: pure functions on what `atspi_focus` and `store_snapshot` steps recorded."""

    def test_focused_node(self) -> None:
        reading = focus_reading("after-card")
        self.assertTrue(analysis.focused_node(reading, "Follow system appearance", "toggle button",
                                              ["focused"], ["pressed"])["passed"])
        for kwargs, reason in ((dict(node="Omarchy theme"), "is 'Follow system appearance', not 'Omarchy theme'"),
                               (dict(role="button"), "is a 'toggle button', not a 'button'"),
                               (dict(states=["focused", "pressed", "checked"]), "lacks pressed, checked"),
                               (dict(not_states=["enabled", "focusable"]), "is enabled, focusable")):
            with self.subTest(kwargs=kwargs):
                result = analysis.focused_node(reading, **kwargs)
                self.assertFalse(result["passed"])
                self.assertIn(reason, result["reasons"][0])
        # The base can report only the window as focused, or nothing at all.
        window = dict(name="GitTurtle", role="frame", states=["focused", "active"], depth=1)
        self.assertFalse(analysis.focused_node(focus_reading("x", window), "Follow system appearance")["passed"])
        nothing = analysis.focused_node(focus_reading("x", None), states=["focused"])
        self.assertEqual(nothing["reasons"], ["AT-SPI reported no focused node"])
        lost = analysis.focused_node(dict(focus_reading("x", None), error="no AT-SPI application"), node="x")
        self.assertEqual(lost["reasons"][0], "no AT-SPI application")

    def test_focused_node_same_as_another_reading(self) -> None:
        before, after = focus_reading("after-card"), focus_reading("after-keys", dict(SWITCH, states=list(
            reversed(SWITCH["states"]))))
        same = analysis.focused_node(after, same=before)
        self.assertTrue(same["passed"], same["reasons"])  # the same states in another order
        self.assertEqual(same["same_as"], SWITCH)
        toggled = focus_reading("after-keys", dict(SWITCH, states=SWITCH["states"] + ["pressed"]))
        result = analysis.focused_node(toggled, same=before)
        self.assertFalse(result["passed"])
        self.assertIn("differs from reading 'after-card' in states", result["reasons"][0])
        moved = analysis.focused_node(focus_reading("after-tab", dict(SWITCH, name="Omarchy theme", role="button")),
                                      same=before)
        self.assertIn("in name, role", moved["reasons"][0])
        self.assertFalse(analysis.focused_node(before, same=focus_reading("gone", None))["passed"])

    def test_store_compare_bytes_and_keys(self) -> None:
        store = b'{"version": 6, "settings": {"theme": "omarchy", "follow_system": false}}'
        a, b = snapshot("after-card", store), snapshot("after-keys", store)
        same = analysis.store_compare(a, b)
        self.assertTrue(same["passed"], same["reasons"])
        self.assertEqual((same["identical"], same["rewritten"], same["changed_keys"]), (True, False, []))
        rewritten = analysis.store_compare(a, snapshot("after-keys", store, inode=8))
        self.assertTrue(rewritten["passed"])  # the same bytes written again pass; the record says so
        self.assertTrue(rewritten["rewritten"])
        changed = snapshot("after-keys", b'{"version": 6, "settings": {"theme": "omarchy", "follow_system": true}}')
        result = analysis.store_compare(a, changed)
        self.assertFalse(result["passed"])
        self.assertEqual(result["changed_keys"], ["settings.follow_system"])
        self.assertIn("the store's bytes differ", result["reasons"][0])
        self.assertIn("changed keys settings.follow_system", result["reasons"][0])
        keys = analysis.store_compare(a, changed, ["settings.theme"])
        self.assertTrue(keys["passed"], keys["reasons"])
        keys = analysis.store_compare(a, changed, ["settings.theme", "settings.follow_system", "settings.absent"])
        self.assertEqual(keys["reasons"], ["settings.follow_system: False against True"])
        self.assertEqual(keys["differing"], [dict(key="settings.follow_system", a=False, b=True)])
        # Absent on both sides is identical; absent on one is not; a file that is not JSON has no keys.
        self.assertTrue(analysis.store_compare(snapshot("a", None), snapshot("b", None))["passed"])
        self.assertFalse(analysis.store_compare(a, snapshot("b", None))["passed"])
        garbled = analysis.store_compare(a, snapshot("b", b"{"), ["settings.theme"])
        self.assertEqual(garbled["reasons"], ["a snapshot is not JSON, so its keys cannot be compared"])
        unsettled = analysis.store_compare(a, dict(b, stable=False))
        self.assertEqual(unsettled["reasons"], ["snapshot 'after-keys' never settled"])

    def test_json_paths(self) -> None:
        document = {"settings": {"theme": "midnight"}, "recent_repositories": ["/tmp/a", "/tmp/b"]}
        self.assertEqual(analysis.json_at(document, "settings.theme"), "midnight")
        self.assertEqual(analysis.json_at(document, "recent_repositories.1"), "/tmp/b")
        for key in ("settings.missing", "recent_repositories.2", "settings.theme.x"):
            self.assertEqual(analysis.json_at(document, key), analysis.ABSENT)
        self.assertEqual(analysis.json_changes(document, {"settings": {"theme": "porcelain"}}),
                         ["recent_repositories", "settings.theme"])
        self.assertEqual(analysis.json_changes([1], [2]), ["(the whole document)"])

    def test_evaluate_reads_readings_not_frames(self) -> None:
        readings = {"after-card": focus_reading("after-card"), "store": snapshot("store", b"{}"),
                    "store-2": snapshot("store-2", b"{}")}
        entry = dict(kind="atspi_focus", focus="after-card", node="Follow system appearance", states=["focused"],
                     not_states=[])
        self.assertEqual(analysis.frame_refs(entry), [])
        self.assertEqual(analysis.refs(entry), ["after-card"])
        self.assertTrue(analysis.evaluate(entry, lambda ref: self.fail("no frame is read"), readings.get)["passed"])
        missing = analysis.evaluate(dict(entry, same_as="gone"), None, readings.get)
        self.assertEqual((missing["passed"], missing["missing"]), (False, ["gone"]))
        wrong = analysis.evaluate(dict(entry, focus="store"), None, readings.get)
        self.assertEqual(wrong["reasons"], ["reading store comes from store_snapshot, but atspi_focus reads only "
                                            "atspi_focus readings"])
        compare = dict(kind="store_compare", a="store", b="store-2")
        self.assertTrue(analysis.evaluate(compare, None, readings.get)["passed"])
        self.assertFalse(analysis.evaluate(compare, None)["passed"])  # no readings at all: missing


CHROME = (40, 44, 56)     # the dialog around a list
CLIP = (20, 10, 180, 110)  # a list's visible bounds in a 200x120 window


def list_frame(ring_top=None, size=(200, 120), clip=CLIP, ring_size=(120, 30), left=40, width=2, glyph=None,
               shift=0, colour=RING, also=None):
    """A list on its surface inside `clip` with the dialog's chrome around it, and a `width` px ring of `colour`
    whose outer box is `ring_size` at (`left`, `ring_top`), cut by the clip as the app's content mask cuts it;
    `also` draws a second ring at that top. `glyph` adds a small block of the ring colour there (an accent icon),
    and `shift` moves a row's text by that many px."""
    from PIL import Image, ImageDraw

    image = Image.new("RGB", size, CHROME)
    draw = ImageDraw.Draw(image)
    draw.rectangle((clip[0], clip[1], clip[2] - 1, clip[3] - 1), fill=SURFACE)
    draw.rectangle((60 + shift, 70, 100 + shift, 74), fill=BORDER)  # a row's text, which scrolls with the list
    for top in (ring_top, also):
        if top is not None:
            x0, y0 = left, top
            x1, y1 = x0 + ring_size[0], y0 + ring_size[1]
            draw.rectangle((x0, y0, x1 - 1, y1 - 1), outline=colour, width=width)
    if glyph is not None:
        draw.rectangle((glyph[0], glyph[1], glyph[0] + 5, glyph[1] + 7), fill=RING)
    painted = Image.new("RGB", size, CHROME)
    painted.paste(image.crop(clip), clip[:2])
    return painted


def probe(*presses, region=(0, 0, 200, 120), settled=True, changed=True, ended=None):
    """A probe record as `session.Session.probe` writes it, with images in place of files: each press is a list of
    (ms, image), the first the frame before the key; `ended` "frame-cap" or "byte-budget" truncates every press."""
    ended = ended or ("quiet" if settled else "timeout")
    return dict(probe="rows", region=list(region), timeout=3.0, stable_within=4.0, settled_before_s=0.2,
                resolution_ms=3.1, interval_ms=dict(min=1.0, median=2.0, max=3.1), grabs=40,
                presses=[dict(press=number, changed=changed, settled=ended == "quiet", ended=ended,
                              truncated=ended in ("frame-cap", "byte-budget"), end_ms=300.0,
                              frames=[dict(index=index, ms=at, image=image) for index, (at, image) in enumerate(frames_)])
                         for number, frames_ in enumerate(presses, 1)])


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ProbeRingTest(unittest.TestCase):
    def test_a_whole_ring_in_every_frame_passes_wherever_it_moves(self) -> None:
        before, moved, scrolled = list_frame(20), list_frame(60), list_frame(70, shift=-10)
        result = analysis.probe_ring(probe([(-1.0, before), (8.0, moved), (14.0, scrolled)]), CLIP, min_contrast=3.0)
        self.assertTrue(result["passed"], result["reasons"])
        frames_ = result["presses"][0]["frames"]
        self.assertEqual([(f["index"], f["ms"]) for f in frames_], [(1, 8.0), (2, 14.0)])  # the frame before the key
        self.assertEqual([f["box"] for f in frames_], [[40, 60, 160, 90], [40, 70, 160, 100]])
        self.assertEqual(frames_[1]["clip_margins"], dict(top=60, right=20, bottom=10, left=20))
        self.assertEqual(frames_[0]["contrast"], frames.contrast(RING, SURFACE))
        self.assertEqual({s["width"] for s in frames_[0]["sides"].values()}, {2})
        self.assertEqual((result["frames"], result["failing"], result["resolution_ms"]), (2, [], 3.1))
        self.assertIn("no frame failed at 3.1 ms resolution", result["summary"])
        self.assertFalse(analysis.probe_ring(probe([(-1.0, before), (8.0, moved)]), CLIP, min_contrast=12.0)["passed"])

    def test_a_ring_cut_at_the_clip_edge_fails_with_its_timing(self) -> None:
        # The base's frame: the newly focused row drawn before the list scrolls, its ring cut by the lower edge.
        cut, revealed = list_frame(85), list_frame(80, shift=-10)
        result = analysis.probe_ring(probe([(-1.0, list_frame(50)), (6.0, cut), (12.0, revealed)]), CLIP,
                                     min_contrast=3.0)
        self.assertFalse(result["passed"])
        [failing] = result["failing"]
        self.assertEqual((failing["press"], failing["index"], failing["ms"], failing["box"]),
                         (1, 1, 6.0, [40, 85, 160, 110]))
        self.assertEqual(failing["reasons"], ["bottom: no ring at 100 of 100 positions"])
        short = analysis.probe_ring(probe([(-1.0, revealed), (5.0, list_frame(95))]), CLIP)  # 15 px left in view
        self.assertIn("left: no position to check between its corners (10 px left out at each end)",
                      short["failing"][0]["reasons"])
        self.assertEqual(result["presses"][0]["frames"][0]["clip_margins"]["bottom"], 0)
        self.assertTrue(result["presses"][0]["frames"][1]["passed"])
        self.assertTrue(result["reasons"][0].startswith("press 1 frame 1 at 6.0 ms: bottom: no ring"))
        self.assertIn("1 of 2 frames after 1 presses failed", result["summary"])
        # A ring that keeps 1 of its 2 px at the edge fails on its width; one with no px in the clip on its ring.
        one_px = analysis.probe_ring(probe([(-1.0, revealed), (5.0, list_frame(81))]), CLIP)
        self.assertIn("the ring's width differs between sides: [1, 2] px", one_px["failing"][0]["reasons"])
        gone = analysis.probe_ring(probe([(-1.0, revealed), (5.0, list_frame(112))]), CLIP)
        self.assertIn("no accent colour", gone["failing"][0]["reasons"][0])

    def test_an_accent_glyph_is_not_a_ring(self) -> None:
        with_glyph = list_frame(20, glyph=(165, 30))  # 6x8 px of the ring colour, 5 px right of the ring
        result = analysis.probe_ring(probe([(-1.0, list_frame(20)), (5.0, with_glyph)]), CLIP)
        self.assertTrue(result["passed"], result["reasons"])
        self.assertEqual(result["presses"][0]["frames"][0]["box"], [40, 20, 160, 50])
        alone = analysis.probe_ring(probe([(-1.0, list_frame(20)), (5.0, list_frame(None, glyph=(100, 60)))]), CLIP)
        self.assertIn("no ring of [117, 224, 187] in the clip", alone["failing"][0]["reasons"][0])

    def test_a_second_ring_in_the_clip_fails_the_frame(self) -> None:
        # The old focus's ring still drawn beside the new one: the larger alone would pass.
        both = list_frame(20, also=70)
        result = analysis.probe_ring(probe([(-1.0, list_frame(20)), (5.0, both), (9.0, list_frame(70))]), CLIP)
        [failing] = result["failing"]
        self.assertEqual((failing["index"], failing["ms"]), (1, 5.0))
        self.assertEqual(failing["reasons"], ["2 rings of [117, 224, 187] in the clip, where only the focused control "
                                              "has one: [[40, 20, 160, 50], [40, 70, 160, 100]]"])
        self.assertEqual(result["presses"][0]["frames"][0]["rings"], [[40, 20, 160, 50], [40, 70, 160, 100]])
        self.assertTrue(result["presses"][0]["frames"][1]["passed"])

    def test_the_ring_colour_is_detected_once_per_press(self) -> None:
        other = (230, 120, 60)  # another accent: a frame painting it is not the press's ring
        frames_ = [(-1.0, list_frame(20)), (5.0, list_frame(60, colour=other)), (9.0, list_frame(60))]
        result = analysis.probe_ring(probe(frames_), CLIP)
        press = result["presses"][0]
        self.assertEqual((press["colour"], press["colour_from"]), (list(RING), 2))  # from the settled frame
        [failing] = result["failing"]
        self.assertEqual(failing["index"], 1)
        self.assertEqual(failing["reasons"], ["no ring of [117, 224, 187] in the clip (no straight run of 12 px)",
                                              "the clip's most frequent accent [230, 120, 60] is not the ring colour "
                                              "[117, 224, 187]"])
        self.assertEqual(press["frames"][0]["accent"], list(other))
        # A settled frame without any ring takes the colour from the first frame that shows one.
        gone = analysis.probe_ring(probe([(-1.0, list_frame(20)), (5.0, list_frame(60)), (9.0, list_frame(None))]),
                                   CLIP)
        self.assertEqual((gone["presses"][0]["colour"], gone["presses"][0]["colour_from"]), (list(RING), 1))
        self.assertEqual([f["index"] for f in gone["failing"]], [2])

    def test_a_probe_that_did_not_settle_or_change_fails(self) -> None:
        frames_ = [(-1.0, list_frame(20)), (5.0, list_frame(60))]
        unsettled = analysis.probe_ring(probe(frames_, settled=False), CLIP)
        self.assertEqual(unsettled["reasons"], ["press 1 did not settle: it ended by timeout 300.0 ms after the press"])
        self.assertTrue(unsettled["inconclusive"])
        for ended in ("frame-cap", "byte-budget"):
            truncated = analysis.probe_ring(probe(frames_, ended=ended), CLIP)
            self.assertEqual((truncated["passed"], truncated["inconclusive"]), (False, True))
            self.assertEqual(truncated["summary"], f"inconclusive: press 1 was truncated by its {ended} 300.0 ms after "
                                                   "the press, before its region settled")
            self.assertTrue(analysis.probe_endpoints(probe(frames_, ended=ended), CLIP)["inconclusive"])
        self.assertFalse(analysis.probe_ring(probe(frames_), CLIP)["inconclusive"])
        unchanged = analysis.probe_ring(probe(frames_[:1], changed=False), CLIP)
        self.assertEqual(unchanged["reasons"], ["press 1 changed nothing in the probe's region within 3.0 s"])
        self.assertFalse(analysis.probe_ring(probe(frames_), (0, 0, 210, 120))["passed"])  # past the region

    def test_a_mask_hides_a_caret_beside_the_ring(self) -> None:
        caret = list_frame(20)
        caret.paste(RING, (43, 26, 44, 44))  # an accent caret 1 px inside the ring's left side, 18 px tall
        frames_ = [(-1.0, list_frame(60)), (5.0, caret)]
        bare = analysis.probe_ring(probe(frames_), CLIP, max_outlines=1)
        self.assertEqual(bare["failing"][0]["reasons"], ["left: 2 outlines in the ring colour, over 1"])
        masked = analysis.probe_ring(probe(frames_), CLIP, max_outlines=1, masks=[(43, 24, 44, 46)])
        self.assertTrue(masked["passed"], masked["reasons"])
        self.assertEqual(masked["masks"], [[43, 24, 44, 46]])

    def test_frames_of_a_region_report_window_coordinates(self) -> None:
        region = (10, 5, 190, 115)
        frames_ = [(-1.0, list_frame(20).crop(region)), (5.0, list_frame(60).crop(region))]
        result = analysis.probe_ring(probe(frames_, region=region), CLIP, surface={"at": (25, 15)})
        self.assertTrue(result["passed"], result["reasons"])
        entry = result["presses"][0]["frames"][0]
        self.assertEqual((entry["box"], entry["surface"]), ([40, 60, 160, 90], list(SURFACE)))


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ProbeEndpointsTest(unittest.TestCase):
    def test_frames_are_the_frame_before_or_the_settled_one(self) -> None:
        before, settled = list_frame(50), list_frame(80, shift=-10)
        self.assertTrue(analysis.probe_endpoints(probe([(-1.0, before), (9.0, settled)]), CLIP)["passed"])
        result = analysis.probe_endpoints(probe([(-1.0, before), (6.0, list_frame(90)), (12.0, settled)]), CLIP)
        self.assertFalse(result["passed"])
        [failing] = result["failing"]
        self.assertEqual((failing["index"], failing["ms"]), (1, 6.0))
        self.assertTrue(failing["reasons"][0].startswith("intermediate: "))
        self.assertEqual([f["state"] for f in result["presses"][0]["frames"]], ["intermediate", "settled"])
        self.assertEqual(failing["bbox"], [40, 70, 160, 110])  # the text and rings that differ, in the window

    def test_a_tolerance_a_mask_and_a_frame_that_reverts(self) -> None:
        before, settled = list_frame(50), list_frame(80, shift=-10)
        caret = settled.copy()
        caret.paste(BORDER, (150, 100, 151, 108))  # 8 px, like a caret blinking off
        frames_ = [(-1.0, before), (5.0, caret), (9.0, settled)]
        self.assertFalse(analysis.probe_endpoints(probe(frames_), CLIP, max_pixels=7)["passed"])
        self.assertTrue(analysis.probe_endpoints(probe(frames_), CLIP, max_pixels=8)["passed"])
        self.assertTrue(analysis.probe_endpoints(probe(frames_), CLIP, masks=[(148, 98, 152, 110)])["passed"])
        reverted = analysis.probe_endpoints(probe([(-1.0, before), (5.0, settled), (9.0, before), (14.0, settled)]),
                                            CLIP)
        self.assertEqual([f["state"] for f in reverted["presses"][0]["frames"]], ["settled", "reverted", "settled"])
        self.assertEqual(reverted["failing"][0]["ms"], 9.0)

    def test_evaluate_reads_a_probe_and_reports_one_missing(self) -> None:
        probes = {"rows": probe([(-1.0, list_frame(20)), (5.0, list_frame(60))])}
        ring = dict(kind="probe_ring", probe="rows", clip=CLIP, colour="detect", surface=None, tolerance=6,
                    corner=10, min_width=1, uniform_width=True, min_contrast=3.0, masks=[])
        self.assertTrue(analysis.evaluate(ring, probes.get)["passed"])
        self.assertEqual(analysis.evaluate(dict(ring, probe="gone"), probes.get)["reasons"], ["probe gone is missing"])
        ends = dict(kind="probe_endpoints", probe="rows", clip=CLIP, masks=[], max_pixels=0)
        self.assertTrue(analysis.evaluate(ends, probes.get)["passed"])
        self.assertEqual(analysis.frame_refs(ends), ["rows"])


if __name__ == "__main__":
    unittest.main()
