"""Unit tests for scripts/ci/libdav1d_elf.py."""

from __future__ import annotations

import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts/ci"))
import libdav1d_elf  # noqa: E402


class Libdav1dElfTests(unittest.TestCase):
    def test_parsers_extract_tags_offset_and_relocation(self) -> None:
        dynamic = (
            " 0x000000000000000c (INIT)               0x1000\n"
            " 0x0000000000000019 (INIT_ARRAY)         0x2000\n"
            " 0x000000000000001b (INIT_ARRAYSZ)       16 (bytes)\n"
        )
        self.assertEqual(libdav1d_elf.parse_dynamic_tag(dynamic, "INIT"), "0x1000")
        self.assertEqual(libdav1d_elf.parse_dynamic_tag(dynamic, "INIT_ARRAY"), "0x2000")
        self.assertEqual(libdav1d_elf.parse_dynamic_tag(dynamic, "INIT_ARRAYSZ"), "16")
        self.assertIsNone(libdav1d_elf.parse_dynamic_tag(dynamic, "FINI"))

        sections = (
            "  [20] .init_array       INIT_ARRAY      0000000000002000 001000 000010 08  WA  0   0  8\n"
        )
        self.assertEqual(libdav1d_elf.parse_init_array_offset(sections), 0x1000)

        relocs = "0000000000002000  0000000000000008 R_X86_64_RELATIVE                 1234\n"
        self.assertEqual(libdav1d_elf.parse_relative_target(relocs, "0000000000002000"), "1234")
        self.assertIsNone(libdav1d_elf.parse_relative_target(relocs, "0000000000002008"))

    def test_missing_tool_fails_with_error_annotation(self) -> None:
        buf = io.StringIO()
        with mock.patch.object(libdav1d_elf.shutil, "which", return_value=None), redirect_stdout(buf):
            rc = libdav1d_elf.inspect_binary_and_libdav1d(Path("/nonexistent/gitturtle"))
        self.assertEqual(rc, 1)
        self.assertIn("::error::readelf is missing", buf.getvalue())

    def test_full_inspection_handles_relative_and_relr_fallback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            libdir = Path(tmp)
            lib = libdir / "libdav1d.so.7"
            # Offset 0x10 holds two 8-byte entries: first covered by RELA, second read in-place (RELR).
            raw = bytearray(64)
            raw[0x18:0x20] = (0x5678).to_bytes(8, "little")
            lib.write_bytes(bytes(raw))

            def fake_run(argv: list[str]) -> str:
                if argv[:3] == ["readelf", "-d", "-W"] and argv[3].endswith("gitturtle"):
                    return " 0x0000000000000001 (NEEDED) Shared library: [libdav1d.so.7]\n"
                if argv[:3] == ["readelf", "--dyn-syms", "-W"]:
                    return "  12: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND dav1d_open\n"
                if argv[:3] == ["pkg-config", "--variable=libdir", "dav1d"]:
                    return f"{libdir}\n"
                if argv[:3] == ["readelf", "-d", "-W"]:
                    return (
                        " 0x0000000000000019 (INIT_ARRAY)         0x2000\n"
                        " 0x000000000000001b (INIT_ARRAYSZ)       16 (bytes)\n"
                    )
                if argv[:3] == ["readelf", "-S", "-W"]:
                    return "  [20] .init_array       INIT_ARRAY      0000000000002000 000010 000010 08  WA  0   0  8\n"
                if argv[:3] == ["readelf", "-r", "-W"]:
                    return "0000000000002000  0000000000000008 R_AARCH64_RELATIVE                1234\n"
                if argv[0] == "objdump":
                    return "Disassembly of section .text:\n  nop\n"
                raise AssertionError(f"unexpected command: {argv}")

            buf = io.StringIO()
            with mock.patch.object(libdav1d_elf.shutil, "which", return_value="/usr/bin/fake"), \
                    mock.patch.object(libdav1d_elf, "run_tool", side_effect=fake_run), \
                    redirect_stdout(buf):
                rc = libdav1d_elf.inspect_binary_and_libdav1d(Path("/tmp/gitturtle"))
            self.assertEqual(rc, 0)
            out = buf.getvalue()
            self.assertIn("dav1d_open", out)
            self.assertIn("== .init_array[0]: 0x1234", out)
            self.assertIn("== .init_array[1]: 0x5678", out)


if __name__ == "__main__":
    unittest.main()
