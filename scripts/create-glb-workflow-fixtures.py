#!/usr/bin/env python3
"""Deterministic public appearance/deformation/animation fixtures; never downloads.

An ordinary invocation writes GLBs plus independent authored expectations to a
new directory. --repository additionally creates a disposable history/working
comparison outside this checkout. Every generated asset follows the repo license.
"""
import argparse
import base64
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import zlib


class Asset:
    def __init__(self):
        self.binary = bytearray()
        self.doc = {"asset": {"version": "2.0", "generator": "GitTurtle public workflow fixture"},
                    "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
                    "meshes": [{"primitives": []}], "bufferViews": [], "accessors": []}

    def view(self, data):
        self.binary.extend(bytes((-len(self.binary)) % 4))
        result = len(self.doc["bufferViews"])
        self.doc["bufferViews"].append({"buffer": 0, "byteOffset": len(self.binary), "byteLength": len(data)})
        self.binary.extend(data)
        return result

    def accessor(self, values, kind, component=5126, normalized=False):
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[kind]
        rows = [[v] if width == 1 else v for v in values]
        flat = [v for row in rows for v in row]
        fmt = {5126: "f", 5123: "H", 5121: "B"}[component]
        result = len(self.doc["accessors"])
        entry = {"bufferView": self.view(struct.pack("<" + fmt * len(flat), *flat)),
                 "componentType": component, "count": len(rows), "type": kind}
        if kind in ("SCALAR", "VEC3"):
            entry.update(min=[min(row[i] for row in rows) for i in range(width)],
                         max=[max(row[i] for row in rows) for i in range(width)])
        if normalized:
            entry["normalized"] = True
        self.doc["accessors"].append(entry)
        return result

    def bytes(self):
        doc = copy.deepcopy(self.doc)
        doc["buffers"] = [{"byteLength": len(self.binary)}]
        return container(doc, self.binary)


def container(doc, binary):
    encoded = json.dumps(doc, separators=(",", ":"), sort_keys=True).encode()
    encoded += b" " * ((-len(encoded)) % 4)
    binary = bytes(binary) + bytes((-len(binary)) % 4)
    return struct.pack("<III", 0x46546C67, 2, 28 + len(encoded) + len(binary)) + struct.pack("<II", len(encoded), 0x4E4F534A) + encoded + struct.pack("<II", len(binary), 0x004E4942) + binary


def png(pixels, width=2, height=2):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    raw = b"".join(b"\0" + bytes(sum(pixels[y * width:(y + 1) * width], [])) for y in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


PANEL = [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]]
INDICES = [0, 1, 2, 0, 2, 3]

# Authored RGBA red/green/blue/white 2x2 panel, encoded by sharp 0.35.3 with
# jpeg({quality:100,chromaSubsampling:'4:4:4'}). Embedded bytes make subsequent
# fixture generation independent of an installed JPEG encoder or its version.
JPEG_PANEL = base64.b64decode("/9j/2wBDAAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/2wBDAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/wAARCAACAAIDAREAAhEBAxEB/8QAFAABAAAAAAAAAAAAAAAAAAAACv/EABsQAAIDAQEBAAAAAAAAAAAAAAQFAwYHAggJ/8QAFQEBAQAAAAAAAAAAAAAAAAAABgf/xAAbEQACAwEBAQAAAAAAAAAAAAAEBgMFBwIIAf/aAAwDAQACEQMRAD8AWB84MIw+y/PHwZYrFjWUv7A/8YeXHT166zuoNXLpy1w+jHNGzZocnnNZM2Rs85h55k8xRhU0pBEsk0nffSD1D58wOq9MeiaurxDIK2trd01sCurgM0TAwQAQ39gHECCEHpYxxRBR444BxoI44YIY+IouOeOeefkfPxfHX0416esnzR0dnQslscXFsRFZjamxqY5u7hhZmZhuKoy3vmC9tzDLS5ubQwqxtLEok44mcmeWXr//2Q==")


