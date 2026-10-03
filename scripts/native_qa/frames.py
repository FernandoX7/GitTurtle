"""Base versus candidate frame comparison with masked regions, scaled and composed crops, and small pixel helpers.

A mask removes a region from the verdict but not from the report: masked
differences are counted separately, so a mask never hides that something
changed there. Boxes are PIL boxes, `(x0, y0, x1, y1)` with exclusive ends.
"""

from __future__ import annotations

from pathlib import Path

BLOCK = 4  # difference cells, in pixels
GAP = 3    # cells: differences within 12 px are one region
SCALES = range(2, 9)    # a committed crop's nearest-neighbour magnification; the privacy scan undoes these
MAX_COMPOSED = 2048     # px on each side of a scaled or composed crop: it magnifies details, not screens


def status_timing(width: int, height: int) -> tuple[int, int, int, int]:
    """The timing digits of the left-aligned status-bar message, at scale 1.

    History and Changes print a per-launch timing there ("2 changed files ·
    file list 2.9 ms"). Launches of one build differed in x 127-169, y 663-671
    at 1000x680; the box leaves room for a longer count or timing. Another
    message, layout or scale needs an explicit rectangle.
    """
    return (96, height - 18, 208, height - 7)


NAMED_MASKS = {"status-timing": status_timing}


def parse_mask(spec: str) -> str | tuple[int, int, int, int]:
    """A named mask, or `x0,y0,x1,y1` with x0 < x1 and y0 < y1."""
    if spec in NAMED_MASKS:
        return spec
    try:
        box = tuple(int(part) for part in spec.split(","))
    except ValueError:
        box = ()
    if len(box) != 4 or box[0] >= box[2] or box[1] >= box[3] or min(box) < 0:
        raise ValueError(f"mask {spec!r} is neither {'/'.join(NAMED_MASKS)} nor x0,y0,x1,y1")
    return box


def resolve_masks(masks, size: tuple[int, int]) -> list[tuple[int, int, int, int]]:
    return [NAMED_MASKS[mask](*size) if isinstance(mask, str) else tuple(mask) for mask in masks]


def difference_mask(a, b):
    """An L image, 255 wherever any channel differs."""
    from PIL import ImageChops

    red, green, blue = ImageChops.difference(a.convert("RGB"), b.convert("RGB")).split()
    return ImageChops.lighter(ImageChops.lighter(red, green), blue).point(lambda v: 255 if v else 0)


def regions(mask) -> list[tuple[int, int, int, int]]:
    """Tight boxes of the differing pixels, grouping those within GAP cells of each other."""
    width, height = mask.size
    cells = mask.reduce(BLOCK)
    pixels = cells.load()
    cw, ch = cells.size
    seen, boxes = set(), []
    for y in range(ch):
        for x in range(cw):
            if not pixels[x, y] or (x, y) in seen:
                continue
            stack, members = [(x, y)], []
            seen.add((x, y))
            while stack:
                cx, cy = stack.pop()
                members.append((cx, cy))
                for nx in range(max(0, cx - GAP), min(cw, cx + GAP + 1)):
                    for ny in range(max(0, cy - GAP), min(ch, cy + GAP + 1)):
                        if pixels[nx, ny] and (nx, ny) not in seen:
                            seen.add((nx, ny))
                            stack.append((nx, ny))
            xs = [cell[0] for cell in members]
            ys = [cell[1] for cell in members]
            box = (min(xs) * BLOCK, min(ys) * BLOCK, min(width, (max(xs) + 1) * BLOCK),
                   min(height, (max(ys) + 1) * BLOCK))
            tight = mask.crop(box).getbbox()
            boxes.append((box[0] + tight[0], box[1] + tight[1], box[0] + tight[2], box[1] + tight[3]))
    return sorted(boxes, key=lambda box: (box[1], box[0]))


def compare(base, candidate, masks=()) -> dict:
    """`identical` (outside masks), `different`, or `size-mismatch`, with counts and regions."""
    from PIL import ImageDraw

    report = dict(base_size=list(base.size), candidate_size=list(candidate.size))
    if base.size != candidate.size:
        report.update(result="size-mismatch", differing_pixels=None)
        return report
    boxes = resolve_masks(masks, base.size)
    mask = difference_mask(base, candidate)
    total = mask.histogram()[255]
    draw = ImageDraw.Draw(mask)
    for box in boxes:
        draw.rectangle((box[0], box[1], box[2] - 1, box[3] - 1), fill=0)
    unmasked = mask.histogram()[255]
    report.update(result="identical" if unmasked == 0 else "different", differing_pixels=unmasked,
                  masked_pixels=total - unmasked, masks=[list(box) for box in boxes],
                  bbox=list(mask.getbbox()) if unmasked else None,
                  regions=[list(box) for box in regions(mask)] if unmasked else [])
    return report


def compare_files(base: Path, candidate: Path, masks=()) -> dict:
    from PIL import Image

    with Image.open(base) as a, Image.open(candidate) as b:
        report = compare(a.convert("RGB"), b.convert("RGB"), masks)
    return dict(base=str(base), candidate=str(candidate), **report)


