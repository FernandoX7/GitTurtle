"""Binary identity: sha256 and `--build-info`, with the base/candidate pair refusals.

A base built into the candidate's CARGO_TARGET_DIR once made the candidate
build a no-op, so two binaries that share a digest or a source revision are
not a base/candidate pair. A `source_tree` other than `clean` is flagged: the
dirty marker alone does not identify which changes were built.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

FIELDS = ("application", "version", "source_revision", "source_tree", "target", "profile", "rustc",
          "build_unix_seconds")


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def parse_build_info(text: str) -> dict:
    """The JSON object `--build-info` prints; missing fields are an error, extra ones are kept."""
    try:
        info = json.loads(text)
    except ValueError as error:
        raise ValueError(f"--build-info did not print JSON: {error}") from None
    if not isinstance(info, dict):
        raise ValueError("--build-info did not print a JSON object")
    missing = [field for field in FIELDS if field not in info]
    if missing:
        raise ValueError(f"--build-info lacks {', '.join(missing)}")
    return info


def build_info(binary: Path, timeout: float = 20.0) -> dict:
    """Run `BINARY --build-info` with no display and a throwaway HOME and XDG_CONFIG_HOME.

    A build too old to know the flag would treat it as a repository path; it
    then has no display to open and no real preference store to write.
    """
    with tempfile.TemporaryDirectory(prefix="gitturtle-build-info-") as scratch:
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8", "HOME": scratch,
               "XDG_CONFIG_HOME": scratch, "XDG_DATA_HOME": scratch, "XDG_CACHE_HOME": scratch,
               "XDG_STATE_HOME": scratch}
        result = subprocess.run([str(binary), "--build-info"], env=env, capture_output=True, text=True,
                                timeout=timeout, stdin=subprocess.DEVNULL)
    if result.returncode != 0:
        raise ValueError(f"{binary} --build-info exited {result.returncode}: {result.stderr.strip()[:200]}")
    return parse_build_info(result.stdout)


def describe(binary: Path) -> dict:
    binary = Path(binary)
    if not binary.is_file():
        raise SystemExit(f"refusing: {binary} is not a file")
    return dict(path=str(binary), sha256=sha256_file(binary), build_info=build_info(binary))


def problems(entries: list[dict], pair: bool, allow_dirty: bool = False) -> list[str]:
    """Why these binaries cannot serve as the identified build(s) of a QA pass."""
    found = []
    for entry in entries:
        tree = entry["build_info"].get("source_tree")
        if tree != "clean" and not allow_dirty:
            found.append(f"{entry['path']}: source_tree is {tree!r}, not 'clean'")
        if entry["build_info"].get("source_revision") in (None, "", "unknown"):
            found.append(f"{entry['path']}: source_revision is unknown")
    if pair:
        if len(entries) != 2:
            found.append(f"a base/candidate pair needs two binaries, got {len(entries)}")
        else:
            base, candidate = entries
            if base["sha256"] == candidate["sha256"]:
                found.append(f"base and candidate have the same sha256 {base['sha256']}: "
                             "the candidate build was probably a no-op (shared CARGO_TARGET_DIR?)")
            if base["build_info"].get("source_revision") == candidate["build_info"].get("source_revision"):
                found.append("base and candidate were built from the same source_revision "
                             f"{base['build_info'].get('source_revision')}")
    return found
