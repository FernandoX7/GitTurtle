#!/usr/bin/env python3
"""Create disposable native GLB comparison history without third-party packages."""
import argparse
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import struct
import subprocess


CUBE = [(-.5, -.5, -.5), (.5, -.5, -.5), (.5, .5, -.5), (-.5, .5, -.5),
        (-.5, -.5, .5), (.5, -.5, .5), (.5, .5, .5), (-.5, .5, .5)]
FACES = [(0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4),
         (3, 7, 6), (3, 6, 2), (0, 4, 7), (0, 7, 3), (1, 2, 6), (1, 6, 5)]


def container(document, binary):
    document = copy.deepcopy(document)
    document["buffers"] = [{"byteLength": len(binary), **document.get("buffers", [{}])[0]}]
    encoded = json.dumps(document, separators=(",", ":"), sort_keys=True).encode()
    encoded += b" " * (-len(encoded) % 4)
    binary += b"\0" * (-len(binary) % 4)
    chunks = struct.pack("<II", len(encoded), 0x4E4F534A) + encoded
    chunks += struct.pack("<II", len(binary), 0x004E4942) + binary
    return struct.pack("<III", 0x46546C67, 2, 12 + len(chunks)) + chunks


def assembly(after=False):
    """A shared cube in a nested, three-part arch; glTF coordinates are meters."""
    vertices = b"".join(struct.pack("<3f", *point) for point in CUBE)
    indices = b"".join(struct.pack("<3H", *face) for face in FACES)
    document = {
        "asset": {"version": "2.0", "generator": "GitTurtle synthetic GLB QA fixture"},
        "scene": 0, "scenes": [{"nodes": [0]}],
        "nodes": [
            {"name": "Assembly", "children": [1, 2, 3],
             "translation": [1.5, 0, 0] if after else [0, 0, 0],
             "scale": [1.5, 1.5, 1.5] if after else [1, 1, 1]},
            {"name": "Left column", "mesh": 0, "translation": [-1, 1, 0], "scale": [.4, 2, .6]},
            {"name": "Right column", "mesh": 0, "translation": [1, 1, 0], "scale": [.4, 2, .6]},
            {"name": "Nested beam", "children": [4], "translation": [0, 2.2, 0]},
            {"name": "Beam", "mesh": 0, "scale": [2.4, .4, .6]},
        ],
        "meshes": [{"primitives": [
            {"attributes": {"POSITION": 0}, "indices": 1},
            {"attributes": {"POSITION": 0}, "indices": 2},
        ]}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(vertices), "target": 34962},
            {"buffer": 0, "byteOffset": len(vertices), "byteLength": len(indices), "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [-.5] * 3, "max": [.5] * 3},
            {"bufferView": 1, "componentType": 5123, "count": 18, "type": "SCALAR"},
            {"bufferView": 1, "byteOffset": 36, "componentType": 5123, "count": 18, "type": "SCALAR"},
        ],
    }
    return document, vertices + indices


def synthetic_files():
    before, binary = assembly()
    after, _ = assembly(True)
    external = copy.deepcopy(before)
    external["buffers"] = [{"uri": "https://example.invalid/do-not-fetch.bin", "byteLength": len(binary)}]
    skin = copy.deepcopy(before)
    skin["skins"] = [{"joints": [0]}]
    skin["nodes"][1]["skin"] = 0
    extension = copy.deepcopy(before)
    extension["extensionsUsed"] = ["KHR_draco_mesh_compression"]
    extension["extensionsRequired"] = ["KHR_draco_mesh_compression"]
    extension["meshes"][0]["primitives"][0]["extensions"] = {
        "KHR_draco_mesh_compression": {"bufferView": 0, "attributes": {"POSITION": 0}}}
    morph = copy.deepcopy(before)
    morph["meshes"][0]["primitives"][0]["targets"] = [{"POSITION": 0}]
    original = container(before, binary)
    return {
        "assembly-before.glb": original,
        "assembly-after.glb": container(after, binary),
        "external-buffer.glb": container(external, binary),
        "skinned.glb": container(skin, binary),
        "compressed.glb": container(extension, binary),
        "morph-target.glb": container(morph, binary),
        "truncated.glb": original[:-13],
    }


def obj(after=False):
    # Match the GLB's final Z-up/mm geometry while retaining OBJ's unknown units.
    vertices = []
    faces = []
    offset = 1500 if after else 0
    factor = 1.5 if after else 1
    for translation, scale in [((-1, 1, 0), (.4, 2, .6)), ((1, 1, 0), (.4, 2, .6)), ((0, 2.2, 0), (2.4, .4, .6))]:
        start = len(vertices)
        for point in CUBE:
            x, y, z = [(point[i] * scale[i] + translation[i]) * 1000 * factor for i in range(3)]
            vertices.append((x + offset, -z, y))
        faces.extend(tuple(index + start + 1 for index in face) for face in FACES)
    return "# Synthetic arch; physical units are unspecified in OBJ\n" + "".join(
        f"v {x:g} {y:g} {z:g}\n" for x, y, z in vertices) + "".join(
        f"f {a} {b} {c}\n" for a, b, c in faces)


