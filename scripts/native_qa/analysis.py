"""Analyses for native-QA evidence: focus-ring sides, clearance, fill contrast, masked compares and probes on
frames, and the focused AT-SPI node and store snapshots on a launch's readings.

Pure functions on Pillow images, with no display. Coordinates are window
pixels and boxes are PIL boxes, `(x0, y0, x1, y1)` with exclusive ends. Each
analysis returns its measurements, a `passed` verdict against the thresholds
it was given, and the `reasons` for a failure. `evaluate` runs one analysis
entry of a scenario (`scenario.py` validates and normalises those entries);
contrast is the WCAG ratio of relative luminances (`frames.contrast`). The
reading kinds read what a launch's `atspi_focus` and `store_snapshot` steps
recorded in its flow-log.json, not frames.

A probe (`session.Session.probe`) is every distinct frame of a region grabbed
back to back after each press of a key or click. `probe_ring` and
`probe_endpoints` read one as `{"region": box, "presses": [{"frames": [...],
...}], ...}`, the probe's record, where each frame's `image` is a Pillow image
or a function that loads one; frame 0 of a press is the region just before it.
"""

from __future__ import annotations

import re
from collections import Counter

from . import frames

SIDES = ("top", "right", "bottom", "left")
# Each reading kind with the step kind whose readings it reads.
READING_KINDS = {"atspi_focus": "atspi_focus", "store_compare": "store_snapshot"}
PROBE_KINDS = ("probe_ring", "probe_endpoints")
KINDS = ("ring", "clearance", "fill", "compare", *PROBE_KINDS, *READING_KINDS)
TOLERANCE = 6     # per channel; a ring is one colour, but blending at its ends can move it a few units
REACH = 4         # px searched on each side of a rect's edge for its ring
CORNER = 10       # px left out at each end of a side, where a ring rounds its corner (8 left blended pixels in 2026)
MIN_CHROMA = 40   # max minus min channel of an accent; neutral borders and surfaces stay far below it
MAX_REGIONS = 64  # difference regions kept per compare, so analysis.json stays readable
RUN = 12          # px: a probe finds a ring by its sides, straight runs of the ring colour; glyph strokes are shorter
MIN_RING_PIXELS = 40  # pixels of one accent colour a probe frame needs before that colour can be a ring
MAX_LISTED = 32   # failing probe frames named in a verdict's reasons; the result lists every one


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
        if not entry["positions"]:
            reasons.append(f"{side}: no position to check between its corners ({corner} px left out at each end)")
        elif not entry["continuous"]:
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


# ---------- readings ----------
ABSENT = "<absent>"  # a JSON key one snapshot lacks


def focused_node(reading: dict, node: str | None = None, role: str | None = None, states=(), not_states=(),
                 same: dict | None = None) -> dict:
    """Whether an `atspi_focus` reading's focused node has this name and role, holds `states` and none of
    `not_states`, and, given `same` (another reading), matches its focused node in name, role and states."""
    focused, reasons = reading.get("focused"), []
    if reading.get("error"):
        reasons.append(reading["error"])
    if focused is None:
        reasons.append("AT-SPI reported no focused node")
    else:
        if node is not None and focused.get("name") != node:
            reasons.append(f"the focused node is {focused.get('name')!r}, not {node!r}")
        if role is not None and focused.get("role") != role:
            reasons.append(f"the focused node is a {focused.get('role')!r}, not a {role!r}")
        held = set(focused.get("states", []))
        lacking = [state for state in states if state not in held]
        if lacking:
            reasons.append(f"the focused node lacks {', '.join(lacking)}")
        unwanted = [state for state in not_states if state in held]
        if unwanted:
            reasons.append(f"the focused node is {', '.join(unwanted)}")
    other = None
    if same is not None:
        other = same.get("focused")
        if other is None:
            reasons.append(f"reading {same.get('label')!r} has no focused node to match")
        elif focused is not None:
            differ = [key for key in ("name", "role") if focused.get(key) != other.get(key)]
            if sorted(focused.get("states", [])) != sorted(other.get("states", [])):
                differ.append("states")
            if differ:
                reasons.append(f"the focused node differs from reading {same.get('label')!r} in {', '.join(differ)}"
                               f": {focused} against {other}")
    return dict(application=reading.get("application"), focused=focused, all_focused=reading.get("all_focused", []),
                same_as=other, passed=not reasons, reasons=reasons)


