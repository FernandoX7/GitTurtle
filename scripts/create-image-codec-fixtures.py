#!/usr/bin/env python3
"""Generate the AVIF and JPEG 2000 fixtures for crates/preview's confined helper.

Every fixture comes from deterministic pixels written here, encoded by tools on
the development host (libaom's `aomenc` for AV1, OpenJPEG's `opj_compress` for
JPEG 2000, Pillow's libavif for the animation) and wrapped, where no encoder
writes the shape, by the pure-Python AVIF and JP2 writers below. Nothing is
downloaded. Run from the repository root:

    python3 scripts/create-image-codec-fixtures.py [--check]

`--check` regenerates into a temporary directory and reports fixtures whose
bytes differ from the checked-in ones (an encoder upgrade may change them; the
tests check decoded pixels within tolerance, not bytes). The script prints each
fixture's SHA-256 for crates/preview/tests/fixtures/README.md.
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "crates" / "preview" / "tests" / "fixtures"

WIDTH, HEIGHT = 64, 48
# Top-left, top-right, bottom-left, bottom-right.
QUADRANTS = ((220, 30, 30), (30, 200, 60), (30, 50, 220), (240, 240, 240))
# Display P3 values whose sRGB rendering differs from the stored numbers.
P3_QUADRANTS = ((200, 80, 60), (60, 180, 90), (70, 90, 200), (128, 128, 128))


def quadrant(x: int, y: int, colours=QUADRANTS) -> tuple[int, int, int]:
    return colours[(1 if x >= WIDTH // 2 else 0) + (2 if y >= HEIGHT // 2 else 0)]


def alpha_at(x: int) -> float:
    """Opaque left half, half-transparent right half."""
    return 1.0 if x < WIDTH // 2 else 128 / 255


# ---------------------------------------------------------------- AV1 / AVIF


def ycbcr(rgb, bits: int) -> tuple[int, int, int]:
    """BT.601 limited range, the AV1 default for unspecified matrices."""
    r, g, b = (c / 255 for c in rgb)
    y = 0.299 * r + 0.587 * g + 0.114 * b
    cb = (b - y) / 1.772
    cr = (r - y) / 1.402
    scale = 1 << (bits - 8)
    return (
        round((16 + 219 * y) * scale),
        round((128 + 224 * cb) * scale),
        round((128 + 224 * cr) * scale),
    )


def y4m(planes: list[list[list[int]]], bits: int, chroma: str) -> bytes:
    tag = {"420": "420jpeg" if bits == 8 else f"420p{bits}", "444": "444" if bits == 8 else f"444p{bits}"}[chroma]
    out = bytearray(f"YUV4MPEG2 W{WIDTH} H{HEIGHT} F1:1 Ip A1:1 C{tag}\n".encode())
    out += b"FRAME\n"
    for plane in planes:
        for row in plane:
            for value in row:
                out += bytes([value]) if bits == 8 else struct.pack("<H", value)
    return bytes(out)


def yuv_planes(pixel, bits: int, chroma: str) -> list[list[list[int]]]:
    """Y, U and V planes for `pixel(x, y) -> (y, u, v)` sampled at the
    top-left sample of each chroma block."""
    luma = [[pixel(x, y)[0] for x in range(WIDTH)] for y in range(HEIGHT)]
    step = 2 if chroma == "420" else 1
    u = [[pixel(x, y)[1] for x in range(0, WIDTH, step)] for y in range(0, HEIGHT, step)]
    v = [[pixel(x, y)[2] for x in range(0, WIDTH, step)] for y in range(0, HEIGHT, step)]
    return [luma, u, v]


def aomenc(work: Path, name: str, source: bytes, bits: int, *, chroma: str = "420",
           monochrome: bool = False, extra: tuple[str, ...] = ()) -> bytes:
    """One AV1 key frame as low-overhead OBUs, temporal delimiters removed."""
    src = work / f"{name}.y4m"
    out = work / f"{name}.obu"
    src.write_bytes(source)
    profile = 2 if bits == 12 else (1 if chroma == "444" and not monochrome else 0)
    command = [
        "aomenc", "--allintra", "--limit=1", "--threads=1", "--row-mt=0", "--cpu-used=6",
        "--end-usage=q", "--cq-level=8", f"--profile={profile}", f"--bit-depth={bits}",
        f"--input-bit-depth={bits}", "--obu", "--quiet", *extra,
    ]
    if monochrome:
        command.append("--monochrome")
    subprocess.run([*command, "-o", str(out), str(src)], check=True, capture_output=True)
    return strip_temporal_delimiters(out.read_bytes())


def leb128(data: bytes, at: int) -> tuple[int, int]:
    value, shift = 0, 0
    while True:
        byte = data[at]
        at += 1
        value |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            return value, at


def encode_leb128(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


def obus(data: bytes):
    at = 0
    while at < len(data):
        header = data[at]
        assert header & 0x02, "aomenc writes OBU sizes"
        assert not header & 0x04, "no extension headers expected"
        size, payload = leb128(data, at + 1)
        yield (header >> 3) & 0x0F, data[at:payload + size], data[payload:payload + size]
        at = payload + size


def strip_temporal_delimiters(data: bytes) -> bytes:
    return b"".join(whole for kind, whole, _ in obus(data) if kind != 2)


def sequence_header(data: bytes) -> bytes:
    return next(whole for kind, whole, _ in obus(data) if kind == 1)


class Bits:
    def __init__(self, data: bytes):
        self.data, self.at = data, 0

    def read(self, count: int) -> int:
        value = 0
        for _ in range(count):
            value = (value << 1) | ((self.data[self.at >> 3] >> (7 - (self.at & 7))) & 1)
            self.at += 1
        return value


class BitWriter:
    def __init__(self):
        self.bits: list[int] = []

    def write(self, value: int, count: int):
        self.bits += [(value >> (count - 1 - i)) & 1 for i in range(count)]

    def bytes(self) -> bytes:
        bits = self.bits + [0] * (-len(self.bits) % 8)
        return bytes(int("".join(map(str, bits[i:i + 8])), 2) for i in range(0, len(bits), 8))


def enlarge_sequence_header(data: bytes, width: int, height: int) -> bytes:
    """Rewrites the sequence header's maximum frame size (AV1 5.5.1). The frame
    header has no size override, so the frame takes this size; the tile data
    no longer matches, which no decoder reaches when its size limit holds."""
    out = bytearray()
    for kind, whole, payload in obus(data):
        if kind != 1:
            out += whole
            continue
        reader, writer = Bits(payload), BitWriter()
        writer.write(reader.read(3), 3)  # seq_profile
        writer.write(reader.read(1), 1)  # still_picture
        reduced = reader.read(1)
        writer.write(reduced, 1)
        if reduced:
            writer.write(reader.read(5), 5)
        else:
            timing = reader.read(1)
            assert not timing, "aomenc writes no timing info for stills"
            writer.write(timing, 1)
            delay = reader.read(1)
            writer.write(delay, 1)
            points = reader.read(5)
            writer.write(points, 5)
            for _ in range(points + 1):
                writer.write(reader.read(12), 12)
                level = reader.read(5)
                writer.write(level, 5)
                if level > 7:
                    writer.write(reader.read(1), 1)
                if delay:
                    present = reader.read(1)
                    writer.write(present, 1)
                    if present:
                        writer.write(reader.read(4), 4)
        width_bits = reader.read(4) + 1
        height_bits = reader.read(4) + 1
        reader.read(width_bits)
        reader.read(height_bits)
        new_width_bits = max(width_bits, (width - 1).bit_length())
        new_height_bits = max(height_bits, (height - 1).bit_length())
        writer.write(new_width_bits - 1, 4)
        writer.write(new_height_bits - 1, 4)
        writer.write(width - 1, new_width_bits)
        writer.write(height - 1, new_height_bits)
        # The rest of the header, without its trailing bits (5.3.4).
        remaining = len(payload) * 8
        while remaining > reader.at and not (payload[(remaining - 1) >> 3] >> (7 - ((remaining - 1) & 7))) & 1:
            remaining -= 1
        remaining -= 1  # the trailing one bit
        while reader.at < remaining:
            writer.write(reader.read(1), 1)
        writer.write(1, 1)
        body = writer.bytes()
        out += bytes([whole[0]]) + encode_leb128(len(body)) + body
    return bytes(out)


def box(kind: bytes, payload: bytes) -> bytes:
    return struct.pack(">I", 8 + len(payload)) + kind + payload


def full_box(kind: bytes, version: int, flags: int, payload: bytes) -> bytes:
    return box(kind, struct.pack(">I", (version << 24) | flags) + payload)


ALPHA_URN = b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0"


def av1c(data: bytes) -> bytes:
    """av1C from the sequence header's own fields (AV1-ISOBMFF 2.3)."""
    header = sequence_header(data)
    _, payload = leb128(header, 1)
    reader = Bits(header[payload:])
    profile = reader.read(3)
    reader.read(1)
    reduced = reader.read(1)
    tier = 0
    if reduced:
        level = reader.read(5)
    else:
        assert not reader.read(1)
        delay = reader.read(1)
        points = reader.read(5)
        level = None
        for index in range(points + 1):
            reader.read(12)
            point_level = reader.read(5)
            point_tier = reader.read(1) if point_level > 7 else 0
            if delay and reader.read(1):
                reader.read(4)
            if index == 0:
                level, tier = point_level, point_tier
    width_bits, height_bits = reader.read(4) + 1, reader.read(4) + 1
    reader.read(width_bits + height_bits)
    if not reduced:
        if reader.read(1):  # frame_id_numbers_present_flag
            reader.read(7)
    reader.read(3)  # 128x128 superblock, filter intra, intra edge
    if not reduced:
        reader.read(4)  # interintra, masked compound, warped motion, dual filter
        order_hint = reader.read(1)
        if order_hint:
            reader.read(2)
        if reader.read(1):  # seq_choose_screen_content_tools
            force_screen = 2
        else:
            force_screen = reader.read(1)
        if force_screen > 0 and not reader.read(1):  # seq_choose_integer_mv
            reader.read(1)
        if order_hint:
            reader.read(3)
    reader.read(3)  # superres, cdef, restoration
    high = reader.read(1)
    twelve = reader.read(1) if profile == 2 and high else 0
    mono = reader.read(1) if profile != 1 else 0
    if reader.read(1):  # color_description_present_flag
        primaries, transfer, matrix = reader.read(8), reader.read(8), reader.read(8)
    else:
        primaries, transfer, matrix = 2, 2, 2
    if mono:
        sub_x, sub_y, position = 1, 1, 0
    elif primaries == 1 and transfer == 13 and matrix == 0:
        sub_x, sub_y, position = 0, 0, 0
    else:
        reader.read(1)  # color_range
        if profile == 0:
            sub_x, sub_y = 1, 1
        elif profile == 1:
            sub_x, sub_y = 0, 0
        elif twelve:
            sub_x = reader.read(1)
            sub_y = reader.read(1) if sub_x else 0
        else:
            sub_x, sub_y = 1, 0
        position = reader.read(2) if sub_x and sub_y else 0
    fields = bytes([
        0x81,
        (profile << 5) | level,
        (tier << 7) | (high << 6) | (twelve << 5) | (mono << 4) | (sub_x << 3) | (sub_y << 2) | position,
        0,
    ])
    return box(b"av1C", fields + header)


