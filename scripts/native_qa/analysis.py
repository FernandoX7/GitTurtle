"""Pixel analyses for native-QA evidence: focus-ring sides, clearance, fill contrast and masked compares.

Pure functions on Pillow images, with no display. Coordinates are window
pixels and boxes are PIL boxes, `(x0, y0, x1, y1)` with exclusive ends. Each
analysis returns its measurements, a `passed` verdict against the thresholds
it was given, and the `reasons` for a failure. `evaluate` runs one analysis
entry of a scenario (`scenario.py` validates and normalises those entries);
contrast is the WCAG ratio of relative luminances (`frames.contrast`).
"""

from __future__ import annotations

from collections import Counter

from . import frames

SIDES = ("top", "right", "bottom", "left")
KINDS = ("ring", "clearance", "fill", "compare")
TOLERANCE = 6     # per channel; a ring is one colour, but blending at its ends can move it a few units
REACH = 4         # px searched on each side of a rect's edge for its ring
CORNER = 10       # px left out at each end of a side, where a ring rounds its corner (8 left blended pixels in 2026)
MIN_CHROMA = 40   # max minus min channel of an accent; neutral borders and surfaces stay far below it
MAX_REGIONS = 64  # difference regions kept per compare, so analysis.json stays readable


def near(pixel, colour, tolerance: int = TOLERANCE) -> bool:
    return all(abs(a - b) <= tolerance for a, b in zip(pixel[:3], colour[:3]))


def chroma(colour) -> int:
    return max(colour[:3]) - min(colour[:3])


def modal(image, box) -> tuple[tuple[int, int, int], int, int]:
    """The most frequent colour in `box`, its pixel count and the box's pixel count (ties: the larger colour)."""
    region = image.convert("RGB").crop(tuple(box))
    total = region.width * region.height
    count, colour = max(region.getcolors(total), key=lambda item: (item[0], item[1]))
    return tuple(colour), count, total


def _lines(rect, side: str, reach: int, corner: int, size):
    """Each position along `side` of `rect` with its cross-section of pixels, ordered from outside to inside.

    The cross-section is `reach` px outside the edge and `reach` px inside it,
    clipped to the image; `corner` px at each end of the side are left out.
    """
    x0, y0, x1, y1 = rect
    width, height = size
    if side == "top":
        cross = [y0 + k for k in range(-reach, reach)]
    elif side == "bottom":
        cross = [y1 - 1 + reach - k for k in range(2 * reach)]
    elif side == "left":
        cross = [x0 + k for k in range(-reach, reach)]
    else:
        cross = [x1 - 1 + reach - k for k in range(2 * reach)]
    if side in ("top", "bottom"):
        for x in range(max(x0 + corner, 0), min(x1 - corner, width)):
            yield x, [(x, y) for y in cross if 0 <= y < height]
    else:
        for y in range(max(y0 + corner, 0), min(y1 - corner, height)):
            yield y, [(x, y) for x in cross if 0 <= x < width]