def json_at(value, key: str):
    """The value at a dotted `key` (`settings.theme`, `recent_repositories.0`), or ABSENT."""
    for part in key.split("."):
        if isinstance(value, dict) and part in value:
            value = value[part]
        elif isinstance(value, list) and part.isdigit() and int(part) < len(value):
            value = value[int(part)]
        else:
            return ABSENT
    return value


def json_changes(a, b, prefix: str = "", limit: int = 32) -> list[str]:
    """The dotted keys whose values differ between two parsed JSON documents, at most `limit`."""
    if isinstance(a, dict) and isinstance(b, dict):
        found = []
        for key in sorted(set(a) | set(b), key=str):
            if a.get(key, ABSENT) != b.get(key, ABSENT):
                found += json_changes(a.get(key, ABSENT), b.get(key, ABSENT), f"{prefix}{key}.", limit - len(found))
            if len(found) >= limit:
                break
        return found[:limit]
    return [] if a == b else [prefix.rstrip(".") or "(the whole document)"]


def store_compare(a: dict, b: dict, keys=None) -> dict:
    """Two `store_snapshot` readings: byte-identical, or, with `keys`, equal at each dotted key of their JSON.

    `changed_keys` lists every key that differs, for the record; `rewritten`
    says whether the file was written between them (another mtime or inode),
    which only the byte or key verdict judges.
    """
    def summary(snapshot):
        return {key: snapshot.get(key) for key in ("label", "exists", "stable", "sha256", "bytes")}

    reasons = [f"snapshot {s.get('label')!r} never settled" for s in (a, b) if not s.get("stable", False)]
    identical = (a.get("exists"), a.get("sha256")) == (b.get("exists"), b.get("sha256"))
    parsed = all(s.get("json") is not None or not s.get("exists") for s in (a, b))
    changed = json_changes(a.get("json"), b.get("json")) if parsed else None
    differing = []
    if keys is None:
        if not identical:
            reasons.append(f"the store's bytes differ: {(a.get('sha256') or 'absent')[:12]} ({a.get('bytes')} bytes) "
                           f"against {(b.get('sha256') or 'absent')[:12]} ({b.get('bytes')} bytes)"
                           + (f"; changed keys {', '.join(changed[:8])}" if changed else ""))
    elif not parsed:
        reasons.append("a snapshot is not JSON, so its keys cannot be compared")
    else:
        for key in keys:
            before, after = json_at(a.get("json"), key), json_at(b.get("json"), key)
            if before != after:
                differing.append(dict(key=key, a=before, b=after))
                reasons.append(f"{key}: {before!r} against {after!r}")
    rewritten = (a.get("mtime_ns"), a.get("inode")) != (b.get("mtime_ns"), b.get("inode"))
    return dict(a=summary(a), b=summary(b), identical=identical, rewritten=rewritten, changed_keys=changed,
                keys=list(keys) if keys is not None else None, differing=differing, passed=not reasons,
                reasons=reasons)


def reading_refs(entry: dict) -> list[str]:
    """The readings a normalised analysis entry reads, in a stable order; none for a frame analysis."""
    if entry["kind"] == "atspi_focus":
        return [entry["focus"]] + ([entry["same_as"]] if entry.get("same_as") else [])
    if entry["kind"] == "store_compare":
        return [entry["a"], entry["b"]]
    return []


def evaluate_readings(entry: dict, reading) -> dict:
    """Run one reading analysis; `reading(ref)` returns that reading, or None if it is missing."""
    found = {ref: reading(ref) for ref in reading_refs(entry)}
    missing = [ref for ref, value in found.items() if value is None]
    if missing:
        return dict(passed=False, missing=missing, reasons=[f"reading {ref} is missing" for ref in missing])
    wrong = [ref for ref, value in found.items() if value.get("kind") != READING_KINDS[entry["kind"]]]
    if wrong:
        return dict(passed=False, reasons=[f"reading {ref} comes from {found[ref].get('kind')}, but {entry['kind']} "
                                           f"reads only {READING_KINDS[entry['kind']]} readings" for ref in wrong])
    if entry["kind"] == "atspi_focus":
        return focused_node(found[entry["focus"]], entry.get("node"), entry.get("role"), entry["states"],
                            entry["not_states"], found.get(entry.get("same_as")))
    return store_compare(found[entry["a"]], found[entry["b"]], entry.get("keys"))