def avif(items: list[dict], *, primary: int = 1, premultiplied: bool = False,
         brands: tuple[bytes, ...] = (b"avif", b"mif1", b"miaf")) -> bytes:
    """A still AVIF (AVIF 1.1, MIAF): each item is a dict with `data`, `size`,
    `properties` (a list of (box, essential)), and `alpha_of` for an alpha
    auxiliary item. Offsets in iloc are absolute."""
    properties: list[bytes] = []
    associations = []
    for number, item in enumerate(items, 1):
        indices = []
        for prop, essential in item["properties"]:
            if prop not in properties:
                properties.append(prop)
            indices.append((properties.index(prop) + 1) | (0x80 if essential else 0))
        associations.append(struct.pack(">HB", number, len(indices)) + bytes(indices))
    ipma = full_box(b"ipma", 0, 0, struct.pack(">I", len(items)) + b"".join(associations))
    iprp = box(b"iprp", box(b"ipco", b"".join(properties)) + ipma)
    references = b""
    for number, item in enumerate(items, 1):
        if "alpha_of" in item:
            references += box(b"auxl", struct.pack(">HHH", number, 1, item["alpha_of"]))
            if premultiplied:
                references += box(b"prem", struct.pack(">HHH", item["alpha_of"], 1, number))
    iref = full_box(b"iref", 0, 0, references) if references else b""
    infe = b"".join(
        full_box(b"infe", 2, 0, struct.pack(">HH", number, 0) + item.get("type", b"av01") + b"\0")
        for number, item in enumerate(items, 1)
    )
    iinf = full_box(b"iinf", 0, 0, struct.pack(">H", len(items)) + infe)
    hdlr = full_box(b"hdlr", 0, 0, b"\0" * 4 + b"pict" + b"\0" * 12 + b"\0")
    pitm = full_box(b"pitm", 0, 0, struct.pack(">H", primary))
    ftyp = box(b"ftyp", brands[0] + b"\0\0\0\0" + b"".join(brands))

    def build(offsets: list[int]) -> bytes:
        entries = b"".join(
            struct.pack(">HHHII", number, 0, 1, offsets[number - 1], len(item["data"]))
            for number, item in enumerate(items, 1)
        )
        iloc = full_box(b"iloc", 0, 0, bytes([0x44, 0x00]) + struct.pack(">H", len(items)) + entries)
        return full_box(b"meta", 0, 0, hdlr + pitm + iloc + iinf + iref + iprp)

    meta = build([0] * len(items))
    start = len(ftyp) + len(meta) + 8
    offsets = []
    for item in items:
        offsets.append(start)
        start += len(item["data"])
    meta = build(offsets)
    mdat = box(b"mdat", b"".join(item["data"] for item in items))
    return ftyp + meta + mdat


