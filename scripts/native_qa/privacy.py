"""Scan frames for personal strings by template matching before they are committed.

The repository is public, so no tracked file holds the strings themselves.
Templates come at run time from a local directory (cropped from a frame that
showed the string, with `privacy crop`) or are rendered from `--text` with a
supplied font; a template directory inside a Git work tree must be ignored.

Matching is zero-mean normalized cross-correlation, so light-on-dark and
dark-on-light renderings of the same text both score (the sign is reported).
A 1000x680 frame against ~30 templates of ~100x12 px is ~10^10 multiply-adds,
minutes per frame in pure Python, so the default engine is `ncc.c`, compiled
once per source digest into a cache directory outside the repository. The
pure-Python engine implements the same rules for hosts without a compiler
and for the tests.

Automated callers (the local gate and CI) load templates anonymously, so a
template's file name, which can itself be private, never reaches their output.
`pack_templates` stores only the grayscale pixels, in a compact form small
enough for a GitHub repository secret.
"""

from __future__ import annotations

import base64
import hashlib
import lzma
import math
import os
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from operator import mul
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "ncc.c"
THRESHOLD = 0.80
FLAT_VARIANCE = 16.0  # per pixel: windows with a standard deviation under 4 cannot hold text
MAX_HITS = 4096
LISTED_HITS = 20
IMAGE_SUFFIXES = (".png", ".jpg", ".jpeg")
PACK_MAGIC = b"GTPRIV1\n"
SECRET_LIMIT = 48_000  # GitHub limits a secret to 48 KB; this takes the smaller reading of KB
MAX_PACKED_TEMPLATES = 256
MAX_TEMPLATE_SIDE = 2048
MAX_UNPACKED_BYTES = 16 * 1024 * 1024


# ---------- engines ----------
def ncc_python(frame: bytes, width: int, height: int, template: bytes, tw: int, th: int,
               threshold: float = THRESHOLD) -> dict:
    n = tw * th
    mean = sum(template) / n
    centred = [value - mean for value in template]
    norm = math.sqrt(sum(value * value for value in centred))
    rows = [centred[i * tw:(i + 1) * tw] for i in range(th)]
    stride = width + 1
    sums = [0] * (stride * (height + 1))
    squares = [0] * (stride * (height + 1))
    for y in range(height):
        row_sum = row_square = 0
        for x in range(width):
            value = frame[y * width + x]
            row_sum += value
            row_square += value * value
            i = (y + 1) * stride + x + 1
            sums[i] = sums[i - stride] + row_sum
            squares[i] = squares[i - stride] + row_square
    best, best_at, negative, hits = 0.0, (-1, -1), False, []
    for y in range(0, height - th + 1 if norm > 0 else 0):
        for x in range(width - tw + 1):
            i0, i1 = y * stride + x, (y + th) * stride + x
            s = sums[i1 + tw] - sums[i1] - sums[i0 + tw] + sums[i0]
            q = squares[i1 + tw] - squares[i1] - squares[i0 + tw] + squares[i0]
            variance = q - s * s / n
            if variance < FLAT_VARIANCE * n:
                continue
            acc = 0.0
            for ty in range(th):
                start = (y + ty) * width + x
                acc += sum(map(mul, frame[start:start + tw], rows[ty]))
            c = acc / (math.sqrt(variance) * norm)
            if abs(c) > best:
                best, best_at, negative = abs(c), (x, y), c < 0
            if abs(c) >= threshold:
                for hit in hits:
                    if abs(hit[0] - x) < tw and abs(hit[1] - y) < th:
                        if abs(c) > abs(hit[2]):
                            hit[:] = [x, y, c]
                        break
                else:
                    if len(hits) < MAX_HITS:
                        hits.append([x, y, c])
    return dict(best=round(best, 4), at=list(best_at), sign="-" if negative else "+", hits=len(hits),
                hit_list=[[x, y, round(c, 4)] for x, y, c in hits[:LISTED_HITS]])


def cache_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME", "")
    root = Path(base) if base and Path(base).is_absolute() else Path.home() / ".cache"
    return root / "gitturtle-native-qa"