# ---------- probes ----------
def colour_mask(image, colour, tolerance: int = TOLERANCE):
    """An L image, 255 wherever every channel is within `tolerance` of `colour`."""
    from PIL import ImageChops

    channels = [band.point([255 if abs(value - target) <= tolerance else 0 for value in range(256)])
                for band, target in zip(image.convert("RGB").split(), colour[:3])]
    return ImageChops.darker(ImageChops.darker(channels[0], channels[1]), channels[2])


def long_runs(mask, length: int = RUN):
    """The pixels of an L mask that lie in a horizontal or vertical run of at least `length` set pixels."""
    from PIL import Image, ImageChops

    pattern = re.compile(rb"\xff{%d,}" % length)

    def rows(image):
        width, height = image.size
        data, out = image.tobytes(), bytearray(width * height)
        for y in range(height):
            for found in pattern.finditer(data, y * width, (y + 1) * width):
                out[found.start():found.end()] = b"\xff" * (found.end() - found.start())
        return Image.frombytes("L", (width, height), bytes(out))

    transposed = rows(mask.transpose(Image.Transpose.TRANSPOSE)).transpose(Image.Transpose.TRANSPOSE)
    return ImageChops.lighter(rows(mask), transposed)


def accent_colour(image, min_chroma: int = MIN_CHROMA, minimum: int = MIN_RING_PIXELS):
    """The most frequent colour of chroma at least `min_chroma` covering `minimum` pixels, or None."""
    image = image.convert("RGB")
    found = [(count, colour) for count, colour in image.getcolors(image.width * image.height)
             if count >= minimum and chroma(colour) >= min_chroma]
    return tuple(max(found)[1]) if found else None


def ring_regions(image, colour, tolerance: int = TOLERANCE, run: int = RUN,
                 minimum: int = MIN_RING_PIXELS) -> list[tuple[tuple[int, int, int, int], int]]:
    """Every ring-shaped region of `colour` in `image`: its outer box and run pixels, the most pixels first.

    A ring's sides are straight runs of its colour at least `run` px long
    (its rounded corners blend, so the runs stop short of them); runs within
    12 px of each other group into one box (`frames.regions`), and a group of
    at least `minimum` run pixels counts as a ring.
    """
    bands = long_runs(colour_mask(image, colour, tolerance), run)
    if bands.getbbox() is None:
        return []
    found = [(box, bands.crop(box).histogram()[255]) for box in frames.regions(bands)]
    return sorted((item for item in found if item[1] >= minimum), key=lambda item: (-item[1], item[0]))


def locate_ring(image, colour, tolerance: int = TOLERANCE, run: int = RUN):
    """The outer box of the largest ring of `colour` in `image` (`ring_regions`), or None."""
    regions = ring_regions(image, colour, tolerance, run)
    return regions[0][0] if regions else None


def prepare_clip(image, offset=(0, 0), surface=None, masks=()):
    """`image`, a frame cut to a clip whose top-left is `offset` in the window, with `masks` (window boxes, such as
    a caret) painted in the list surface, and that surface: `surface` as a colour, `{"at": [x, y]}` in the window,
    or None for the clip's most frequent colour."""
    image = image.convert("RGB")
    if surface is None:
        surface = modal(image, (0, 0, *image.size))[0]
    elif isinstance(surface, dict):
        surface = image.getpixel((surface["at"][0] - offset[0], surface["at"][1] - offset[1]))
    surface = tuple(surface[:3])
    if masks:
        image = image.copy()
        for x0, y0, x1, y1 in masks:
            box = (max(x0 - offset[0], 0), max(y0 - offset[1], 0), min(x1 - offset[0], image.width),
                   min(y1 - offset[1], image.height))
            if box[0] < box[2] and box[1] < box[3]:
                image.paste(surface, box)
    return image, surface


