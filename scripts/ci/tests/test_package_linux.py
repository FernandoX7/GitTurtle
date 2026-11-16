"""Linux packager refusal fixtures; no app, archive, or desktop validation.

Run the real shell entry point and shared identity checks against disposable
source trees. Native tools/notice collection are explicit test doubles; tar is
an error sentinel so these tests never manufacture a distributable archive.
The packager's ELF checks run the real readelf: a developer host without it
skips these tests, and a job that sets CI fails them instead.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[2]
# The packager always targets the machine it runs on.
TARGET, ELF_MACHINE = {"x86_64": ("x86_64-unknown-linux-gnu", 62),
                       "aarch64": ("aarch64-unknown-linux-gnu", 183)}.get(platform.machine(), (None, None))
READELF_MISSING = "readelf (GNU binutils) is not installed; the Linux packager tests need it"


def require_readelf() -> None:
    """Skip the packager tests on a developer host without readelf. Where the
    environment sets CI, as hosted jobs do, fail instead: the job that builds
    the package must never skip them silently."""
    if shutil.which("readelf"):
        return
    if "CI" in os.environ:
        raise AssertionError(f"{READELF_MISSING}, and CI is set, so they must run here")
    raise unittest.SkipTest(READELF_MISSING)


def shared_object(message: bytes) -> bytes:
    """An ELF shared object for the host with no PT_INTERP program header, as a
    shared library has, that writes `message` to standard output and exits 0
    when run: one read-execute PT_LOAD of the whole file, at its start, holding
    position-independent code for write(1, message, length) and exit(0)."""
    if ELF_MACHINE == 62:
        code = (b"\xb8\x01\x00\x00\x00"  # mov eax, 1 (write)
                b"\xbf\x01\x00\x00\x00"  # mov edi, 1
                b"\x48\x8d\x35\x10\x00\x00\x00"  # lea rsi, [rip + 16], the message
                + b"\xba" + struct.pack("<I", len(message))  # mov edx, length
                + b"\x0f\x05"  # syscall
                b"\xb8\x3c\x00\x00\x00"  # mov eax, 60 (exit)
                b"\x31\xff"  # xor edi, edi
                b"\x0f\x05")  # syscall
    else:
        code = struct.pack("<8I", 0xD2800020,  # mov x0, #1
                           0x100000E1,  # adr x1, #28, the message
                           0xD2800002 | len(message) << 5,  # mov x2, #length
                           0xD2800808,  # mov x8, #64 (write)
                           0xD4000001,  # svc #0
                           0xD2800000,  # mov x0, #0
                           0xD2800BA8,  # mov x8, #93 (exit)
                           0xD4000001)  # svc #0
    entry = 64 + 56  # after the ELF header and the one program header
    size = entry + len(code) + len(message)
    header = struct.pack("<4sBBBB8xHHIQQQIHHHHHH", b"\x7fELF", 2, 1, 1, 0,
                         3, ELF_MACHINE, 1, entry, 64, 0, 0, 64, 56, 1, 64, 0, 0)  # ET_DYN
    load = struct.pack("<IIQQQQQQ", 1, 5, 0, 0, 0, size, size, 0x10000)  # PT_LOAD, R+X
    return header + load + code + message


# The codegen unit's FILE symbol in the hosted x86-64 link of dav1d-sys.
DAV1D_SYS_UNIT = "dav1d_sys.66874b14c357f7fb-cgu.0"


def dynamic_executable(interpreter: Path, defines: tuple[str, ...] = (),
                       symbol_table: bool = True) -> bytes:
    """A dynamically linked ELF executable for the host, as the packager's
    readelf checks see one: PT_INTERP names `interpreter`, which the kernel
    runs in its place; PT_DYNAMIC lists NEEDED libdav1d.so.7; .dynsym imports
    dav1d_close and dav1d_get_picture. Its .symtab holds those imports and two
    local symbols that define neither code nor data: the FILE symbol of
    dav1d-sys's codegen unit and the SECTION symbol of a section named
    dav1d_tables. Each name in `defines` adds a global function defined in that
    section, as a static copy of dav1d has. Without `symbol_table` the file has
    no .symtab or .strtab, as `strip --strip-all` leaves it. One read-only
    PT_LOAD maps the whole file at address 0, so every address is its file
    offset."""
    imports = ("dav1d_close", "dav1d_get_picture")

    def strings(*names):
        table, offsets = bytearray(b"\0"), {}
        for name in names:
            offsets[name] = len(table)
            table += name.encode() + b"\0"
        return bytes(table), offsets

    def symbol(name, info, section):  # st_value and st_size are 0
        return struct.pack("<IBBHQQ", name, info, 0, section, 0, 0)

    tables = 5  # the dav1d_tables section's index
    local, func, section_symbol, file_symbol = 0x00, 0x12, 0x03, 0x04  # binding << 4 | type
    dynstr, dynamic_names = strings("libdav1d.so.7", *imports)
    strtab, names = strings(DAV1D_SYS_UNIT, *imports, *defines)
    dynsym = symbol(0, local, 0) + b"".join(symbol(dynamic_names[name], func, 0) for name in imports)
    symtab = (symbol(0, local, 0) + symbol(names[DAV1D_SYS_UNIT], file_symbol, 0xFFF1)  # SHN_ABS
              + symbol(0, section_symbol, tables)
              + b"".join(symbol(names[name], func, 0) for name in imports)
              + b"".join(symbol(names[name], func, tables) for name in defines))
    # Name, type, flags, link, info, alignment, entry size; SHF_ALLOC is 2.
    # Section 6 is .strtab, which .symtab links to, when both are present.
    layout = [(".interp", 1, 2, 0, 0, 1, 0), (".dynstr", 3, 2, 0, 0, 1, 0),
              (".dynsym", 11, 2, 2, 1, 8, 24), (".dynamic", 6, 2, 2, 0, 8, 16),
              ("dav1d_tables", 1, 2, 0, 0, 8, 0)]
    contents = [str(interpreter).encode() + b"\0", dynstr, dynsym, bytes(6 * 16), bytes(8)]
    if symbol_table:
        layout += [(".strtab", 3, 0, 0, 0, 1, 0), (".symtab", 2, 0, 6, 3, 8, 24)]
        contents += [strtab, symtab]
    layout.append((".shstrtab", 3, 0, 0, 0, 1, 0))
    shstrtab, section_names = strings(*(name for name, *_ in layout))
    contents.append(shstrtab)
    offsets, position = [], 64 + 3 * 56  # after the ELF header and three program headers
    for data in contents:
        position += -position % 8
        offsets.append(position)
        position += len(data)
    section_headers = position + -position % 8
    count = len(layout) + 1  # with the null section header first
    size = section_headers + count * 64
    contents[3] = struct.pack("<12Q", 1, dynamic_names["libdav1d.so.7"],  # DT_NEEDED
                              5, offsets[1], 6, offsets[2],  # DT_STRTAB, DT_SYMTAB
                              10, len(dynstr), 11, 24, 0, 0)  # DT_STRSZ, DT_SYMENT, DT_NULL
    header = struct.pack("<4sBBBB8xHHIQQQIHHHHHH", b"\x7fELF", 2, 1, 1, 0, 3, ELF_MACHINE, 1, 0,
                         64, section_headers, 0, 64, 56, 3, 64, count, count - 1)  # ET_DYN
    programs = (struct.pack("<IIQQQQQQ", 3, 4, offsets[0], offsets[0], offsets[0],  # PT_INTERP
                            len(contents[0]), len(contents[0]), 1)
                + struct.pack("<IIQQQQQQ", 1, 4, 0, 0, 0, size, size, 0x10000)  # PT_LOAD, R
                + struct.pack("<IIQQQQQQ", 2, 4, offsets[3], offsets[3], offsets[3],  # PT_DYNAMIC
                              len(contents[3]), len(contents[3]), 8))
    sections = bytes(64) + b"".join(
        struct.pack("<IIQQQQIIQQ", section_names[name], kind, flags, offset if flags else 0, offset,
                    len(data), link, info, alignment, entry)
        for (name, kind, flags, link, info, alignment, entry), offset, data
        in zip(layout, offsets, contents))
    image = bytearray(header + programs)
    for offset, data in zip(offsets, contents):
        image += bytes(offset - len(image)) + data
    return bytes(image + bytes(section_headers - len(image)) + sections)


@unittest.skipUnless(platform.system() == "Linux" and TARGET,
                     "Linux packager entry point requires Linux x86-64 or AArch64")
class LinuxPackageRefusalTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        require_readelf()

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-package-refusal-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source with spaces"
        self.scripts = self.source / "scripts"
        self.scripts.mkdir(parents=True)
        for name in ("package-linux.sh", "package-identity.py", "install-linux.py"):
            shutil.copyfile(SCRIPTS / name, self.scripts / name)
        for name in ("assets/app-icon.png", "docs/linux.md"):
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Synthetic fixture resource; never distributed.\n")
        (self.source / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n')
        (self.source / "Cargo.lock").write_text("version = 4\n")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.events = self.root / "events"
        # The import check is all that can run: refused inputs never reach image
        # rendering. This avoids requiring Pillow in tooling-only test jobs.
        modules = self.root / "modules" / "PIL"
        modules.mkdir(parents=True)
        (modules / "__init__.py").write_text("Image = None\n")
        # The packager refuses every SYSTEM_DEPS_ variable; one in the caller's
        # environment must not decide these fixtures.
        inherited = {name: value for name, value in os.environ.items() if not name.startswith("SYSTEM_DEPS_")}
        self.environment = dict(inherited, PATH=f"{self.tools}:{os.environ['PATH']}",
                                PYTHONPATH=str(modules.parent), FIXTURE_EVENTS=str(self.events))
        self.tool("cargo", 'printf "cargo %s\\n" "$*" >> "$FIXTURE_EVENTS"\nexit 73')
        self.tool("desktop-file-validate", "exit 0")
        self.tool("pkg-config", "exit 0")
        self.tool("tar", 'printf "unexpected archive\\n" >> "$FIXTURE_EVENTS"\nexit 89')
        collector = '''import hashlib, json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ["FIXTURE_EVENTS"], "a") as stream:
    stream.write("collector " + json.dumps(args) + "\\n")
out = pathlib.Path(args[-1]); out.mkdir()
(out / "LICENSE").write_text("Synthetic fixture notice\\n")
(out / "THIRD_PARTY_NOTICES.md").write_text("Synthetic fixture notice\\n")
(out / "REVIEW_REQUIRED.md").write_text("Fixture missing upstream notice\\n")
root = pathlib.Path(__file__).resolve().parents[1]
(out / "dependencies.json").write_text(json.dumps(dict(
 target=args[args.index("--target") + 1], cargo_lock_sha256=hashlib.sha256((root/"Cargo.lock").read_bytes()).hexdigest(),
 review_required=["fixture-0.1.0"], dependencies=[{"name":"fixture"}])))
if "--require-complete" in args:
    sys.stderr.write("Fixture unresolved notices block distribution\\n")
    raise SystemExit(12)
'''
        (self.scripts / "collect-third-party-licenses.py").write_text(collector)
        for args in (["init", "-q"], ["add", "."],
                     ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                      "-c", "commit.gpgSign=false", "commit", "-qm", "fixture"]):
            subprocess.run(["git", "-C", str(self.source), *args], check=True, capture_output=True)
        self.binary = self.root / "fixture executable"
        header = bytearray(32)
        header[:6] = b"\x7fELF\x02\x01"
        header[18:20] = ELF_MACHINE.to_bytes(2, "little")
        self.binary.write_bytes(header)
        self.binary.chmod(0o755)
        self.output = self.root / "output with spaces" / "gitturtle"

    def tool(self, name, source):
        path = self.tools / name
        path.write_text("#!/bin/sh\n" + source + "\n")
        path.chmod(0o755)

    def run_package(self, *arguments, no_build=True, binary=None):
        args = ["bash", str(self.scripts / "package-linux.sh")]
        if no_build:
            args += ["--no-build", "--binary", str(binary or self.binary)]
        result = subprocess.run([*args, *arguments, str(self.output)], env=self.environment,
                                text=True, capture_output=True, timeout=15)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def assert_no_output(self):
        for suffix in ("", ".tar.gz", ".tar.gz.sha256", ".tar.gz.manifest.json"):
            path = Path(str(self.output) + suffix)
            self.assertFalse(path.exists() or path.is_symlink(), path)
        if self.output.parent.exists():
            self.assertEqual(list(self.output.parent.iterdir()), [])
        if self.events.exists():
            self.assertNotIn("unexpected archive", self.events.read_text())

    def test_existing_outputs_and_dangling_symlinks_are_never_replaced(self):
        self.output.parent.mkdir()
        for suffix in ("", ".tar.gz", ".tar.gz.sha256", ".tar.gz.manifest.json"):
            for symbolic in (False, True):
                with self.subTest(suffix=suffix, symlink=symbolic):
                    collision = Path(str(self.output) + suffix)
                    if symbolic:
                        collision.symlink_to(self.root / "absent unrelated destination")
                    else:
                        collision.write_text("keep unrelated output\n")
                    result = self.run_package()
                    self.assertIn("Output already exists", result.stderr)
                    if symbolic:
                        self.assertTrue(collision.is_symlink())
                    else:
                        self.assertEqual(collision.read_text(), "keep unrelated output\n")
                    self.assertEqual(list(self.output.parent.iterdir()), [collision])
                    self.assertFalse(self.events.exists())
                    collision.unlink()

    def test_existing_directory_and_contents_are_preserved(self):
        self.output.mkdir(parents=True)
        marker = self.output / "unrelated.txt"
        marker.write_text("preserve")
        result = self.run_package()
        self.assertIn("Output already exists", result.stderr)
        self.assertEqual(marker.read_text(), "preserve")
        self.assertFalse(self.events.exists())

    def test_strict_notice_failure_removes_partial_stage(self):
        result = self.run_package("--distribution")
        self.assertIn("unresolved notices", result.stderr)
        events = self.events.read_text()
        self.assertIn("--require-complete", events)
        self.assertNotIn("cargo ", events)
        self.assert_no_output()

    def test_stale_expected_revision_version_and_hash_leave_no_output(self):
        for option, value, error in (("--expected-revision", "f" * 40, "Expected revision differs"),
                                     ("--expected-version", "99.0.0", "Expected version differs"),
                                     ("--expected-sha256", "f" * 64, "SHA-256 differs")):
            with self.subTest(option=option):
                result = self.run_package(option, value)
                self.assertIn(error, result.stderr)
                self.assert_no_output()

    def test_wrong_architecture_is_rejected_without_execution(self):
        self.binary.write_bytes(b"not an ELF executable\n")
        result = self.run_package()
        self.assertIn("format/architecture", result.stderr)
        self.assert_no_output()

    def test_symlink_or_missing_executable_is_refused(self):
        linked = self.root / "linked executable"
        linked.symlink_to(self.binary)
        result = self.run_package(binary=linked)
        self.assertIn("not a symlink", result.stderr)
        self.assert_no_output()
        result = self.run_package(binary=self.root / "missing executable")
        self.assertIn("Release executable missing", result.stderr)
        self.assert_no_output()

    def test_binary_option_requires_no_build_before_any_tool_runs(self):
        result = self.run_package("--binary", str(self.binary), no_build=False)
        self.assertIn("--binary requires --no-build", result.stderr)
        self.assertFalse(self.events.exists())
        self.assert_no_output()

    def test_system_deps_variable_is_refused_before_any_tool_runs(self):
        # Any SYSTEM_DEPS_ variable can make dav1d-sys build and statically link
        # its own libdav1d, so the build must not start.
        self.environment["SYSTEM_DEPS_X"] = "1"
        result = self.run_package(no_build=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("GitTurtle links the system libdav1d: SYSTEM_DEPS_X", result.stderr)
        self.assertFalse(self.events.exists())
        self.assert_no_output()

    def test_every_system_deps_variable_is_named_in_the_refusal(self):
        self.environment.update(SYSTEM_DEPS_DAV1D_BUILD_INTERNAL="always", SYSTEM_DEPS_LINK="static")
        result = self.run_package()
        self.assertEqual(result.returncode, 1)
        self.assertIn("Unset these variables before packaging", result.stderr)
        self.assertIn("SYSTEM_DEPS_DAV1D_BUILD_INTERNAL", result.stderr)
        self.assertIn("SYSTEM_DEPS_LINK", result.stderr)
        self.assertFalse(self.events.exists())
        self.assert_no_output()

    def test_build_failure_uses_explicit_supported_target_and_publishes_nothing(self):
        result = self.run_package(no_build=False)
        self.assertEqual(result.returncode, 73)
        command = self.events.read_text()
        self.assertIn(f"build --release --locked -p gitturtle --target {TARGET}", command)
        self.assertIn(f"--target-dir {self.source}/target", command)
        self.assert_no_output()

    def test_default_bundle_is_named_for_the_host_architecture(self):
        default = self.source / "dist" / f"gitturtle-linux-{platform.machine()}"
        default.mkdir(parents=True)
        result = subprocess.run(["bash", str(self.scripts / "package-linux.sh"), "--no-build",
                                 "--binary", str(self.binary)], env=self.environment,
                                text=True, capture_output=True, timeout=15)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(f"Output already exists: {default}.", result.stderr)
        self.assertFalse(self.events.exists())

    def test_wrong_architecture_for_the_host_is_rejected(self):
        header = bytearray(self.binary.read_bytes())
        header[18:20] = (62 if ELF_MACHINE == 183 else 183).to_bytes(2, "little")
        self.binary.write_bytes(header)
        result = self.run_package()
        self.assertIn(f"does not match {TARGET}", result.stderr)
        self.assert_no_output()

    def identity_printer(self) -> bytes:
        """A shared object that prints the fixture's build identity when run, so
        the notices and the identity check pass."""
        revision = subprocess.run(["git", "-C", str(self.source), "rev-parse", "HEAD"], check=True,
                                  capture_output=True, text=True).stdout.strip()
        identity = dict(application="GitTurtle", source_revision=revision, version="0.1.0",
                        target=TARGET, profile="release", source_tree="clean",
                        rustc="rustc 1.0.0 (fixture)", build_unix_seconds="0")
        return shared_object(json.dumps(identity).encode() + b"\n")

    def test_a_shared_library_is_refused_once_its_identity_passes(self):
        # A shared library in the executable's place: it runs only to print the
        # fixture's build identity, so the notices and the identity check pass
        # and the real readelf finds no PT_INTERP. It is the only ELF staged,
        # so the refusal does not depend on the order find lists the files.
        self.binary.write_bytes(self.identity_printer())
        result = self.run_package()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("The package ships an ELF that is not an executable: bin/gitturtle has no "
                      "PT_INTERP program header.", result.stderr)
        self.assertIn("collector ", self.events.read_text())
        self.assert_no_output()

    def linked_executable(self, defines=(), symbol_table=True):
        # The kernel runs the identity printer as the executable's interpreter,
        # so the real readelf checks its dynamic section and symbol tables.
        interpreter = self.root / "fixture interpreter"
        interpreter.write_bytes(self.identity_printer())
        interpreter.chmod(0o755)
        self.binary.write_bytes(dynamic_executable(interpreter, defines, symbol_table))

    def test_file_and_section_symbols_named_for_dav1d_are_not_a_static_copy(self):
        # The hosted x86-64 link keeps dav1d-sys's codegen unit as an ABS FILE
        # symbol in .symtab. The check passes and packaging stops only at icon
        # rendering, which the PIL test double cannot do.
        self.linked_executable()
        result = self.run_package()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Checked bin/gitturtle: NEEDED libdav1d.so.7, .symtab present, 2 dav1d_* symbols "
                      "imported and none defined, no RPATH or RUNPATH", result.stdout)
        self.assertNotIn("static copy", result.stderr)
        self.assert_no_output()

    def test_a_fully_stripped_executable_is_refused(self):
        # Without .symtab nothing lists a static copy's definitions, so "defines
        # none" would hold vacuously. Everything else the check reads passes:
        # NEEDED libdav1d.so.7, two imports in .dynsym and no RPATH.
        self.linked_executable(symbol_table=False)
        result = self.run_package()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Packaged executable bin/gitturtle has no .symtab section, so a static copy of "
                      "dav1d would go unseen. Package an executable that keeps its symbol table, as "
                      "the release profile's strip = \"debuginfo\" does.", result.stderr)
        self.assertNotIn("Checked bin/gitturtle", result.stdout)
        self.assert_no_output()

    def test_a_defined_dav1d_function_is_still_a_static_copy(self):
        self.linked_executable(defines=("dav1d_open",))
        result = self.run_package()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Packaged executable bin/gitturtle defines dav1d_* symbols, a static copy of "
                      "dav1d: .symtab defined dav1d_open \n", result.stderr)
        self.assertNotIn("Checked bin/gitturtle", result.stdout)
        self.assert_no_output()


class ReadelfRequirementTests(unittest.TestCase):
    """The packager tests' class hook, with the readelf lookup faked; it runs on
    every platform, whatever the host has installed."""

    def set_up_packager_tests(self, readelf, environment):
        """What the hook raises, or None. The exception is returned, not
        raised: a SkipTest escaping a test would report it skipped, not
        failed, whatever it asserts."""
        with mock.patch.dict(os.environ, environment, clear=True), \
                mock.patch("shutil.which", return_value=readelf) as which:
            try:
                LinuxPackageRefusalTests.setUpClass()
            except Exception as raised:
                return raised
            finally:
                which.assert_called_once_with("readelf")
        return None

    def without_ci(self):
        return {name: value for name, value in os.environ.items() if name != "CI"}

    def test_a_developer_host_without_readelf_skips_the_packager_tests(self):
        raised = self.set_up_packager_tests(None, self.without_ci())
        self.assertIs(type(raised), unittest.SkipTest, raised)
        self.assertEqual(str(raised),
                         "readelf (GNU binutils) is not installed; the Linux packager tests need it")

    def test_a_ci_job_without_readelf_fails_the_packager_tests(self):
        raised = self.set_up_packager_tests(None, dict(self.without_ci(), CI="true"))
        self.assertIs(type(raised), AssertionError, raised)
        self.assertEqual(str(raised),
                         "readelf (GNU binutils) is not installed; the Linux packager tests need it, "
                         "and CI is set, so they must run here")

    def test_with_readelf_the_packager_tests_run_with_or_without_ci(self):
        for environment in (self.without_ci(), dict(self.without_ci(), CI="true")):
            with self.subTest(ci="CI" in environment):
                self.assertIsNone(self.set_up_packager_tests("/usr/bin/readelf", environment))


if __name__ == "__main__":
    unittest.main()