def ispe(width: int, height: int) -> bytes:
    return full_box(b"ispe", 0, 0, struct.pack(">II", width, height))


def pixi(bits: int, channels: int) -> bytes:
    return full_box(b"pixi", 0, 0, bytes([channels] + [bits] * channels))


def nclx(primaries: int, transfer: int, matrix: int, full: bool) -> bytes:
    return box(b"colr", b"nclx" + struct.pack(">HHHB", primaries, transfer, matrix, 0x80 if full else 0))


def colour_item(data: bytes, bits: int, channels: int, extra: list[tuple[bytes, bool]] = (),
                size: tuple[int, int] = (WIDTH, HEIGHT)) -> dict:
    return {
        "data": data,
        "properties": [(av1c(data), True), (ispe(*size), False), (pixi(bits, channels), False), *extra],
    }


def alpha_item(data: bytes, bits: int, size: tuple[int, int] = (WIDTH, HEIGHT)) -> dict:
    return {
        "data": data,
        "alpha_of": 1,
        "properties": [
            (av1c(data), True), (ispe(*size), False), (pixi(bits, 1), False),
            (full_box(b"auxC", 0, 0, ALPHA_URN), True),
        ],
    }


def colour_source(bits: int, chroma: str, colours=QUADRANTS, premultiplied: bool = False) -> bytes:
    def pixel(x, y):
        rgb = quadrant(x, y, colours)
        if premultiplied:
            rgb = tuple(round(c * alpha_at(x)) for c in rgb)
        return ycbcr(rgb, bits)

    return y4m(yuv_planes(pixel, bits, chroma), bits, chroma)