def ring_in_clip(image, offset=(0, 0), colour=None, surface=None, tolerance: int = TOLERANCE, corner: int = CORNER,
                 min_width: int = 1, uniform_width: bool = True, min_contrast: float | None = None,
                 max_outlines: int | None = None, masks=()) -> dict:
    """The focused ring wherever it lies in `image`, a frame already cut to a list's clip whose top-left is
    `offset` in the window.

    Nothing outside the clip is seen, so a ring that reaches past it loses
    that side. The ring of `colour` (None: the clip's most frequent accent)
    passes as `ring_sides` judges its located box: all four sides
    continuous, at least `min_width` and, with `uniform_width`, one width (a
    side cut to 1 of 2 px fails), within `max_outlines`; and its colour at
    least `min_contrast` against the list surface (`prepare_clip`) and
    against what lies just outside each side, where that is inside the clip.
    It fails when the clip holds a second ring of that colour (an old focus
    left drawn beside the new one; `rings` lists every one), or when the
    clip's most frequent accent is another colour (`accent`). `masks`
    (window boxes, such as a text field's blinking caret) are painted with
    the surface first, so nothing in them counts. `clip_margins` gives the
    px between the ring's outer box and each edge of the clip.
    """
    image, surface = prepare_clip(image, offset, surface, masks)
    x, y = offset
    accent = accent_colour(image)
    if colour is None:
        colour = accent
        if colour is None:
            return dict(colour=None, accent=None, box=None, rings=[], passed=False,
                        reasons=[f"no accent colour (chroma >= {MIN_CHROMA}, {MIN_RING_PIXELS} px) in the clip"])
    colour = tuple(colour[:3])
    reasons = []
    if accent is not None and not near(accent, colour, tolerance):
        reasons.append(f"the clip's most frequent accent {list(accent)} is not the ring colour {list(colour)}")
    regions = ring_regions(image, colour, tolerance)
    rings = [[box[0] + x, box[1] + y, box[2] + x, box[3] + y] for box, _ in regions]
    if not regions:
        return dict(colour=list(colour), accent=None if accent is None else list(accent), box=None, rings=[],
                    passed=False,
                    reasons=[f"no ring of {list(colour)} in the clip (no straight run of {RUN} px)", *reasons])
    found = regions[0][0]
    result = ring_sides(image, found, colour, SIDES, REACH, corner, tolerance, min_width, uniform_width, None,
                        max_outlines)
    contrast = frames.contrast(colour, surface)
    reasons = [*result["reasons"], *reasons]
    if len(regions) > 1:
        reasons.append(f"{len(regions)} rings of {list(colour)} in the clip, where only the focused control has one: "
                       f"{rings}")
    if min_contrast is not None:
        if contrast < min_contrast:
            reasons.append(f"contrast {contrast} against the list surface {list(surface)}, under {min_contrast}")
        for side, entry in result["sides"].items():
            if entry["contrast"] is not None and entry["contrast"] < min_contrast:
                reasons.append(f"{side}: contrast {entry['contrast']} against {entry['surface']} just outside the "
                               f"ring, under {min_contrast}")
    width, height = image.size
    keep = ("positions", "gaps", "width", "width_min", "width_max", "outlines", "contrast")
    return dict(colour=list(colour), accent=None if accent is None else list(accent), surface=list(surface),
                contrast=contrast, box=rings[0], rings=rings,
                clip_margins=dict(top=found[1], right=width - found[2], bottom=height - found[3], left=found[0]),
                sides={side: {key: entry[key] for key in keep} for side, entry in result["sides"].items()},
                passed=not reasons, reasons=reasons)


def _image(frame):
    image = frame["image"]
    return image() if callable(image) else image


def _local(box, region):
    """`box` in the coordinates of a frame grabbed from `region`, or None when it is not inside the region."""
    x0, y0, x1, y1 = box
    rx0, ry0, rx1, ry1 = region
    if not (rx0 <= x0 < x1 <= rx1 and ry0 <= y0 < y1 <= ry1):
        return None
    return x0 - rx0, y0 - ry0, x1 - rx0, y1 - ry0


TIMING = ("index", "ms", "previous_ms", "last_ms", "grabs", "file")
PRESS = ("press", "settled_before_s", "reaimed", "changed", "settled", "truncated", "ended", "send_ms",
         "first_grab_ms", "first_change_ms", "last_change_ms", "end_ms", "grabs", "bytes", "interval_ms",
         "resolution_ms", "released")


def probe_problems(probe: dict) -> list[str]:
    """Why a probe's frames are not every frame from each press until the region settled; any of them leaves an
    analysis of the probe inconclusive."""
    problems = []
    if probe.get("stable_within") and probe.get("settled_before_s") is None:
        problems.append(f"the region had not settled within {probe['stable_within']} s before the first press")
    for press in probe["presses"]:
        if press["press"] > 1 and probe.get("stable_within") and "settled_before_s" in press \
                and press["settled_before_s"] is None:  # a click probe waits again after aiming at its point
            problems.append(f"the region had not settled within {probe['stable_within']} s before press "
                            f"{press['press']}")
        if not press["changed"]:
            problems.append(f"press {press['press']} changed nothing in the probe's region within "
                            f"{probe.get('timeout')} s")
        elif press.get("truncated"):
            problems.append(f"press {press['press']} was truncated by its {press['ended']} {press.get('end_ms')} ms "
                            "after the press, before its region settled")
        elif not press["settled"]:
            problems.append(f"press {press['press']} did not settle: it ended by {press['ended']} "
                            f"{press.get('end_ms')} ms after the press")
    return problems