def detect_ring_colour(image, rect, reach: int = REACH, corner: int = CORNER, min_chroma: int = MIN_CHROMA):
    """The most saturated colour covering at least a quarter of the positions around `rect`, or None.

    An accent ring is the most saturated colour around a control in every
    palette; requiring a quarter of the perimeter keeps antialiased text and
    icon pixels from qualifying.
    """
    image = image.convert("RGB")
    pixels = image.load()
    counts: Counter = Counter()
    positions = 0
    for side in SIDES:
        for _, points in _lines(rect, side, reach, corner, image.size):
            positions += 1
            counts.update(pixels[x, y][:3] for x, y in points)
    floor = max(4, positions // 4)
    found = [(chroma(colour), count, colour) for colour, count in counts.items()
             if count >= floor and chroma(colour) >= min_chroma]
    return max(found)[2] if found else None


def usual(values: list[int]) -> int:
    """The most frequent value (ties: the smaller), 0 for none."""
    if not values:
        return 0
    counts = Counter(values)
    return min(counts, key=lambda value: (-counts[value], value))


def ring_sides(image, rect, colour=None, sides=SIDES, reach: int = REACH, corner: int = CORNER,
               tolerance: int = TOLERANCE, min_width: int = 1, uniform_width: bool = True,
               min_contrast: float | None = None, max_outlines: int | None = None) -> dict:
    """Which sides of `rect` show a continuous ring of `colour` (None: detect it), its width and contrast.

    Per side: the positions holding a ring pixel, the ring's width (the run of
    ring pixels nearest the outside, as most positions show it, with the
    narrowest and widest), the number of separate outlines in that colour (2
    when a control's own border repeats the ring), the outer edge's
    coordinate, and the contrast of the ring against the surface just outside
    it. A side passes when it is continuous, at least `min_width` wide, within
    `max_outlines` and at least `min_contrast`; with `uniform_width`, every
    checked side must also have the same width, so a ring clipped by 1 px on
    one side fails. The usual width ignores the antialiased pixels where a
    side meets its rounded corner.
    """
    image = image.convert("RGB")
    detected = colour is None
    if colour is None:
        colour = detect_ring_colour(image, rect, reach, corner)
        if colour is None:
            return dict(colour=None, detected=True, sides={}, box=None, passed=False,
                        reasons=[f"no accent colour (chroma >= {MIN_CHROMA}) around the rect"])
    colour = tuple(colour[:3])
    pixels = image.load()
    report = {}
    for side in SIDES:
        widths, outlines, outers, surfaces, positions = [], [], [], Counter(), 0
        for _, points in _lines(rect, side, reach, corner, image.size):
            positions += 1
            hits = [index for index, point in enumerate(points) if near(pixels[point], colour, tolerance)]
            if not hits:
                continue
            first = 1
            while first < len(hits) and hits[first] == hits[0] + first:
                first += 1
            widths.append(first)
            outlines.append(1 + sum(1 for a, b in zip(hits, hits[1:]) if b != a + 1))
            x, y = points[hits[0]]
            outers.append(y if side in ("top", "bottom") else x)
            if hits[0] > 0:
                surfaces[pixels[points[hits[0] - 1]][:3]] += 1
        surface = surfaces.most_common(1)[0][0] if surfaces else None
        report[side] = dict(
            positions=positions, ring_positions=len(widths), gaps=positions - len(widths),
            continuous=positions > 0 and len(widths) == positions,
            width=usual(widths), width_min=min(widths) if widths else 0, width_max=max(widths) if widths else 0,
            outlines=Counter(outlines).most_common(1)[0][0] if outlines else 0,
            outer=Counter(outers).most_common(1)[0][0] if outers else None,
            surface=list(surface) if surface else None,
            contrast=frames.contrast(colour, surface) if surface else None)
    reasons = []
    for side in sides:
        entry = report[side]
        if not entry["continuous"]:
            reasons.append(f"{side}: no ring at {entry['gaps']} of {entry['positions']} positions")
        elif entry["width"] < min_width:
            reasons.append(f"{side}: ring {entry['width']} px wide, under {min_width}")
        if min_contrast is not None and entry["ring_positions"] and (
                entry["contrast"] is None or entry["contrast"] < min_contrast):
            reasons.append(f"{side}: contrast {entry['contrast']} against the surface, under {min_contrast}")
        if max_outlines is not None and entry["outlines"] > max_outlines:
            reasons.append(f"{side}: {entry['outlines']} outlines in the ring colour, over {max_outlines}")
    if uniform_width:
        widths = {report[side]["width"] for side in sides if report[side]["ring_positions"]}
        if len(widths) > 1:
            reasons.append(f"the ring's width differs between sides: {sorted(widths)} px")
    box = None
    if all(report[side]["outer"] is not None for side in SIDES):
        box = [report["left"]["outer"], report["top"]["outer"], report["right"]["outer"] + 1,
               report["bottom"]["outer"] + 1]
    return dict(colour=list(colour), detected=detected, sides=report, box=box, passed=not reasons,
                reasons=reasons)


def clearance(image, rect, side: str, surface, ring=None, at=None, reach: int = REACH,
              tolerance: int = TOLERANCE, limit: int = 40, min_px: int | None = None,
              max_px: int | None = None) -> dict:
    """Surface pixels between a ring (or the rect's own edge) and the next non-surface pixel, outward from `side`.

    With `ring` (a colour), each scan line starts at the ring's outermost pixel
    within `reach` of the edge; without it, at the rect's edge. It then counts
    pixels matching `surface` until the first that does not: the neighbour's
    border. `at` lists the scan lines (x for top and bottom, y for left and
    right; default the side's centre); the clearance is their minimum.
    """
    image = image.convert("RGB")
    pixels = image.load()
    x0, y0, x1, y1 = rect
    vertical = side in ("top", "bottom")
    step = -1 if side in ("top", "left") else 1
    edge = {"top": y0, "bottom": y1 - 1, "left": x0, "right": x1 - 1}[side]
    extent = image.height if vertical else image.width
    if at is None:
        at = [(x0 + x1) // 2] if vertical else [(y0 + y1) // 2]
    lines, reasons = [], []
    for position in at:
        def point(c, p=position):
            return (p, c) if vertical else (c, p)

        start = edge
        if ring is not None:
            band = [edge + step * k for k in range(reach, -reach, -1)]  # outside first
            hits = [c for c in band if 0 <= c < extent and near(pixels[point(c)], ring, tolerance)]
            if not hits:
                lines.append(dict(at=position, ring_found=False))
                reasons.append(f"no ring at {position}")
                continue
            start = hits[0]
        count, border, border_colour, c = 0, None, None, start + step
        while 0 <= c < extent and count < limit:
            pixel = pixels[point(c)]
            if not near(pixel, surface, tolerance):
                border, border_colour = c, list(pixel[:3])
                break
            count += 1
            c += step
        lines.append(dict(at=position, ring_found=None if ring is None else True, start=start, clearance=count,
                          border=border, border_colour=border_colour))
    measured = [line["clearance"] for line in lines if "clearance" in line]
    value = min(measured) if measured and len(measured) == len(lines) else None
    if value is not None:
        if min_px is not None and value < min_px:
            reasons.append(f"clearance {value} px, under {min_px}")
        if max_px is not None:
            if any(line["border"] is None for line in lines):
                reasons.append(f"no neighbour within {limit} px")
            elif value > max_px:
                reasons.append(f"clearance {value} px, over {max_px}")
    return dict(side=side, surface=list(surface[:3]), ring=None if ring is None else list(ring[:3]), lines=lines,
                clearance=value, passed=not reasons, reasons=reasons)


def fill_contrast(image, region, reference, min_contrast: float | None = None,
                  max_contrast: float | None = None) -> dict:
    """The contrast of `region`'s dominant fill against a `reference` colour."""
    colour, count, total = modal(image, region)
    ratio = frames.contrast(colour, reference)
    reasons = []
    if min_contrast is not None and ratio < min_contrast:
        reasons.append(f"contrast {ratio} under {min_contrast}")
    if max_contrast is not None and ratio > max_contrast:
        reasons.append(f"contrast {ratio} over {max_contrast}")
    return dict(fill=list(colour), share=round(count / total, 4), reference=list(reference[:3]), contrast=ratio,
                passed=not reasons, reasons=reasons)


def row_bands(mask, minimum: int = 1, offset: int = 0) -> list[list[int]]:
    """Inclusive [y0, y1] runs of rows holding at least `minimum` set pixels of an L mask."""
    bands: list[list[int]] = []
    for y in range(mask.height):
        if mask.crop((0, y, mask.width, y + 1)).histogram()[255] >= minimum:
            if bands and bands[-1][1] == y + offset - 1:
                bands[-1][1] = y + offset
            else:
                bands.append([y + offset, y + offset])
    return bands


def masked_compare(a, b, region=None, masks=(), max_pixels: int | None = None, min_pixels: int | None = None,
                   bands=None, band_min: int = 1) -> dict:
    """Pixels that differ between two frames inside `region` and outside `masks` (named or boxes, window
    coordinates), with their regions and changed row bands; `frames.compare` semantics, so masked
    differences are still counted. Without a threshold the frames must be identical outside the masks."""
    from PIL import ImageDraw

    if max_pixels is None and min_pixels is None and bands is None:
        max_pixels = 0
    a, b = a.convert("RGB"), b.convert("RGB")
    if a.size != b.size:
        return dict(result="size-mismatch", a_size=list(a.size), b_size=list(b.size), passed=False,
                    reasons=[f"frame sizes differ: {a.size} and {b.size}"])
    boxes = frames.resolve_masks(masks, a.size)
    ox, oy = (region[0], region[1]) if region else (0, 0)
    if region:
        a, b = a.crop(tuple(region)), b.crop(tuple(region))
    mask = frames.difference_mask(a, b)
    total = mask.histogram()[255]
    draw = ImageDraw.Draw(mask)
    for x0, y0, x1, y1 in boxes:
        bx0, by0, bx1, by1 = max(x0 - ox, 0), max(y0 - oy, 0), min(x1 - ox, mask.width), min(y1 - oy, mask.height)
        if bx0 < bx1 and by0 < by1:
            draw.rectangle((bx0, by0, bx1 - 1, by1 - 1), fill=0)
    unmasked = mask.histogram()[255]

    def shifted(box):
        return [box[0] + ox, box[1] + oy, box[2] + ox, box[3] + oy]

    found = [shifted(box) for box in frames.regions(mask)] if unmasked else []
    changed_rows = row_bands(mask, band_min, oy) if unmasked else []
    reasons = []
    if max_pixels is not None and unmasked > max_pixels:
        reasons.append(f"{unmasked} px differ outside the masks, over {max_pixels}")
    if min_pixels is not None and unmasked < min_pixels:
        reasons.append(f"{unmasked} px differ outside the masks, under {min_pixels}")
    if bands is not None and changed_rows != [list(band) for band in bands]:
        reasons.append(f"changed rows {changed_rows}, expected {[list(band) for band in bands]}")
    return dict(result="identical" if unmasked == 0 else "different", differing_pixels=unmasked,
                masked_pixels=total - unmasked, region=list(region) if region else None,
                masks=[list(box) for box in boxes], bbox=shifted(mask.getbbox()) if unmasked else None,
                regions=found[:MAX_REGIONS], region_count=len(found), row_bands=changed_rows,
                passed=not reasons, reasons=reasons)


# ---------- scenario entries ----------
def frame_refs(entry: dict) -> list[str]:
    """The frames a normalised analysis entry reads, in a stable order."""
    if entry["kind"] == "compare":
        return [entry["a"], entry["b"]]
    refs = [entry["frame"]]
    reference = entry.get("reference") or {}
    if reference.get("frame") and reference["frame"] not in refs:
        refs.append(reference["frame"])
    return refs


def evaluate(entry: dict, resolve) -> dict:
    """Run one normalised analysis entry; `resolve(ref)` returns that frame as an image, or None if it is missing."""
    images = {ref: resolve(ref) for ref in frame_refs(entry)}
    missing = [ref for ref, image in images.items() if image is None]
    if missing:
        return dict(passed=False, missing=missing, reasons=[f"frame {ref} is missing" for ref in missing])
    kind = entry["kind"]
    if kind == "compare":
        return masked_compare(images[entry["a"]], images[entry["b"]], entry.get("region"), entry["masks"],
                              entry.get("max_pixels"), entry.get("min_pixels"), entry.get("bands"),
                              entry["band_min"])
    image = images[entry["frame"]].convert("RGB")
    if kind == "ring":
        colour = None if entry["colour"] == "detect" else entry["colour"]
        return ring_sides(image, entry["rect"], colour, entry["sides"], entry["reach"], entry["corner"],
                          entry["tolerance"], entry["min_width"], entry["uniform_width"], entry.get("min_contrast"),
                          entry.get("max_outlines"))
    if kind == "clearance":
        surface = entry["surface"]
        if isinstance(surface, dict):
            surface = image.getpixel(tuple(surface["at"]))[:3]
        ring = entry.get("ring")
        if ring == "detect":
            ring = detect_ring_colour(image, entry["rect"], entry["reach"])
            if ring is None:
                return dict(passed=False, reasons=["no accent colour around the rect to measure from"])
        return clearance(image, entry["rect"], entry["side"], surface, ring, entry.get("at"), entry["reach"],
                         entry["tolerance"], entry["limit"], entry.get("min_px"), entry.get("max_px"))
    if kind == "fill":
        reference = entry["reference"]
        if "colour" in reference:
            colour = reference["colour"]
        else:
            colour = modal(images[reference.get("frame") or entry["frame"]], reference["region"])[0]
        return fill_contrast(image, entry["region"], colour, entry.get("min_contrast"), entry.get("max_contrast"))
    raise ValueError(f"unknown analysis kind {kind!r}")
