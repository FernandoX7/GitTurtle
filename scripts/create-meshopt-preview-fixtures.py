#!/usr/bin/env python3
"""Create disposable History/Working meshopt cases from publishable fixtures."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile


def raster_limit(helper):
    """1,000 coincident triangles: small geometry, excessive settled raster work."""
    positions = struct.pack("<9f", -1, 0, 0, 1, 0, 0, 0, 2, 0)
    indices = struct.pack("<3H", 0, 1, 2) * 1000
    document = {
        "asset": {"version": "2.0", "generator": "GitTurtle synthetic raster budget fixture"},
        "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(positions), "target": 34962},
            {"buffer": 0, "byteOffset": len(positions), "byteLength": len(indices), "target": 34963},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [-1, 0, 0], "max": [1, 2, 0]},
            {"bufferView": 1, "componentType": 5123, "count": 3000, "type": "SCALAR"},
        ],
    }
    return helper.container(document, positions + indices)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new directory outside the checkout")
    parser.add_argument("--upstream-js", type=Path, required=True,
                        help="local meshoptimizer 0.25 encoder/decoder JS directory; no downloads")
    args = parser.parse_args()
    checkout = Path(__file__).resolve().parents[1]
    destination = args.destination.expanduser().resolve()
    if destination.exists() or destination == checkout or checkout in destination.parents:
        raise SystemExit("Choose a new disposable directory outside the checkout")
    for filename in ("meshopt_encoder.js", "meshopt_decoder.js"):
        if not (args.upstream_js / filename).is_file():
            raise SystemExit(f"Missing local upstream module: {filename}")
    # Load only authored geometry helpers; avoid writing __pycache__ in checkout.
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location("glb_fixtures", checkout / "scripts/create-glb-preview-fixtures.py")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    destination.mkdir(parents=True)
    assets = checkout / "crates/preview/tests/fixtures/models/glb"
    before_bytes = (assets / "meshopt-arch-before.glb").read_bytes()
    after_bytes = (assets / "meshopt-arch-after.glb").read_bytes()
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)

    def git(*arguments):
        return subprocess.check_output(["git", "-C", str(destination), *arguments], env=env, text=True).rstrip("\n")

    def write(name, data):
        (destination / name).write_bytes(data)

    def pointer(data):
        digest = hashlib.sha256(data).hexdigest()
        return f"version https://git-lfs.github.com/spec/v1\noid sha256:{digest}\nsize {len(data)}\n".encode()

    git("init", "-b", "main")
    (destination / ".git/qa-empty-hooks").mkdir()
    for key, value in [("user.name", "Meshopt Preview Fixture"), ("user.email", "meshopt@example.invalid"),
                       ("commit.gpgSign", "false"), ("tag.gpgSign", "false"), ("core.hooksPath", ".git/qa-empty-hooks")]:
        git("config", key, value)
    for name in ("assembly.glb", "working-deleted.glb", "one-side-failed.glb", "raster-limit.glb"):
        write(name, before_bytes)
    write("lfs-one-side.glb", pointer(before_bytes))
    write("lfs-available.glb", pointer(before_bytes))
    write("lfs-missing.glb", pointer(after_bytes))
    avocado = (assets / "meshopt-avocado.glb").read_bytes()
    write("lfs-corrupt.glb", pointer(avocado))
    # Pointer recognition does not require installed LFS filters. Seed only
    # local objects, without invoking git-lfs, network actions or smudge helpers.
    local_objects = {}
    for original, payload in ((before_bytes, before_bytes), (avocado, b"deliberately corrupt local LFS object")):
        digest = hashlib.sha256(original).hexdigest()
        object_path = destination / ".git/lfs/objects" / digest[:2] / digest[2:4] / digest
        object_path.parent.mkdir(parents=True, exist_ok=True)
        object_path.write_bytes(payload)
        local_objects[str(object_path.relative_to(destination))] = hashlib.sha256(payload).hexdigest()
    write("deleted.glb", (assets / "meshopt-avocado.glb").read_bytes())
    for name in ("meshopt-avocado.glb", "Avocado.glb", "BoxInterleaved.glb", "RiggedSimple.glb"):
        shutil.copyfile(assets / name, destination / name)
    write("dense-torus-uncompressed.glb", helper.dense_torus())
    dense_reference = json.loads(subprocess.check_output([
        "node", str(checkout / "scripts/encode-meshopt-glb.cjs"), str(args.upstream_js.resolve()),
        str(destination / "dense-torus-uncompressed.glb"), str(destination / "dense-torus.glb")], text=True))
    (destination / "baseline.obj").write_text(helper.obj())
    git("add", ".")
    git("commit", "-m", "Original compressed arch, public avocado and dense torus")
    before = git("rev-parse", "HEAD")
    write("assembly.glb", after_bytes)
    write("added.GLB", after_bytes)
    write("one-side-failed.glb", before_bytes[:-13])
    write("lfs-one-side.glb", pointer(after_bytes))
    with tempfile.TemporaryDirectory(prefix="gitturtle-raster-source-") as temporary:
        source = Path(temporary) / "raster.glb"
        source.write_bytes(raster_limit(helper))
        raster_reference = json.loads(subprocess.check_output([
            "node", str(checkout / "scripts/encode-meshopt-glb.cjs"), str(args.upstream_js.resolve()),
            str(source), str(destination / "raster-limit.glb")], text=True))
    (destination / "deleted.glb").unlink()
    # Valid framing, invalid compressed ATTRIBUTES bitstream: readable side error.
    corrupt = bytearray(after_bytes)
    binary_start = 28 + struct.unpack_from("<I", corrupt, 12)[0]
    corrupt[binary_start] = 0
    write("malformed-stream.glb", corrupt)
    for name in ("external-buffer.glb", "skinned.glb", "morph-target.glb", "compressed.glb"):
        shutil.copyfile(assets / name, destination / name)
    (destination / "baseline.obj").write_text(helper.obj(True))
    git("add", ".")
    git("commit", "-m", "Move and enlarge compressed arch; missing and failed comparison sides")
    after = git("rev-parse", "HEAD")
    write("assembly.glb", before_bytes)
    write("working-added.GLB", after_bytes)
    (destination / "working-deleted.glb").unlink()
    print(json.dumps({
        "repository": str(destination), "before": before, "after": after,
        "before_bounds_mm": {"minimum": [-1200, -300, 0], "maximum": [1200, 300, 2400]},
        "after_bounds_mm": {"minimum": [-300, -450, 0], "maximum": [3300, 450, 3600]},
        "union_bounds_mm": {"minimum": [-1200, -450, 0], "maximum": [3300, 450, 3600]},
        "assembly_triangles": 36, "dense_torus_triangles": 65536,
        "local_lfs_object_sha256": local_objects,
        "dense_reference": dense_reference,
        "raster_limit_reference": raster_reference,
        "history_name_status": git("diff", "--find-renames", "--name-status", before, after),
        "status": git("status", "--porcelain=v1"), "refs": git("show-ref"),
        "index_sha256": hashlib.sha256((destination / ".git/index").read_bytes()).hexdigest(),
        "files": {p.name: {"bytes": p.stat().st_size, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
                  for p in sorted(destination.iterdir()) if p.is_file()},
        "native_cases": [
            "History assembly.glb: two directly decoded compressed arches, preserved changed scale/position",
            "History added.GLB and deleted.glb: added and deleted compressed models",
            "History one-side-failed.glb: useful Before canvas and independent truncated After error",
            "Working assembly.glb: larger index version versus original working geometry",
            "Working working-added.GLB and working-deleted.glb: opposite absent sides",
            "Quick Open dense-torus.glb and dense-torus-uncompressed.glb: same 65,536 triangles",
            "Quick Open meshopt-avocado.glb and Avocado.glb: same CC0 static geometry",
            "Malformed stream, skin, morph, Draco and external-buffer explicit error states",
            "History raster-limit.glb: useful Before plus decoded After exceeding the settled solid raster budget",
            "History lfs-one-side.glb: locally resolved compressed Before and missing local After",
            "Quick Open lfs-available/lfs-missing/lfs-corrupt.glb: resolved, unavailable and corrupt local bytes",
        ],
    }, indent=2))


if __name__ == "__main__":
    main()
