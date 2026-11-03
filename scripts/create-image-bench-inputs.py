#!/usr/bin/env python3
"""Write the Linux AVIF and JPEG 2000 release-measurement inputs into a directory.

The set is section 5's "Release measurement" in
docs/development/2026-11-02-linux-avif-jpeg2000-architecture.md, measured by
crates/app/tests/image_decode_bench.rs. Every image starts from deterministic
pixels drawn here with Pillow (gradients, a Mandelbrot detail band and
hard-edged shapes), converted by ffmpeg and encoded by tools on the host:
libaom's `aomenc` for AV1 (ffmpeg on the development host has no libaom
encoder), OpenJPEG's `opj_compress` for JPEG 2000 and Pillow for PNG and
JPEG. The AVIF boxes for aomenc's AV1 and the JP2 boxes
no encoder writes (an ICC `colr`, a CMYK `colr`, a profile over 10 MB) come
from the box writers of scripts/create-image-codec-fixtures.py. Nothing is
downloaded. The inputs are large and local only; never commit them.

    python3 scripts/create-image-bench-inputs.py OUTPUT_DIRECTORY [--only PATTERN ...]

`--only` writes just the inputs whose names match one of the shell patterns.
The script prints each tool's version, then each input's name, size and
SHA-256. A run writes about 200 MB and needs about 1 GB beside the output for
intermediates. Measure with (release, the host idle):

    GITTURTLE_IMAGE_BENCH_DIR=<this directory> GITTURTLE_IMAGE_BENCH_OUT=<record.json> \\
      cargo test --locked --release -p gitturtle --test image_decode_bench -- --ignored --nocapture

The set: AVIF at 4032 x 3024 8-bit 4:2:0, and 10-bit 4:2:0 with alpha; the
largest accepted AVIF at 6720 x 4480 (30.1 MP, under the 32-million-pixel
limit), 12-bit 4:4:4 with alpha and film grain; a hostile 32 MP AVIF at
6912 x 4608 whose primary and alpha item are both 12-bit 4:4:4 with film
grain (see `avif_hostile_alpha`), whose peak the bench reports on a line of
its own; JP2 at 6000 x 4000 lossy (decoded at reduced resolution) and
lossless; one at 5600 x 3700 in 1024 x 1024 tiles with no decomposition
levels (always decoded whole, the slowest accepted, about the largest the
helper's budget accepts whole); a 64 x 64 raw J2K; RGB JP2s with an ICC
profile at 3264 x 1836, one of them padded past 10 MB, and the same at
3000 x 2000, which the helper's budget passes only one level deeper than the
1,600-pixel edge asks (see `jp2_icc`); a CMYK JP2 (`colr` enumerated space
12) at 3000 x 2000; and PNG and JPEG of the 4032 x 3024 and 6000 x 4000
pixels, which the app decodes in process.

Two inputs stay within the 512 KiB a Markdown side can send for each of its
32 images, for the slowest confined image at Markdown's 1,000-pixel edge
(the security review's T): the hostile-alpha AVIF and the no-levels pixels
again at a 140:1 rate. The script fails where either grows past 512 KiB.

Each JPEG 2000 input is checked against the helper's budget arithmetic
(`jpeg2000_check`, mirroring `decode_bytes` and the preflight's planes in
crates/preview/src/jpeg2000.rs): generation fails where one meant to be
accepted would be refused.
"""

from __future__ import annotations

import argparse
import fnmatch
import functools
import hashlib
import importlib.util
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    from PIL import Image, ImageChops, ImageDraw, features
except ImportError:
    sys.exit("Pillow is required: python3 -m pip install Pillow")

# The decoders' input limit (gitturtle_preview::MAX_INPUT_BYTES): a larger
# input would be refused unread by the measurement.
MAX_INPUT_BYTES = 32 * 1024 * 1024
# The most a Markdown side can send to a helper for each of its 32 images (its
# 16 MiB of captured asset bytes over 32): the inputs that measure the slowest
# such image at Markdown's edge must stay within it.
MARKDOWN_SHARE_BYTES = 512 * 1024
TOOLS = ("ffmpeg", "aomenc", "opj_compress")


