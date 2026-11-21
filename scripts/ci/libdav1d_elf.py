#!/usr/bin/env python3
"""Inspect Linux ELF linkage and initializers for system libdav1d.

Requirement 15 of the AVIF and JPEG 2000 security review on Linux CI shards:
prints the built GitTurtle binary's dynamic section and dav1d_* symbols, then
prints system libdav1d's DT_INIT and .init_array entries with a short
disassembly at each initializer target.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


def run_tool(argv: list[str]) -> str:
    env = {**os.environ, "LC_ALL": "C"}
    result = subprocess.run(argv, env=env, capture_output=True, text=True, check=True)
    return result.stdout


def parse_dynamic_tag(dynamic_output: str, tag_name: str) -> str | None:
    wanted = f"({tag_name})"
    for line in dynamic_output.splitlines():
        fields = line.split()
        if len(fields) >= 3 and fields[1] == wanted:
            return fields[2]
    return None


def parse_init_array_offset(section_output: str) -> int | None:
    for line in section_output.splitlines():
        fields = line.split()
        for idx, token in enumerate(fields[:-3]):
            if token == ".init_array":
                try:
                    return int(fields[idx + 3], 16)
                except ValueError:
                    return None
    return None


def parse_relative_target(reloc_output: str, entry_hex: str) -> str | None:
    for line in reloc_output.splitlines():
        fields = line.split()
        if len(fields) >= 3 and fields[0] == entry_hex and fields[2].endswith("_RELATIVE"):
            return fields[-1]
    return None


def inspect_binary_and_libdav1d(app_path: Path) -> int:
    for tool in ("readelf", "objdump"):
        if shutil.which(tool) is None:
            print(f"::error::{tool} is missing")
            return 1

    print(f"== readelf -d -W {app_path}")
    print(run_tool(["readelf", "-d", "-W", str(app_path)]), end="")

    print("== dav1d_* in its .dynsym")
    dynsym = run_tool(["readelf", "--dyn-syms", "-W", str(app_path)])
    matches = [line for line in dynsym.splitlines() if "dav1d_" in line]
    if matches:
        print("\n".join(matches))
    else:
        print("none")

    if shutil.which("pkg-config") is None:
        print("::notice::pkg-config is missing, so libdav1d's initializers are not printed")
        return 0

    libdir = run_tool(["pkg-config", "--variable=libdir", "dav1d"]).strip()
    lib = (Path(libdir) / "libdav1d.so.7").resolve()
    dynamic = run_tool(["readelf", "-d", "-W", str(lib)])
    print(f"== readelf -d -W {lib}")
    print(dynamic, end="")

    init = parse_dynamic_tag(dynamic, "INIT")
    array_str = parse_dynamic_tag(dynamic, "INIT_ARRAY")
    size_str = parse_dynamic_tag(dynamic, "INIT_ARRAYSZ")
    offset = parse_init_array_offset(run_tool(["readelf", "-S", "-W", str(lib)]))
    relocations = run_tool(["readelf", "-r", "-W", str(lib)])

    targets: list[tuple[int, str]] = []
    if init is not None:
        targets.append((int(init, 0), "DT_INIT"))

    if array_str is not None and size_str is not None and offset is not None:
        array_addr = int(array_str, 0)
        array_size = int(size_str, 0)
        print(f"== .init_array at {array_str}, {size_str} bytes: each entry's relocation and target")
        raw_lib = lib.read_bytes()
        for slot_offset in range(0, array_size, 8):
            at = array_addr + slot_offset
            entry_hex = f"{at:016x}"
            reloc_lines = [line for line in relocations.splitlines() if entry_hex in line]
            if reloc_lines:
                print("\n".join(reloc_lines))
            else:
                print(f"{entry_hex}: no relocation")
            target_hex = parse_relative_target(relocations, entry_hex)
            if not target_hex:
                file_pos = offset + slot_offset
                word = int.from_bytes(raw_lib[file_pos : file_pos + 8], "little")
                target_hex = f"{word:x}"
            if re.fullmatch(r"[0-9a-fA-F]+", target_hex):
                targets.append((int(target_hex, 16), f".init_array[{slot_offset // 8}]"))

    for address, label in targets:
        print(f"== {label}: {address:#x}")
        disasm = run_tool(
            [
                "objdump",
                "-d",
                f"--start-address={address:#x}",
                f"--stop-address={address + 24:#x}",
                "--",
                str(lib),
            ]
        )
        lines = disasm.splitlines()
        start_idx = 0
        for idx, line in enumerate(lines):
            if line.startswith("Disassembly of section"):
                start_idx = idx + 1
                break
        if lines[start_idx:]:
            print("\n".join(lines[start_idx:]))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--app",
        type=Path,
        default=None,
        help="Path to built gitturtle ELF binary (default: ${CARGO_TARGET_DIR:-target}/debug/gitturtle)",
    )
    args = parser.parse_args(argv)
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR") or "target")
    app_path = args.app or (target_dir / "debug" / "gitturtle")
    return inspect_binary_and_libdav1d(app_path)


if __name__ == "__main__":
    raise SystemExit(main())