def pairs(base: Path, candidate: Path) -> list[tuple[Path, Path]]:
    """Two files, or every image named alike in two directories (a name on one side only is a pair with a gap)."""
    if base.is_dir() and candidate.is_dir():
        names = sorted({p.name for p in base.iterdir() if p.suffix.lower() in (".png", ".jpg", ".jpeg")}
                       | {p.name for p in candidate.iterdir() if p.suffix.lower() in (".png", ".jpg", ".jpeg")})
        return [(base / name, candidate / name) for name in names]
    if base.is_dir() or candidate.is_dir():
        raise SystemExit("compare needs two files or two directories")
    return [(base, candidate)]


# ---------- scaled and composed crops ----------
def composed_size(boxes, scale: int = 1, gap: int = 0) -> tuple[int, int]:
    """The size of `compose`'s image: the boxes side by side, `gap` px apart, as tall as the tallest, times `scale`."""
    width = sum(x1 - x0 for x0, _, x1, _ in boxes) + gap * (len(boxes) - 1)
    height = max(y1 - y0 for _, y0, _, y1 in boxes)
    return width * scale, height * scale


def compose(image, boxes, scale: int = 1, gap: int = 0, fill=None):
    """`boxes` of `image` side by side, left to right and top-aligned, `gap` px apart, then magnified `scale` times
    by nearest neighbour, so every source pixel becomes one exact `scale` x `scale` block; with the fill colour used.

    The gap and the space under a box shorter than the tallest are painted
    `fill`: a colour, `{"at": [x, y]}` (that pixel of `image`), or None for the
    most frequent colour of all the boxes' pixels (ties: the larger colour),
    usually the surface they share. Top alignment keeps row r of every box at
    row r of the composite. One box has no gap or fill, and its fill is None.
    """
    from collections import Counter

    from PIL import Image

    image = image.convert("RGB")
    pieces = [image.crop(tuple(box)) for box in boxes]
    colour = None
    if len(pieces) == 1:
        canvas = pieces[0]
    else:
        if fill is None:
            counts: Counter = Counter()
            for piece in pieces:
                counts.update({found: count for count, found in piece.getcolors(piece.width * piece.height)})
            colour = max(counts.items(), key=lambda item: (item[1], item[0]))[0]
        elif isinstance(fill, dict):
            colour = image.getpixel(tuple(fill["at"]))[:3]
        else:
            colour = tuple(fill[:3])
        canvas = Image.new("RGB", composed_size(boxes, 1, gap), colour)
        left = 0
        for piece in pieces:
            canvas.paste(piece, (left, 0))
            left += piece.width + gap
    if scale > 1:
        canvas = canvas.resize((canvas.width * scale, canvas.height * scale), Image.Resampling.NEAREST)
    return canvas, None if colour is None else tuple(colour)


def magnifications(image) -> list[tuple[int, object]]:
    """Every factor of SCALES by which `image` is an exact nearest-neighbour magnification, every factor x factor
    block one colour, with the image it magnifies at that factor, smallest factor first; none for most frames.

    The privacy scan also reads the image at each of them: a 4x crop of
    content that is itself 2x2-blocky is an exact 8x magnification as well,
    and only its 1/4 reading has the app's own size, which its templates
    were cut at.
    """
    from PIL import Image

    width, height = image.size
    data, found = image.tobytes(), []
    for factor in SCALES:
        if width % factor or height % factor:
            continue
        small = image.resize((width // factor, height // factor), Image.Resampling.NEAREST)
        if small.resize(image.size, Image.Resampling.NEAREST).tobytes() == data:
            found.append((factor, small))
    return found


# ---------- measurement helpers for drivers ----------
def modal(image, box, exclude=None):
    """The most frequent colour in `box` (optionally skipping `exclude`), its count and the total."""
    counts: dict = {}
    x0, y0, x1, y1 = box
    for y in range(y0, y1):
        for x in range(x0, x1):
            if exclude and exclude[0] <= x < exclude[2] and exclude[1] <= y < exclude[3]:
                continue
            pixel = image.getpixel((x, y))
            counts[pixel] = counts.get(pixel, 0) + 1
    if not counts:
        return None, 0, 0
    colour, count = max(counts.items(), key=lambda item: item[1])
    return colour, count, sum(counts.values())


def luminance(rgb) -> float:
    def channel(value: int) -> float:
        value /= 255
        return value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4

    red, green, blue = (channel(v) for v in rgb[:3])
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue


def contrast(a, b) -> float:
    """WCAG contrast ratio of two 8-bit RGB colours."""
    la, lb = luminance(a), luminance(b)
    return round((max(la, lb) + 0.05) / (min(la, lb) + 0.05), 4)


def find_hline(image, colour, min_length: int = 300, top: int = 0, min_x: int = 0):
    """The first row at or below `top` holding a run of `colour` at least `min_length` long.

    Drivers use it to find a card by its border instead of hard-coding a scroll offset.
    Returns (y, x0, x1) or None.
    """
    for y in range(top, image.height):
        run = best = start = best_start = 0
        for x in range(image.width):
            if image.getpixel((x, y)) == colour:
                if run == 0:
                    start = x
                run += 1
                if run > best:
                    best, best_start = run, start
            else:
                run = 0
        if best >= min_length and best_start >= min_x:
            return y, best_start, best_start + best
    return None