def _probe_result(probe: dict, clip, presses: list, failing: list, count: int, what: str) -> dict:
    problems = probe_problems(probe)
    reasons = list(problems)
    for item in failing[:MAX_LISTED]:
        reasons.append(f"press {item['press']} frame {item['index']} at {item['ms']} ms: "
                       f"{'; '.join(item['reasons'])}")
    if len(failing) > MAX_LISTED:
        reasons.append(f"and {len(failing) - MAX_LISTED} more failing frames")
    resolution = probe.get("resolution_ms")
    if problems:
        summary = f"inconclusive: {problems[0]}"
    elif not reasons:
        summary = (f"{count} frames after {len(presses)} presses, {what} in every one; no frame failed at "
                   f"{resolution} ms resolution (the longest time without a grab)")
    else:
        summary = f"{len(failing)} of {count} frames after {len(presses)} presses failed at {resolution} ms resolution"
    return dict(probe=probe.get("probe"), clip=list(clip), region=probe.get("region"), presses=presses, frames=count,
                failing=failing, grabs=probe.get("grabs"), interval_ms=probe.get("interval_ms"),
                resolution_ms=resolution, summary=summary, inconclusive=bool(problems), passed=not reasons,
                reasons=reasons)


def press_colour(press: dict, local, offset, surface=None, masks=()):
    """A press's ring colour and the index of the frame it came from: the most frequent accent of the settled
    (last) frame, or else of the first frame after the press that shows one; (None, None) without any."""
    after = press["frames"][1:]
    for frame in after[-1:] + after[:-1]:
        image, _ = prepare_clip(_image(frame).crop(local), offset, surface, masks)
        found = accent_colour(image)
        if found is not None:
            return found, frame["index"]
    return None, None


def probe_ring(probe: dict, clip, colour=None, surface=None, tolerance: int = TOLERANCE, corner: int = CORNER,
               min_width: int = 1, uniform_width: bool = True, min_contrast: float | None = None,
               max_outlines: int | None = None, masks=()) -> dict:
    """`ring_in_clip` on every frame a probe kept after each press, cut to `clip` (window pixels), with `masks`
    (window boxes, such as a caret) painted over with the list surface.

    With `colour` None, each press's ring colour is detected once
    (`press_colour`) and every frame of the press is judged against it, so a
    frame painting another accent fails. A press passes when every frame
    drawn after it shows that one ring, whole, inside the clip. A press that
    changed nothing, never settled or was truncated leaves the analysis
    `inconclusive` and failed, since its frames are not every frame up to the
    settled one. Each failing frame is listed with its timing: first seen
    `ms` after the press (its send returned at 0), not yet seen at
    `previous_ms`, last seen at `last_ms`.
    """
    local = _local(clip, probe["region"])
    if local is None:
        return dict(passed=False, reasons=[f"the clip {list(clip)} is not inside the probe's region {probe['region']}"])
    presses, failing, count = [], [], 0
    for press in probe["presses"]:
        if colour is not None:
            judged, source = tuple(colour[:3]), "spec"
        else:
            judged, source = press_colour(press, local, clip[:2], surface, masks)
        entries = []
        for frame in press["frames"][1:]:
            result = ring_in_clip(_image(frame).crop(local), clip[:2], judged, surface, tolerance, corner, min_width,
                                  uniform_width, min_contrast, max_outlines, masks)
            count += 1
            entries.append({**{key: frame.get(key) for key in TIMING}, **result})
            if not result["passed"]:
                failing.append(dict(press=press["press"], **{key: frame.get(key) for key in TIMING},
                                    box=result["box"], reasons=result["reasons"]))
        presses.append({**{key: press.get(key) for key in PRESS}, "colour": None if judged is None else list(judged),
                        "colour_from": source, "frames": entries})
    result = _probe_result(probe, clip, presses, failing, count, "the one ring whole inside the clip")
    result["masks"] = [list(box) for box in masks]
    return result


def _differing(a, b, masks) -> tuple[int, object]:
    from PIL import ImageDraw

    mask = frames.difference_mask(a, b)
    draw = ImageDraw.Draw(mask)
    for x0, y0, x1, y1 in masks:
        draw.rectangle((x0, y0, x1 - 1, y1 - 1), fill=0)
    return mask.histogram()[255], mask