def panel(color=(1., 1., 1., 1.), pixels=None, vertex_color=None, alpha="OPAQUE", double_sided=False, wrap=10497, uv_scale=1.):
    a = Asset()
    attrs = {"POSITION": a.accessor(PANEL, "VEC3"),
             "TEXCOORD_0": a.accessor([[0, uv_scale], [uv_scale, uv_scale], [uv_scale, 0], [0, 0]], "VEC2")}
    if vertex_color:
        attrs["COLOR_0"] = a.accessor([vertex_color] * 4, "VEC4", 5121, True)
    a.doc["meshes"][0]["primitives"] = [{"attributes": attrs, "indices": a.accessor(INDICES, "SCALAR", 5123), "material": 0}]
    material = {"pbrMetallicRoughness": {"baseColorFactor": list(color)}, "alphaMode": alpha,
                "doubleSided": double_sided, "extensions": {"KHR_materials_unlit": {}}}
    a.doc.update(materials=[material], extensionsUsed=["KHR_materials_unlit"])
    if pixels:
        a.doc.update(images=[{"bufferView": a.view(png(pixels)), "mimeType": "image/png"}],
                     textures=[{"source": 0, "sampler": 0}],
                     samplers=[{"magFilter": 9728, "minFilter": 9728, "wrapS": wrap, "wrapT": wrap}])
        material["pbrMetallicRoughness"]["baseColorTexture"] = {"index": 0}
    return a


def channel(a, clip, node, path, times, values, kind, interpolation="LINEAR"):
    sampler = len(clip["samplers"])
    clip["samplers"].append({"input": a.accessor(times, "SCALAR"),
                             "output": a.accessor(values, kind), "interpolation": interpolation})
    clip["channels"].append({"sampler": sampler, "target": {"node": node, "path": path}})


def clip(name):
    return {"name": name, "samplers": [], "channels": []}


def morph():
    a = panel((.12, .55, 1., 1.))
    delta = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]
    a.doc["meshes"][0].update(weights=[.25])
    a.doc["nodes"][0].update(weights=[.5], translation=[2., 1., -1.])
    a.doc["meshes"][0]["primitives"][0]["targets"] = [{"POSITION": a.accessor(delta, "VEC3")}]
    linear = clip("Linear morph 2s")
    channel(a, linear, 0, "weights", [0, 2], [0., 1.], "SCALAR")
    cubic = clip("Cubic morph 2s")
    channel(a, cubic, 0, "weights", [0, 2], [0., 0., 1., 0., 1., 0.], "SCALAR", "CUBICSPLINE")
    step = clip("Step morph 1s")
    channel(a, step, 0, "weights", [0, .5, 1], [0., 1., .25], "SCALAR", "STEP")
    a.doc["animations"] = [linear, cubic, step]
    return a


def skin():
    a = panel((1., .45, .06, 1.))
    # The nonidentity mesh placement is deliberately irrelevant to world skinning.
    a.doc["nodes"] = [{"children": [1, 2], "translation": [3., 2., -1.]},
                      {"mesh": 0, "skin": 0, "translation": [4., 0., 0.]},
                      {"name": "Root joint", "children": [3]},
                      {"name": "Tip joint", "translation": [0., 1., 0.]}]
    primitive = a.doc["meshes"][0]["primitives"][0]
    primitive["attributes"]["JOINTS_0"] = a.accessor([[0, 1, 0, 0]] * 4, "VEC4", 5123)
    primitive["attributes"]["WEIGHTS_0"] = a.accessor([[1., 0., 0., 0.], [.75, .25, 0., 0.], [0., 1., 0., 0.], [.25, .75, 0., 0.]], "VEC4")
    identity = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
    inverse = identity.copy()
    inverse[13] = -1.
    a.doc["skins"] = [{"joints": [2, 3], "inverseBindMatrices": a.accessor([identity, inverse], "MAT4")}]
    linear = clip("Joint bend 2s")
    channel(a, linear, 3, "rotation", [0., 2.], [[0., 0., 0., 1.], [0., 0., 1., 0.]], "VEC4")
    channel(a, linear, 2, "translation", [0., 2.], [[0., 0., 0.], [1., 0., 0.]], "VEC3")
    a.doc["animations"] = [linear]
    return a


def skin_morph():
    a = skin()
    delta = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]
    a.doc["meshes"][0]["weights"] = [.25]
    a.doc["meshes"][0]["primitives"][0]["targets"] = [{"POSITION": a.accessor(delta, "VEC3")}]
    a.doc["animations"][0]["name"] = "Morph then joint bend 2s"
    channel(a, a.doc["animations"][0], 1, "weights", [0., 2.], [0., 1.], "SCALAR")
    return a