def dense_torus():
    """65,536 triangles with UINT indices, kept in the disposable repository."""
    rings, segments = 256, 128
    vertices = bytearray()
    indices = bytearray()
    for ring in range(rings):
        theta = math.tau * ring / rings
        for segment in range(segments):
            phi = math.tau * segment / segments
            radius = 1 + .35 * math.cos(phi)
            vertices.extend(struct.pack("<3f", radius * math.cos(theta),
                                        .35 * math.sin(phi), radius * math.sin(theta)))
            a = ring * segments + segment
            b = ((ring + 1) % rings) * segments + segment
            c = ((ring + 1) % rings) * segments + (segment + 1) % segments
            d = ring * segments + (segment + 1) % segments
            indices.extend(struct.pack("<6I", a, b, c, a, c, d))
    outer = struct.unpack("<f", struct.pack("<f", 1.35))[0]
    minor = struct.unpack("<f", struct.pack("<f", .35))[0]
    document = {
        "asset": {"version": "2.0", "generator": "GitTurtle synthetic GLB QA fixture"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(vertices), "target": 34962},
            {"buffer": 0, "byteOffset": len(vertices), "byteLength": len(indices), "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": rings * segments, "type": "VEC3",
             "min": [-outer, -minor, -outer], "max": [outer, minor, outer]},
            {"bufferView": 1, "componentType": 5125, "count": rings * segments * 6, "type": "SCALAR"},
        ],
    }
    return container(document, bytes(vertices + indices))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new disposable directory outside the checkout")
    args = parser.parse_args()
    destination = args.destination.expanduser().resolve()
    checkout = Path(__file__).resolve().parents[1]
    if destination.exists() or destination == checkout or checkout in destination.parents:
        raise SystemExit("Choose a new disposable directory outside the checkout")
    destination.mkdir(parents=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)

    def git(*arguments):
        return subprocess.check_output(["git", "-C", str(destination), *arguments], env=env, text=True).rstrip("\n")

    files = synthetic_files()
    assets = checkout / "crates/preview/tests/fixtures/models/glb"
    git("init", "-b", "main")
    (destination / ".git/qa-empty-hooks").mkdir()
    for key, value in [("user.name", "GLB Preview Fixture"), ("user.email", "glb@example.invalid"),
                       ("commit.gpgSign", "false"), ("tag.gpgSign", "false"), ("core.hooksPath", ".git/qa-empty-hooks")]:
        git("config", key, value)
    (destination / "assembly.glb").write_bytes(files["assembly-before.glb"])
    (destination / "baseline.obj").write_text(obj())
    # Distinct bytes prevent rename detection pairing this deletion with the
    # deliberately truncated assembly added in the next commit.
    shutil.copyfile(assets / "BoxInterleaved.glb", destination / "deleted.glb")
    (destination / "working-deleted.glb").write_bytes(files["assembly-before.glb"])
    (destination / "dense-torus.glb").write_bytes(dense_torus())
    # Copy already captured official assets. This script never downloads anything.
    for source in sorted(assets.glob("*.glb")):
        if source.name not in files:
            shutil.copyfile(source, destination / source.name)
    git("add", ".")
    git("commit", "-m", "Original GLB assembly and existing OBJ baseline")
    before = git("rev-parse", "HEAD")
    (destination / "assembly.glb").write_bytes(files["assembly-after.glb"])
    (destination / "baseline.obj").write_text(obj(True))
    (destination / "added.GLB").write_bytes(files["assembly-after.glb"])
    (destination / "deleted.glb").unlink()
    for name, data in files.items():
        if name not in ("assembly-before.glb", "assembly-after.glb"):
            (destination / name).write_bytes(data)
    git("add", ".")
    git("commit", "-m", "Move and enlarge assembly; add, delete and refuse unsupported GLBs")
    after = git("rev-parse", "HEAD")
    (destination / "assembly.glb").write_bytes(files["assembly-before.glb"])
    (destination / "working-added.GLB").write_bytes(files["assembly-after.glb"])
    (destination / "working-deleted.glb").unlink()
    manifest = {
        "repository": str(destination), "before": before, "after": after,
        "after_glb_bounds_mm": {"min": [-300, -450, 0], "max": [3300, 450, 3600]},
        "before_glb_bounds_mm": {"min": [-1200, -300, 0], "max": [1200, 300, 2400]},
        "triangle_count_per_assembly": 36,
        "history_name_status": git("diff", "--find-renames", "--name-status", before, after),
        "status": git("status", "--porcelain=v1"),
        "refs": git("show-ref"),
        "index_sha256": hashlib.sha256((destination / ".git/index").read_bytes()).hexdigest(),
        "files": {path.name: {"bytes": path.stat().st_size, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                  for path in sorted(destination.iterdir()) if path.is_file()},
        "native_cases": [
            "History HEAD baseline.obj: existing-format baseline with shared camera position/scale difference",
            "History HEAD assembly.glb: 36 triangles each; After is 1.5x larger and +1500 mm on X",
            "History HEAD added.GLB and deleted.glb: uppercase recognition and opposite missing sides",
            "History HEAD malformed/unsupported fixtures: readable errors without referenced resource access",
            "Working Changes assembly.glb: captured HEAD compared with smaller original worktree geometry",
            "Working Changes working-added.GLB / working-deleted.glb: missing sides",
            "Quick Open official assets: static neutral geometry; appearance omission is disclosed",
            "Quick Open dense-torus.glb: 65,536 triangles for bounded decode/render and rapid-selection checks",
        ],
    }
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