def grey_source(bits: int, value) -> bytes:
    scale = 1 << (bits - 8)
    return y4m(
        yuv_planes(lambda x, y: (value(x, y), 128 * scale, 128 * scale), bits, "420"), bits, "420"
    )


def ramp(bits: int):
    """Limited-range grey, 0 to 255 across the width once normalised."""
    scale = 1 << (bits - 8)
    return lambda x, _y: round((16 + 219 * x / (WIDTH - 1)) * scale)


def alpha_source(bits: int, width: int = WIDTH, height: int = HEIGHT) -> bytes:
    scale = 1 << (bits - 8)
    # Limited range, as the encoder signals for every plane it writes.
    value = lambda x, _y: round((16 + 219 * alpha_at(x * WIDTH // width)) * scale)
    if (width, height) == (WIDTH, HEIGHT):
        return grey_source(bits, value)
    planes = [
        [[value(x, y) for x in range(width)] for y in range(height)],
        [[128 * scale] * (width // 2) for _ in range(height // 2)],
        [[128 * scale] * (width // 2) for _ in range(height // 2)],
    ]
    header = f"YUV4MPEG2 W{width} H{height} F1:1 Ip A1:1 C{'420jpeg' if bits == 8 else f'420p{bits}'}\n"
    body = bytearray(header.encode() + b"FRAME\n")
    for plane in planes:
        for row in plane:
            for sample in row:
                body += bytes([sample]) if bits == 8 else struct.pack("<H", sample)
    return bytes(body)


def avif_fixtures(work: Path) -> dict[str, bytes]:
    out = {}
    bt601 = (nclx(1, 13, 6, False), False)

    colour = aomenc(work, "yuv420-8", colour_source(8, "420"), 8)
    alpha = aomenc(work, "alpha-8", alpha_source(8), 8, monochrome=True)
    out["avif-yuv420-8bit-alpha.avif"] = avif(
        [colour_item(colour, 8, 3, [bt601]), alpha_item(alpha, 8)]
    )

    colour = aomenc(work, "yuv444-10-prem", colour_source(10, "444", premultiplied=True), 10, chroma="444")
    alpha = aomenc(work, "alpha-10", alpha_source(10), 10, monochrome=True)
    out["avif-yuv444-10bit-premultiplied.avif"] = avif(
        [colour_item(colour, 10, 3, [bt601]), alpha_item(alpha, 10)], premultiplied=True
    )

    # libaom 3.15's aomenc cannot set 4:4:4 subsampling at 12 bits.
    colour = aomenc(work, "yuv420-12", colour_source(12, "420"), 12)
    alpha = aomenc(work, "alpha-12", alpha_source(12), 12, monochrome=True)
    out["avif-yuv420-12bit-alpha.avif"] = avif(
        [colour_item(colour, 12, 3, [bt601]), alpha_item(alpha, 12)]
    )

    for bits in (8, 10, 12):
        grey = aomenc(work, f"yuv400-{bits}", grey_source(bits, ramp(bits)), bits, monochrome=True)
        # No colr: the matrix and range come from the sequence header.
        out[f"avif-yuv400-{bits}bit.avif"] = avif([colour_item(grey, bits, 1)])

    colour = aomenc(work, "oriented", colour_source(8, "420"), 8)
    irot = box(b"irot", bytes([1]))  # 90 degrees anticlockwise
    imir = box(b"imir", bytes([1]))  # left and right exchanged
    out["avif-irot-imir.avif"] = avif([colour_item(colour, 8, 3, [bt601, (irot, True), (imir, True)])])

    grain = aomenc(work, "grain", colour_source(8, "420"), 8, extra=("--film-grain-test=1",))
    out["avif-film-grain.avif"] = avif([colour_item(grain, 8, 3, [bt601])])

    p3 = aomenc(work, "p3", colour_source(8, "420", P3_QUADRANTS), 8)
    out["avif-display-p3.avif"] = avif([colour_item(p3, 8, 3, [(nclx(12, 13, 6, False), False)])])

    # Hostile: a 64 x 64 ispe over a 16384 x 16384 frame header.
    square = aomenc(work, "square", grey_source(8, lambda x, y: 128), 8, monochrome=True)
    out["avif-frame-larger-than-ispe.avif"] = avif(
        [colour_item(enlarge_sequence_header(square, 16384, 16384), 8, 1, size=(WIDTH, WIDTH))]
    )
    # Hostile: a 32 x 32 alpha item under the 64 x 48 image.
    small = aomenc(work, "alpha-small", alpha_source(8, 32, 32).replace(b"W64 H48", b"W32 H32"), 8,
                   monochrome=True)
    colour = aomenc(work, "yuv420-8-small-alpha", colour_source(8, "420"), 8)
    out["avif-alpha-smaller-than-image.avif"] = avif(
        [colour_item(colour, 8, 3, [bt601]), alpha_item(small, 8, size=(32, 32))]
    )
    return out


def animation(work: Path) -> bytes:
    from PIL import Image

    frames = []
    for colours in (QUADRANTS, QUADRANTS[::-1]):
        frame = Image.new("RGB", (WIDTH, HEIGHT))
        frame.putdata([quadrant(x, y, colours) for y in range(HEIGHT) for x in range(WIDTH)])
        frames.append(frame)
    path = work / "animation.avif"
    frames[0].save(path, save_all=True, append_images=frames[1:], duration=100, quality=90,
                   speed=8, max_threads=1)
    return path.read_bytes()


# ---------------------------------------------------------------- JPEG 2000


def ppm(width: int, height: int, pixel) -> bytes:
    body = bytearray(f"P6\n{width} {height}\n255\n".encode())
    for y in range(height):
        for x in range(width):
            body += bytes(pixel(x, y))
    return bytes(body)


def opj(work: Path, name: str, source: bytes, suffix: str, *args: str, extension: str = "ppm") -> bytes:
    src = work / f"{name}.{extension}"
    out = work / f"{name}.{suffix}"
    src.write_bytes(source)
    subprocess.run(["opj_compress", "-i", str(src), "-o", str(out), *args], check=True,
                   capture_output=True)
    return out.read_bytes()


def jp2_boxes(data: bytes) -> list[tuple[bytes, bytes]]:
    boxes, at = [], 0
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at:at + 8])
        end = len(data) if length == 0 else at + length
        boxes.append((kind, data[at + 8:end]))
        at = end
    return boxes


def jp2(codestream: bytes, header: list[bytes], brand: bytes = b"jp2 ",
        compatible: tuple[bytes, ...] = (b"jp2 ",)) -> bytes:
    signature = box(b"jP  ", b"\r\n\x87\n")
    ftyp = box(b"ftyp", brand + b"\0\0\0\0" + b"".join(compatible))
    return signature + ftyp + box(b"jp2h", b"".join(header)) + box(b"jp2c", codestream)


def ihdr(width: int, height: int, components: int, bits: int) -> bytes:
    return box(b"ihdr", struct.pack(">IIHBBBB", height, width, components, bits - 1, 7, 0, 0))


def enumerated_colr(space: int) -> bytes:
    return box(b"colr", bytes([1, 0, 0]) + struct.pack(">I", space))


def s15(value: float) -> bytes:
    return struct.pack(">i", round(value * 65536))


def display_p3_icc() -> bytes:
    """A minimal ICC v2 display profile: Display P3 primaries adapted to D50
    (as Apple's profile records them) with a 2.2 gamma curve."""
    def xyz(x, y, z):
        return b"XYZ \0\0\0\0" + s15(x) + s15(y) + s15(z)

    description = b"Display P3 (GitTurtle fixture)\0"
    desc = (b"desc\0\0\0\0" + struct.pack(">I", len(description)) + description
            + b"\0" * 8 + b"\0" * 3 + b"\0" * 67)
    gamma = b"curv\0\0\0\0" + struct.pack(">IH", 1, round(2.2 * 256)) + b"\0\0"
    tags = [
        (b"desc", desc),
        (b"cprt", b"text\0\0\0\0Public domain test profile\0"),
        (b"wtpt", xyz(0.9642, 1.0, 0.8249)),
        (b"rXYZ", xyz(0.51512, 0.24120, -0.00105)),
        (b"gXYZ", xyz(0.29198, 0.69225, 0.04189)),
        (b"bXYZ", xyz(0.15710, 0.06657, 0.78407)),
        (b"rTRC", gamma),
        (b"gTRC", gamma),
        (b"bTRC", gamma),
    ]
    table_end = 128 + 4 + 12 * len(tags)
    entries, data = b"", b""
    for signature, payload in tags:
        while (table_end + len(data)) % 4:
            data += b"\0"
        entries += signature + struct.pack(">II", table_end + len(data), len(payload))
        data += payload
    size = table_end + len(data)
    header = (
        struct.pack(">I", size) + b"\0\0\0\0" + struct.pack(">I", 0x02100000) + b"mntrRGB XYZ "
        + struct.pack(">6H", 2026, 11, 3, 0, 0, 0) + b"acsp" + b"\0" * 24 + struct.pack(">I", 0)
        + s15(0.9642) + s15(1.0) + s15(0.8249) + b"\0" * 4 + b"\0" * 16 + b"\0" * 28
    )
    assert len(header) == 128
    return header + struct.pack(">I", len(tags)) + entries + data


def jpeg2000_fixtures(work: Path) -> dict[str, bytes]:
    out = {}
    rgb = ppm(WIDTH, HEIGHT, quadrant)
    out["jpeg2000-rgb.jp2"] = opj(work, "rgb", rgb, "jp2")
    out["jpeg2000-rgb.j2k"] = opj(work, "rgb", rgb, "j2k")

    grey = bytearray(f"P5\n{WIDTH} {HEIGHT}\n65535\n".encode())
    for y in range(HEIGHT):
        for x in range(WIDTH):
            grey += struct.pack(">H", round(x * 65535 / (WIDTH - 1)))
    out["jpeg2000-grey16.jp2"] = opj(work, "grey16", bytes(grey), "jp2", extension="pgm")

    pam = bytearray(
        f"P7\nWIDTH {WIDTH}\nHEIGHT {HEIGHT}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".encode()
    )
    for y in range(HEIGHT):
        for x in range(WIDTH):
            pam += bytes(quadrant(x, y)) + bytes([round(255 * alpha_at(x))])
    out["jpeg2000-alpha.jp2"] = opj(work, "alpha", bytes(pam), "jp2", extension="pam")

    p3 = opj(work, "p3", ppm(WIDTH, HEIGHT, lambda x, y: quadrant(x, y, P3_QUADRANTS)), "j2k")
    icc = box(b"colr", bytes([2, 0, 0]) + display_p3_icc())
    out["jpeg2000-display-p3-icc.jp2"] = jp2(p3, [ihdr(WIDTH, HEIGHT, 3, 8), icc])

    # Palette: one 8-bit index component and a four-entry RGB palette.
    indices = bytearray(f"P5\n{WIDTH} {HEIGHT}\n255\n".encode())
    for y in range(HEIGHT):
        for x in range(WIDTH):
            indices.append((1 if x >= WIDTH // 2 else 0) + (2 if y >= HEIGHT // 2 else 0))
    index_stream = opj(work, "indices", bytes(indices), "j2k", extension="pgm")
    pclr = box(b"pclr", struct.pack(">HB", 4, 3) + bytes([7, 7, 7]) + b"".join(bytes(c) for c in QUADRANTS))
    cmap = box(b"cmap", b"".join(struct.pack(">HBB", 0, 1, column) for column in range(3)))
    out["jpeg2000-palette.jp2"] = jp2(
        index_stream, [ihdr(WIDTH, HEIGHT, 1, 8), enumerated_colr(16), pclr, cmap]
    )

    # sYCC with both chroma components subsampled 2 x 2 (full resolution only).
    planes = [bytearray(), bytearray(), bytearray()]
    for y in range(HEIGHT):
        for x in range(WIDTH):
            r, g, b = quadrant(x, y)
            planes[0].append(round(0.299 * r + 0.587 * g + 0.114 * b))
    for y in range(0, HEIGHT, 2):
        for x in range(0, WIDTH, 2):
            r, g, b = quadrant(x, y)
            planes[1].append(round(128 - 0.168736 * r - 0.331264 * g + 0.5 * b))
            planes[2].append(round(128 + 0.5 * r - 0.418688 * g - 0.081312 * b))
    raw = work / "sycc.raw"
    raw.write_bytes(b"".join(planes))
    stream = work / "sycc.j2k"
    subprocess.run(["opj_compress", "-i", str(raw), "-o", str(stream), "-F",
                    f"{WIDTH},{HEIGHT},3,8,u@1x1:2x2:2x2"], check=True, capture_output=True)
    out["jpeg2000-subsampled.jp2"] = jp2(
        stream.read_bytes(), [ihdr(WIDTH, HEIGHT, 3, 8), enumerated_colr(18)]
    )

    # A larger lossy image that decodes at reduced resolution.
    large = ppm(1024, 768, lambda x, y: (x * 255 // 1023, y * 255 // 767, 128))
    out["jpeg2000-large.jp2"] = opj(work, "large", large, "jp2", "-r", "100", "-n", "6")
    return out


# ---------------------------------------------------------------- expected colours


def srgb_decode(v: float) -> float:
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def srgb_encode(v: float) -> float:
    v = min(max(v, 0.0), 1.0)
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


def rgb_to_xyz(primaries, white):
    def column(x, y):
        return (x / y, 1.0, (1 - x - y) / y)

    columns = [column(*p) for p in primaries]
    wx, wy, wz = column(*white)
    m = [[columns[c][r] for c in range(3)] for r in range(3)]
    s = solve(m, (wx, wy, wz))
    return [[m[r][c] * s[c] for c in range(3)] for r in range(3)]


def solve(m, v):
    import copy

    a = copy.deepcopy(m)
    b = list(v)
    for i in range(3):
        pivot = max(range(i, 3), key=lambda r: abs(a[r][i]))
        a[i], a[pivot], b[i], b[pivot] = a[pivot], a[i], b[pivot], b[i]
        for r in range(3):
            if r != i:
                f = a[r][i] / a[i][i]
                a[r] = [a[r][c] - f * a[i][c] for c in range(3)]
                b[r] -= f * b[i]
    return [b[i] / a[i][i] for i in range(3)]


def p3_to_srgb(rgb, decode) -> tuple[int, ...]:
    d65 = (0.3127, 0.3290)
    p3 = rgb_to_xyz(((0.680, 0.320), (0.265, 0.690), (0.150, 0.060)), d65)
    srgb = rgb_to_xyz(((0.640, 0.330), (0.300, 0.600), (0.150, 0.060)), d65)
    linear = [decode(c / 255) for c in rgb]
    xyz = [sum(p3[r][c] * linear[c] for c in range(3)) for r in range(3)]
    out = solve(srgb, xyz)
    return tuple(round(srgb_encode(c) * 255) for c in out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="compare without writing")
    arguments = parser.parse_args()
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        fixtures = avif_fixtures(work)
        fixtures["avif-animation.avif"] = animation(work)
        fixtures.update(jpeg2000_fixtures(work))
        differs = 0
        for name, data in sorted(fixtures.items()):
            target = FIXTURES / name
            digest = hashlib.sha256(data).hexdigest()
            if arguments.check:
                same = target.exists() and target.read_bytes() == data
                differs += not same
                print(f"{'same   ' if same else 'differs'} {name} {digest}")
            else:
                target.write_bytes(data)
                print(f"{name} {len(data)} {digest}")
    print("Expected sRGB of the Display P3 quadrants (sRGB transfer):",
          [p3_to_srgb(c, srgb_decode) for c in P3_QUADRANTS])
    print("Expected sRGB of the Display P3 quadrants (2.2 gamma):",
          [p3_to_srgb(c, lambda v: v ** 2.2) for c in P3_QUADRANTS])
    for tool in ("aomenc", "opj_compress"):
        if shutil.which(tool):
            version = subprocess.run([tool, "--help" if tool == "aomenc" else "-h"],
                                     capture_output=True, text=True)
            line = next((l for l in (version.stdout + version.stderr).splitlines()
                         if "AV1 Encoder" in l or "opj_compress" in l and "version" in l), "")
            print(f"{tool}: {line.strip()}")
    return 1 if differs else 0


if __name__ == "__main__":
    sys.exit(main())