def helper(build: bool = True) -> Path | None:
    """The compiled `ncc` for this source digest, built on first use; None without a compiler."""
    digest = hashlib.sha256(SOURCE.read_bytes()).hexdigest()[:16]
    binary = cache_dir() / f"ncc-{digest}"
    if binary.is_file() and os.access(binary, os.X_OK):
        return binary
    compiler = os.environ.get("CC") or shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")
    if not build or not compiler:
        return None
    binary.parent.mkdir(parents=True, exist_ok=True)
    partial = binary.with_name(f"{binary.name}.{os.getpid()}.partial")
    subprocess.run([compiler, "-O2", "-o", str(partial), str(SOURCE), "-lm"], check=True)
    os.replace(partial, binary)
    return binary


def parse_ncc_output(text: str, names: list[str]) -> dict:
    results, current = {}, None
    for line in text.splitlines():
        parts = line.split()
        if parts[0] == "tpl":
            current = names[int(parts[1])]
            results[current] = dict(best=float(parts[3]), at=[int(parts[4]), int(parts[5])], sign=parts[7],
                                     hits=int(parts[9]), hit_list=[])
        elif parts[0] == "hit" and current is not None:
            results[current]["hit_list"].append([int(parts[1]), int(parts[2]), float(parts[3])])
    missing = [name for name in names if name not in results]
    if missing:
        raise RuntimeError(f"ncc printed no result for {', '.join(missing)}")
    return results


def ncc_c(binary: Path, frame: bytes, width: int, height: int, templates: dict, threshold: float) -> dict:
    names = list(templates)
    with tempfile.TemporaryDirectory(prefix="gitturtle-privacy-") as scratch:
        scratch = Path(scratch)
        (scratch / "frame.raw").write_bytes(frame)
        argv = [str(binary), str(scratch / "frame.raw"), str(width), str(height), str(threshold)]
        for index, name in enumerate(names):
            data, tw, th = templates[name]
            (scratch / f"{index}.raw").write_bytes(data)
            argv += [str(scratch / f"{index}.raw"), str(tw), str(th)]
        output = subprocess.run(argv, capture_output=True, text=True, check=True).stdout
    return parse_ncc_output(output, names)


def match(frame: bytes, width: int, height: int, templates: dict, threshold: float = THRESHOLD,
          engine: str = "auto") -> tuple[dict, str]:
    """Per template name: best |ncc|, where, its sign and the hits at or above `threshold`."""
    binary = helper(build=engine != "python") if engine in ("auto", "c") else None
    if engine == "c" and binary is None:
        raise SystemExit("no C compiler for the ncc helper; use --engine python")
    if binary is not None:
        return ncc_c(binary, frame, width, height, templates, threshold), "c"
    if engine == "auto":
        print("warning: no C compiler; the pure-Python engine takes minutes per frame", file=sys.stderr)
    return {name: ncc_python(frame, width, height, data, tw, th, threshold)
            for name, (data, tw, th) in templates.items()}, "python"