def probe_endpoints(probe: dict, clip, masks=(), max_pixels: int = 0) -> dict:
    """Every frame a probe kept after each press, cut to `clip`, equals that press's frame before the key or its
    settled frame (its last), within `max_pixels` differing pixels outside `masks` (window boxes).

    Any other frame is `intermediate`, and one equal to the frame before the
    key after the settled frame was shown is `reverted`; both are listed with
    their timing and the box where they differ from the settled frame. A
    press that changed nothing, never settled or was truncated leaves the
    analysis inconclusive, as in `probe_ring`.
    """
    local = _local(clip, probe["region"])
    if local is None:
        return dict(passed=False, reasons=[f"the clip {list(clip)} is not inside the probe's region {probe['region']}"])
    shifted = [(x0 - clip[0], y0 - clip[1], x1 - clip[0], y1 - clip[1]) for x0, y0, x1, y1 in masks]
    presses, failing, count = [], [], 0
    for press in probe["presses"]:
        kept = press["frames"]
        before, settled = _image(kept[0]).crop(local), _image(kept[-1]).crop(local)
        entries, reached = [], False
        for frame in kept[1:]:
            image = _image(frame).crop(local)
            from_before, _ = _differing(before, image, shifted)
            from_settled, mask = _differing(settled, image, shifted)
            count += 1
            if from_settled <= max_pixels:
                state = "settled"
            elif from_before <= max_pixels:
                state = "reverted" if reached else "before"
            else:
                state = "intermediate"
            reached = reached or state == "settled"
            entry = {**{key: frame.get(key) for key in TIMING}, "state": state, "from_before": from_before,
                     "from_settled": from_settled}
            if state in ("intermediate", "reverted"):
                x0, y0, x1, y1 = mask.getbbox()
                entry["bbox"] = [x0 + clip[0], y0 + clip[1], x1 + clip[0], y1 + clip[1]]
                failing.append(dict(press=press["press"], **{key: frame.get(key) for key in TIMING},
                                    bbox=entry["bbox"], reasons=[f"{state}: {from_before} px from the frame before "
                                                                 f"the key, {from_settled} from the settled frame"]))
            entries.append(entry)
        presses.append({**{key: press.get(key) for key in PRESS}, "frames": entries})
    result = _probe_result(probe, clip, presses, failing, count, "the frame before the press or the settled frame")
    result.update(max_pixels=max_pixels, masks=[list(box) for box in masks])
    return result


# ---------- scenario entries ----------
def frame_refs(entry: dict) -> list[str]:
    """The frames (or, for a probe analysis, the probe) a normalised analysis entry reads, in a stable order;
    none for a reading analysis."""
    if entry["kind"] in READING_KINDS:
        return []
    if entry["kind"] in PROBE_KINDS:
        return [entry["probe"]]
    if entry["kind"] == "compare":
        return [entry["a"], entry["b"]]
    refs = [entry["frame"]]
    reference = entry.get("reference") or {}
    if reference.get("frame") and reference["frame"] not in refs:
        refs.append(reference["frame"])
    return refs


def refs(entry: dict) -> list[str]:
    """Every frame, probe and reading a normalised analysis entry reads."""
    return frame_refs(entry) + reading_refs(entry)


def evaluate(entry: dict, resolve, reading=None) -> dict:
    """Run one normalised analysis entry; `resolve(ref)` returns that frame as an image (for a probe analysis, the
    probe's record with its frames) and `reading(ref)` that reading, or None if it is missing."""
    if entry["kind"] in READING_KINDS:
        return evaluate_readings(entry, reading or (lambda ref: None))
    images = {ref: resolve(ref) for ref in frame_refs(entry)}
    missing = [ref for ref, image in images.items() if image is None]
    kind = entry["kind"]
    if missing:
        what = "probe" if kind in PROBE_KINDS else "frame"
        return dict(passed=False, missing=missing, reasons=[f"{what} {ref} is missing" for ref in missing])
    if kind == "probe_ring":
        return probe_ring(images[entry["probe"]], entry["clip"], None if entry["colour"] == "detect" else entry["colour"],
                          entry["surface"], entry["tolerance"], entry["corner"], entry["min_width"],
                          entry["uniform_width"], entry.get("min_contrast"), entry.get("max_outlines"), entry["masks"])
    if kind == "probe_endpoints":
        return probe_endpoints(images[entry["probe"]], entry["clip"], entry["masks"], entry["max_pixels"])
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