def trs():
    a = panel((.7, .2, 1., 1.))
    a.doc["nodes"] = [{"translation": [2., 1., -1.], "children": [1]}, {"mesh": 0}]
    linear = clip("TRS linear 2s")
    channel(a, linear, 1, "translation", [0., 2.], [[0., 0., 0.], [2., 0., 0.]], "VEC3")
    channel(a, linear, 1, "scale", [0., 2.], [[1., 1., 1.], [2., .5, 1.]], "VEC3")
    channel(a, linear, 1, "rotation", [0., 2.], [[0., 0., 0., 1.], [0., 0., 1., 0.]], "VEC4")
    cubic = clip("Translation cubic 2s")
    channel(a, cubic, 1, "translation", [0., 2.], [[0., 0., 0.], [0., 0., 0.], [2., 0., 0.], [0., 0., 0.], [2., 0., 0.], [0., 0., 0.]], "VEC3", "CUBICSPLINE")
    step = clip("Translation step 1s")
    channel(a, step, 1, "translation", [0., .5, 1.], [[0., 0., 0.], [2., 0., 0.], [0., 1., 0.]], "VEC3", "STEP")
    a.doc["animations"] = [linear, cubic, step]
    return a


def authored_expectations():
    """Closed-form authored math, independent of GLB parsing or production code."""
    def converted(points):
        return [[1000 * x, -1000 * z, 1000 * y] for x, y, z in points]
    result = []
    for t in [0., .25, .5, 1., 1.5, 2.]:
        u = t / 2
        angle = math.pi * u
        ca, sa = math.cos(angle), math.sin(angle)
        posed = []
        for (x, y, z), weight in zip(PANEL, [0., .25, 1., .75]):
            rx, ry = ca * x - sa * (y - 1), sa * x + ca * (y - 1) + 1
            posed.append([3 + u + (1 - weight) * x + weight * rx, 2 + (1 - weight) * y + weight * ry, z - 1])
        result.append({"file": "skin-animated.glb", "clip": 0, "time": t, "vertices_mm": converted(posed)})
        combined = []
        for i, ((px, py, z), weight) in enumerate(zip(PANEL, [0., .25, 1., .75])):
            x, y = px + (u if i in [1, 2] else 0), py + (u if i in [2, 3] else 0)
            rx, ry = ca * x - sa * (y - 1), sa * x + ca * (y - 1) + 1
            combined.append([3 + u + (1 - weight) * x + weight * rx, 2 + (1 - weight) * y + weight * ry, z - 1])
        result.append({"file": "skin-morph-animation.glb", "clip": 0, "time": t, "vertices_mm": converted(combined)})
        linear_trs = [[2 + t + ca * x * (1 + u) - sa * y * (1 - .5 * u),
                       1 + sa * x * (1 + u) + ca * y * (1 - .5 * u), z - 1] for x, y, z in PANEL]
        result.append({"file": "trs-animation.glb", "clip": 0, "time": t, "vertices_mm": converted(linear_trs)})
        # Cubic Hermite: outgoing tangent=2 m/s, incoming tangent=0, dt=2s.
        translation = (-2 * u**3 + 3 * u**2) * 2 + (u**3 - 2 * u**2 + u) * 4
        result.append({"file": "trs-animation.glb", "clip": 1, "time": t,
                       "vertices_mm": converted([[2 + translation + x, 1 + y, z - 1] for x, y, z in PANEL])})
        for index, w in [(0, u), (1, (-2 * u**3 + 3 * u**2) + 2 * (u**3 - 2 * u**2 + u))]:
            points = [[x + 2 + (w if i in [1, 2] else 0), y + 1 + (w if i in [2, 3] else 0), z - 1] for i, (x, y, z) in enumerate(PANEL)]
            result.append({"file": "morph-animation.glb", "clip": index, "time": t, "vertices_mm": converted(points)})
    for t in [0., .25, .499, .5, .75, 1.]:
        x, y = (0, 0) if t < .5 else (2, 0) if t < 1 else (0, 1)
        result.append({"file": "trs-animation.glb", "clip": 2, "time": t,
                       "vertices_mm": converted([[2 + x + px, 1 + y + py, pz - 1] for px, py, pz in PANEL])})
    return {"indices": INDICES, "samples": result,
            "notes": "glTF meters Y-up -> [1000*x,-1000*z,1000*y] mm. Authored equations include dt-scaled Hermite tangents, quaternion slerp and joint inverse bind."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--repository", action="store_true")
    args = parser.parse_args()
    destination = args.destination.resolve()
    checkout = Path(__file__).resolve().parents[1]
    if destination.exists() or (args.repository and (destination == checkout or checkout in destination.parents)):
        raise SystemExit("Choose a new destination; disposable repositories must be outside the checkout")
    destination.mkdir(parents=True)
    before = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255]]
    after = [[0, 255, 255, 255], [255, 0, 255, 255], [255, 255, 0, 255], [0, 0, 0, 255]]
    assets = {"material-before.glb": panel((1., .08, .02, 1.)),
              "material-after.glb": panel((.02, .7, .2, 1.)),
              "texture-before.glb": panel(pixels=before), "texture-after.glb": panel(pixels=after),
              "vertex-color.glb": panel(vertex_color=[128, 255, 64, 128]),
              "alpha-blend.glb": panel((1., .1, .02, .25), alpha="BLEND"),
              "alpha-mask.glb": panel(pixels=[[255, 0, 0, 0], [0, 255, 0, 128], [0, 0, 255, 127], [255, 255, 255, 255]], alpha="MASK"),
              "double-sided.glb": panel((1., .6, .02, 1.), double_sided=True),
              "sampler-repeat.glb": panel(pixels=before, uv_scale=2.5),
              "sampler-mirror.glb": panel(pixels=before, uv_scale=2.5, wrap=33648),
              "sampler-clamp.glb": panel(pixels=before, uv_scale=2.5, wrap=33071),
              "morph-animation.glb": morph(), "skin-animated.glb": skin(),
              "skin-morph-animation.glb": skin_morph(), "trs-animation.glb": trs()}
    jpeg = panel(pixels=before)
    jpeg.doc["images"] = [{"bufferView": jpeg.view(JPEG_PANEL), "mimeType": "image/jpeg"}]
    assets["texture-jpeg.glb"] = jpeg
    malformed = copy.deepcopy(assets["texture-before.glb"])
    malformed.doc["images"][0]["mimeType"] = "image/jpeg"
    assets["malformed-texture.glb"] = malformed
    external = copy.deepcopy(assets["texture-before.glb"])
    external.doc["images"][0] = {"uri": "https://example.invalid/never-fetch.png"}
    assets["external-texture.glb"] = external
    for name, asset in assets.items():
        (destination / name).write_bytes(asset.bytes())
    (destination / "authored-expectations.json").write_text(json.dumps(authored_expectations(), indent=2) + "\n")
    manifest = {"generator": "scripts/create-glb-workflow-fixtures.py", "license": "Repository license",
                "files": {name: {"bytes": len(asset.bytes()), "sha256": hashlib.sha256(asset.bytes()).hexdigest()} for name, asset in assets.items()}}
    if args.repository:
        env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        def git(*argv):
            return subprocess.check_output(["git", "-C", str(destination), *argv], env=env, text=True).strip()
        git("init", "-b", "main")
        (destination / ".git/empty-hooks").mkdir()
        for key, value in [("user.name", "GLB Workflow Fixture"), ("user.email", "fixture@example.invalid"),
                           ("commit.gpgSign", "false"), ("core.hooksPath", ".git/empty-hooks")]:
            git("config", key, value)
        for prefix in ["material", "texture"]:
            (destination / f"{prefix}.glb").write_bytes(assets[f"{prefix}-before.glb"].bytes())
        (destination / "animated.glb").write_bytes(assets["skin-animated.glb"].bytes())
        git("add", ".")
        git("commit", "-m", "Public GLB workflow before")
        manifest["before"] = git("rev-parse", "HEAD")
        for prefix in ["material", "texture"]:
            (destination / f"{prefix}.glb").write_bytes(assets[f"{prefix}-after.glb"].bytes())
        (destination / "animated.glb").write_bytes(assets["morph-animation.glb"].bytes())
        (destination / "added.GLB").write_bytes(assets["trs-animation.glb"].bytes())
        git("add", ".")
        git("commit", "-m", "Change only material, only embedded texture, and animation clips")
        manifest["after"] = git("rev-parse", "HEAD")
        (destination / "material.glb").write_bytes(assets["material-before.glb"].bytes())
        manifest["working_status"] = git("status", "--porcelain=v1")
    manifest_path = destination / (".git/workflow-manifest.json" if args.repository else "manifest.json")
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