# ---------- templates ----------
def git_unignored(path: Path) -> str | None:
    """The work tree that would track `path`, or None when it is outside one or ignored there."""
    path = path.resolve()
    probe = next(p for p in (path, *path.parents) if p.is_dir())
    top =subprocess.run(["git", "-C", str(probe), "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    if top.returncode != 0:
        return None
    ignored = subprocess.run(["git", "-C", top.stdout.strip(), "check-ignore", "-q", "--", str(path)],
                             capture_output=True)
    return None if ignored.returncode == 0 else top.stdout.strip()


def refuse_tracked(path: Path, what: str) -> None:
    tree = git_unignored(path)
    if tree is not None:
        raise SystemExit(f"refusing: {what} {path} is inside the work tree {tree} and not ignored; "
                         "keep personal-string templates out of any tracked path")


def gray(image) -> tuple[bytes, int, int]:
    image = image.convert("L")
    return image.tobytes(), image.width, image.height


def load_templates(directory: Path, anonymous: bool = False) -> dict:
    """Templates keyed by file stem, or by position (`t1`, `t2`, ...) so no name reaches the output."""
    from PIL import Image

    refuse_tracked(directory, "template directory")
    templates = {}
    paths = sorted(path for path in directory.iterdir() if path.suffix.lower() in IMAGE_SUFFIXES)
    for number, path in enumerate(paths, 1):
        try:
            with Image.open(path) as image:
                templates[f"t{number}" if anonymous else path.stem] = gray(image)
        except (OSError, ValueError):
            if not anonymous:
                raise
            raise SystemExit(f"template {number} in {directory} is not a readable image") from None
    if not templates:
        raise SystemExit(f"no PNG or JPEG templates in {directory}")
    return templates


def pack_templates(templates: dict) -> str:
    """Base64 of an xz stream of each template's size and grayscale pixels: no names, no file metadata."""
    records = bytearray()
    for _, (data, width, height) in sorted(templates.items()):
        if not (0 < width <= MAX_TEMPLATE_SIDE and 0 < height <= MAX_TEMPLATE_SIDE):
            raise SystemExit(f"a template is larger than {MAX_TEMPLATE_SIDE} px on a side")
        records += width.to_bytes(2, "big") + height.to_bytes(2, "big") + data
    if len(templates) > MAX_PACKED_TEMPLATES or len(records) > MAX_UNPACKED_BYTES:
        raise SystemExit("too many or too large templates to pack")
    packed = PACK_MAGIC + lzma.compress(bytes(records), preset=9 | lzma.PRESET_EXTREME)
    return base64.b64encode(packed).decode("ascii")


def unpack_templates(text: str) -> dict:
    """The templates `pack_templates` wrote, keyed `t1`, `t2`, ...; refuses anything malformed or unbounded."""
    corrupt = "the packed templates are malformed"
    try:
        raw = base64.b64decode("".join(text.split()), validate=True)
    except ValueError:
        raise SystemExit(corrupt) from None
    if not raw.startswith(PACK_MAGIC):
        raise SystemExit(corrupt)
    decompressor = lzma.LZMADecompressor(format=lzma.FORMAT_XZ)
    try:
        records = decompressor.decompress(raw[len(PACK_MAGIC):], max_length=MAX_UNPACKED_BYTES + 1)
    except lzma.LZMAError:
        raise SystemExit(corrupt) from None
    if len(records) > MAX_UNPACKED_BYTES or not decompressor.eof or decompressor.unused_data:
        raise SystemExit(corrupt)
    templates, offset = {}, 0
    while offset < len(records):
        width = int.from_bytes(records[offset:offset + 2], "big")
        height = int.from_bytes(records[offset + 2:offset + 4], "big")
        end = offset + 4 + width * height
        if (len(templates) == MAX_PACKED_TEMPLATES or offset + 4 > len(records) or end > len(records)
                or not (0 < width <= MAX_TEMPLATE_SIDE and 0 < height <= MAX_TEMPLATE_SIDE)):
            raise SystemExit(corrupt)
        templates[f"t{len(templates) + 1}"] = (records[offset + 4:end], width, height)
        offset = end
    if not templates:
        raise SystemExit(corrupt)
    return templates


def render_templates(texts: list[str], font: Path, sizes: list[int]) -> dict:
    """Each text at each size, dark on light and trimmed; names never repeat the text itself."""
    from PIL import Image, ImageDraw, ImageFont

    templates = {}
    for number, text in enumerate(texts, 1):
        for size in sizes:
            face = ImageFont.truetype(str(font), size)
            left, top, right, bottom = face.getbbox(text)
            image = Image.new("L", (right - left + 2, bottom - top + 2), 255)
            ImageDraw.Draw(image).text((1 - left, 1 - top), text, font=face, fill=0)
            templates[f"text{number}@{size}px"] = gray(image)
    return templates


def frame_paths(paths: list[Path]) -> list[Path]:
    found = []
    for path in paths:
        if path.is_dir():
            found += sorted(p for p in path.rglob("*") if p.suffix.lower() in IMAGE_SUFFIXES)
        else:
            found.append(path)
    return found


def scan(frames: list[Path], templates: dict, threshold: float = THRESHOLD, engine: str = "auto",
         jobs: int = 1) -> dict:
    """Per frame, in order: its size, the engine and each template's result; `jobs` frames at a time."""
    from PIL import Image

    def one(path: Path) -> tuple[str, dict]:
        with Image.open(path) as image:
            data, width, height = gray(image)
        results, used = match(data, width, height, templates, threshold, engine)
        return str(path), dict(engine=used, size=[width, height], templates=results,
                               hits=sum(result["hits"] for result in results.values()))

    if engine in ("auto", "c"):
        helper()  # build once, before worker threads race for the same partial file
    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        return dict(pool.map(one, frame_paths(frames)))


def crop(frame: Path, box: tuple[int, int, int, int], output: Path) -> None:
    """Cut a template out of a frame that shows a personal string, into an untracked path."""
    from PIL import Image

    refuse_tracked(output, "template")
    output.parent.mkdir(parents=True, exist_ok=True)
    with Image.open(frame) as image:
        image.crop(box).convert("L").save(output)
