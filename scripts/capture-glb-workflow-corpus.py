#!/usr/bin/env python3
"""Read-only no-follow GLB capture, private manifest, and later integrity check.

Keep destination outside source repositories and this checkout. Manifest source
paths are private; publish only sanitized counts, ordinal labels and hashes.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import stat
import struct


def regular_bytes(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError("not a regular file")
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read()
        after = os.fstat(fd)
        if (before.st_size, before.st_mtime_ns, before.st_ino) != (after.st_size, after.st_mtime_ns, after.st_ino):
            raise ValueError("source changed while capturing")
        return data
    finally:
        os.close(fd)


def files(root):
    result = []
    for directory, children, names in os.walk(root, followlinks=False):
        children[:] = sorted(child for child in children if not Path(directory, child).is_symlink())
        result.extend(Path(directory, name) for name in names if name.lower().endswith(".glb") and not Path(directory, name).is_symlink())
    return sorted(result)


def inspect(data):
    if len(data) < 28 or data[:4] != b"glTF":
        return {"container": "not GLB"}
    length = struct.unpack_from("<I", data, 12)[0]
    try:
        doc = json.loads(data[20:20 + length])
    except (ValueError, UnicodeError):
        return {"container": "invalid JSON"}
    return {"extensions_required": doc.get("extensionsRequired", []),
            "skins": len(doc.get("skins", [])), "joints": sum(len(skin.get("joints", [])) for skin in doc.get("skins", [])),
            "clips": len(doc.get("animations", [])),
            "channels": sum(len(clip.get("channels", [])) for clip in doc.get("animations", [])),
            "morph_primitives": sum(bool(primitive.get("targets")) for mesh in doc.get("meshes", []) for primitive in mesh.get("primitives", [])),
            "image_mime_types": [image.get("mimeType", "URI") for image in doc.get("images", [])]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, nargs="?")
    parser.add_argument("destination", type=Path, nargs="?")
    parser.add_argument("--verify", type=Path, help="read prior private inventory and verify all source/captured bytes")
    args = parser.parse_args()
    if args.verify:
        manifest = json.loads(args.verify.read_text())
        root = Path(manifest["source"])
        current = {str(path.relative_to(root)) for path in files(root)}
        expected = {entry["path"] for entry in manifest["entries"]}
        differences = [{"added": sorted(current - expected), "removed": sorted(expected - current)}] if current != expected else []
        for entry in manifest["entries"]:
            for kind, path in [("source", root / entry["path"]), ("capture", args.verify.parent / "captures" / f"{entry['id']}.glb")]:
                try:
                    digest = hashlib.sha256(regular_bytes(path)).hexdigest()
                    if digest != entry["sha256"]:
                        differences.append({"id": entry["id"], "kind": kind, "sha256": digest})
                except OSError as error:
                    differences.append({"id": entry["id"], "kind": kind, "error": str(error)})
        print(json.dumps({"checked_at": datetime.now(timezone.utc).isoformat(), "files": len(expected), "unchanged": not differences, "differences": differences}, indent=2))
        raise SystemExit(bool(differences))
    if not args.source or not args.destination:
        parser.error("Supply SOURCE_MODEL_DIRECTORY NEW_PRIVATE_DESTINATION or --verify INVENTORY")
    root, destination = args.source.resolve(), args.destination.resolve()
    checkout = Path(__file__).resolve().parents[1]
    if destination.exists() or root == destination or root in destination.parents or checkout == destination or checkout in destination.parents:
        parser.error("Choose a new private destination outside source and checkout")
    destination.mkdir(parents=True)
    (destination / "captures").mkdir()
    entries = []
    for index, path in enumerate(files(root), 1):
        data = regular_bytes(path)
        name = f"asset-{index:04}"
        (destination / "captures" / f"{name}.glb").write_bytes(data)
        entries.append({"id": name, "path": str(path.relative_to(root)), "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(), **inspect(data)})
    manifest = {"source": str(root), "captured_at": datetime.now(timezone.utc).isoformat(),
                "policy": "Regular no-follow file reads only; no Git, LFS, network, filters, resource resolution or source writes", "entries": entries}
    (destination / "inventory.json").write_text(json.dumps(manifest, indent=2) + "\n")
    summary = {"files": len(entries), "bytes": sum(entry["bytes"] for entry in entries),
               "skinned_assets": sum(bool(entry.get("skins")) for entry in entries),
               "morph_assets": sum(bool(entry.get("morph_primitives")) for entry in entries),
               "animated_assets": sum(bool(entry.get("clips")) for entry in entries),
               "required_extensions": dict(Counter(extension for entry in entries for extension in entry.get("extensions_required", []))),
               "image_mime_types": dict(Counter(mime for entry in entries for mime in entry.get("image_mime_types", [])))}
    (destination / "inventory-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