def codec_fixtures():
    """The AVIF and JP2 box writers of scripts/create-image-codec-fixtures.py."""
    path = Path(__file__).resolve().with_name("create-image-codec-fixtures.py")
    spec = importlib.util.spec_from_file_location("create_image_codec_fixtures", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


boxes = codec_fixtures()


def run(command: list[str]) -> None:
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        output = "\n".join(line for line in (result.stdout + result.stderr).splitlines()
                           if not line.startswith("dlopen:"))
        sys.exit(f"{command[0]} failed ({result.returncode}):\n{output}")


def versions() -> list[str]:
    """Each tool's version line; exits naming any tool that is missing."""
    missing = [tool for tool in TOOLS if shutil.which(tool) is None]
    if missing:
        sys.exit(f"missing tools: {', '.join(missing)}")
    lines = []
    probes = {
        "ffmpeg": (["ffmpeg", "-hide_banner", "-version"], "ffmpeg version"),
        "aomenc": (["aomenc", "--help"], "AV1 Encoder"),
        "opj_compress": (["opj_compress", "-h"], "openjp2 library"),
    }
    for tool, (command, marker) in probes.items():
        result = subprocess.run(command, capture_output=True, text=True)
        line = next((line.strip() for line in (result.stdout + result.stderr).splitlines()
                     if marker in line), "version not reported")
        lines.append(f"{tool}: {line}")
    lines.append(f"Pillow: {Image.__version__} (libjpeg {features.version('jpg')}, "
                 f"zlib {features.version('zlib')}, OpenJPEG {features.version('jpg_2000')})")
    return lines


# ---------------------------------------------------------------- pixels


@functools.cache
def master(width: int, height: int) -> Image.Image:
    """Deterministic RGBA pixels: red across, green down and blue from the
    centre, two of them mixed with a Mandelbrot band for detail, a grid of
    hard-edged shapes, a fine grain from SHAKE-256 bytes so the codecs see
    texture as in a photograph, and alpha opaque at the centre falling to a
    quarter at the corners. The grain is as strong as keeps the 24 MP PNG and
    lossless JP2 under the 32 MiB input limit."""
    def stretched(image: Image.Image) -> Image.Image:
        return image.resize((width, height), Image.Resampling.BILINEAR)

    down = Image.linear_gradient("L")
    across = stretched(down.transpose(Image.Transpose.ROTATE_90))
    radial = stretched(Image.radial_gradient("L"))
    down = stretched(down)
    detail = Image.effect_mandelbrot((width, height), (-2.2, -1.2, 0.8, 1.2), 96)
    green = Image.blend(down, detail, 0.35)
    blue = Image.blend(radial, ImageChops.invert(detail), 0.35)
    image = Image.merge("RGB", (across, green, blue))
    draw = ImageDraw.Draw(image)
    columns, rows = 12, 8
    cell_w, cell_h = width / columns, height / rows
    for row in range(rows):
        for column in range(columns):
            if (row + column) % 3:
                continue
            left, top = column * cell_w, row * cell_h
            box = (left + cell_w * 0.2, top + cell_h * 0.2, left + cell_w * 0.8, top + cell_h * 0.8)
            colour = ((column * 53) % 256, (row * 97) % 256, ((row + column) * 31) % 256)
            if column % 2:
                draw.ellipse(box, fill=colour, outline=(255, 255, 255), width=max(1, width // 1000))
            else:
                draw.rectangle(box, fill=colour, outline=(0, 0, 0), width=max(1, width // 1000))
    for line in range(0, width + height, max(8, (width + height) // 400)):
        draw.line((line, 0, line - height, height), fill=(250, 250, 250), width=1)
    half = (width // 2, height // 2)
    noise = hashlib.shake_256(b"gitturtle image bench grain").digest(half[0] * half[1])
    grain = Image.frombytes("L", half, noise).resize((width, height), Image.Resampling.BICUBIC)
    image = Image.merge("RGB", [Image.blend(plane, grain, 0.04) for plane in image.split()])
    alpha = Image.eval(radial, lambda value: 255 - value * 3 // 4)
    image.putalpha(alpha)
    return image


@functools.cache
def smooth(width: int, height: int) -> Image.Image:
    """`master`'s pixels drawn at a sixteenth of the size and enlarged:
    gradients and soft shapes with no fine texture, which a codec stores in
    few bytes however large the image."""
    small = master(max(1, width // 16), max(1, height // 16))
    return small.resize((width, height), Image.Resampling.BICUBIC)


def save(image: Image.Image, path: Path, *options) -> Path:
    image.save(path, *options)
    return path


def y4m(work: Path, name: str, image: Image.Image, pixel_format: str, alpha: bool = False) -> Path:
    """The colour (BT.601 limited range) or the alpha plane (as limited-range
    luma over neutral chroma) of `image` as one Y4M frame, through ffmpeg."""
    source = save(image, work / f"{name}-source.png")
    target = work / f"{name}.y4m"
    if alpha:
        chain = f"alphaextract,scale=in_range=pc:out_range=tv,format={pixel_format}"
    else:
        chain = f"scale=out_color_matrix=bt601:out_range=tv,format={pixel_format}"
    run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(source),
         "-vf", chain, "-frames:v", "1", "-strict", "-1", "-f", "yuv4mpegpipe", str(target)])
    return target


def aomenc(work: Path, name: str, source: Path, bits: int, profile: int, *extra: str,
           quality: int = 24) -> bytes:
    """One AV1 key frame as low-overhead OBUs, temporal delimiters removed."""
    out = work / f"{name}.obu"
    run(["aomenc", "--allintra", "--limit=1", f"--threads={os.cpu_count() or 1}", "--row-mt=1",
         "--cpu-used=6", "--end-usage=q", f"--cq-level={quality}", f"--profile={profile}",
         f"--bit-depth={bits}", f"--input-bit-depth={bits}", "--obu", "--quiet", *extra,
         "-o", str(out), str(source)])
    return boxes.strip_temporal_delimiters(out.read_bytes())


def aom_avif(work: Path, name: str, image: Image.Image, bits: int, chroma: str,
             alpha: bool, *extra: str, alpha_chroma: str = "400", quality: int = 24) -> bytes:
    """A still AVIF of aomenc's colour item, and alpha item where asked: a
    monochrome one (`alpha_chroma` "400"), as encoders write, or with
    "444" a 4:4:4 picture whose luma is the alpha over neutral chroma,
    encoded with the colour item's `extra` options (its film grain too)."""
    depth = "p" if bits == 8 else f"p{bits}le"
    # aomenc 3.15 writes 12 bits as the professional profile (2) by itself
    # when asked for 1 (4:4:4) or 0, and refuses `--profile=2` for 4:4:4,
    # whose subsampling it then cannot set.
    profile = 1 if chroma == "444" else 0
    colour_source = y4m(work, f"{name}-colour", image, f"yuv{chroma}{depth}")
    colour = aomenc(work, f"{name}-colour", colour_source, bits, profile, *extra,
                    quality=quality)
    size = image.size
    bt601 = (boxes.nclx(1, 13, 6, False), False)
    items = [boxes.colour_item(colour, bits, 3, [bt601], size=size)]
    if alpha and alpha_chroma == "444":
        alpha_source = y4m(work, f"{name}-alpha", image, f"yuv444{depth}", alpha=True)
        plane = aomenc(work, f"{name}-alpha", alpha_source, bits, 1, *extra, quality=quality)
        items.append(boxes.alpha_item(plane, bits, size=size))
    elif alpha:
        alpha_source = y4m(work, f"{name}-alpha", image, f"yuv420{depth}", alpha=True)
        plane = aomenc(work, f"{name}-alpha", alpha_source, bits, 0, "--monochrome",
                       quality=quality)
        items.append(boxes.alpha_item(plane, bits, size=size))
    return boxes.avif(items)


def opj(work: Path, name: str, source: Path, suffix: str, *options: str) -> bytes:
    out = work / f"{name}.{suffix}"
    run(["opj_compress", "-i", str(source), "-o", str(out), *options])
    return out.read_bytes()


def rgb_ppm(work: Path, name: str, image: Image.Image) -> Path:
    return save(image.convert("RGB"), work / f"{name}.ppm")


def padded_icc(profile: bytes, size: int) -> bytes:
    """`profile` with one private `data` tag of zeros appended so the whole
    profile is `size` bytes: the existing tags move 12 bytes for the new
    table entry, and the header's size follows."""
    count = struct.unpack(">I", profile[128:132])[0]
    table = profile[132:132 + 12 * count]
    tags = profile[132 + 12 * count:]
    entries = b""
    for index in range(count):
        signature, offset, length = struct.unpack(">4sII", table[12 * index:12 * index + 12])
        entries += signature + struct.pack(">II", offset + 12, length)
    start = 132 + 12 * (count + 1) + len(tags)
    padding = -start % 4
    length = size - start - padding
    assert length > 12, "the profile is smaller than asked"
    payload = b"data" + b"\0" * 4 + struct.pack(">I", 1) + b"\0" * (length - 12)
    entries += b"gtpd" + struct.pack(">II", start + padding, length)
    body = struct.pack(">I", count + 1) + entries + tags + b"\0" * padding + payload
    header = struct.pack(">I", 128 + len(body)) + profile[4:128]
    padded = header + body
    assert len(padded) == size
    return padded


# ------------------------------------------------- the helper's JP2 budget

# gitturtle_preview::MAX_DECODER_BYTES and the helper's image edges: History's
# (MAX_HELPER_IMAGE_EDGE) and a Markdown document's.
MAX_DECODER_BYTES = 256 * 1024 * 1024
HISTORY_EDGE = 1600
MARKDOWN_EDGE = 1000


def scaled_dimensions(width: int, height: int, edge: int) -> tuple[int, int]:
    """gitturtle_preview's `scaled_dimensions`: the output at `edge`."""
    longest = max(width, height)
    if longest <= edge:
        return width, height
    return tuple(max(1, (value * edge + longest // 2) // longest) for value in (width, height))


def jpeg2000_layout(data: bytes) -> dict:
    """What the helper's preflight (crates/preview/src/jpeg2000.rs) reads from
    a JP2 file or raw codestream: SIZ, COD's decomposition levels and the
    first `colr` payload. Only the shapes this set writes are modelled; a
    palette, a channel definition, a per-component COC or subsampling stops
    generation rather than being budgeted wrongly."""
    colour, cdef = None, False
    if data.startswith(b"\xff\x4f\xff\x51"):
        codestream, raw = data, True
    else:
        raw, codestream, children = False, None, b""
        for kind, payload in boxes.jp2_boxes(data):
            if kind == b"jp2h":
                children = payload
            elif kind == b"jp2c":
                codestream = payload
        for kind, payload in boxes.jp2_boxes(children):
            if kind == b"colr" and colour is None:
                colour = payload
            assert kind not in (b"pclr", b"cmap"), "a palette is not modelled"
            cdef |= kind == b"cdef"
        assert not cdef, "a channel definition is not modelled"
    length = struct.unpack(">H", codestream[4:6])[0]
    fields = struct.unpack(">8I", codestream[8:40])
    components = struct.unpack(">H", codestream[40:42])[0]
    table = codestream[42:4 + length]
    assert all(table[3 * index + 1:3 * index + 3] == b"\x01\x01" for index in range(components)), \
        "subsampling is not modelled"
    levels, at = None, 4 + length
    while codestream[at:at + 2] != b"\xff\x90":
        marker, size = struct.unpack(">HH", codestream[at:at + 4])
        assert marker != 0xFF53, "a per-component COC is not modelled"
        if marker == 0xFF52:
            levels = codestream[at + 9]
        at += 2 + size
    area = (fields[0] - fields[2], fields[1] - fields[3])
    return {
        "area": area,
        "tile": (min(fields[4], area[0]), min(fields[5], area[1])),
        "components": components,
        "levels": levels,
        "colour": None if raw else (colour,),
    }


def jpeg2000_planes(layout: dict) -> tuple[int, int]:
    """The preflight's f32 planes and decoder contexts, for a file with no
    palette and no channel definition: one plane per component; with a
    profile conversion (`converts_with_a_profile`) `profile_planes` of them
    and two contexts; read as CMYK (`reads_as_cmyk`, four components under the
    CMYK, YCCK or greyscale space, or no `colr`) at least `cmyk_planes`."""
    components = layout["components"]
    if layout["colour"] is None:
        return components, 1
    (colour,) = layout["colour"]
    space = struct.unpack(">I", colour[3:7])[0] if colour and colour[0] == 1 and len(colour) >= 7 \
        else None
    if colour and (colour[0] == 2 or space in (14, 21)):
        return components + -(-components // 4) + max(components, 2 * components + 1), 2
    if components == 4 and (space in (12, 13, 17) or not colour or colour[0] not in (1, 2)):
        return max(components, components + -(-components // 4) + 1), 1
    return components, 1


def jpeg2000_budget(data: bytes, edge: int) -> tuple[int, tuple[int, int], int, int]:
    """The helper's decode of `data` at `edge` (`jpeg2000_hayro::decode`): the
    level hayro-jpeg2000 skips to, `min(log2(width / target), log2(height /
    target), levels)` (`j2c/codestream.rs`), its size, and `decode_bytes`
    there: every plane at that size and each context's largest tile. Returns
    that level, its size, its bytes and the same bytes one level deeper (0
    where there is no deeper level)."""
    layout = jpeg2000_layout(data)
    planes, contexts = jpeg2000_planes(layout)
    width, height = layout["area"]
    target = scaled_dimensions(width, height, edge)
    levels = layout["levels"]
    level = 0
    if target != (width, height):
        level = min((width // target[0]).bit_length() - 1, (height // target[1]).bit_length() - 1,
                    levels)

    def at(level: int) -> tuple[tuple[int, int], int]:
        size = (-(-width // (1 << level)), -(-height // (1 << level)))
        tile = [-(-side // (1 << level)) for side in layout["tile"]]
        return size, size[0] * size[1] * planes * 4 + contexts * tile[0] * tile[1] * 4

    size, needed = at(level)
    deeper = at(level + 1)[1] if level < levels else 0
    return level, size, needed, deeper


def jpeg2000_check(name: str, data: bytes, expected: str, edges: tuple[int, ...]) -> str | None:
    """Whether the helper's budget treats `data` as `expected` at each edge:
    "fits" at the level it decodes, or "deeper" for a file over the budget
    there that fits one level deeper, where the helper then decodes it.
    Returns the mismatch, or None."""
    for edge in edges:
        level, size, needed, deeper = jpeg2000_budget(data, edge)
        rgba = size[0] * size[1] * 4 <= MAX_DECODER_BYTES
        print(f"  budget at {edge}: level {level}, {size[0]} x {size[1]}, {needed:,} of "
              f"{MAX_DECODER_BYTES:,} bytes" + (f" ({deeper:,} one level deeper)" if deeper else ""))
        fits = needed <= MAX_DECODER_BYTES and rgba
        if expected == "fits" and not fits:
            return f"{name} is meant to be accepted at {edge} but the helper's budget refuses it"
        if expected == "deeper" and (fits or not 0 < deeper <= MAX_DECODER_BYTES):
            return f"{name} is meant to pass the budget only one level deeper at {edge}"
    return None


# ---------------------------------------------------------------- inputs


def avif_8bit(work: Path) -> bytes:
    return aom_avif(work, "avif-8", master(4032, 3024).convert("RGB"), 8, "420", False)


def avif_10bit_alpha(work: Path) -> bytes:
    return aom_avif(work, "avif-10", master(4032, 3024), 10, "420", True)


def avif_12bit_444_alpha_grain(work: Path) -> bytes:
    # Section 5's largest accepted AVIF: dav1d holds 12 bits in 16-bit
    # samples, and with film grain a second picture beside the first.
    return aom_avif(work, "avif-12", master(6720, 4480), 12, "444", True, "--film-grain-test=1")


def avif_hostile_alpha(work: Path) -> bytes:
    # 32 MP (6912 x 4608, within the 32-million-pixel limit) 12-bit 4:4:4
    # with film grain, and an alpha item that is also 12-bit 4:4:4 with
    # grain, not 4:0:0: nothing refuses that shape, and with grain each
    # item's decode holds its reference picture and the grain copy (2 x 191
    # MB) while the primary's RGBA (127 MB) waits for the alpha, about 512
    # MB beside the helper's fixed ~96 MiB, past requirement 18's 512 MiB.
    # Smooth pixels at a coarse quantizer keep it small (within Markdown's
    # 512 KiB share of a side's assets), as a hostile document's would be.
    return aom_avif(work, "avif-hostile-alpha", smooth(6912, 4608), 12, "444", True,
                    "--film-grain-test=1", alpha_chroma="444", quality=40)


@functools.cache
def large_ppm(work: Path) -> Path:
    return rgb_ppm(work, "large", master(6000, 4000))


def jp2_lossy(work: Path) -> bytes:
    return opj(work, "lossy", large_ppm(work), "jp2", "-r", "20")


def jp2_lossless(work: Path) -> bytes:
    return opj(work, "lossless", large_ppm(work), "jp2")


@functools.cache
def no_levels_ppm(work: Path) -> Path:
    return rgb_ppm(work, "no-levels", master(5600, 3700))


def jp2_no_levels(work: Path) -> bytes:
    # One resolution: no wavelet decomposition, so every decode is at full
    # size, the slowest accepted JPEG 2000. 5600 x 3700 in 1024 x 1024 tiles
    # is about as large as the helper's budget accepts at full size (three
    # f32 planes and one tile, 252.8 of 268.4 MB; untiled, the whole image is
    # one tile and 6000 x 4000 needs 384 MB). A 2.2:1 rate cap (28 MB, within
    # the 32 MiB input limit) keeps every coded bit these pixels give, about
    # 23 MB, the most coded data an accepted whole decode here reads.
    return opj(work, "no-levels", no_levels_ppm(work), "jp2", "-n", "1", "-t", "1024,1024",
               "-r", "2.2")


def jp2_no_levels_small(work: Path) -> bytes:
    # The no-levels pixels at a 140:1 rate (about 440 KB): within Markdown's
    # share, every decode is still of the whole 5600 x 3700 image, the
    # slowest JPEG 2000 a Markdown document can send 32 times.
    return opj(work, "no-levels-small", no_levels_ppm(work), "jp2", "-n", "1", "-t", "1024,1024",
               "-r", "140")


def j2k_small(work: Path) -> bytes:
    image = master(6000, 4000).convert("RGB").crop((2968, 1968, 3032, 2032))
    return opj(work, "small", rgb_ppm(work, "small", image), "j2k")


@functools.cache
def medium_codestream(work: Path, width: int, height: int) -> bytes:
    name = f"medium-{width}x{height}"
    return opj(work, name, rgb_ppm(work, name, master(width, height)), "j2k", "-r", "20")


def jp2_icc(width: int, height: int, profile_bytes: int = 0):
    """An RGB JP2 whose `colr` is the Display P3 test profile, padded to
    `profile_bytes` where given: one of 10 MB or more makes moxcms refuse it
    and hayro-jpeg2000 decode the image a second time.

    At History's 1,600-pixel edge a 3264 x 1836 image (6.0 MP) decodes one
    resolution level down. A 3000 x 2000 one has no level to skip (the next
    is 1,500 wide), and at full size the helper's budget for a profile
    conversion (11 f32 planes and two tile contexts, 312 MB) passes the
    256 MiB limit, so the helper decodes it one level deeper, at 1,500 x
    1,000 (78 MB): it measures that path."""
    def write(work: Path) -> bytes:
        profile = boxes.display_p3_icc()
        if profile_bytes:
            profile = padded_icc(profile, profile_bytes)
        icc = boxes.box(b"colr", bytes([2, 0, 0]) + profile)
        stream = medium_codestream(work, width, height)
        return boxes.jp2(stream, [boxes.ihdr(width, height, 3, 8), icc])
    return write


def jp2_cmyk(work: Path) -> bytes:
    cmyk = master(3000, 2000).convert("RGB").convert("CMYK")
    raw = work / "cmyk.raw"
    # opj_compress reads raw components one plane after another.
    raw.write_bytes(b"".join(plane.tobytes() for plane in cmyk.split()))
    stream = opj(work, "cmyk", raw, "j2k", "-F", "3000,2000,4,8,u", "-mct", "0", "-r", "20")
    return boxes.jp2(stream, [boxes.ihdr(3000, 2000, 4, 8), boxes.enumerated_colr(12)])


def png(width: int, height: int):
    def write(work: Path) -> bytes:
        path = save(master(width, height).convert("RGB"), work / f"png-{width}.png")
        return path.read_bytes()
    return write


def jpeg(width: int, height: int):
    def write(work: Path) -> bytes:
        image = master(width, height).convert("RGB")
        path = work / f"jpeg-{width}.jpg"
        image.save(path, quality=90, subsampling="4:2:0")
        return path.read_bytes()
    return write


INPUTS = {
    "avif-4032x3024-8bit-420.avif": avif_8bit,
    "avif-4032x3024-10bit-420-alpha.avif": avif_10bit_alpha,
    "avif-6720x4480-12bit-444-alpha-grain.avif": avif_12bit_444_alpha_grain,
    "avif-6912x4608-12bit-444-grain-hostile-alpha.avif": avif_hostile_alpha,
    "jp2-6000x4000-lossy.jp2": jp2_lossy,
    "jp2-6000x4000-lossless.jp2": jp2_lossless,
    "jp2-5600x3700-no-levels.jp2": jp2_no_levels,
    "jp2-5600x3700-no-levels-512k.jp2": jp2_no_levels_small,
    "j2k-64x64.j2k": j2k_small,
    "jp2-3264x1836-icc.jp2": jp2_icc(3264, 1836),
    "jp2-3264x1836-icc-10mb.jp2": jp2_icc(3264, 1836, 10_500_000),
    "jp2-3000x2000-icc.jp2": jp2_icc(3000, 2000),
    "jp2-3000x2000-cmyk.jp2": jp2_cmyk,
    "png-4032x3024.png": png(4032, 3024),
    "jpg-4032x3024.jpg": jpeg(4032, 3024),
    "png-6000x4000.png": png(6000, 4000),
    "jpg-6000x4000.jpg": jpeg(6000, 4000),
}

# How the helper's budget treats each JPEG 2000 input (`jpeg2000_check`); any
# not listed is meant to be accepted at the level it decodes.
JPEG2000_EXPECTED = {"jp2-3000x2000-icc.jp2": "deeper"}

# The inputs that measure the slowest confined image within Markdown's share
# (the security review's T): each must stay within MARKDOWN_SHARE_BYTES.
MARKDOWN_SHARE = ("avif-6912x4608-12bit-444-grain-hostile-alpha.avif",
                  "jp2-5600x3700-no-levels-512k.jp2")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("output", type=Path, help="directory to write the inputs into")
    parser.add_argument("--only", nargs="+", metavar="PATTERN",
                        help="write only the inputs whose names match a shell pattern")
    arguments = parser.parse_args()
    names = [name for name in INPUTS
             if not arguments.only or any(fnmatch.fnmatch(name, p) for p in arguments.only)]
    if not names:
        parser.error("no input matches --only")
    for line in versions():
        print(line)
    arguments.output.mkdir(parents=True, exist_ok=True)
    oversized, budget, share = [], [], []
    # Intermediates (Y4M frames of up to 180 MB) stay beside the output, not
    # in a memory-backed temporary directory.
    with tempfile.TemporaryDirectory(dir=arguments.output, prefix=".work-") as directory:
        work = Path(directory)
        for name in names:
            data = INPUTS[name](work)
            (arguments.output / name).write_bytes(data)
            print(f"{name} {len(data)} {hashlib.sha256(data).hexdigest()}", flush=True)
            if len(data) > MAX_INPUT_BYTES:
                oversized.append(name)
            if name in MARKDOWN_SHARE and len(data) > MARKDOWN_SHARE_BYTES:
                share.append(name)
            if name.endswith((".jp2", ".j2k")):
                expected = JPEG2000_EXPECTED.get(name, "fits")
                # An input within Markdown's share is decoded at its edge too.
                edges = (HISTORY_EDGE, MARKDOWN_EDGE) if len(data) <= MARKDOWN_SHARE_BYTES \
                    else (HISTORY_EDGE,)
                mismatch = jpeg2000_check(name, data, expected, edges)
                if mismatch:
                    budget.append(mismatch)
    if oversized:
        print(f"over the 32 MiB input limit, so refused unread: {', '.join(oversized)}",
              file=sys.stderr)
    if share:
        print(f"over Markdown's 512 KiB share, so not the image they measure: {', '.join(share)}",
              file=sys.stderr)
    for mismatch in budget:
        print(mismatch, file=sys.stderr)
    return 1 if oversized or budget or share else 0


if __name__ == "__main__":
    sys.exit(main())
