"""Behavioral tests for cache identity, bounded cleanup and miss recovery."""
from contextlib import contextmanager
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import textwrap
import time
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("rust_cache", ROOT / ".github/actions/setup-rust/cache.py")
cache = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cache)
SPEC_TOOLCHAIN = cache.toolchain_identity
# `dpkg-query` in a disposable ubuntu:24.04 arm64 container holding exactly the
# setup action's install list, which names no cmake, taken on 2026-11-19 with
# cache.DPKG_SNAPSHOT's format: package:architecture, version, status,
# Pre-Depends, Depends and Provides, tab-separated; 271 packages. A build of
# the workspace's tests, traced a day earlier in the same container with apt's
# cmake still installed, read files of 75 packages, 71 of them in the closure
# this key takes and none among the 35 that only apt's cmake brought into it.
UBUNTU_SNAPSHOT = (ROOT / "scripts/ci/tests/fixtures/ubuntu-24.04-arm64-dpkg.tsv").read_text()
# What build scripts and the cc and cmake crates run bare, and the linker, the
# assembler and the make that gcc and CMake run in their turn; cmake3 is optional.
TOOL_NAMES = ("cc", "c++", "ar", "ld", "as", "make", "pkg-config", "clang", "cmake")
# Tool answers for the tests that start no process.
UBUNTU_TOOLS ={**{name: {"run": f"/usr/bin/{name}", "path": f"/usr/bin/{name}-13", "matches": [f"/usr/bin/{name}"],
                          "version": f"{name} 13.0\n"} for name in TOOL_NAMES}, "cmake3": None}
# pkg-config's answers on an image where neither unsupplied name resolves.
UBUNTU_PROBES = {"aws-lc": None, "libcrypto-awslc": None}
# The top level of the two directories no dpkg package owns, with nothing in them.
UBUNTU_LOCAL = {"/usr/local/include": [], "/usr/local/lib": []}


# Crates through which a build script looks for something on the system.
PROBING_CRATES = {"pkg-config", "system-deps", "cmake", "bindgen"}
# A pkg-config name that no build package ships. cache.UBUNTU_UNSUPPLIED_PROBES
# must hold exactly these names: the key then holds what each resolves to.
UNSUPPLIED = "no build package ships it"
# Every locked package that depends on one of them, as `name version`, with
# what that version's build script (read in the Cargo registry) asks for on
# Linux: the pkg-config names, each with the package of
# cache.UBUNTU_BUILD_PACKAGES that ships its .pc file (as a pair with the
# dependency of that package the file is in, when it is not the build package's
# own) or UNSUPPLIED; then the build packages behind the commands and compilers
# it runs; then, for a command no build package ships, its name, which must be
# a required tool of cache.UBUNTU_TOOLS: keyed by the file PATH selects and its
# version, and failing the step when absent. A package that asks for nothing on
# Linux has the reason instead. The
# Ubuntu key holds those packages' closure and not the hosted image's other
# packages, over a thousand, so a prober may find nothing outside it. The .pc owners are
# those of the traced container's dpkg file lists. A row is one locked version:
# an update gets a new row only once its build script has been read again.
UBUNTU_PROBERS = {
    # builder/system_detect.rs: the four names, in this order, to adopt a system
    # AWS-LC, which it takes only from headers that define OPENSSL_IS_AWSLC;
    # libssl-dev's do not. It then compiles its own sources with cc, or with
    # cmake3 or cmake when asked, and loads libclang only with its bindgen feature.
    # It is the only locked crate that can run cmake, and builder/main.rs chooses
    # its builder by variables, features and target, never by whether cmake is
    # there. The action installs no CMake: the image's own is the keyed tool.
    "aws-lc-sys 0.45.0": ({"openssl": "libssl-dev", "aws-lc": UNSUPPLIED, "libcrypto": "libssl-dev",
                           "libcrypto-awslc": UNSUPPLIED}, ("pkg-config", "build-essential", "clang"), ("cmake",)),
    # system-deps for the dav1d of its manifest, 1.3.0 or later. Its other path,
    # a clone built with meson and ninja, needs a SYSTEM_DEPS_ variable, which
    # the install step refuses.
    "dav1d-sys 0.8.3": ({"dav1d": "libdav1d-dev"}, ("pkg-config",)),
    # freetype2 at 24.3.18 or later; without it, it compiles its bundled copy with c++.
    "freetype-sys 0.20.1": ({"freetype2": ("libfontconfig-dev", "libfreetype-dev")}, ("pkg-config", "build-essential")),
    "gpui-pre 0.3.4": "bindgen is its build dependency on macOS only, and its build script acts on Windows only",
    "gpui-pre-media 0.3.4": "its build script runs bindgen on macOS only; elsewhere its main is empty",
    "khronos-egl 6.0.0": "it asks for egl only with its static feature and without no-pkg-config; wgpu-hal turns on "
                         "dynamic here, and static together with no-pkg-config on emscripten",
    "system-deps 7.0.8": "a library for build scripts: it probes what its user's manifest names, here dav1d-sys's",
    # One name for each of its client, cursor, egl and server features, and
    # nothing at all with its dlopen feature, which wgpu-hal turns on.
    "wayland-sys 0.31.11": ({"wayland-client": "libwayland-dev", "wayland-cursor": "libwayland-dev",
                             "wayland-egl": "libwayland-dev", "wayland-server": "libwayland-dev"}, ("pkg-config",)),
    # One name for each feature. Its users in the lock turn on xlib alone (xcb,
    # for its xlib_xcb feature; its other user, the screen capture crate, names
    # none), which asks for x11. A feature turned on later asks for a name of
    # its own and leaves this version as it is: read the features with the row.
    "x11 2.21.0": ({"x11": ("libx11-xcb-dev", "libx11-dev")}, ("pkg-config",)),
    # fontconfig, unless its dlopen feature or RUST_FONTCONFIG_DLOPEN is on.
    "yeslogic-fontconfig-sys 6.0.1": ({"fontconfig": "libfontconfig-dev"}, ("pkg-config",)),
}


# The locked versions whose sources the flags identity's read set was taken from.
FLAG_CRATES = {"cc": "1.4.5", "cmake": "0.1.58", "pkg-config": "0.3.34"}
# The names each of them looks up as <VAR>_<target>, <VAR>_<target with
# underscores>, HOST_<VAR> or TARGET_<VAR>, and <VAR>, with the line of src/lib.rs
# that asks for each.
FLAG_TARGETED = {
    # Build::target_envs builds the four forms (4296 to 4307).
    "cc": {"AR": 3707, "ARFLAGS": 3690, "CC": 3268, "CFLAGS": 2137, "CXX": 3266, "CXXFLAGS": 2137, "CXXSTDLIB": 3630,
           "NVCC": 3409, "RANLIB": 3748, "RANLIBFLAGS": 3737},
    # getenv_target_os (947 to 960).
    "cmake": {"CMAKE": 901, "CMAKE_GENERATOR": 513, "CMAKE_PREFIX_PATH": 572, "CMAKE_TOOLCHAIN_FILE": 451,
              "EMCMAKE": 913, "EMMAKE": 926},
    # targeted_env_var (659 to 680).
    "pkg-config": {"PKG_CONFIG": 694, "PKG_CONFIG_ALLOW_CROSS": 640, "PKG_CONFIG_LIBDIR": 738, "PKG_CONFIG_PATH": 735,
                   "PKG_CONFIG_SYSROOT_DIR": 741},
}
# The names read in one form. cc: src/lib.rs unless another file is named;
# CC_ENABLE_DEBUG_OUTPUT is command_helpers.rs:48, RUSTC target/llvm.rs:279 and
# target/parser.rs:581, the CARGO_CFG_TARGET_ names target/parser.rs:60 to 67
# and lib.rs:2225. pkg-config: src/lib.rs.
FLAG_SINGLE = {
    "cc": {"CRATE_CC_NO_DEFAULTS": 2110, "CC_KNOWN_WRAPPER_CUSTOM": 3595, "CROSS_COMPILE": 3929, "CC_SHELL_ESCAPED_FLAGS": 4202,
           "CC_FORCE_DISABLE": 4353, "SDKROOT": 4369, "MACOSX_DEPLOYMENT_TARGET": 4526, "IPHONEOS_DEPLOYMENT_TARGET": 4538,
           "WATCHOS_DEPLOYMENT_TARGET": 4543, "TVOS_DEPLOYMENT_TARGET": 4547, "XROS_DEPLOYMENT_TARGET": 4551,
           "WASM_MUSL_SYSROOT": 4568, "WASI_SYSROOT": 4579, "WASI_SDK_PATH": 4670, "PAUTHTEST_SYSROOT": 4683,
           "PAUTHTEST_RESOURCE_DIR": 4698, "CC_ENABLE_DEBUG_OUTPUT": 48, "RUSTC": 279, "CARGO_ENCODED_RUSTFLAGS": 2777,
           "CARGO_TRIM_PATHS_SCOPE": 2797, "CARGO_TRIM_PATHS_REMAP": 2800, "RUSTC_WRAPPER": 3540, "RUSTC_LINKER": 3935,
           "CARGO_CFG_TARGET_FEATURE": 2225, "CARGO_CFG_TARGET_ARCH": 60, "CARGO_CFG_TARGET_VENDOR": 61,
           "CARGO_CFG_TARGET_OS": 62, "CARGO_CFG_TARGET_ENV": 63, "CARGO_CFG_TARGET_ABI": 67},
    "pkg-config": {"PKG_CONFIG_PATH_FOR_TARGET": 380, "PKG_CONFIG_ALL_STATIC": 784, "PKG_CONFIG_ALL_DYNAMIC": 786, "SYSROOT": 898},
}
# The forms built around a caller's name: cmake's DEP_<dependency>_ROOT (567) and
# pkg-config's <LIBRARY>_NO_PKG_CONFIG (597), <LIBRARY>_STATIC (780) and <LIBRARY>_DYNAMIC (782).
FLAG_CALLER_FORMS = ("DEP_AWS_LC_ROOT", "OPENSSL_NO_PKG_CONFIG", "FREETYPE2_STATIC", "FREETYPE2_DYNAMIC")
# What the compilers and the linker read themselves, and rustc's and Cargo's own settings.
FLAG_OTHERS = ("CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH", "CPPFLAGS", "LDFLAGS", "RUSTC_BOOTSTRAP",
               "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_INCREMENTAL")
# Read by the three crates and out of the identity all the same: Cargo sets
# each for every build script, so the job's value never reaches one; and PATH,
# whose selected tools the native identity keys.
FLAG_CARGO_SETS = ("TARGET", "HOST", "OUT_DIR", "OPT_LEVEL", "DEBUG", "PROFILE", "NUM_JOBS", "CARGO_MANIFEST_DIR",
                   "CARGO_MAKEFLAGS", "CARGO_PKG_NAME", "PATH")
# Real build-script outputs of a release build of this workspace on aarch64
# Linux (target/release/build/<crate>-<hash>/output, 2026-11-15), the home
# prefix replaced: ring and freetype-sys and dav1d-sys whole, and of
# aws-lc-sys, which prints 7,879 lines, its 116 distinct
# rerun-if-env-changed lines in their first order.
BUILD_OUTPUTS = ROOT / "scripts/ci/tests/fixtures/build-script-outputs"
# What those scripts declare themselves, or Cargo sets for every build script:
# out of the identity by decision. Everything else they declare is read by cc,
# cmake or pkg-config for them.
OWN_DECLARATION = re.compile(
    r"AWS_LC_SYS_.+|SYSTEM_DEPS_.+|OPENSSL_(?:DIR|INCLUDE_DIR|LIB_DIR)(?:_aarch64_unknown_linux_gnu)?"
    r"|HOST_(?:CC|CFLAGS|CXX)_aarch64_unknown_linux_gnu|RING_PREGENERATE_ASM|RUST_FONTCONFIG_DLOPEN"
    r"|CARGO_FEATURE_SSL|CARGO_PKG_(?:NAME|VERSION_(?:MAJOR|MINOR|PATCH|PRE))|CARGO_MANIFEST_(?:DIR|LINKS)"
    r"|CARGO_CFG_TARGET_ENDIAN|OUT_DIR")


def declared(output):
    """The names a build-script output declares with rerun-if-env-changed, each once, in first order."""
    return list(dict.fromkeys(re.findall(r"^cargo:rerun-if-env-changed=(.+)$", (BUILD_OUTPUTS / output).read_text(), re.MULTILINE)))


def locked_versions(lock, names):
    """Every locked version of each named package, as {name: [versions]}."""
    return {name: sorted(package["version"] for package in tomllib.loads(lock)["package"] if package["name"] == name)
            for name in names}


PROBER_ROW = re.compile(r"[A-Za-z0-9_-]+ \d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?")


def lock_probers(lock):
    """The locked packages that depend on a probing crate, as `name version`."""
    return {f"{package['name']} {package['version']}" for package in tomllib.loads(lock)["package"]
            if PROBING_CRATES & {dependency.split(" ")[0] for dependency in package.get("dependencies", ())}}


def setup_jobs(workflow):
    """The jobs of a workflow that use the setup action, as {name: text}."""
    jobs = re.split(r"\n(?=  [A-Za-z0-9_-]+:\n)", workflow.split("\njobs:\n", 1)[1])
    return {job.strip().split(":", 1)[0]: job for job in jobs if "uses: ./.github/actions/setup-rust" in job}


def later_installs(workflow):
    """(job, line) for every apt or dpkg installation written in a job that uses the setup action."""
    return [(name, line.strip()) for name, job in setup_jobs(workflow).items() for line in job.splitlines()
            if re.search(r"(?<![\w-])(?:apt(?:-get)?|dpkg\s+(?:-i|--install))(?![\w-])", line)]


def snapshot_rows(text=UBUNTU_SNAPSHOT):
    """A snapshot as {package:architecture: [version, status, Pre-Depends, Depends, Provides]}."""
    return {fields[0]: fields[1:] for fields in (line.split("\t") for line in text.splitlines())}


def snapshot_text(rows):
    return "".join("\t".join((identity, *fields)) + "\n" for identity, fields in rows.items())


# apt's cmake as the container's snapshot had it on 2026-11-18, while the action
# still installed it: version, status, Pre-Depends, Depends and Provides.
APT_CMAKE = ("3.28.3-1build7", "ii ", "", "libarchive13t64 (>= 3.3.3), libc6 (>= 2.38), libcurl4t64 (>= 7.16.2), "
             "libexpat1 (>= 2.0.1), libgcc-s1 (>= 3.0), libjsoncpp25 (>= 1.9.5), librhash0 (>= 1.2.6), "
             "libstdc++6 (>= 13.1), libuv1t64 (>= 1.38.0), zlib1g (>= 1:1.1.4), cmake-data (= 3.28.3-1build7), procps", "")


class UbuntuRunner:
    """A PATH of stand-ins for everything the Ubuntu key runs.

    `dpkg-query` answers with a snapshot file and records its arguments; each
    tool is a link in a PATH directory to its own real file, as the image's
    alternatives are, and answers `--version` from a file of its own. As
    pkg-config a tool also answers `--modversion`: it records who was asked for
    what and finds no module, until `probe` gives it something to do for a name.
    `local` stands for /usr/local/include and /usr/local/lib, both empty.
    """

    def __init__(self, base, snapshot=UBUNTU_SNAPSHOT):
        self.bin, self.real, self.state, self.pc = (base / name for name in ("bin", "real", "state", "pkgconfig"))
        self.local = (base / "usr-local/include", base / "usr-local/lib")
        for directory in (self.bin, self.real, self.state, self.pc, *self.local):
            directory.mkdir(parents=True)
        self.snapshot = self.state / "snapshot"
        self.snapshot.write_text(snapshot)
        self.queries = self.state / "queries"
        self.probes = self.state / "probes"
        self.pc_path = self.state / "pc_path"
        self.pc_path.write_text(f"{self.pc}:/usr/lib/pkgconfig:/usr/share/pkgconfig\n")
        self.script(self.bin / "dpkg-query", f"printf '%s\\n' \"$@\" >> {shlex.quote(str(self.queries))}\n"
                                           f"exec /bin/cat {shlex.quote(str(self.snapshot))}")
        for name in TOOL_NAMES:
            self.tool(name)

    def script(self, path, body):
        path.write_text(f"#!/bin/sh\n{body}\n")
        path.chmod(0o755)

    def tool(self, name, *, directory=None, variant="13", version=None):
        """Put `name` on PATH as a link to the real file `name-variant`; returns that file."""
        answer = shlex.quote(version or f"{name} (Ubuntu) {variant}.0\nconfigured for the fixture\n")
        real = self.real / f"{name}-{variant}"
        module = shlex.quote(str(self.state / "modversion-")) + '"$2"'
        self.script(real, f'case "$*" in\n  --version) printf \'%s\' {answer}; exit 0 ;;\n'
                          f'  "--variable pc_path pkg-config") exec /bin/cat {shlex.quote(str(self.pc_path))} ;;\n'
                          f'  "--modversion "*)\n    printf \'%s\\n\' "$0 $*" >> {shlex.quote(str(self.probes))}\n'
                          f'    [ "$#" -eq 2 ] && [ -f {module} ] || {{ echo "Package $2 was not found" >&2; exit 1; }}\n'
                          f'    . {module}\n    exit 0 ;;\nesac\nexit 64')
        link = (directory or self.bin) / name
        link.unlink(missing_ok=True)
        link.symlink_to(real)
        return real

    def probe(self, name, action):
        """What every stand-in pkg-config does when asked `--modversion name`, as shell; None finds no module again."""
        answer = self.state / f"modversion-{name}"
        answer.unlink(missing_ok=True)
        if action is not None:
            answer.write_text(action + "\n")

    @contextmanager
    def environment(self, *earlier, later=(), variables=None):
        path = os.pathsep.join(str(directory) for directory in (*earlier, self.bin, *later))
        with patch.dict(os.environ, {"PATH": path}), \
                patch.object(cache, "UBUNTU_LOCAL_DIRECTORIES", tuple(str(directory) for directory in self.local)):
            # No PKG_CONFIG variable of the machine the tests run on, only the test's own.
            for name in [name for name in os.environ if "PKG_CONFIG" in name]:
                del os.environ[name]
            os.environ.update(variables or {})
            yield

    def native(self, *earlier, later=(), variables=None):
        """The native digest and the lines the step prints, with `earlier` directories first on PATH and `later` last."""
        with self.environment(*earlier, later=later, variables=variables), patch("builtins.print") as shown:
            native = cache.native_identity("Linux")
        return native, shown.call_args.args[0].split("\n")


class FakeClock:
    """Monotonic time that advances only when the fixture's fake work runs."""

    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


class CacheFixture(unittest.TestCase):
    def setUp(self):
        (ROOT / ".local").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="cache-test-", dir=ROOT / ".local")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "repo"
        self.root.mkdir()
        self.runner = self.base / "runner"
        self.runner.mkdir()
        self.cargo = self.runner / "gitturtle-cargo"
        self.cargo.mkdir()
        self.paths = (self.root / "target", self.cargo / "registry", self.cargo / "git")
        self.env = {"GITHUB_WORKSPACE": str(self.root), "RUNNER_TEMP": str(self.runner),
                    "CARGO_HOME": str(self.cargo), "CARGO_TARGET_DIR": str(self.paths[0]),
                    "GITHUB_ENV": str(self.runner / "env"), "GITHUB_OUTPUT": str(self.runner / "output")}
        self.patch = patch.dict(os.environ, self.env)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        # prepare keys on `rustc -vV`; tests that vary it patch it themselves.
        toolchain = patch.object(cache, "toolchain_identity", return_value={"rustc": "rustc", "target": "x86_64-unknown-linux-gnu"})
        toolchain.start()
        self.addCleanup(toolchain.stop)

    def file(self, path, content=b"x"):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def init_git(self):
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates/app/src/main.rs",
                     "crates/app/build.rs", "crates/app/Cargo.toml", "vendor/tool/src/lib.rs",
                     "vendor/tool/native/source.c", "vendor/tool/build.rs", ".github/actions/setup-rust/action.yml"):
            self.file(self.root / name, name.encode())
        subprocess.run(["git", "-C", str(self.root), "add", "--all"], check=True)

    def test_source_only_edits_reuse_but_real_build_inputs_invalidate(self):
        self.init_git()
        original = cache.source_identity(self.root)
        self.file(self.root / "crates/app/src/main.rs", b"changed product source")
        self.assertEqual(original, cache.source_identity(self.root))
        for path in ("Cargo.lock", "Cargo.toml", "crates/app/Cargo.toml", "rust-toolchain.toml",
                     "crates/app/build.rs", "vendor/tool/src/lib.rs", "vendor/tool/native/source.c",
                     "vendor/tool/build.rs", ".github/actions/setup-rust/action.yml"):
            with self.subTest(path=path):
                before = cache.source_identity(self.root)
                self.file(self.root / path, b"changed build input")
                self.assertNotEqual(before, cache.source_identity(self.root))

    def test_deleted_build_input_refuses_cache_identity(self):
        self.init_git()
        (self.root / "vendor/tool/build.rs").unlink()
        with self.assertRaises(FileNotFoundError):
            cache.source_identity(self.root)

    def prepared(self, vendor_reuse="true"):
        settings = {"CACHE_PROFILE": "debug", "RUNNER_OS": "Linux"}
        if vendor_reuse is not None:
            settings["CACHE_VENDOR_REUSE"] = vendor_reuse
        with patch.dict(os.environ, settings), patch.object(cache, "native_identity", return_value="native"), patch("builtins.print"):
            if vendor_reuse is None:
                os.environ.pop("CACHE_VENDOR_REUSE", None)
            cache.prepare()
        lines = Path(self.env["GITHUB_ENV"]).read_text().splitlines()
        Path(self.env["GITHUB_ENV"]).write_text("")
        return dict(line.split("=", 1) for line in lines)

    def test_rewound_vendor_set_equals_the_hashed_vendor_inputs(self):
        self.init_git()
        inputs = cache.keyed_inputs(self.root)
        hashed = {self.root / path for raw, path in inputs if raw.startswith(b"vendor/")}
        member = self.root / "crates/app/src/main.rs"
        member_time = member.stat().st_mtime_ns
        count = cache.normalize_vendor_mtimes(self.root, inputs)
        rewound = {path for path in self.root.rglob("*") if ".git" not in path.parts
                   and path.stat().st_mtime_ns == cache.VENDOR_MTIME_NS}
        self.assertEqual({path for path in rewound if path.is_file()}, hashed)
        # Directories up to vendor/ are rewound for directory rerun-if-changed
        # scans; nothing above vendor/ and nothing under crates/ is touched.
        self.assertEqual({path for path in rewound if path.is_dir()},
                         {self.root / "vendor", self.root / "vendor/tool", self.root / "vendor/tool/src",
                          self.root / "vendor/tool/native"})
        self.assertEqual(count, len(rewound))
        self.assertEqual(member.stat().st_mtime_ns, member_time)
        self.assertNotEqual(self.root.stat().st_mtime_ns, cache.VENDOR_MTIME_NS)

    def test_vendor_symlink_refuses_without_rewinding_anything(self):
        self.init_git()
        (self.root / "vendor/tool/alias.rs").symlink_to("src/lib.rs")
        subprocess.run(["git", "-C", str(self.root), "add", "vendor/tool/alias.rs"], check=True)
        before = {path: path.lstat().st_mtime_ns for path in (self.root / "vendor").rglob("*")}
        with self.assertRaises(ValueError):
            cache.normalize_vendor_mtimes(self.root, cache.keyed_inputs(self.root))
        self.assertEqual(before, {path: path.lstat().st_mtime_ns for path in (self.root / "vendor").rglob("*")})
        # prepare still produces a key; vendored packages simply rebuild.
        values = self.prepared()
        self.assertEqual(values["CI_RUST_CACHE_VENDOR_MTIMES"], "refused")
        self.assertTrue(values["CI_RUST_CACHE_KEY"].startswith("debug-"))
        self.assertNotEqual((self.root / "vendor/tool/src/lib.rs").stat().st_mtime_ns, cache.VENDOR_MTIME_NS)

    def test_prepare_keys_on_content_and_rewinds_vendor_before_restore(self):
        self.init_git()
        first = self.prepared()
        self.assertEqual((self.root / "vendor/tool/src/lib.rs").stat().st_mtime_ns, cache.VENDOR_MTIME_NS)
        self.assertEqual(int(first["CI_RUST_CACHE_VENDOR_MTIMES"]), 7)
        # A later checkout's fresh times alone keep the key; content never does.
        os.utime(self.root / "vendor/tool/src/lib.rs")
        self.assertEqual(self.prepared()["CI_RUST_CACHE_KEY"], first["CI_RUST_CACHE_KEY"])
        for path in ("vendor/tool/src/lib.rs", "Cargo.lock", "rust-toolchain.toml"):
            with self.subTest(path=path):
                before = self.prepared()["CI_RUST_CACHE_KEY"]
                self.file(self.root / path, b"changed " + path.encode())
                self.assertNotEqual(before, self.prepared()["CI_RUST_CACHE_KEY"])

    def test_both_upstream_steps_root_at_crates_and_keep_members_out(self):
        steps = (ROOT / ".github/actions/setup-rust/action.yml").read_text().split("uses: Swatinem/rust-cache@")[1:]
        self.assertEqual(len(steps), 2)
        for step in steps:
            with self.subTest(step=step.splitlines()[0]):
                options = dict(line.strip().split(": ", 1) for line in step.splitlines()
                               if line.startswith("        ") and ": " in line and not line.strip().startswith("#"))
                self.assertEqual(options["workspaces"], "crates -> ../target")
                self.assertEqual(options["cache-workspace-crates"], "false")
                self.assertEqual(options["prefix-key"], "gitturtle-rust-v2")

    def test_vendor_reuse_is_opt_in_and_leaves_checkout_times_by_default(self):
        self.init_git()
        source = self.root / "vendor/tool/src/lib.rs"
        checkout = source.stat().st_mtime_ns
        default = self.prepared(vendor_reuse=None)
        self.assertEqual(default["CI_RUST_CACHE_VENDOR_MTIMES"], "disabled")
        self.assertEqual(self.prepared(vendor_reuse="false")["CI_RUST_CACHE_VENDOR_MTIMES"], "disabled")
        self.assertEqual(source.stat().st_mtime_ns, checkout)
        # The setting changes only source times, never the restored payload.
        opted = self.prepared(vendor_reuse="true")
        self.assertEqual(opted["CI_RUST_CACHE_KEY"], default["CI_RUST_CACHE_KEY"])
        self.assertEqual(source.stat().st_mtime_ns, cache.VENDOR_MTIME_NS)
        with self.assertRaises(ValueError):
            self.prepared(vendor_reuse="yes")

    def test_only_quality_opts_into_vendor_reuse_and_release_builds_from_source(self):
        def setup_steps(workflow):
            text = (ROOT / ".github/workflows" / workflow).read_text()
            steps = [part.split("\n      - ", 1)[0] for part in text.split("uses: ./.github/actions/setup-rust")[1:]]
            return ([step for step in steps if "phase: finish" not in step],
                    [step for step in steps if "phase: finish" in step])

        quality_setup, quality_finish = setup_steps("quality.yml")
        # The Ubuntu and macOS tests, their Clippy, their release builds and Arch.
        self.assertEqual(len(quality_setup), 4)
        self.assertTrue(all("vendor-reuse: true" in step for step in quality_setup))
        self.assertFalse(any("vendor-reuse" in step for step in quality_finish))
        release_setup, _ = setup_steps("release.yml")
        self.assertTrue(release_setup)
        for workflow in sorted((ROOT / ".github/workflows").glob("*.yml")):
            if workflow.name != "quality.yml":
                with self.subTest(workflow=workflow.name):
                    self.assertNotIn("vendor-reuse", workflow.read_text())

    def test_action_rewinds_only_in_prepare_before_the_restore_step(self):
        text = (ROOT / ".github/actions/setup-rust/action.yml").read_text()
        inputs, steps = text.split("\nruns:\n", 1)
        self.assertIn('  vendor-reuse:\n', inputs)
        self.assertIn('default: "false"', inputs.split("  vendor-reuse:\n", 1)[1].split("\n  ", 2)[1])
        steps = steps.split("\n    - name: ")[1:]
        prepare = next(index for index, step in enumerate(steps) if "cache.py prepare" in step)
        restore = next(index for index, step in enumerate(steps) if "id: restore" in step)
        validate = next(index for index, step in enumerate(steps) if "CACHE_VENDOR_REUSE" in step)
        self.assertLess(validate, prepare)
        self.assertLess(prepare, restore)
        for index in (prepare, restore):
            self.assertIn("if: inputs.phase == 'setup'", steps[index])
        self.assertIn("CACHE_VENDOR_REUSE: ${{ inputs.vendor-reuse }}", steps[prepare])
        self.assertEqual(sum("CACHE_VENDOR_REUSE" in step for step in steps), 2)

    def arch_key(self, versions, rustc, profile="debug", environ=None):
        """prepare's key in the Arch container, with pacman and rustc answered by the fixture."""
        real = cache.run
        commands = []

        def answer(command, **kwargs):
            commands.append(command)
            if command[:2] == ["pacman", "-Q"]:
                return "".join(f"{name} {versions[name]}\n" for name in command[2:]).encode()
            if command == ["rustc", "-vV"]:
                return rustc.encode()
            return real(command, **kwargs)

        settings = {"CACHE_PROFILE": profile, "RUNNER_OS": "Linux", "CACHE_CONTAINER": "archlinux",
                    "CACHE_VENDOR_REUSE": "true", **(environ or {})}
        with patch.dict(os.environ, settings), patch.object(cache, "run", side_effect=answer), \
                patch.object(cache, "toolchain_identity", side_effect=SPEC_TOOLCHAIN), patch("builtins.print"):
            cache.prepare()
        lines = Path(self.env["GITHUB_ENV"]).read_text().splitlines()
        Path(self.env["GITHUB_ENV"]).write_text("")
        self.assertIn(["pacman", "-Q", *sorted(cache.ARCH_NATIVE_PACKAGES)], commands)
        return dict(line.split("=", 1) for line in lines)["CI_RUST_CACHE_KEY"]

    def test_arch_key_changes_with_every_keyed_input_and_the_old_entry_is_not_reused(self):
        self.init_git()
        versions = {name: "1.0-1" for name in cache.ARCH_NATIVE_PACKAGES}
        rustc = "rustc 1.98.0 (abc 2026-09-01)\nbinary: rustc\nhost: x86_64-unknown-linux-gnu\nrelease: 1.98.0\n"
        baseline = self.arch_key(versions, rustc)
        self.assertRegex(baseline, r"^debug-archlinux-[0-9a-f]{64}$")
        # Checkout times, product sources, unkeyed packages and unrelated
        # environment keep the entry.
        self.file(self.root / "crates/app/src/main.rs", b"changed product source")
        self.assertEqual(baseline, self.arch_key(versions, rustc, environ={"CARGO_TERM_COLOR": "always", "GITHUB_SHA": "f" * 40,
                                                                            "CARGO_TARGET_DIR": str(self.base / "elsewhere")}))
        changes = {
            "target": lambda: (versions, rustc.replace("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"), "debug", None),
            "toolchain": lambda: (versions, rustc.replace("1.98.0", "1.99.0"), "debug", None),
            "profile": lambda: (versions, rustc, "release", None),
            # Every named flag and one variable under every keyed prefix.
            **{f"flag {name}": (lambda name=name: (versions, rustc, "debug", {name: "-C changed"}))
               for name in sorted(cache.FLAG_NAMES) + [prefix + "X86_64_UNKNOWN_LINUX_GNU" if prefix.endswith("_") else prefix
                                                       for prefix in cache.FLAG_PREFIXES]},
            **{f"package {name}": (lambda name=name: ({**versions, name: "1.0-2"}, rustc, "debug", None))
               for name in cache.ARCH_NATIVE_PACKAGES},
        }
        keys = {baseline}
        for label, change in changes.items():
            with self.subTest(change=label):
                changed_versions, changed_rustc, profile, environ = change()
                key = self.arch_key(changed_versions, changed_rustc, profile, environ)
                self.assertNotIn(key, keys)
                keys.add(key)
        for path in ("Cargo.lock", "Cargo.toml", "crates/app/Cargo.toml", "vendor/tool/native/source.c"):
            with self.subTest(change=path):
                self.file(self.root / path, b"changed " + path.encode())
                key = self.arch_key(versions, rustc)
                self.assertNotIn(key, keys)
                keys.add(key)
        # A changed key finds at best an older entry by prefix, never the exact
        # one: upstream reports that as no hit, and the helper deletes the
        # restored payload before Cargo runs, so every crate rebuilds.
        args = self.restore(hit="false")
        self.assertFalse(any(path.exists() for path in self.paths))
        self.assertFalse(args.kwargs["cache"]["hit"])

    def test_toolchain_identity_names_the_full_version_and_one_target(self):
        verbose = b"rustc 1.98.0\nhost: x86_64-unknown-linux-gnu\nLLVM version: 21.1.0\n"
        with patch.object(cache, "run", return_value=verbose):
            identity = SPEC_TOOLCHAIN()
        self.assertEqual(identity["target"], "x86_64-unknown-linux-gnu")
        with patch.object(cache, "run", return_value=verbose.replace(b"21.1.0", b"21.1.1")):
            self.assertNotEqual(SPEC_TOOLCHAIN()["rustc"], identity["rustc"])
        for output in (b"rustc 1.98.0\n", verbose + b"host: aarch64-unknown-linux-gnu\n", b"host: x86 64\n"):
            with self.subTest(output=output), patch.object(cache, "run", return_value=output):
                with self.assertRaises(ValueError):
                    SPEC_TOOLCHAIN()

    def test_arch_keys_every_library_the_job_installs_and_never_the_whole_inventory(self):
        text = (ROOT / ".github/workflows/quality.yml").read_text()
        job = text.split("\n  rust-arch:\n", 1)[1].split("\n  image-privacy:\n", 1)[0]
        command = re.search(r"pacman -Su ((?:[^\n]*\\\n)+[^\n]*)\n", job).group(1)
        installed = set(command.replace("\\", " ").split()) - {"--noconfirm", "--noprogressbar", "--needed"}
        # Tools whose versions never reach compiled output; rustup's toolchain
        # is keyed by `rustc -vV` instead.
        tools = {"base-devel", "git", "openssh", "python", "rustup", "ca-certificates"}
        runtime = set(cache.ARCH_RUNTIME_PACKAGES)
        self.assertLessEqual(runtime, installed)
        self.assertFalse(runtime & set(cache.ARCH_NATIVE_PACKAGES))
        self.assertLessEqual(installed - tools - runtime, set(cache.ARCH_NATIVE_PACKAGES))
        self.assertLessEqual({"gcc", "glibc", "binutils", "clang", "linux-api-headers"}, set(cache.ARCH_NATIVE_PACKAGES))
        with patch.object(cache, "run", return_value=b"gcc 15.2.1-1\n") as run, patch("builtins.print") as shown:
            cache.native_identity("archlinux")
        self.assertEqual(run.call_args.args[0], ["pacman", "-Q", *sorted(cache.ARCH_NATIVE_PACKAGES)])
        self.assertEqual(shown.call_args.args, ("Keyed Arch packages:", "gcc 15.2.1-1"))

    def test_run_as_becomes_the_account_and_refuses_root_or_a_different_user(self):
        account = type("Entry", (), {"pw_uid": 1001, "pw_gid": 1001})()
        ids = {"uid": 0, "gid": 0}
        calls = []

        def become(kind):
            def change(value):
                calls.append((kind, value))
                ids[kind] = value
            return change

        group = type("Group", (), {"gr_gid": 1001})()
        with patch.object(cache.pwd, "getpwnam", return_value=account), patch.object(cache.grp, "getgrnam", return_value=group), \
                patch.object(os, "geteuid", side_effect=lambda: ids["uid"]), patch.object(os, "getuid", side_effect=lambda: ids["uid"]), \
                patch.object(os, "getegid", side_effect=lambda: ids["gid"]), patch.object(os, "getgid", side_effect=lambda: ids["gid"]), \
                patch.object(os, "initgroups", side_effect=lambda name, gid: calls.append(("groups", name, gid)), create=True), \
                patch.object(os, "setgid", side_effect=become("gid")), patch.object(os, "setuid", side_effect=become("uid")):
            cache.run_as("builder")
            # Groups and group before the user, which gives up the right to change them.
            self.assertEqual(calls, [("groups", "builder", 1001), ("gid", 1001), ("uid", 1001)])
            calls.clear()
            cache.run_as("builder")
            self.assertEqual(calls, [])
            ids.update(uid=1002, gid=1002)
            with self.assertRaises(ValueError):
                cache.run_as("builder")
            ids.update(uid=1001, gid=1001)
            # setpriv and chown name the group after the account.
            group.gr_gid = 1002
            with self.assertRaises(ValueError):
                cache.run_as("builder")
            group.gr_gid = 1001
            account.pw_uid = 0
            with self.assertRaises(ValueError):
                cache.run_as("builder")
        for name in ("", "Builder", "builder;id", "-builder", "b" * 33):
            with self.subTest(name=name), self.assertRaises(ValueError):
                cache.run_as(name)

    def test_container_mode_hands_every_cargo_command_and_the_restore_to_the_account(self):
        text = (ROOT / ".github/actions/setup-rust/action.yml").read_text()
        steps = text.split("\nruns:\n", 1)[1].split("\n    - name: ")[1:]
        helper = [step for step in steps if "cache.py " in step]
        self.assertEqual(len(helper), 4)
        for step in helper:
            with self.subTest(step=step.splitlines()[0]):
                self.assertIn("CACHE_RUN_AS: ${{ inputs.run-as }}", step)
        upstream = [step for step in steps if "uses: Swatinem/rust-cache@" in step]
        for step in upstream:
            self.assertIn("cmd-format: ${{ inputs.run-as != '' && format('setpriv --reuid={0} --regid={0} --init-groups -- {{0}}', inputs.run-as) || '{0}' }}", step)
        index = {name: next(i for i, step in enumerate(steps) if marker in step) for name, marker in {
            "restore": "id: restore", "owner": "chown -R -h -P", "restored": "cache.py restored"}.items()}
        self.assertEqual(sorted(index, key=index.get), list(index))
        self.assertIn("if: inputs.phase == 'setup' && inputs.run-as != ''", steps[index["owner"]])
        # The container brings its own packages and toolchain.
        for marker in ("apt-get install", "rustup show active-toolchain"):
            step = next(step for step in steps if marker in step)
            self.assertIn("&& inputs.container == ''", step.split("\n", 2)[1])
        validation = steps[0]
        self.assertIn('case "$CACHE_CONTAINER" in ""|archlinux) ;; *) exit 2 ;; esac', validation)
        self.assertIn('if [ -n "$CACHE_CONTAINER" ] && [ -z "$CACHE_RUN_AS" ]; then exit 2; fi', validation)
        self.init_git()
        with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "RUNNER_OS": "Linux", "CACHE_CONTAINER": "alpine"}), \
                patch.object(cache, "native_identity", return_value="native"):
            with self.assertRaises(ValueError):
                cache.prepare()

    def test_native_versions_are_part_of_compatibility(self):
        # Linux has its own tests below (UbuntuPackages, UbuntuNativeIdentity).
        with patch.object(cache, "run", return_value=b"old native inputs"):
            old = cache.native_identity("macOS")
        with patch.object(cache, "run", return_value=b"updated native inputs"):
            self.assertNotEqual(old, cache.native_identity("macOS"))
        with self.assertRaises(ValueError):
            cache.native_identity("Windows")

    def test_prepared_ubuntu_key_follows_the_closure_and_not_the_rest_of_the_image(self):
        self.init_git()
        runner = UbuntuRunner(self.base / "ubuntu")
        (runner.bin / "git").symlink_to(shutil.which("git"))
        rows = snapshot_rows()

        def key():
            runner.snapshot.write_text(snapshot_text(rows))
            with runner.environment(), patch("builtins.print"), \
                    patch.dict(os.environ, {"CACHE_PROFILE": "debug", "RUNNER_OS": "Linux", "CACHE_VENDOR_REUSE": "true"}):
                cache.prepare()
            lines = Path(self.env["GITHUB_ENV"]).read_text().splitlines()
            Path(self.env["GITHUB_ENV"]).write_text("")
            return dict(line.split("=", 1) for line in lines)["CI_RUST_CACHE_KEY"]

        baseline = key()
        self.assertRegex(baseline, r"^debug-[0-9a-f]{64}$")
        # The other cloud's image: the same build packages beside another inventory.
        rows["git:arm64"][0] += "+other"
        rows["azure-cli:all"] = ["2.79.0-1~noble", "ii ", "", "python3:any", ""]
        del rows["xdg-utils:all"]
        self.assertEqual(key(), baseline)
        rows["libc6-dev:arm64"][0] += "+updated"
        self.assertNotEqual(key(), baseline)

    def test_prepare_prints_the_flag_names_and_never_a_value(self):
        self.init_git()
        secret = "s3cret-value"
        hostile = "CC_x\n::error::stop"
        flags = {"CC": f"/opt/{secret}/bin/cc", "HOST_CFLAGS": f"-D{secret}", "OPENSSL_NO_PKG_CONFIG": secret,
                 "RUSTFLAGS": f"--cfg {secret}", "CPATH": f"/opt/{secret}/include", hostile: secret}
        others = {"HOSTNAME": secret, "HOST_FOO": secret, "CARGO_TARGET_DIR": str(self.paths[0]), "CFLAGS": ""}
        clean = {name: os.environ[name] for name in ("PATH", "HOME") if name in os.environ}

        def printed(environ):
            with patch.dict(os.environ, {**clean, **self.env, "CACHE_PROFILE": "debug", "RUNNER_OS": "Linux", **environ}, clear=True), \
                    patch.object(cache, "native_identity", return_value="native"), patch("builtins.print") as shown:
                cache.prepare()
            Path(self.env["GITHUB_ENV"]).write_text("")
            return [" ".join(map(str, call.args)) for call in shown.call_args_list]

        lines = printed({**flags, **others})
        # The selected names, sorted, on one line that no name can break or turn into a command.
        self.assertEqual(lines, ["Keyed flag variables: CC, CC_x\\n::error::stop, CPATH, HOST_CFLAGS, OPENSSL_NO_PKG_CONFIG, RUSTFLAGS"])
        self.assertNotIn(secret, "\n".join(lines))
        self.assertEqual(printed(others), ["Keyed flag variables: none"])

    def test_profiles_have_distinct_identity_and_private_paths(self):
        self.init_git()
        identities = []
        for profile in sorted(cache.PROFILES):
            with patch.dict(os.environ, {"CACHE_PROFILE": profile, "RUNNER_OS": "Linux"}), \
                    patch.object(cache, "native_identity", return_value="native"), patch("builtins.print"):
                cache.prepare()
            values = dict(line.split("=", 1) for line in Path(self.env["GITHUB_ENV"]).read_text().splitlines())
            identities.append(values["CI_RUST_CACHE_KEY"])
            self.assertEqual(values["CARGO_HOME"], str(self.cargo))
            self.assertEqual(values["CARGO_TARGET_DIR"], str(self.paths[0]))
        self.assertEqual(len(set(identities)), len(cache.PROFILES))

    def test_unexpected_target_or_cargo_home_refuses_cleanup(self):
        for key in ("CARGO_HOME", "CARGO_TARGET_DIR"):
            with self.subTest(key=key), patch.dict(os.environ, {key: str(self.base)}):
                with self.assertRaises(ValueError):
                    cache.roots()

    def test_payload_never_follows_symlinks(self):
        external = self.file(self.base / "external", b"preserve")
        self.paths[0].mkdir()
        (self.paths[0] / "alias").symlink_to(external)
        with self.assertRaises(ValueError):
            cache.entries(self.paths)
        cache.clear(self.paths)
        self.assertEqual(external.read_bytes(), b"preserve")

    def test_entry_budget_and_hardlink_accounting_are_conservative(self):
        first = self.file(self.paths[0] / "first", b"1234")
        second = self.paths[0] / "second"
        os.link(first, second)
        self.assertEqual(sum(size for _, size in cache.entries(self.paths)), 2 * (4096 + 4))
        with patch.object(cache, "MAX_FILES", 1):
            with self.assertRaises(ValueError):
                cache.entries(self.paths)

    def restore(self, hit="true", outcome="success"):
        self.file(self.root / "Cargo.lock", b"locked")
        self.file(self.runner / "gitturtle-cache-start", b"1")
        for path in self.paths:
            self.file(path / "partial", b"cached")
        with patch.dict(os.environ, {"CACHE_HIT": hit, "CACHE_OUTCOME": outcome,
                                    "CI_RUST_CACHE_LOCK": cache.digest(b"locked"),
                                    "CI_RUST_CACHE_KEY": "fixture-key"}), patch.object(cache, "record") as record:
            cache.restored()
        return record.call_args

    def test_exact_successful_restore_keeps_payload(self):
        args = self.restore()
        self.assertTrue(all(path.exists() for path in self.paths))
        self.assertTrue(args.kwargs["cache"]["hit"])
        self.assertNotIn("size_bytes", args.kwargs["cache"])
        self.assertNotIn("save_seconds", args.kwargs["cache"])

    def test_miss_fallback_error_or_missing_result_discards_partial_payload(self):
        for hit, outcome in (("false", "success"), ("", "failure"), ("true", "failure"), ("", "success")):
            with self.subTest(hit=hit, outcome=outcome):
                args = self.restore(hit, outcome)
                self.assertFalse(any(path.exists() for path in self.paths))
                self.assertFalse(args.kwargs["cache"]["hit"])

    def test_bounds_clean_whole_packages_then_keep_useful_dependencies(self):
        local = self.file(self.paths[0] / "debug/deps/libapp-abcd.rlib", b"L" * 100)
        large = self.file(self.paths[0] / "debug/deps/liblarge-abcd.rlib", b"G" * 200)
        small = self.file(self.paths[0] / "debug/deps/libsmall-abcd.rlib", b"S")
        calls = []
        packages = [{"name": name, "id": name, "source": None if name == "app" else "registry", "targets": [{"name": name, "kind": ["lib"]}]} for name in ("app", "large", "small")]

        def execute(command, **_):
            calls.append(command)
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            {"app": local, "large": large, "small": small}[command[-1]].unlink(missing_ok=True)
            return b""

        # Two directories plus one small dependency fit after Cargo removes app/large.
        with patch.dict(cache.PROFILE_LIMITS, {"debug-release": 3 * 4096 + 5}):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=execute)
        self.assertTrue(result["save"])
        self.assertFalse(result["dropped_target"])
        self.assertFalse(local.exists())
        self.assertFalse(large.exists())
        self.assertTrue(small.exists())
        self.assertEqual(result["removed_dependency_packages"], 1)
        self.assertTrue(all("--locked" in command for command in calls))

    def test_members_are_cleaned_but_vendored_path_packages_are_kept_and_never_evicted(self):
        member = self.file(self.paths[0] / "debug/deps/libapp-abcd.rlib", b"M" * 100)
        vendored = [self.file(self.paths[0] / "debug/deps/libtool-abcd.rlib", b"V" * 300),
                    self.file(self.paths[0] / "debug/.fingerprint/tool-abcd/lib-tool", b"F")]
        dependency = self.file(self.paths[0] / "debug/deps/libdep-abcd.rlib", b"D" * 200)
        packages = [
            {"name": "app", "id": "app", "source": None, "manifest_path": str(self.root / "crates/app/Cargo.toml"),
             "targets": [{"name": "app", "kind": ["lib"]}]},
            {"name": "tool", "id": "tool", "source": None, "manifest_path": str(self.root / "vendor/tool/Cargo.toml"),
             "targets": [{"name": "tool", "kind": ["lib"]}]},
            # A path package outside vendor/ is not keyed, so it is cleaned too.
            {"name": "helper", "id": "helper", "source": None, "manifest_path": str(self.root / "tools/helper/Cargo.toml"),
             "targets": [{"name": "helper", "kind": ["lib"]}]},
            {"name": "dep", "id": "dep", "source": "registry", "targets": [{"name": "dep", "kind": ["lib"]}]}]
        calls = []

        def execute(command, **_):
            calls.append(command)
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            for package in command[command.index("--package") + 1::2]:
                if package in ("app", "dep"):
                    {"app": member, "dep": dependency}[package].unlink(missing_ok=True)
            return b""

        # After member cleanup: four directories, the vendored library and
        # fingerprint and the registry library (29,173 bytes). Evicting only the
        # registry dependency fits; the vendored package is never a candidate.
        with patch.dict(cache.PROFILE_LIMITS, {"debug": 25_000}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        cleaned = [command[command.index("--package") + 1::2] for command in calls if command[1] == "clean"]
        self.assertEqual(cleaned, [["app", "helper"], ["dep"]])
        self.assertFalse(member.exists())
        self.assertFalse(dependency.exists())
        self.assertTrue(all(path.exists() for path in vendored))
        self.assertEqual([row["name"] for row in result["evicted_packages"]], ["dep"])
        self.assertTrue(result["save"])
        self.assertFalse(result["dropped_target"])
        self.assertEqual(result["retained_vendor_packages"], 1)
        self.assertEqual(result["retained_vendor_bytes"], 300 + 1 + 3 * 4096)

    def test_oversized_target_is_discarded_without_partial_native_outputs(self):
        self.file(self.paths[0] / "debug/build/sys-hash/out/include.h", b"native")
        download = self.file(self.paths[1] / "dependency.crate", b"download")
        with patch.dict(cache.PROFILE_LIMITS, {"debug-release": 8192}):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=lambda *a, **k: b'{"packages": []}')
        self.assertTrue(result["save"])
        self.assertTrue(result["dropped_target"])
        self.assertFalse(self.paths[0].exists())
        self.assertTrue(download.exists())

    def test_oversized_downloads_prevent_save_without_deleting_credentials(self):
        self.file(self.paths[1] / "dependency.crate", b"large")
        credential = self.file(self.cargo / "credentials.toml", b"not in cache")
        with patch.dict(cache.PROFILE_LIMITS, {"debug-release": 10}):
            result = cache.bound_payload(self.root, self.paths, "debug-release", execute=lambda *a, **k: b'{"packages": []}')
        self.assertFalse(result["save"])
        self.assertEqual(credential.read_bytes(), b"not in cache")

    def test_projection_prunes_only_disposable_sources_after_all_cargo_commands(self):
        library = self.file(self.paths[0] / "debug/deps/libkeep-abcd.rlib", b"compiled")
        generated = self.file(self.paths[0] / "debug/build/keep-sys-abcd/out/native.a", b"native")
        fingerprint = self.file(self.paths[0] / "debug/.fingerprint/keep-sys-abcd/lib-keep", b"fingerprint")
        pure = self.file(self.paths[1] / "src/registry/pure-1.0.0/src/lib.rs", b"R" * 200_000)
        outdated = self.file(self.paths[1] / "src/registry/old-sys-0.1.0/native.c", b"old")
        native = self.file(self.paths[1] / "src/registry/keep-sys-1.0.0/native.c", b"timestamp-sensitive")
        checkout = self.file(self.paths[2] / "checkouts/dep/ref/src/lib.rs", b"git source")
        database = self.file(self.paths[2] / "db/dep/objects/pack/pack.data", b"git database")
        timestamp = native.stat().st_mtime_ns
        packages = [{"name": "keep-sys", "version": "1.0.0", "id": "keep", "source": "registry"},
                    {"name": "app", "id": "app", "source": None}]
        calls = []

        def execute(command, **_):
            calls.append(command)
            self.assertTrue(pure.exists(), "Cargo commands must run before source pruning")
            return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 180_000}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertTrue(result["save"])
        self.assertFalse(result["dropped_target"])
        self.assertEqual(result["removed_dependency_packages"], 0)
        self.assertEqual(result["removed_source_directories"], 2)
        self.assertEqual(len(calls), 2)
        self.assertFalse(pure.exists())
        self.assertFalse(outdated.exists())
        self.assertEqual(native.stat().st_mtime_ns, timestamp)
        for path in (library, generated, fingerprint, checkout, database):
            self.assertTrue(path.exists())
        initial, _, final = result["snapshots"]
        self.assertGreater(initial["bytes"], result["limit_bytes"])
        self.assertEqual(initial["projected_bytes"], final["bytes"])
        self.assertEqual(final["projected_bytes"], final["bytes"])
        self.assertEqual(final["compiled"]["libraries"]["files"], 1)
        self.assertEqual(final["compiled"]["native_generated"]["files"], 1)
        self.assertEqual(final["compiled"]["fingerprints"]["files"], 1)
        self.assertEqual(final["bytes"], final["logical_bytes"] + final["entry_overhead_bytes"])
        self.assertEqual(final["subtrees"]["registry_src_removable"]["entries"], 0)
        self.assertNotIn(str(self.base), json.dumps(result))

    def test_excluded_sources_still_enforce_type_and_entry_limits(self):
        source = self.file(self.paths[1] / "src/registry/pure-1.0.0/lib.rs", b"source")
        execute = lambda *a, **k: b'{"packages": []}'
        with patch.object(cache, "MAX_FILES", 2), self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertTrue(source.exists())
        source.unlink()
        source.symlink_to(self.file(self.base / "outside", b"preserved"))
        with self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        source.unlink()
        os.mkfifo(source)
        with self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual((self.base / "outside").read_bytes(), b"preserved")

    def test_accounting_refuses_expired_time_nondirectory_and_symlink_roots(self):
        source = self.file(self.paths[1] / "src/registry/pure-1.0.0/lib.rs")
        with patch.object(cache, "BUDGET_SECONDS", 0), self.assertRaises(TimeoutError):
            cache.bound_payload(self.root, self.paths, "debug", execute=lambda *a, **k: b'{"packages": []}')
        self.assertTrue(source.exists())
        self.file(self.paths[0])
        with self.assertRaises(ValueError):
            cache.entries(self.paths)
        self.paths[0].unlink()
        self.paths[0].symlink_to(self.paths[1], target_is_directory=True)
        with self.assertRaises(ValueError):
            cache.entries(self.paths)

    def test_cleanup_never_exceeds_eight_dependency_groups(self):
        packages = [{"name": f"p{i}", "id": f"p{i}", "source": "registry"} for i in range(12)]
        for package in packages:
            self.file(self.paths[0] / f"debug/deps/{package['name']}-hash.rlib")
        calls = []

        def execute(command, **_):
            calls.append(command)
            return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 1}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["removed_dependency_packages"], 8)
        self.assertEqual(len([command for command in calls if command[1] == "clean"]), 8)
        self.assertTrue(result["dropped_target"])

    def test_eviction_diagnostics_keep_safe_names_without_source_ids(self):
        package_id = "git+https://example.invalid/private?token=private#revision"
        packages = [{"name": "fixture", "version": "1.2.3", "id": package_id, "source": "git"}]
        artifact = self.file(self.paths[0] / "debug/deps/fixture-hash.rlib", b"artifact")

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            artifact.unlink()
            return b""

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 8192}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "version": "1.2.3", "cleanup_completed": True}])
        self.assertNotIn("private", json.dumps(result))
        # Unexpected metadata names are still identifiable without emitting
        # delimiters, a URL, a path or an unbounded string into public logs.
        packages[0]["version"] = "private/path" + "x" * 200
        artifact.write_bytes(b"again")
        with patch.dict(cache.PROFILE_LIMITS, {"debug": 8192}):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute)
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "cleanup_completed": True}])
        self.assertNotIn("private", json.dumps(result))

    def test_failed_budget_preserves_completed_snapshots_and_fixed_reason(self):
        self.file(self.paths[0] / "debug/deps/fixture-hash.rlib")
        snapshot = {"stage": "before_local_cleanup", "bytes": 123}

        def unavailable(*_, snapshots, diagnostics, **__):
            snapshots.append(snapshot)
            diagnostics.update(limit_bytes=123_456, stage="after_dependency_cleanup_1", stage_seconds=[{"stage": "after_dependency_cleanup_1", "elapsed_seconds": 0.5}],
                               evicted_packages=[{"metadata_index": 0, "name": "fixture", "cleanup_completed": True}])
            raise ValueError("private/path or command output must not be logged")

        with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "CI_RUST_CACHE_PROFILE": "debug"}), \
                patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", side_effect=unavailable), \
                patch.object(cache, "record") as record, patch("builtins.print"):
            cache.main()
        result = record.call_args.kwargs["details"]
        self.assertFalse(result["save"])
        self.assertEqual(result["snapshots"], [snapshot])
        self.assertEqual(result["reason"], "cache budget preparation unavailable")
        self.assertEqual(result["failure_stage"], "after_dependency_cleanup_1")
        self.assertEqual(result["limit_bytes"], 123_456)
        self.assertEqual(result["stage_seconds"], [{"stage": "after_dependency_cleanup_1", "elapsed_seconds": 0.5}])
        self.assertEqual(result["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "cleanup_completed": True}])
        self.assertNotIn("private", json.dumps(result))
        self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), "save=false\n")

    def test_finish_account_failure_refuses_the_save_without_failing_the_job(self):
        # Tests already passed when finish runs; only the save may be lost.
        for failure in (ValueError("cache helper is not running as its account"), KeyError("builder")):
            with self.subTest(failure=type(failure).__name__):
                Path(self.env["GITHUB_OUTPUT"]).write_text("")
                with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "CI_RUST_CACHE_PROFILE": "debug", "CACHE_RUN_AS": "builder"}), \
                        patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "run_as", side_effect=failure), \
                        patch.object(cache, "bound_payload") as bound, patch.object(cache, "record") as record, patch("builtins.print"):
                    cache.main()
                bound.assert_not_called()
                self.assertFalse(record.call_args.kwargs["details"]["save"])
                self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), "save=false\n")

    def test_profile_cleanup_failure_retains_fixed_stage_and_incomplete_group(self):
        self.file(self.paths[0] / "release/deps/libfixture-hash.rlib", b"artifact")
        diagnostics = {}
        packages = [{"name": "fixture", "version": "1.0.0", "id": "private/package-id", "source": "registry", "targets": [{"name": "fixture", "kind": ["lib"]}]}]

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            raise subprocess.TimeoutExpired(command, 30, output=b"private output")

        with patch.dict(cache.PROFILE_LIMITS, {"release": 1}), self.assertRaises(subprocess.TimeoutExpired):
            cache.bound_payload(self.root, self.paths, "release", execute=execute, diagnostics=diagnostics)
        self.assertEqual(diagnostics["stage"], "dependency_cleanup_1_release")
        self.assertEqual(diagnostics["evicted_packages"], [{"metadata_index": 0, "name": "fixture", "version": "1.0.0", "cleanup_completed": False}])
        self.assertEqual(diagnostics["stage_seconds"][-1]["stage"], "dependency_cleanup_1_release")
        self.assertTrue(all(row["elapsed_seconds"] >= 0 for row in diagnostics["stage_seconds"]))
        self.assertNotIn("private", json.dumps(diagnostics))

    def test_selection_attributes_structured_artifacts_not_colliding_names(self):
        # Test/example targets named "debug" or "build" and every build script's
        # "build_script_build" binary once matched whole subtrees. The root sits
        # below an absolute "build/repo-0" directory: components above the root,
        # entries outside it, and test binaries named after test targets never score.
        target = self.base / "build/repo-0/target"
        packages = [
            {"name": "serde_json", "id": "serde_json", "source": "registry",
             "targets": [{"name": "serde_json", "kind": ["lib"]}, {"name": "debug", "kind": ["test"]},
                         {"name": "build-script-build", "kind": ["custom-build"]}]},
            {"name": "clang-sys", "id": "clang-sys", "source": "registry",
             "targets": [{"name": "clang_sys", "kind": ["lib"]}, {"name": "build", "kind": ["test"]},
                         {"name": "lib", "kind": ["test"]}, {"name": "build-script-build", "kind": ["custom-build"]}]},
            {"name": "bulky", "id": "bulky", "source": "registry", "targets": [{"name": "bulky", "kind": ["lib"]}]},
            {"name": "repo", "id": "repo", "source": "registry", "targets": [{"name": "repo", "kind": ["lib"]}]},
        ]
        for relative, data in (("debug/deps/libbulky-5e6f.rlib", b"B" * 100_000),
                               ("debug/deps/libserde_json-1a2b.rlib", b"j" * 30_000),
                               ("debug/build/serde_json-1a2b/build_script_build-1a2b", b"s" * 1_000),
                               ("debug/deps/libclang_sys-3c4d.rlib", b"c" * 10_000),
                               ("debug/build/clang-sys-3c4d/out/bindings.rs", b"o" * 8_000),
                               ("debug/build/clang-sys-3c4d/build_script_build-3c4d", b"s" * 1_000),
                               ("debug/.fingerprint/clang-sys-3c4d/lib-clang_sys", b"f"),
                               ("debug/app-9f9f", b"a" * 500_000), ("debug/deps/app-9f9f", b"a" * 500_000),
                               ("debug/deps/debug-9a9a", b"t" * 400_000), ("debug/deps/lib-9b9b", b"t" * 400_000)):
            self.file(target / relative, data)
        self.file(self.paths[1] / "src/build/bulky-1.0.0/lib.rs", b"r" * 400_000)
        owned = {
            "bulky": {"debug/deps/libbulky-5e6f.rlib"},
            "serde_json": {"debug/deps/libserde_json-1a2b.rlib", "debug/build/serde_json-1a2b",
                           "debug/build/serde_json-1a2b/build_script_build-1a2b"},
            "clang-sys": {"debug/deps/libclang_sys-3c4d.rlib", "debug/build/clang-sys-3c4d", "debug/build/clang-sys-3c4d/out",
                          "debug/build/clang-sys-3c4d/out/bindings.rs", "debug/build/clang-sys-3c4d/build_script_build-3c4d",
                          "debug/.fingerprint/clang-sys-3c4d", "debug/.fingerprint/clang-sys-3c4d/lib-clang_sys"},
        }
        measured = cache.entries((target, self.paths[1]))
        expected = {name: sum(size for path, size in measured
                              if path.is_relative_to(target) and str(path.relative_to(target)) in members)
                    for name, members in owned.items()}
        self.assertEqual(cache.package_artifact_bytes(packages, measured, target), expected)
        self.assertEqual(cache.largest_packages(packages, measured, target), ["bulky", "clang-sys", "serde_json"])

    def test_every_cleanup_command_selects_the_job_profile(self):
        packages = [{"name": "app", "id": "app", "source": None, "targets": [{"name": "app", "kind": ["lib"]}]},
                    {"name": "dep", "id": "dep", "source": "registry", "targets": [{"name": "dep", "kind": ["lib"]}]}]
        for profile, clean_profiles in (("debug", ["dev"]), ("release", ["release"]), ("debug-release", ["dev", "release"])):
            with self.subTest(profile=profile):
                for directory in ("debug", "release"):
                    self.file(self.paths[0] / f"{directory}/deps/libapp-hash.rlib", b"a" * 10_000)
                    self.file(self.paths[0] / f"{directory}/deps/libdep-hash.rlib", b"d" * 10_000)
                calls = []

                def execute(command, **_):
                    calls.append(command)
                    return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

                with patch.dict(cache.PROFILE_LIMITS, {profile: 1}):
                    result = cache.bound_payload(self.root, self.paths, profile, execute=execute)
                self.assertEqual(result["removed_dependency_packages"], 1)
                # Local cleanup and each eviction name the job's Cargo profile explicitly.
                self.assertEqual([command for command in calls if command[1] == "clean"],
                                 [["cargo", "clean", "--locked", "--profile", clean_profile, "--package", package]
                                  for package in ("app", "dep") for clean_profile in clean_profiles])

    def test_target_jobs_clean_both_layouts_and_refuse_unsafe_triples(self):
        packages = [{"name": "app", "id": "app", "source": None, "targets": [{"name": "app", "kind": ["lib"]}]},
                    {"name": "dep", "id": "dep", "source": "registry", "targets": [{"name": "dep", "kind": ["lib"]}]}]
        triple = "x86_64-unknown-linux-gnu"
        # A release job compiling with --target keeps artifacts under
        # target/<triple>/release; a debug job without --target is unchanged.
        for profile, target, layouts in (("release", triple, [[], ["--target", triple]]), ("debug", None, [[]])):
            with self.subTest(profile=profile):
                for directory in (f"{triple}/release", "debug"):
                    self.file(self.paths[0] / f"{directory}/deps/libapp-hash.rlib", b"a" * 10_000)
                    self.file(self.paths[0] / f"{directory}/deps/libdep-hash.rlib", b"d" * 10_000)
                calls = []

                def execute(command, **_):
                    calls.append(command)
                    return json.dumps({"packages": packages}).encode() if command[1] == "metadata" else b""

                with patch.dict(cache.PROFILE_LIMITS, {profile: 1}):
                    result = cache.bound_payload(self.root, self.paths, profile, execute=execute, target=target)
                self.assertEqual(result["removed_dependency_packages"], 1)
                self.assertEqual([command for command in calls if command[1] == "clean"],
                                 [["cargo", "clean", "--locked", "--profile", "dev" if profile == "debug" else "release",
                                   *layout, "--package", package] for package in ("app", "dep") for layout in layouts])
                self.assertNotIn(triple, json.dumps(result))
        # main() forwards only an empty or safe CACHE_TARGET; anything else is
        # refused before any Cargo command with the fixed save=false diagnostics.
        bound_payload, seen = cache.bound_payload, []

        def bound_without_cargo(*args, **kwargs):
            seen.append(kwargs.get("target"))
            return bound_payload(*args, execute=lambda *a, **k: b'{"packages": []}', **kwargs)

        for value, forwarded, save in ((triple, triple, "true"), ("", None, "true"), ("../evil triple", None, "false"),
                                       ("-", None, "false"), ("--x", None, "false")):
            with self.subTest(target=value):
                Path(self.env["GITHUB_OUTPUT"]).unlink(missing_ok=True)
                calls_before = len(seen)
                with patch.dict(os.environ, {"CACHE_PROFILE": "release", "CI_RUST_CACHE_PROFILE": "release", "CACHE_TARGET": value}), \
                        patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", bound_without_cargo), \
                        patch.object(subprocess, "run", side_effect=AssertionError("no subprocess may run")), \
                        patch.object(cache, "record") as record, patch("builtins.print"):
                    cache.main()
                result = record.call_args.kwargs["details"]
                self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), f"save={save}\n")
                if save == "true":
                    self.assertEqual(seen[-1], forwarded)
                else:
                    self.assertEqual(len(seen), calls_before)
                    self.assertEqual(result["reason"], "cache budget preparation unavailable")
                    self.assertEqual(result["failure_stage"], "unavailable")
                    self.assertNotIn("evil", json.dumps(result))

    def test_projection_mismatch_fails_in_its_own_stage(self):
        self.file(self.paths[1] / "src/registry/pure-1.0.0/lib.rs", b"pure")
        retained = self.file(self.paths[1] / "cache/dependency.crate", b"download")
        clear, bound_payload = cache.clear, cache.bound_payload

        def diverging_clear(paths):
            clear(paths)
            retained.unlink(missing_ok=True)  # physical pruning removes more than projected

        def bound_without_cargo(*args, **kwargs):
            # main() binds the real runner as the default executor; inject the
            # fake here so no Cargo process can ever run from this fixture.
            return bound_payload(*args, execute=lambda *a, **k: b'{"packages": []}', **kwargs)

        with patch.dict(os.environ, {"CACHE_PROFILE": "debug", "CI_RUST_CACHE_PROFILE": "debug"}), \
                patch("sys.argv", ["cache.py", "bound"]), patch.object(cache, "bound_payload", bound_without_cargo), \
                patch.object(subprocess, "run", side_effect=AssertionError("no subprocess may run")), \
                patch.object(cache, "clear", diverging_clear), patch.object(cache, "record") as record, patch("builtins.print"):
            cache.main()
        result = record.call_args.kwargs["details"]
        self.assertFalse(result["save"])
        self.assertEqual(result["reason"], "cache budget preparation unavailable")
        self.assertEqual(result["failure_stage"], "projection_verification")
        self.assertEqual(result["stage_seconds"][-1]["stage"], "projection_verification")
        self.assertEqual(result["snapshots"][-1]["stage"], "after_source_pruning")
        self.assertEqual(result["limit_bytes"], cache.PROFILE_LIMITS["debug"])
        self.assertEqual(Path(self.env["GITHUB_OUTPUT"]).read_text(), "save=false\n")

    def budget_fixture(self):
        packages = [{"name": f"dep{i}", "id": f"dep{i}", "source": "registry", "targets": [{"name": f"dep{i}", "kind": ["lib"]}]}
                    for i in range(4)]
        libraries = {package["id"]: self.file(self.paths[0] / f"debug/deps/lib{package['name']}-hash.rlib", b"x" * 10_000)
                     for package in packages}
        self.file(self.paths[1] / "cache/index/dependency.crate", b"download")
        return packages, libraries

    def test_scheduling_stops_before_an_unaffordable_eviction_and_still_finalizes(self):
        packages, libraries = self.budget_fixture()
        clock = FakeClock()
        entries = cache.entries

        def slow_entries(paths, **kwargs):
            clock.now += 10  # each scan costs 10 s, checked against its deadline on completion
            return entries(paths, **kwargs)

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            clock.now += 25  # each cleanup costs 25 s of the 90 s scheduling budget
            libraries[command[-1]].unlink()
            return b""

        with patch.dict(cache.PROFILE_LIMITS, {"debug": 20_000}), patch.object(cache, "entries", slow_entries):
            result = cache.bound_payload(self.root, self.paths, "debug", execute=execute, clock=clock)
        # Two scans and one eviction end at 55 s. A second eviction would end
        # its scan exactly at the 90 s deadline and fail closed, so only the
        # one-measurement headroom refuses it; the loop stops with candidates
        # left and finalization still yields the downloads-only save.
        self.assertEqual(result["removed_dependency_packages"], 1)
        self.assertTrue(result["dropped_target"])
        self.assertTrue(result["save"])
        self.assertEqual(result["snapshots"][-1]["stage"], "after_target_fallback")

    def test_finalization_allowance_is_separate_and_fails_closed(self):
        clock = FakeClock()
        source = self.paths[1] / "src/registry/pure-1.0.0/lib.rs"
        clear = cache.clear

        def slow_clear(paths):
            clock.now += 25  # pruning and fallback removal count against finalization only
            clear(paths)

        def execute(command, **_):
            if command[1] == "metadata":
                return json.dumps({"packages": packages}).encode()
            clock.now += 40
            libraries[command[-1]].unlink()
            return b""

        # Evictions use 80 s of the 90 s scheduling budget; the verification
        # scan then starts 105 s after the beginning, inside a 60 s allowance
        # but past a 20 s one. The 20,488-byte retained downloads fit 25,000.
        for finalize_seconds, saves in ((60, True), (20, False)):
            with self.subTest(finalize_seconds=finalize_seconds):
                packages, libraries = self.budget_fixture()
                self.file(source, b"pure")
                clock.now = 0.0
                with patch.dict(cache.PROFILE_LIMITS, {"debug": 25_000}), patch.object(cache, "clear", slow_clear), \
                        patch.object(cache, "FINALIZE_SECONDS", finalize_seconds):
                    if saves:
                        result = cache.bound_payload(self.root, self.paths, "debug", execute=execute, clock=clock)
                        self.assertTrue(result["save"])
                        self.assertTrue(result["dropped_target"])
                        self.assertEqual(result["removed_dependency_packages"], 2)
                        self.assertEqual(result["removed_source_directories"], 1)
                    else:
                        with self.assertRaises(TimeoutError):
                            cache.bound_payload(self.root, self.paths, "debug", execute=execute, clock=clock)
                        self.assertFalse(source.exists())

    def test_profiles_apply_their_own_limits(self):
        execute = lambda *a, **k: b'{"packages": []}'
        with patch.dict(cache.PROFILE_LIMITS, {"debug": 40_000, "release": 20_000}):
            # The same 37,288-byte compiled payload fits debug but not release.
            for profile, dropped in (("release", True), ("debug", False)):
                with self.subTest(profile=profile):
                    self.file(self.paths[0] / f"{profile}/deps/libdep-hash.rlib", b"d" * 25_000)
                    result = cache.bound_payload(self.root, self.paths, profile, execute=execute)
                    self.assertEqual(result["limit_bytes"], cache.PROFILE_LIMITS[profile])
                    self.assertEqual(result["dropped_target"], dropped)
                    self.assertTrue(result["save"])
        with self.assertRaises(ValueError):
            cache.bound_payload(self.root, self.paths, "nightly", execute=execute)

    @unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                         "Set GITTURTLE_CACHE_CARGO_QA=1 for the real Cargo recovery fixture")
    def test_real_cargo_cleanup_rebuilds_local_crate_and_native_generated_output(self):
        self.file(self.root / "Cargo.toml", b'[workspace]\n[package]\nname="cache_recovery_fixture"\nversion="0.1.0"\nedition="2021"\n')
        self.file(self.root / "build.rs", b'use std::{env,fs,path::Path}; fn main() { fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("answer.rs"), "pub const ANSWER:u32=42;").unwrap(); }')
        self.file(self.root / "src/lib.rs", b'include!(concat!(env!("OUT_DIR"),"/answer.rs")); #[test] fn generated_answer(){assert_eq!(ANSWER,42);}')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True, capture_output=True)
        commands = [["cargo", "test", "--locked", "--offline", "--profile", profile] for profile in ("dev", "release")]
        for directory, command in zip(("debug", "release"), commands):
            first = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
            self.assertIn(b"1 passed", first.stdout)
            self.assertTrue(list(self.paths[0].glob(f"{directory}/build/*/out/answer.rs")))
        result = cache.bound_payload(self.root, self.paths, "debug-release")
        self.assertTrue(result["save"])
        for directory, command in zip(("debug", "release"), commands):
            self.assertFalse(list(self.paths[0].glob(f"{directory}/build/*/out/answer.rs")))
            second = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
            self.assertIn(b"1 passed", second.stdout)
            self.assertTrue(list(self.paths[0].glob(f"{directory}/build/*/out/answer.rs")))
            self.assertIn(b"Compiling cache_recovery_fixture", second.stderr)

    @unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                         "Set GITTURTLE_CACHE_CARGO_QA=1 for the real Cargo target-layout fixture")
    def test_real_cargo_release_target_layout_is_cleaned_only_with_the_triple(self):
        self.file(self.root / "Cargo.toml", b'[workspace]\n[package]\nname="cache_recovery_fixture"\nversion="0.1.0"\nedition="2021"\n')
        self.file(self.root / "build.rs", b'use std::{env,fs,path::Path}; fn main() { fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("answer.rs"), "pub const ANSWER:u32=42;").unwrap(); }')
        self.file(self.root / "src/lib.rs", b'include!(concat!(env!("OUT_DIR"),"/answer.rs")); #[test] fn generated_answer(){assert_eq!(ANSWER,42);}')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True, capture_output=True)
        version = subprocess.run(["rustc", "-vV"], cwd=self.root, check=True, capture_output=True).stdout.decode()
        host = next(line.split(": ", 1)[1].strip() for line in version.splitlines() if line.startswith("host: "))
        command = ["cargo", "test", "--locked", "--offline", "--profile", "release", "--target", host]
        first = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
        self.assertIn(b"1 passed", first.stdout)
        layout = self.paths[0] / host / "release"

        def artifacts():
            return sorted([*layout.glob("deps/*cache_recovery_fixture*"), *layout.glob(".fingerprint/cache_recovery_fixture-*"),
                           *layout.glob("build/cache_recovery_fixture-*/out/answer.rs")])

        built = artifacts()
        self.assertTrue(any(path.parent.name == "deps" for path in built))
        self.assertTrue(any(path.parent.name == ".fingerprint" for path in built))
        self.assertTrue(any(path.name == "answer.rs" for path in built))
        # Without the triple, pinned Cargo cleans only the host layout: every
        # triple-layout artifact survives, which is the hosted release no-op.
        untouched = cache.bound_payload(self.root, self.paths, "release")
        self.assertTrue(untouched["save"])
        self.assertEqual(artifacts(), built)
        cleaned = cache.bound_payload(self.root, self.paths, "release", target=host)
        self.assertTrue(cleaned["save"])
        self.assertEqual(artifacts(), [])
        self.assertEqual([row["stage"] for row in cleaned["stage_seconds"] if row["stage"].startswith("local_cleanup")],
                         ["local_cleanup_release", "local_cleanup_release_target"])
        self.assertNotIn(host, json.dumps(cleaned))
        second = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
        self.assertIn(b"1 passed", second.stdout)
        self.assertIn(b"Compiling cache_recovery_fixture", second.stderr)
        self.assertTrue(list(layout.glob("build/cache_recovery_fixture-*/out/answer.rs")))

    @unittest.skipUnless(os.environ.get("GITTURTLE_CACHE_CARGO_QA") == "1" and shutil.which("cargo"),
                         "Set GITTURTLE_CACHE_CARGO_QA=1 for the real Cargo vendored-reuse fixture")
    def test_real_cargo_vendored_path_package_is_fresh_only_after_rewinding_checkout_times(self):
        # Mirrors the workspace: an excluded vendor/ path package under the
        # root, a member under crates/, a build script watching a directory.
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.file(self.root / "Cargo.toml", b'[workspace]\nmembers=["crates/app"]\nexclude=["vendor/tool"]\nresolver="2"\n')
        self.file(self.root / "crates/app/Cargo.toml",
                  b'[package]\nname="cache_member"\nversion="0.1.0"\nedition="2021"\n'
                  b'[dependencies]\ncache_vendored={path="../../vendor/tool"}\n')
        self.file(self.root / "crates/app/src/lib.rs", b"#[test] fn answer(){assert_eq!(cache_vendored::answer(),42);}")
        self.file(self.root / "vendor/tool/Cargo.toml", b'[package]\nname="cache_vendored"\nversion="0.1.0"\nedition="2021"\n')
        self.file(self.root / "vendor/tool/build.rs",
                  b'fn main(){println!("cargo:rerun-if-changed=build.rs");println!("cargo:rerun-if-changed=data");}')
        self.file(self.root / "vendor/tool/data/answer.txt", b"42")
        self.file(self.root / "vendor/tool/src/lib.rs",
                  b'pub fn answer()->u32{include_str!("../data/answer.txt").parse().unwrap()}')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True, capture_output=True)
        subprocess.run(["git", "-C", str(self.root), "add", "--all"], check=True)
        command = ["cargo", "test", "--locked", "--offline", "-v"]

        def build():
            result = subprocess.run(command, cwd=self.root, check=True, capture_output=True)
            self.assertIn(b"1 passed", result.stdout)
            return result.stderr

        def checkout():
            # A fresh checkout writes every tracked file (and so its directory)
            # after the cached outputs were built.
            later = time.time_ns() + 2 * 10**9
            for path in sorted({*(self.root / p for _, p in cache.keyed_inputs(self.root)),
                                *(self.root / p for p in ("vendor/tool/data", "vendor/tool/src", "vendor/tool", "vendor"))}):
                os.utime(path, ns=(later, later))

        inputs = cache.keyed_inputs(self.root)
        key = cache.source_identity(self.root, inputs)
        cache.normalize_vendor_mtimes(self.root, inputs)
        build()
        first = cache.bound_payload(self.root, self.paths, "debug")
        self.assertTrue(first["save"])
        self.assertEqual(first["retained_vendor_packages"], 1)
        self.assertGreater(first["retained_vendor_bytes"], 0)
        self.assertFalse(list(self.paths[0].glob("debug/.fingerprint/cache_member-*")))
        self.assertTrue(list(self.paths[0].glob("debug/.fingerprint/cache_vendored-*")))
        # Control: kept outputs alone are not enough, checkout times dirty them.
        checkout()
        self.assertIn(b"Compiling cache_vendored", build())
        cache.bound_payload(self.root, self.paths, "debug")
        checkout()
        self.assertEqual(cache.source_identity(self.root), key)
        cache.normalize_vendor_mtimes(self.root, cache.keyed_inputs(self.root))
        warm = build()
        self.assertIn(b"Fresh cache_vendored", warm)
        self.assertNotIn(b"Compiling cache_vendored", warm)
        self.assertIn(b"Compiling cache_member", warm)
        # A content change is a different key, so CI discards the target
        # before Cargo could compare the rewound times.
        self.file(self.root / "vendor/tool/data/answer.txt", b"43")
        self.assertNotEqual(cache.source_identity(self.root), key)


class UbuntuPackages(unittest.TestCase):
    """The package part of the Ubuntu key, on the real snapshot and edits of it."""

    def setUp(self):
        self.rows = snapshot_rows()

    def packages(self):
        return cache.ubuntu_packages(snapshot_text(self.rows))

    def keyed(self):
        mode, keyed, *_ = self.packages()
        self.assertEqual(mode, "closure")
        return keyed

    def key(self, text=None, **changed):
        inputs = {"tools": UBUNTU_TOOLS, "pc_path": "/usr/lib/pkgconfig", "pc_files": {}, "probes": UBUNTU_PROBES,
                  "local": UBUNTU_LOCAL, "pc_added": None, "directed": (), **changed}
        return cache.ubuntu_identity(snapshot_text(self.rows) if text is None else text, **inputs)[0]

    def depend(self, identity, term):
        """One more Depends group on a real package."""
        self.rows[identity][3] = ", ".join(filter(None, (self.rows[identity][3], term)))

    def test_closure_of_the_real_snapshot_is_what_a_traced_build_read_and_no_tool(self):
        mode, keyed, count, installed, others, cause = self.packages()
        self.assertEqual((mode, count, installed, others, cause), ("closure", 160, 271, 0, None))
        self.assertEqual(keyed, sorted(keyed))
        self.assertLessEqual(set(keyed), {f"{identity}={fields[0]}" for identity, fields in self.rows.items()})
        names = {line.split(":", 1)[0] for line in keyed}
        self.assertLessEqual(set(cache.UBUNTU_BUILD_PACKAGES), names)
        # Version- and architecture-named toolchain packages no list would name,
        # and the kernel headers libc6-dev includes.
        self.assertLessEqual({"gcc-13", "gcc-13-aarch64-linux-gnu", "cpp-13", "g++-13", "binutils-aarch64-linux-gnu",
                              "libc6", "libc6-dev", "linux-libc-dev", "libgcc-13-dev", "libstdc++-13-dev", "pkgconf-bin",
                              "libclang-common-18-dev", "libllvm18", "libdav1d7", "libssl3t64"}, names)
        # No CMake is a dpkg package here: the image's own is keyed as a tool.
        self.assertEqual([identity for identity in self.rows if identity.startswith("cmake")], [])
        # The four packages the traced build read outside the closure, and the
        # action's tools: none reaches a cached dependency output.
        self.assertFalse(names & {"dash", "git", "libc-bin", "openssl", "openssh-client", "ca-certificates",
                                  "python3-pil", "desktop-file-utils", "xdg-utils"})

    def test_any_one_keyed_package_version_changes_the_key(self):
        baseline = self.key()
        keys = {baseline}
        for line in self.keyed():
            identity = line.split("=", 1)[0]
            with self.subTest(package=identity):
                original = self.rows[identity][0]
                self.rows[identity][0] = original + "+1"
                keys.add(self.key())
                self.rows[identity][0] = original
        self.assertEqual(len(keys), 161)
        self.assertEqual(self.key(), baseline)

    def test_packages_outside_the_closure_never_change_the_key(self):
        baseline = self.key()
        outside = sorted(set(self.rows) - {line.split("=", 1)[0] for line in self.keyed()})
        self.assertEqual(len(outside), 111)
        for identity in outside:
            with self.subTest(package=identity):
                fields = self.rows[identity]
                self.rows[identity] = [fields[0] + "+1", *fields[1:]]
                self.assertEqual(self.key(), baseline)
                self.rows[identity] = [fields[0], "rc ", *fields[2:]]
                self.assertEqual(self.key(), baseline)
                del self.rows[identity]
                self.assertEqual(self.key(), baseline)
                self.rows[identity] = fields
        # The rest of a hosted image, with its own dependencies on keyed packages.
        self.rows["azure-cli:all"] = ["2.79.0-1~noble", "ii ", "", "python3:any, libc6 (>= 2.38)", "az"]
        self.rows["firefox:amd64"] = ["1:1snap1-0ubuntu5", "ii ", "debconf", "snapd | zlib1g", ""]
        self.assertEqual(self.key(), baseline)
        self.assertEqual(self.packages()[2:4], (160, 273))

    def test_new_dependency_edge_brings_its_package_into_the_key(self):
        baseline = self.key()
        self.rows["libprobe1:arm64"] = ["1.0-1", "ii ", "", "libprobe-data", ""]
        self.rows["libprobe-data:all"] = ["1.0-1", "ii ", "", "", ""]
        self.assertEqual(self.key(), baseline)
        self.depend("libssl-dev:arm64", "libprobe1 (= 1.0-1)")
        edged = self.key()
        self.assertNotEqual(edged, baseline)
        self.assertLessEqual({"libprobe1:arm64=1.0-1", "libprobe-data:all=1.0-1"}, set(self.keyed()))
        self.assertEqual(len(self.keyed()), 162)
        # Reached through a Pre-Depends edge as well, and keyed by version from then on.
        self.rows["libssl-dev:arm64"][3] = self.rows["libssl-dev:arm64"][3].replace(", libprobe1 (= 1.0-1)", "")
        self.assertEqual(self.key(), baseline)
        self.rows["libssl-dev:arm64"][2] = "libprobe1"
        self.assertEqual(self.key(), edged)
        self.rows["libprobe-data:all"][0] = "1.0-2"
        self.assertNotIn(self.key(), {baseline, edged})

    def test_dependency_spellings_of_the_real_snapshot_all_resolve(self):
        keyed = set(self.keyed())
        # `perl:any` (dpkg-dev) and `python3:any` (libvulkan-dev).
        self.assertIn("dpkg-dev:all", {line.split("=")[0] for line in keyed})
        self.assertLessEqual({"perl:arm64=" + self.rows["perl:arm64"][0], "python3:arm64=" + self.rows["python3:arm64"][0]}, keyed)
        # `libc6-dev | libc-dev` (build-essential): one alternative by name, the
        # other through the versioned `Provides: libc-dev (= 2.39-0ubuntu8.12)`.
        self.assertIn("libc-dev (= 2.39-0ubuntu8.12)", self.rows["libc6-dev:arm64"][4])
        self.assertIn("libc6-dev:arm64=2.39-0ubuntu8.12", keyed)
        # A group with one installed alternative of eight (fontconfig-config).
        self.assertIn("fonts-dejavu-core | fonts-liberation |", self.rows["fontconfig-config:arm64"][3])
        self.assertIn("fonts-dejavu-core:all=2.37-8", keyed)
        spellings = {
            "qualifier": ("libprobe1:any (>= 1.0)", {"libprobe1:arm64": ""}),
            "versioned Provides": ("libprobe-abi", {"libprobe1:arm64": "libprobe (= 1.0-1), libprobe-abi (= 3)"}),
            "plain Provides": ("libprobe-abi (>> 2)", {"libprobe1:arm64": "libprobe-abi"}),
            "two installed alternatives, both followed": ("libprobe1 | libprobe2 (<< 9)", {"libprobe1:arm64": "", "libprobe2:arm64": ""}),
            "one installed alternative of two": ("libprobe-absent (<= 2) | libprobe1", {"libprobe1:arm64": ""}),
            "one name on two architectures": ("libprobe1", {"libprobe1:arm64": "", "libprobe1:armhf": ""}),
            "a name and a provider of it": ("libprobe1", {"libprobe1:arm64": "", "libprobe-ng:arm64": "libprobe1 (= 1.0-1)"}),
        }
        for label, (term, added) in spellings.items():
            with self.subTest(spelling=label):
                self.rows = snapshot_rows()
                for identity, provides in added.items():
                    self.rows[identity] = ["1.0-1", "ii ", "", "", provides]
                self.assertEqual(set(self.keyed()), keyed)
                self.depend("libdav1d-dev:arm64", term)
                self.assertEqual(set(self.keyed()) - keyed, {f"{identity}=1.0-1" for identity in added})

    def test_root_that_is_missing_or_not_installed_fails_the_step(self):
        for root in cache.UBUNTU_BUILD_PACKAGES:
            identity = f"{root}:arm64"
            for label, edit in (("missing", lambda: self.rows.pop(identity)),
                                ("removed, configuration left", lambda: self.rows[identity].__setitem__(1, "rc ")),
                                ("unpacked only", lambda: self.rows[identity].__setitem__(1, "iU "))):
                with self.subTest(root=root, state=label):
                    self.rows = snapshot_rows()
                    edit()
                    with self.assertRaisesRegex(ValueError, f"^Ubuntu build package {re.escape(root)} is not installed$"):
                        self.key()
        # Another package that merely provides the name is not the root.
        self.rows = snapshot_rows()
        del self.rows["pkg-config:arm64"]
        self.assertIn("pkg-config (= 1.8.1-2build1)", self.rows["pkgconf:arm64"][4])
        with self.assertRaises(ValueError):
            self.key()
        with self.assertRaises(ValueError):
            self.key(text="")

    def test_missing_root_fails_the_step_whatever_else_is_wrong_with_the_snapshot(self):
        # Each abnormality alone is a fallback; beside a missing root it must not hide the failure.
        damages = {"a malformed line before the root's": lambda: self.rows["adduser:all"].append("extra"),
                   "a malformed line after the root's": lambda: self.rows["zlib1g:arm64"].pop(),
                   "a dependency outside the grammar": lambda: self.depend("libssl-dev:arm64", "libprobe1 [amd64]"),
                   "a closure member that is only unpacked": lambda: self.rows["libssl3t64:arm64"].__setitem__(1, "iU ")}
        for label, damage in damages.items():
            with self.subTest(damage=label):
                self.rows = snapshot_rows()
                damage()
                self.assertEqual(self.packages()[0], "inventory")
                del self.rows["clang:arm64"]
                with self.assertRaisesRegex(ValueError, "^Ubuntu build package clang is not installed$"):
                    self.key()
        whole = snapshot_text(snapshot_rows())
        self.assertEqual(cache.ubuntu_packages(whole + "garbage\n")[0], "inventory")
        clang = next(line for line in whole.splitlines(keepends=True) if line.startswith("clang:arm64\t"))
        with self.assertRaisesRegex(ValueError, "^Ubuntu build package clang is not installed$"):
            self.key(text=whole.replace(clang, "") + "garbage\n")
        # A root's own line that no longer says `ii ` does not count as installed.
        with self.assertRaisesRegex(ValueError, "^Ubuntu build package clang is not installed$"):
            self.key(text=whole.replace(clang, "clang:arm64\t1:18.0-59~exp2\n"))
        # Nor does an unsupplied name that resolves turn the failure into a fallback,
        # nor a dpkg CMake beside the missing root.
        self.rows = snapshot_rows()
        del self.rows["clang:arm64"]
        with self.assertRaisesRegex(ValueError, "^Ubuntu build package clang is not installed$"):
            self.key(probes={"aws-lc": "1.61.0", "libcrypto-awslc": None})
        self.rows["cmake:arm64"] = list(APT_CMAKE)
        with self.assertRaisesRegex(ValueError, "^Ubuntu build package clang is not installed$"):
            self.key()

    def test_unsupplied_pkg_config_name_that_resolves_takes_the_whole_inventory_and_is_named(self):
        closure = self.key()
        text = snapshot_text(self.rows)
        keys = {closure}
        for probes, named in (({"aws-lc": "1.61.0", "libcrypto-awslc": None}, "aws-lc"),
                              ({"aws-lc": "1.61.1", "libcrypto-awslc": None}, "aws-lc"),
                              ({"aws-lc": "", "libcrypto-awslc": None}, "aws-lc"),
                              ({"aws-lc": None, "libcrypto-awslc": "1.61.0"}, "libcrypto-awslc"),
                              ({"aws-lc": "1.61.0", "libcrypto-awslc": "1.61.0"}, "aws-lc, libcrypto-awslc")):
            with self.subTest(probes=probes):
                key, shown = cache.ubuntu_identity(text, UBUNTU_TOOLS, "/usr/lib/pkgconfig", {}, probes, UBUNTU_LOCAL)
                self.assertEqual(shown[:3], [
                    "::warning::Ubuntu cache key fell back to the complete package inventory: "
                    f"pkg-config resolves {named}, which no build package ships",
                    "Ubuntu native key mode: inventory", "Keyed Ubuntu packages: 271 of 271 installed"])
                # Every line of the snapshot, and each name's answer.
                self.assertEqual(shown.index("::endgroup::") - shown.index("::group::Keyed Ubuntu packages"), 272)
                self.assertEqual(shown[-5:-3], [f"Keyed pkg-config probe {name}: " + ("absent" if version is None else f"version {version}")
                                                for name, version in probes.items()])
                keys.add(key)
        self.assertEqual(len(keys), 6)
        # The closure is then not all a build reads: a package outside it is keyed too.
        resolved = {"aws-lc": "1.61.0", "libcrypto-awslc": None}
        self.rows["git:arm64"][0] += "+1"
        self.assertEqual(self.key(), closure)
        self.assertNotIn(self.key(probes=resolved), keys)
        # The snapshot's own abnormality stays the first cause.
        self.depend("libssl-dev:arm64", "libprobe1 [amd64]")
        self.assertEqual(cache.ubuntu_packages(snapshot_text(self.rows), ["aws-lc"])[5],
                         "libssl-dev:arm64 has a dependency outside dpkg's grammar: 'libprobe1 [amd64]'")
        self.assertEqual(cache.ubuntu_packages(snapshot_text(self.rows), ["aws-lc"], ["PKG_CONFIG_PATH"])[5],
                         "libssl-dev:arm64 has a dependency outside dpkg's grammar: 'libprobe1 [amd64]'")

    def test_set_pkg_config_variable_takes_the_whole_inventory_and_is_named(self):
        closure = self.key()
        text = snapshot_text(self.rows)
        leads = "and a digest keys a .pc file's text, not what it leads to"
        keys = {closure}
        for directed in (["PKG_CONFIG_PATH"], ["PKG_CONFIG_LIBDIR"], ["PKG_CONFIG_SYSROOT_DIR"],
                         ["HOST_PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR_x86_64-unknown-linux-gnu"]):
            with self.subTest(directed=directed):
                mode, keyed, count, installed, others, cause = cache.ubuntu_packages(text, (), directed)
                self.assertEqual((mode, count, installed, others), ("inventory", 271, 271, 0))
                self.assertEqual(cause, f"the environment sets {', '.join(directed)}, {leads}")
                key, shown = cache.ubuntu_identity(text, UBUNTU_TOOLS, "/usr/lib/pkgconfig", {}, UBUNTU_PROBES, UBUNTU_LOCAL,
                                                   None, directed)
                self.assertEqual(shown[:3], [f"::warning::Ubuntu cache key fell back to the complete package inventory: {cause}",
                                             "Ubuntu native key mode: inventory", "Keyed Ubuntu packages: 271 of 271 installed"])
                # The mode is in the digest and the names are in the flags identity: variables alone give one fallback key.
                keys.add(key)
        self.assertEqual(len(keys), 2)
        # A package outside the closure is then keyed, as for every other cause.
        fallback = self.key(directed=["PKG_CONFIG_PATH"])
        self.rows["git:arm64"][0] += "+1"
        self.assertEqual(self.key(), closure)
        self.assertNotEqual(self.key(directed=["PKG_CONFIG_PATH"]), fallback)
        # Beside a resolved name both causes are named, and what the directories hold is keyed in either mode.
        self.assertEqual(cache.ubuntu_packages(text, ["aws-lc"], ["PKG_CONFIG_PATH"])[5],
                         f"the environment sets PKG_CONFIG_PATH, {leads}; pkg-config resolves aws-lc, which no build package ships")
        listed = {self.key(directed=["PKG_CONFIG_PATH"], pc_added=added) for added in (
            None, {"PKG_CONFIG_PATH": {}}, {"PKG_CONFIG_PATH": {"/opt/pc": []}}, {"PKG_CONFIG_LIBDIR": {"/opt/pc": []}},
            {"PKG_CONFIG_PATH": {"/opt/pc": "unreadable EACCES"}}, {"PKG_CONFIG_PATH": {"/opt/pc": [["openssl.pc", "0" * 64]]}},
            {"PKG_CONFIG_PATH": {"/opt/pc": [["openssl.pc", "1" * 64]]}}, {"PKG_CONFIG_PATH": {"/opt/other": [["openssl.pc", "1" * 64]]}})}
        self.assertEqual(len(listed), 8)
        # With no variable set the digest's input is the one a key without them had: no empty entry is hashed.
        self.assertEqual(self.key(pc_added={}), closure)
        hashed = {"packages": self.keyed(), "pc_path": "/usr/lib/pkgconfig", "pc_files": {}, "probes": UBUNTU_PROBES, "local": UBUNTU_LOCAL,
                  "tools": {name: tool and {"path": tool["path"], "version": tool["version"]} for name, tool in UBUNTU_TOOLS.items()}}
        self.rows = snapshot_rows()
        hashed["packages"] = self.keyed()
        self.assertEqual(self.key(), cache.digest(b"closure\n" + json.dumps(hashed, sort_keys=True).encode()))

    def fallbacks(self):
        """Every cause that sends the key to the inventory: label -> (snapshot, the cause the warning names)."""
        def edited(edit):
            self.rows = snapshot_rows()
            edit()
            return snapshot_text(self.rows)

        whole = snapshot_text(snapshot_rows())
        return {
            "a line without six fields": (edited(lambda: self.rows["zlib1g:arm64"].pop()), "snapshot line 270 has 5 fields, not six"),
            "a line with a seventh field": (edited(lambda: self.rows["adduser:all"].append("extra")), "snapshot line 1 has 7 fields, not six"),
            "an empty line": (whole.replace("\n", "\n\n", 1), "snapshot line 2 has 1 fields, not six"),
            "a package listed twice": (whole + whole.splitlines()[0] + "\n", "snapshot line 272 does not name one package and its version"),
            "a dependency outside the grammar": (edited(lambda: self.depend("libssl-dev:arm64", "libprobe1 [amd64]")),
                                                 "libssl-dev:arm64 has a dependency outside dpkg's grammar: 'libprobe1 [amd64]'"),
            "a substitution left in a dependency": (edited(lambda: self.depend("libzstd-dev:arm64", "${shlibs:Depends}")),
                                                    "libzstd-dev:arm64 has a dependency outside dpkg's grammar: '${shlibs:Depends}'"),
            "an empty dependency group": (edited(lambda: self.rows["clang:arm64"].__setitem__(3, "clang-18 (>= 18~),")),
                                          "clang:arm64 has a dependency outside dpkg's grammar: ''"),
            "a provided name outside the grammar": (edited(lambda: self.rows["git:arm64"].__setitem__(4, "git-core | git-all")),
                                                    "git:arm64 provides a name outside dpkg's grammar: 'git-core | git-all'"),
            "a group in which no alternative resolves": (edited(lambda: self.depend("libssl-dev:arm64", "libprobe1 | libprobe2")),
                                                         "nothing installed satisfies 'libprobe1 | libprobe2' of libssl-dev:arm64"),
            "a dependency whose package was removed": (edited(lambda: self.rows.pop("libdav1d7:arm64")),
                                                       "nothing installed satisfies 'libdav1d7' of libdav1d-dev:arm64"),
            "a closure member that is only unpacked": (edited(lambda: self.rows["libssl3t64:arm64"].__setitem__(1, "iU ")),
                                                       "libssl3t64:arm64 is in dpkg state 'iU ', not installed"),
            "an alternative whose configuration is left": (
                edited(lambda: (self.rows.__setitem__("libprobe1:arm64", ["1.0-1", "rc ", "", "", ""]),
                                self.depend("libssl-dev:arm64", "libssl3t64 | libprobe1"))),
                "libprobe1:arm64 is in dpkg state 'rc ', not installed"),
            "a second architecture that is half installed": (
                edited(lambda: self.rows.__setitem__("clang:armhf", ["1:18.0-59~exp2", "iH ", "", "", ""])),
                "clang:armhf is in dpkg state 'iH ', not installed"),
            # apt's cmake is not a root, so the closure would not key it: its line as the
            # earlier container had it, on another architecture, and removed with its configuration left.
            "a dpkg CMake": (edited(lambda: self.rows.__setitem__("cmake:arm64", list(APT_CMAKE))),
                             "the snapshot holds cmake:arm64, which no build package keys"),
            "a dpkg CMake of another architecture": (
                edited(lambda: self.rows.__setitem__("cmake:armhf", ["3.28.3-1build7", "ii ", "", "", ""])),
                "the snapshot holds cmake:armhf, which no build package keys"),
            "a dpkg CMake whose configuration is left": (
                edited(lambda: self.rows.__setitem__("cmake:arm64", ["3.28.3-1build7", "rc ", "", "", ""])),
                "the snapshot holds cmake:arm64, which no build package keys"),
            "two dpkg CMakes": (
                edited(lambda: self.rows.update({"cmake:arm64": list(APT_CMAKE), "cmake:armhf": ["3.28.3-1build7", "iU ", "", "", ""]})),
                "the snapshot holds cmake:arm64, cmake:armhf, which no build package keys"),
        }

    def test_every_abnormal_snapshot_falls_back_to_the_whole_inventory_and_says_why(self):
        closure = self.key()
        for label, (text, cause) in self.fallbacks().items():
            with self.subTest(cause=label):
                mode, keyed, count, installed, others, named = cache.ubuntu_packages(text)
                self.assertEqual((mode, named), ("inventory", cause))
                # Never a partial closure: every line of the snapshot is keyed.
                lines = text.split("\n")[:-1]
                self.assertEqual(len(keyed), len(lines))
                self.assertEqual(count + others, len(lines))
                key, shown = cache.ubuntu_identity(text, UBUNTU_TOOLS, "/usr/lib/pkgconfig", {}, UBUNTU_PROBES, UBUNTU_LOCAL)
                self.assertEqual(shown[:2], [f"::warning::Ubuntu cache key fell back to the complete package inventory: {cause}",
                                             "Ubuntu native key mode: inventory"])
                self.assertNotEqual(key, closure)
                # Any package's version changes the fallback key, in the closure or
                # not: every line for the four causes a real image could show, a
                # sample of them for the rest.
                every = label in {"a line without six fields", "a dependency outside the grammar",
                                  "a group in which no alternative resolves", "a closure member that is only unpacked",
                                  "a dpkg CMake"}
                for index, line in list(enumerate(lines))[::1 if every else 23]:
                    fields = line.split("\t")
                    if len(fields) > 1:
                        fields[1] += "+1"
                        changed = "".join(row + "\n" for row in (*lines[:index], "\t".join(fields), *lines[index + 1:]))
                        self.assertNotEqual(self.key(text=changed), key, line)

    def test_fallback_keys_status_and_can_never_equal_a_closure_key(self):
        # A snapshot of the closure alone: both modes then key the same lines.
        keyed = self.keyed()
        self.rows = {identity: fields for identity, fields in self.rows.items() if f"{identity}={fields[0]}" in keyed}
        self.assertEqual(len(self.rows), 160)
        closure = self.key()
        self.assertEqual(self.keyed(), keyed)
        self.depend("libssl-dev:arm64", "libprobe1 [amd64]")
        mode, inventory, *_ = self.packages()
        self.assertEqual((mode, inventory), ("inventory", keyed))
        fallback = self.key()
        self.assertNotEqual(fallback, closure)
        # The fallback keys every line's status too.
        self.rows["perl:arm64"][1] = "hi "
        held = self.packages()
        self.assertIn(f"perl:arm64={self.rows['perl:arm64'][0]} [hi ]", held[1])
        self.assertEqual(held[2:5], (159, 159, 1))
        self.assertNotEqual(self.key(), fallback)
        # A line that names no package is keyed whole.
        text = snapshot_text(self.rows) + "garbage\n"
        self.assertIn("unparsed: garbage", cache.ubuntu_packages(text)[1])
        self.assertNotEqual(self.key(text=text), self.key())

    def test_tools_and_pkg_config_view_are_part_of_the_key(self):
        baseline = self.key()
        keys = {baseline}
        for name in TOOL_NAMES:
            for field, value in (("path", f"/usr/local/bin/{name}"), ("version", f"{name} 13.1\n")):
                with self.subTest(tool=name, changed=field):
                    keys.add(self.key(tools={**UBUNTU_TOOLS, name: {**UBUNTU_TOOLS[name], field: value}}))
        keys.add(self.key(tools={**UBUNTU_TOOLS, "cmake3": {**UBUNTU_TOOLS["cmake"], "matches": ["/usr/bin/cmake3"]}}))
        keys.add(self.key(pc_path="/usr/local/lib/pkgconfig:/usr/lib/pkgconfig"))
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": []}))
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": "unreadable EACCES"}))
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": [["openssl.pc", "0" * 64]]}))
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": [["openssl.pc", "1" * 64]]}))
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": [["openssl.pc", "unreadable EACCES"]]}))
        for directory in UBUNTU_LOCAL:
            for found in ("missing", "unreadable EACCES", ["libprobe.a 5"], ["libprobe.a 6"]):
                keys.add(self.key(local={**UBUNTU_LOCAL, directory: found}))
        self.assertEqual(len(keys), 1 + 2 * len(TOOL_NAMES) + 7 + 8)
        self.assertEqual(len(TOOL_NAMES), 9)

    def test_no_keyed_value_can_form_a_workflow_command(self):
        self.assertEqual(cache.shown("a##[b\n::c\x1b[0m\u2028d"), "a#\\x23[b\\n::c\\x1b[0m\\u2028d")
        self.assertEqual(cache.shown("###[####["), "##\\x23[###\\x23[")
        # A command in every value the step prints: one a line of its own would
        # carry, and the older spelling the runner reads anywhere in a line.
        hostile = "##[error]stop\n::error::stop\r##[set-output name=x;]y"
        escaped = r"#\x23[error]stop\n::error::stop\r#\x23[set-output name=x;]y"
        rows = snapshot_rows()
        rows["libssl3t64:arm64"][1] = "##[error]state"
        # The snapshot's last two lines name no package: `##[error]stop` and the rest.
        text = snapshot_text(rows) + hostile + "\n"
        tools = {name: tool and {"run": hostile, "path": hostile, "matches": [hostile, hostile], "version": hostile}
                 for name, tool in UBUNTU_TOOLS.items()}
        _, lines = cache.ubuntu_identity(text, tools, hostile, {hostile: [[hostile, hostile]], "/opt": hostile},
                                         {"aws-lc": hostile, "libcrypto-awslc": None}, {hostile: [hostile], "/usr/local/lib": hostile},
                                         {hostile: {hostile: [[hostile, hostile]], "/opt": hostile}}, [hostile])
        # Nothing a value holds starts a line: the fixed lines around 273 keyed ones, and a line for each other value.
        self.assertEqual(len(lines), 4 + 273 + 1 + len(UBUNTU_TOOLS) + 1 + 2 + 2 + 2 + 2 + 1)
        self.assertEqual([line for line in lines if "##[" in line or "\n" in line or "\r" in line], [])
        self.assertEqual([line.split(" ")[0] for line in lines if line.lstrip().startswith("::")],
                         ["::warning::Ubuntu", "::group::Keyed", "::endgroup::"])
        # Each value is still there to read, escaped.
        self.assertIn(r"libssl3t64:arm64=3.0.13-0ubuntu3.16 [#\x23[error]state]", lines)
        self.assertLessEqual({r"unparsed: #\x23[error]stop", r"unparsed: ::error::stop\r#\x23[set-output name=x;]y"}, set(lines))
        self.assertEqual(sum(escaped in line for line in lines), len(TOOL_NAMES) + 8)
        self.assertIn(f"Keyed pkg-config files in /opt, which {escaped} names: {escaped}", lines)
        # A variable's name is a value too, in the line and in the cause.
        _, directed = cache.ubuntu_identity(snapshot_text(snapshot_rows()), UBUNTU_TOOLS, "/usr/lib/pkgconfig", {}, UBUNTU_PROBES,
                                            UBUNTU_LOCAL, None, [hostile])
        self.assertEqual(directed[0], "::warning::Ubuntu cache key fell back to the complete package inventory: "
                                      f"the environment sets {escaped}, and a digest keys a .pc file's text, not what it leads to")
        self.assertEqual([line for line in directed if "##[" in line or "\n" in line or "\r" in line], [])
        self.assertIn(f"Keyed pkg-config probe aws-lc: version {escaped}", lines)
        # The cause of a fallback is a value too.
        rows = snapshot_rows()
        rows["libssl-dev:arm64"][3] += ", ##[error]stop"
        _, lines = cache.ubuntu_identity(snapshot_text(rows), UBUNTU_TOOLS, "/usr/lib/pkgconfig", {}, UBUNTU_PROBES, UBUNTU_LOCAL)
        self.assertEqual(lines[0], "::warning::Ubuntu cache key fell back to the complete package inventory: "
                                   r"libssl-dev:arm64 has a dependency outside dpkg's grammar: '#\x23[error]stop'")
        self.assertEqual([line for line in lines if "##[" in line], [])

    def test_install_line_names_only_build_packages_and_tools(self):
        text = (ROOT / ".github/actions/setup-rust/action.yml").read_text()
        command = re.search(r"sudo apt-get install ((?:[^\n]*\\\n)+[^\n]*)\n", text).group(1)
        installed = [word for word in command.replace("\\", " ").split() if not word.startswith("-")]
        self.assertEqual(len(installed), len(set(installed)))
        self.assertEqual(text.count("apt-get install"), 1)
        build, tools = set(cache.UBUNTU_BUILD_PACKAGES), set(cache.UBUNTU_TOOL_PACKAGES)
        # Every installed name is in exactly one of the two lists, and a root
        # the action does not install would fail the step.
        self.assertFalse(build & tools)
        self.assertEqual(set(installed), build | tools)
        self.assertIn("--no-install-recommends", command.split())
        self.assertEqual(len(cache.UBUNTU_BUILD_PACKAGES), 11)
        # No CMake is installed, by apt or by anything else the action could run
        # on a condition: the installed set must not depend on the image. The
        # image's own stays a required keyed tool, so one without it fails by name.
        self.assertNotIn("cmake", text.lower())
        self.assertNotIn("cmake", build | tools)
        self.assertIn("cmake", cache.UBUNTU_TOOLS)
        self.assertEqual((cache.UBUNTU_OPTIONAL_TOOLS, cache.UBUNTU_UNKEYED_PACKAGES), (("cmake3",), ("cmake",)))

    def test_dpkg_cmake_takes_the_whole_inventory_because_the_closure_no_longer_keys_it(self):
        closure = self.key()
        self.assertEqual(self.packages()[:3], ("closure", self.keyed(), 160))
        # apt's cmake back, as the earlier container had it, with the package only it needs.
        self.rows["cmake:arm64"] = list(APT_CMAKE)
        self.rows["cmake-data:all"] = ["3.28.3-1build7", "ii ", "", "", ""]
        mode, keyed, count, installed, others, cause = self.packages()
        self.assertEqual((mode, count, installed, others), ("inventory", 273, 273, 0))
        self.assertEqual(cause, "the snapshot holds cmake:arm64, which no build package keys")
        key, shown = cache.ubuntu_identity(snapshot_text(self.rows), UBUNTU_TOOLS, "/usr/lib/pkgconfig", {}, UBUNTU_PROBES, UBUNTU_LOCAL)
        self.assertEqual(shown[:3], [f"::warning::Ubuntu cache key fell back to the complete package inventory: {cause}",
                                     "Ubuntu native key mode: inventory", "Keyed Ubuntu packages: 273 of 273 installed"])
        self.assertNotEqual(key, closure)
        # Its version is then keyed, as every other line's is; in the closure mode nothing would hold it.
        self.assertIn("cmake:arm64=3.28.3-1build7", keyed)
        self.rows["cmake:arm64"][0] = "3.28.3-1build8"
        self.assertNotEqual(self.key(), key)
        # Any state counts, and a package that only has cmake in its name does not.
        self.rows["cmake:arm64"][1] = "rc "
        self.assertEqual(self.packages()[5], cause)
        del self.rows["cmake:arm64"]
        self.assertEqual((self.packages()[0], self.key()), ("closure", closure))
        self.rows["cmake-extras:all"] = ["1.7-1", "ii ", "", "", "cmake-modules"]
        self.assertEqual((self.packages()[0], self.key()), ("closure", closure))


class UbuntuNativeIdentity(unittest.TestCase):
    """`native_identity("Linux")` against stand-in commands on PATH."""

    def setUp(self):
        (ROOT / ".local").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="cache-test-", dir=ROOT / ".local")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.runner = UbuntuRunner(self.base)

    def test_one_snapshot_feeds_the_key_and_every_printed_line_is_a_hashed_value(self):
        native, lines = self.runner.native()
        # One query, in the format every part reads.
        self.assertEqual(self.runner.queries.read_text().splitlines(), [
            "-W", r"-f=${Package}:${Architecture}\t${Version}\t${db:Status-Abbrev}\t${Pre-Depends}\t${Depends}\t${Provides}\n"])
        start, end = lines.index("::group::Keyed Ubuntu packages"), lines.index("::endgroup::")
        self.assertEqual(lines[:start], ["Ubuntu native key mode: closure", "Keyed Ubuntu packages: 160 of 271 installed"])
        keyed = lines[start + 1:end]
        self.assertEqual(keyed, cache.ubuntu_packages(UBUNTU_SNAPSHOT)[1])
        self.assertEqual((len(keyed), keyed[0], keyed[-1]), (160, "binutils-aarch64-linux-gnu:arm64=2.42-4ubuntu2.10", "zlib1g:arm64=1:1.3.dfsg-3.1ubuntu2.3"))
        tools = {}
        expected = []
        for name in TOOL_NAMES:
            real = os.path.realpath(self.runner.real / f"{name}-13")
            version = f"{name} (Ubuntu) 13.0\nconfigured for the fixture\n"
            tools[name] = {"path": real, "version": version}
            expected.append(f"Keyed tool {name}: {real}; on PATH: {self.runner.bin / name}; "
                            f"version {cache.digest(version.encode())[:12]}: {name} (Ubuntu) 13.0")
        tools["cmake3"] = None
        pc_path = f"{self.runner.pc}:/usr/lib/pkgconfig:/usr/share/pkgconfig"
        self.assertEqual(lines[end + 1:], [*expected, "Keyed tool cmake3: absent", f"Keyed pkg-config pc_path: {pc_path}",
                                           f"Keyed pkg-config files in {self.runner.pc}: none",
                                           "Keyed pkg-config probe aws-lc: absent", "Keyed pkg-config probe libcrypto-awslc: absent",
                                           *(f"Keyed top level of {directory}: empty" for directory in self.runner.local),
                                           f"Ubuntu native digest: {native}"])
        # The digest is the mode, then exactly the values the lines show.
        hashed = {"packages": keyed, "tools": tools, "pc_path": pc_path, "pc_files": {str(self.runner.pc): []},
                  "probes": {"aws-lc": None, "libcrypto-awslc": None},
                  "local": {str(directory): [] for directory in self.runner.local}}
        self.assertEqual(native, cache.digest(b"closure\n" + json.dumps(hashed, sort_keys=True).encode()))
        self.assertEqual(self.runner.native()[0], native)
        self.assertEqual(len(self.runner.queries.read_text().splitlines()), 4)

    def test_each_tool_is_keyed_by_the_file_path_selects_and_by_its_version(self):
        baseline, _ = self.runner.native()
        keys = {baseline}
        for name in TOOL_NAMES:
            with self.subTest(tool=name):
                # Another real file behind the same name, saying the same thing.
                self.runner.tool(name, variant="14", version=f"{name} (Ubuntu) 13.0\nconfigured for the fixture\n")
                keys.add(self.runner.native()[0])
                self.runner.tool(name, version=f"{name} (Ubuntu) 13.0\nconfigured otherwise\n")
                keys.add(self.runner.native()[0])
                self.runner.tool(name)
                self.assertEqual(self.runner.native()[0], baseline)
                (self.runner.bin / name).unlink()
                with self.assertRaisesRegex(ValueError, f"^build tool {re.escape(name)} is not on PATH$"):
                    self.runner.native()
                self.runner.tool(name)
        self.assertEqual(len(keys), 1 + 2 * len(TOOL_NAMES))
        # cmake3 may be absent; once it resolves, its file and version are keyed.
        self.runner.tool("cmake3")
        appeared, lines = self.runner.native()
        self.assertNotIn(appeared, keys)
        version = cache.digest(b"cmake3 (Ubuntu) 13.0\nconfigured for the fixture\n")[:12]
        self.assertIn(f"Keyed tool cmake3: {os.path.realpath(self.runner.real / 'cmake3-13')}; on PATH: {self.runner.bin / 'cmake3'}; "
                      f"version {version}: cmake3 (Ubuntu) 13.0", lines)
        self.runner.tool("cmake3", version="cmake3 version 3.31.6\n")
        self.assertNotIn(self.runner.native()[0], {*keys, appeared})

    def test_path_order_selects_the_tool_and_every_match_is_shown(self):
        baseline, _ = self.runner.native()
        local = self.base / "local"
        local.mkdir()
        # The hosted image's CMake under /usr/local, which no dpkg package owns.
        real = self.runner.tool("cmake", directory=local, variant="3.31.6", version="cmake version 3.31.6\n")
        self.runner.tool("cmake")
        first, lines = self.runner.native(local)
        self.assertNotEqual(first, baseline)
        version = cache.digest(b"cmake version 3.31.6\n")[:12]
        self.assertIn(f"Keyed tool cmake: {os.path.realpath(real)}; on PATH: {local / 'cmake'}, {self.runner.bin / 'cmake'}; "
                      f"version {version}: cmake version 3.31.6", lines)
        # Later on PATH it is shown and not run, so the key is the first CMake's.
        later, lines = self.runner.native(later=(local,))
        self.assertEqual(later, baseline)
        self.assertIn(f"on PATH: {self.runner.bin / 'cmake'}, {local / 'cmake'};", "\n".join(lines))

    def pc_lines(self, lines):
        return [line.removeprefix("Keyed pkg-config files in ") for line in lines if line.startswith("Keyed pkg-config files in ")]

    def test_pc_file_outside_the_dpkg_directories_changes_the_key(self):
        baseline, lines = self.runner.native()
        self.assertEqual(self.pc_lines(lines), [f"{self.runner.pc}: none"])
        # Only .pc files count, by name and content.
        (self.runner.pc / "notes.txt").write_text("not a module")
        self.assertEqual(self.runner.native()[0], baseline)
        (self.runner.pc / "openssl.pc").write_text("Name: OpenSSL\n")
        shadowed, lines = self.runner.native()
        self.assertNotEqual(shadowed, baseline)
        self.assertEqual(self.pc_lines(lines), [f"{self.runner.pc}: openssl.pc {hashlib.sha256(b'Name: OpenSSL' + bytes([10])).hexdigest()}"])
        (self.runner.pc / "openssl.pc").write_text("Name: OpenSSL\nVersion: 4\n")
        (self.runner.pc / "dav1d.pc").symlink_to("missing.pc")
        grown, lines = self.runner.native()
        self.assertNotIn(grown, {baseline, shadowed})
        # A dangling link still occupies the name pkg-config looks up.
        version = hashlib.sha256(b"Name: OpenSSL\nVersion: 4\n").hexdigest()
        self.assertEqual(self.pc_lines(lines), [f"{self.runner.pc}: dav1d.pc unreadable ENOENT, openssl.pc {version}"])
        # The search order is keyed, a directory dpkg owns is not listed, and one that does not exist is empty.
        other = self.base / "elsewhere"
        self.runner.pc_path.write_text(f"/usr/lib/pkgconfig:/usr/share/pkgconfig:{self.runner.pc}:{other}\n")
        reordered, lines = self.runner.native()
        self.assertNotIn(reordered, {baseline, shadowed, grown})
        self.assertEqual(self.pc_lines(lines), [f"{self.runner.pc}: dav1d.pc unreadable ENOENT, openssl.pc {version}", f"{other}: none"])

    def pc_files(self):
        """The .pc files `pkg_config_view` finds through the stand-in pkg-config, by directory."""
        order, files, added = cache.pkg_config_view(str(self.runner.bin / "pkg-config"), {})
        self.assertEqual(order, self.runner.pc_path.read_text().strip())
        self.assertEqual(added, {})
        return {Path(directory): found for directory, found in files.items()}

    def test_directories_a_pkg_config_variable_names_are_listed_by_content_under_its_name(self):
        closure, clean = self.runner.native()
        self.assertEqual(clean[0], "Ubuntu native key mode: closure")
        first, second, absent, blocked = (self.base / name for name in ("first", "second", "absent", "blocked"))
        first.mkdir()
        second.mkdir()
        (first / "openssl.pc").write_text("Name: OpenSSL\nVersion: 1.2.3\n")
        (first / "notes.txt").write_text("not a module")
        (second / "dav1d.pc").symlink_to("missing.pc")
        blocked.write_text("a file where pkg-config looks for a directory")
        digest = hashlib.sha256(b"Name: OpenSSL\nVersion: 1.2.3\n").hexdigest()
        tool = str(self.runner.bin / "pkg-config")
        # Every form the pkg-config crate maps to the two variables, each under its own name; an empty one is not set.
        triple = "x86_64-unknown-linux-gnu"
        variables = {"PKG_CONFIG_PATH": f"{first}{os.pathsep}{absent}{os.pathsep}", "PKG_CONFIG_LIBDIR": str(second),
                     "HOST_PKG_CONFIG_PATH": str(blocked), "TARGET_PKG_CONFIG_LIBDIR": str(first),
                     f"PKG_CONFIG_PATH_{triple}": str(second), f"PKG_CONFIG_LIBDIR_{triple.replace('-', '_')}": str(absent),
                     "HOST_PKG_CONFIG_LIBDIR": "", "PKG_CONFIG_TOP_BUILD_DIR": str(first), "PKG_CONFIG_SYSROOT_DIR": str(first),
                     "PKG_CONFIG": tool, "HOST_PKG_CONFIG_PATHS": str(first), "XPKG_CONFIG_PATH": str(first)}
        listed = [["openssl.pc", digest]]
        dangling = [["dav1d.pc", "unreadable ENOENT"]]
        expected = {"HOST_PKG_CONFIG_PATH": {str(blocked): "unreadable ENOTDIR"}, "PKG_CONFIG_LIBDIR": {str(second): dangling},
                    f"PKG_CONFIG_LIBDIR_{triple.replace('-', '_')}": {str(absent): []},
                    "PKG_CONFIG_PATH": {str(first): listed, str(absent): []}, f"PKG_CONFIG_PATH_{triple}": {str(second): dangling},
                    "TARGET_PKG_CONFIG_LIBDIR": {str(first): listed}}
        order, files, added = cache.pkg_config_view(tool, variables)
        self.assertEqual((order, files), (self.runner.pc_path.read_text().strip(), {str(self.runner.pc): []}))
        self.assertEqual(added, expected)
        self.assertEqual(list(added), sorted(expected))
        native, lines = self.runner.native(variables=variables)
        self.assertNotEqual(native, closure)
        named = [line for line in lines if ", which " in line]
        self.assertEqual(named, [
            f"Keyed pkg-config files in {blocked}, which HOST_PKG_CONFIG_PATH names: unreadable ENOTDIR",
            f"Keyed pkg-config files in {second}, which PKG_CONFIG_LIBDIR names: dav1d.pc unreadable ENOENT",
            f"Keyed pkg-config files in {absent}, which PKG_CONFIG_LIBDIR_x86_64_unknown_linux_gnu names: none",
            f"Keyed pkg-config files in {first}, which PKG_CONFIG_PATH names: openssl.pc {digest}",
            f"Keyed pkg-config files in {absent}, which PKG_CONFIG_PATH names: none",
            f"Keyed pkg-config files in {second}, which PKG_CONFIG_PATH_{triple} names: dav1d.pc unreadable ENOENT",
            f"Keyed pkg-config files in {first}, which TARGET_PKG_CONFIG_LIBDIR names: openssl.pc {digest}"])
        # Beside the built-in path's own lines, which stay as they were, and before the probes.
        self.assertEqual(lines[lines.index(named[0]) - 1], f"Keyed pkg-config files in {self.runner.pc}: none")
        self.assertEqual(lines[lines.index(named[-1]) + 1], "Keyed pkg-config probe aws-lc: absent")
        self.assertIn(f"Keyed pkg-config pc_path: {self.runner.pc}:/usr/lib/pkgconfig:/usr/share/pkgconfig", lines)
        # The digest holds the listing: the mode, then the values of a clean key and the variables' directories.
        hashed = json.loads(json.dumps({"packages": cache.ubuntu_packages(UBUNTU_SNAPSHOT, (), ["x"])[1], "pc_added": expected,
                                        "pc_path": order, "pc_files": files, "probes": UBUNTU_PROBES,
                                        "local": {str(directory): [] for directory in self.runner.local},
                                        "tools": {name: {"path": os.path.realpath(self.runner.real / f"{name}-13"),
                                                         "version": f"{name} (Ubuntu) 13.0\nconfigured for the fixture\n"}
                                                  for name in TOOL_NAMES} | {"cmake3": None}}))
        self.assertEqual(native, cache.digest(b"inventory\n" + json.dumps(hashed, sort_keys=True).encode()))
        # A file's content, a new file and a directory that appears each move the key; anything but a .pc file does not.
        keys = {closure, native}
        (first / "notes.txt").write_text("still not a module")
        self.assertEqual(self.runner.native(variables=variables)[0], native)
        for change in (lambda: (first / "openssl.pc").write_text("Name: OpenSSL\nVersion: 1.2.4\n"),
                       lambda: (second / "zstd.pc").write_text("Name: zstd\n"),
                       lambda: (absent.mkdir(), (absent / "x11.pc").write_text("Name: X11\n"))):
            change()
            changed = self.runner.native(variables=variables)[0]
            self.assertNotIn(changed, keys)
            keys.add(changed)
        # A directory the built-in path also holds is listed under the variable all the same,
        # and one that dpkg owns is: the variable is the job's own choice.
        (self.runner.pc / "openssl.pc").write_text("Name: OpenSSL\n")
        builtin = hashlib.sha256(b"Name: OpenSSL\n").hexdigest()
        _, files, added = cache.pkg_config_view(tool, {"PKG_CONFIG_LIBDIR": f"{self.runner.pc}{os.pathsep}/usr/share/pkgconfig"})
        self.assertEqual(files, {str(self.runner.pc): [["openssl.pc", builtin]]})
        self.assertEqual(added["PKG_CONFIG_LIBDIR"][str(self.runner.pc)], [["openssl.pc", builtin]])
        self.assertEqual(list(added["PKG_CONFIG_LIBDIR"]), [str(self.runner.pc), "/usr/share/pkgconfig"])

    def test_set_pkg_config_variable_falls_back_and_an_unset_one_changes_nothing(self):
        closure, clean = self.runner.native()
        directory = self.base / "added"
        directory.mkdir()
        warning = "::warning::Ubuntu cache key fell back to the complete package inventory: the environment sets {}, " \
                  "and a digest keys a .pc file's text, not what it leads to"
        fallbacks = set()
        for name in ("PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR", "HOST_PKG_CONFIG_PATH", "TARGET_PKG_CONFIG_PATH",
                     "HOST_PKG_CONFIG_LIBDIR", "TARGET_PKG_CONFIG_LIBDIR", "HOST_PKG_CONFIG_SYSROOT_DIR", "TARGET_PKG_CONFIG_SYSROOT_DIR",
                     "PKG_CONFIG_PATH_aarch64-unknown-linux-gnu", "PKG_CONFIG_LIBDIR_aarch64_unknown_linux_gnu",
                     "PKG_CONFIG_SYSROOT_DIR_aarch64_unknown_linux_gnu"):
            with self.subTest(variable=name):
                native, lines = self.runner.native(variables={name: str(directory)})
                # The name, never the value.
                self.assertEqual(lines[:3], [warning.format(name), "Ubuntu native key mode: inventory",
                                             "Keyed Ubuntu packages: 271 of 271 installed"])
                self.assertNotEqual(native, closure)
                listed = [line for line in lines if ", which " in line]
                self.assertEqual(listed, [] if "SYSROOT" in name else
                                 [f"Keyed pkg-config files in {directory}, which {name} names: none"])
                fallbacks.add(native)
        # The sysroot forms list nothing, so they share one fallback digest: their values are in the flags identity.
        self.assertEqual(len(fallbacks), 8 + 1)
        native, lines = self.runner.native(variables={"PKG_CONFIG_PATH": str(directory), "PKG_CONFIG_SYSROOT_DIR": "/sysroot"})
        self.assertEqual(lines[0], warning.format("PKG_CONFIG_PATH, PKG_CONFIG_SYSROOT_DIR"))
        # Nothing changes when none is set, or when one is set and empty, or for a variable that is keyed by value only.
        for variables in ({}, {"PKG_CONFIG_PATH": "", "PKG_CONFIG_LIBDIR": "", "PKG_CONFIG_SYSROOT_DIR": ""},
                          {"PKG_CONFIG_TOP_BUILD_DIR": str(directory)}, {"PKG_CONFIG_ALLOW_CROSS": "1", "PKG_CONFIG_ALL_STATIC": "1"},
                          {"XPKG_CONFIG_PATH": str(directory), "HOST_PKG_CONFIG_PATHS": str(directory)}):
            with self.subTest(variables=variables):
                self.assertEqual(self.runner.native(variables=variables), (closure, clean))
        # Those are keyed by value in the flags identity, the directory variables too.
        for name in ("PKG_CONFIG_TOP_BUILD_DIR", "PKG_CONFIG_PATH", "HOST_PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR"):
            self.assertNotEqual(cache.flags_identity({name: str(directory)}), cache.flags_identity({}))
        self.assertEqual((cache.PKG_CONFIG_DIRECTORIES, cache.PKG_CONFIG_SYSROOT),
                         (("PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR"), "PKG_CONFIG_SYSROOT_DIR"))

    def test_pc_files_are_keyed_by_content_and_what_cannot_be_read_is_never_empty(self):
        pc, module = self.runner.pc, self.runner.pc / "openssl.pc"
        module.write_text("Name: OpenSSL\nVersion: 1.2.3\n")
        first = hashlib.sha256(module.read_bytes()).hexdigest()
        self.assertEqual(self.pc_files(), {pc: [["openssl.pc", first]]})
        # An edit that keeps the size.
        module.write_text("Name: OpenSSL\nVersion: 1.2.4\n")
        edited = hashlib.sha256(module.read_bytes()).hexdigest()
        self.assertNotEqual(edited, first)
        self.assertEqual(self.pc_files(), {pc: [["openssl.pc", edited]]})
        # A name pkg-config would find and nothing could read, each with its reason.
        module.unlink()
        module.mkdir()
        (pc / "loop.pc").symlink_to("loop.pc")
        states = [["loop.pc", "unreadable ELOOP"], ["openssl.pc", "not a regular file"]]
        if os.geteuid():
            (pc / "closed.pc").write_text("Name: closed\n")
            (pc / "closed.pc").chmod(0)
            states.insert(0, ["closed.pc", "unreadable EACCES"])
        self.assertEqual(self.pc_files(), {pc: states})
        # A search directory that is there and cannot be listed is not an empty one.
        absent, blocked = self.base / "absent", self.base / "blocked"
        self.runner.pc_path.write_text(f"{pc}:{absent}:{blocked}\n")
        self.assertEqual(self.pc_files(), {pc: states, absent: [], blocked: []})
        blocked.write_text("a file where pkg-config looks for a directory")
        self.assertEqual(self.pc_files(), {pc: states, absent: [], blocked: "unreadable ENOTDIR"})
        empty, _ = self.runner.native()
        if os.geteuid():
            blocked.unlink()
            blocked.mkdir()
            (blocked / "openssl.pc").write_text("Name: OpenSSL\n")
            blocked.chmod(0)
            self.addCleanup(blocked.chmod, 0o700)
            self.assertEqual(self.pc_files(), {pc: states, absent: [], blocked: "unreadable EACCES"})
        # Each state is in the key and in the lines as it is.
        native, lines = self.runner.native()
        self.assertEqual(native == empty, not os.geteuid())
        listed = ", ".join(f"{name} {state}" for name, state in states)
        self.assertEqual(self.pc_lines(lines), [f"{pc}: {listed}", f"{absent}: none",
                                                f"{blocked}: unreadable {'EACCES' if os.geteuid() else 'ENOTDIR'}"])

    def test_real_path_decides_by_whole_components_which_directories_dpkg_owns(self):
        usr = Path(os.path.realpath(self.base)) / "usr"
        owned, linked, opt = usr / "lib/aarch64-linux-gnu/pkgconfig", usr / "lib/linked/pkgconfig", self.base / "opt/pkgconfig"
        lib64, libexec, share, local = usr / "lib64/pkgconfig", usr / "libexec/pkgconfig", usr / "share", self.base / "local/pkgconfig"
        for directory in (owned, opt, lib64, libexec, share, linked.parent, local.parent):
            directory.mkdir(parents=True)
            if directory.name in {"pkgconfig", "share"}:
                (directory / f"{directory.parent.name}.pc").write_text("Name: probe\n")
        # A link from under /usr/lib to another place, and one from another place into it.
        linked.symlink_to(opt)
        local.symlink_to(owned)
        self.runner.pc_path.write_text(":".join(map(str, (owned, linked, lib64, libexec, share, local))) + "\n")
        state = hashlib.sha256(b"Name: probe\n").hexdigest()
        with patch.object(cache, "DPKG_PC_ROOTS", (str(usr / "lib"), str(usr / "share"))):
            # Listed: behind the link, and beside /usr/lib. Not listed: under it, the root itself, and a link into it.
            self.assertEqual(self.pc_files(), {linked: [["opt.pc", state]], lib64: [["lib64.pc", state]],
                                               libexec: [["libexec.pc", state]]})
            baseline, lines = self.runner.native()
            self.assertEqual(self.pc_lines(lines), [f"{linked}: opt.pc {state}", f"{lib64}: lib64.pc {state}",
                                                    f"{libexec}: libexec.pc {state}"])
            # A file dpkg owns does not move the key; one behind the link does.
            (owned / "added.pc").write_text("Name: added\n")
            (share / "added.pc").write_text("Name: added\n")
            self.assertEqual(self.runner.native()[0], baseline)
            (opt / "added.pc").write_text("Name: added\n")
            self.assertNotEqual(self.runner.native()[0], baseline)

    def test_each_unsupplied_name_is_asked_of_the_selected_pkg_config_and_its_answer_is_keyed(self):
        closure, lines = self.runner.native()
        selected = str(self.runner.bin / "pkg-config")
        asked = [f"{selected} --modversion {name}" for name in ("aws-lc", "libcrypto-awslc")]
        self.assertEqual(self.runner.probes.read_text().splitlines(), asked)
        self.assertEqual(lines[0], "Ubuntu native key mode: closure")
        self.assertEqual(lines[-5:-3], ["Keyed pkg-config probe aws-lc: absent", "Keyed pkg-config probe libcrypto-awslc: absent"])
        # Status 1 is pkg-config's own answer for a name it cannot resolve; success is its version, whatever it is.
        for action, answer in (("exit 1", None), ("echo 1.61.0", "1.61.0"), ("printf '1.61.0\\n\\n'", "1.61.0\n"), ("true", "")):
            with self.subTest(action=action):
                self.runner.probe("libcrypto-awslc", action)
                self.assertEqual(cache.pkg_config_probe(selected, "libcrypto-awslc"), answer)
                self.assertIsNone(cache.pkg_config_probe(selected, "aws-lc"))
        self.runner.probe("libcrypto-awslc", None)
        # Any other failure status (a wrapper that cannot run it says 126 or 127) is no answer: the step fails.
        for status in (2, 126, 127):
            with self.subTest(status=status):
                self.runner.probe("aws-lc", f"exit {status}")
                with self.assertRaises(subprocess.CalledProcessError) as unanswered:
                    cache.pkg_config_probe(selected, "aws-lc")
                self.assertEqual(unanswered.exception.returncode, status)
        self.runner.probe("aws-lc", None)
        keys = {closure}
        for name, version in (("aws-lc", "1.61.0"), ("aws-lc", "1.61.1"), ("libcrypto-awslc", "1.61.0")):
            with self.subTest(resolves=name, version=version):
                self.runner.probe(name, f"echo {version}")
                native, lines = self.runner.native()
                self.assertEqual(lines[:3], [
                    "::warning::Ubuntu cache key fell back to the complete package inventory: "
                    f"pkg-config resolves {name}, which no build package ships",
                    "Ubuntu native key mode: inventory", "Keyed Ubuntu packages: 271 of 271 installed"])
                self.assertIn(f"Keyed pkg-config probe {name}: version {version}", lines[-5:-3])
                keys.add(native)
                self.runner.probe(name, None)
        self.assertEqual(len(keys), 4)
        # The pkg-config that PATH selects is the one asked.
        local = self.base / "local"
        local.mkdir()
        self.runner.tool("pkg-config", directory=local, variant="2")
        self.runner.tool("pkg-config")
        self.runner.probes.unlink()
        self.runner.native(local)
        self.assertEqual(self.runner.probes.read_text().splitlines(),
                         [line.replace(str(self.runner.bin), str(local)) for line in asked])
        # A pkg-config that is killed gave no answer: the step fails, as for any other command.
        self.runner.probe("aws-lc", "kill -KILL $$")
        with self.assertRaises(subprocess.CalledProcessError) as killed:
            self.runner.native()
        self.assertEqual((killed.exception.returncode, killed.exception.cmd[1:]), (-9, ["--modversion", "aws-lc"]))

    def test_top_level_of_usr_local_is_a_tripwire_for_a_changed_image_and_not_a_content_key(self):
        include, lib = self.runner.local
        directories = (str(include), str(lib))
        self.assertEqual(cache.UBUNTU_LOCAL_DIRECTORIES, ("/usr/local/include", "/usr/local/lib"))

        def listing(expected, directory=lib):
            found = cache.local_listing(directories)
            self.assertEqual(list(found), list(directories))
            self.assertEqual(found[str(directory)], expected)

        def lines():
            native, shown = self.runner.native()
            return native, [line for line in shown if line.startswith("Keyed top level of ")]

        baseline, shown = lines()
        self.assertEqual(shown, [f"Keyed top level of {include}: empty", f"Keyed top level of {lib}: empty"])
        listing([])
        # Each name, sorted: a regular file with its size, a directory with a slash, a link with its target.
        (lib / "python3.12").mkdir()
        (lib / "libprobe.so").symlink_to("libprobe.so.1")
        (lib / "libprobe.a").write_bytes(b"12345")
        listing(["libprobe.a 5", "libprobe.so -> libprobe.so.1", "python3.12/"])
        listed, shown = lines()
        self.assertNotEqual(listed, baseline)
        self.assertEqual(shown, [f"Keyed top level of {include}: empty",
                                 f"Keyed top level of {lib}: libprobe.a 5, libprobe.so -> libprobe.so.1, python3.12/"])
        # Top level only, and no content: neither moves the listing.
        (lib / "libprobe.a").write_bytes(b"67890")
        (lib / "python3.12/dist-packages").mkdir()
        (lib / "python3.12/sitecustomize.py").write_text("import os\n")
        listing(["libprobe.a 5", "libprobe.so -> libprobe.so.1", "python3.12/"])
        # A size and a target do.
        (lib / "libprobe.a").write_bytes(b"678901")
        (lib / "libprobe.so").unlink()
        (lib / "libprobe.so").symlink_to("libprobe.so.2")
        listing(["libprobe.a 6", "libprobe.so -> libprobe.so.2", "python3.12/"])
        # A link to a directory is a link, and anything else says what it is not.
        (lib / "linked").symlink_to("python3.12")
        os.mkfifo(lib / "queue")
        listing(["libprobe.a 6", "libprobe.so -> libprobe.so.2", "linked -> python3.12", "python3.12/",
                 "queue (not a file, a directory or a link)"])
        # The headers' directory is listed the same way.
        listing([], include)
        (include / "openssl").mkdir()
        (include / "probe.h").write_text("#define PROBE 1\n")
        listing(["openssl/", "probe.h 16"], include)
        # A directory that is not there is missing; one that cannot be listed is not empty.
        shutil.rmtree(lib)
        listing("missing")
        lib.write_text("a file where the linker looks for a directory")
        listing("unreadable ENOTDIR")
        lib.unlink()
        lib.mkdir()
        if os.geteuid():
            (lib / "libprobe.a").write_bytes(b"12345")
            lib.chmod(0)
            self.addCleanup(lib.chmod, 0o700)
            listing("unreadable EACCES")
        refused, shown = lines()
        self.assertNotIn(refused, {baseline, listed})
        self.assertEqual(shown, [f"Keyed top level of {include}: openssl/, probe.h 16",
                                 f"Keyed top level of {lib}: {'unreadable EACCES' if os.geteuid() else 'empty'}"])

    def test_abnormal_snapshot_warns_first_and_keys_the_whole_inventory(self):
        closure, _ = self.runner.native()
        rows = snapshot_rows()
        rows["libssl3t64:arm64"][1] = "iU "
        self.runner.snapshot.write_text(snapshot_text(rows))
        native, lines = self.runner.native()
        self.assertNotEqual(native, closure)
        self.assertEqual(lines[:4], [
            "::warning::Ubuntu cache key fell back to the complete package inventory: libssl3t64:arm64 is in dpkg state 'iU ', not installed",
            "Ubuntu native key mode: inventory", "Keyed Ubuntu packages: 270 of 270 installed, and 1 other snapshot lines",
            "::group::Keyed Ubuntu packages"])
        self.assertEqual(lines.index("::endgroup::"), 4 + 271)
        self.assertIn("libssl3t64:arm64=3.0.13-0ubuntu3.16 [iU ]", lines)
        self.assertIn("git:arm64=" + rows["git:arm64"][0], lines)
        # A snapshot that cannot be taken fails the step, as a failing command always did.
        self.runner.script(self.runner.bin / "dpkg-query", "exit 2")
        with self.assertRaises(subprocess.CalledProcessError):
            self.runner.native()


class FlagsIdentity(unittest.TestCase):
    """The environment part of the key: what cc, cmake, pkg-config and the compilers read, by name."""

    def changes(self, name, value="value"):
        return cache.flags_identity({name: value}) != cache.flags_identity({})

    def test_every_form_the_three_crates_build_changes_the_digest(self):
        targeted = [name for names in FLAG_TARGETED.values() for name in names]
        self.assertEqual(len(targeted), 21)
        forms = [form for name in targeted for form in (
            name, f"HOST_{name}", f"TARGET_{name}", f"{name}_x86_64-unknown-linux-gnu", f"{name}_x86_64_unknown_linux_gnu",
            f"{name}_thumbv8m_main_none_eabi")]
        forms += [name for names in FLAG_SINGLE.values() for name in names]
        forms += [*FLAG_CALLER_FORMS, *FLAG_OTHERS]
        self.assertEqual(len(forms), len(set(forms)))
        digests = {}
        for name in forms:
            with self.subTest(variable=name):
                self.assertTrue(self.changes(name))
                digests[name] = cache.flags_identity({name: "value", "HOSTNAME": "runner"})
                # By value too, and a variable that is set and empty is not set.
                self.assertNotEqual(cache.flags_identity({name: "other"}), digests[name])
                self.assertFalse(self.changes(name, ""))
        # Each by its own name: no two of them share a digest.
        self.assertEqual(len(set(digests.values())), len(forms))
        # The compilers' own search paths, which nothing declares.
        self.assertLessEqual({"CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH"}, cache.FLAG_NAMES)

    def test_names_that_only_begin_like_a_form_leave_the_digest_unchanged(self):
        # A runner's own variables: the host forms are exact names, never a prefix.
        for name in ("HOSTNAME", "HOST_FOO", "TARGET_FOO", "HOSTTYPE", "HOST_", "TARGET_", "HOST_CC_x86_64_unknown_linux_gnu",
                     "TARGET_CFLAGS_x86_64-unknown-linux-gnu", "HOST_PKG_CONFIG_PATH_x86_64_unknown_linux_gnu", "TARGET_DIR",
                     "HOST_CMAKE_BUILD_PARALLEL_LEVEL", "DEPLOYMENT_BASEPATH", "DEP_ROOT", "STATIC", "DYNAMIC", "NO_PKG_CONFIG",
                     "ARCH", "CCACHE_DIR", "RANLIBX", "CARGO_TARGET_DIR", "CARGO_HOME", "CARGO_TERM_COLOR"):
            with self.subTest(variable=name):
                self.assertFalse(self.changes(name))
        for name in ("HOST_FOO", "TARGET_FOO"):
            self.assertFalse(name.startswith(cache.FLAG_PREFIXES))
            self.assertIsNone(cache.FLAG_FORMS.fullmatch(name))
        self.assertEqual([prefix for prefix in cache.FLAG_PREFIXES if "HOST_X".startswith(prefix) or "TARGET_X".startswith(prefix)
                          or prefix.startswith(("HOST", "TARGET"))], [])
        # Out by decision: what Cargo sets for every build script, PATH, and what a build script declares itself.
        for name in (*FLAG_CARGO_SETS, "AWS_LC_SYS_CMAKE_BUILDER", "AWS_LC_SYS_NO_ASM", "LIBCLANG_PATH", "BINDGEN_EXTRA_CLANG_ARGS",
                     "SYSTEM_DEPS_LINK", "SYSTEM_DEPS_DAV1D_BUILD_INTERNAL", "RING_PREGENERATE_ASM"):
            with self.subTest(variable=name):
                self.assertFalse(self.changes(name))

    def test_what_real_build_scripts_have_cc_and_pkg_config_read_changes_the_digest(self):
        # (declared names, those of them in the identity); then the names a script
        # declares itself that have one of pkg-config's forms and are keyed with them.
        outputs = {"ring-0.17.14.output": ((23, 13), []), "freetype-sys-0.20.1.output": ((22, 22), []),
                   "dav1d-sys-0.8.3.output": ((29, 19), ["SYSTEM_DEPS_DAV1D_NO_PKG_CONFIG"]),
                   "aws-lc-sys-0.45.0.rerun-if-env-changed": ((116, 55), ["AWS_LC_SYS_STATIC"])}
        self.assertEqual(sorted(path.name for path in BUILD_OUTPUTS.iterdir()), sorted(outputs))
        for output, (counts, overlap) in outputs.items():
            with self.subTest(output=output):
                names = declared(output)
                keyed = [name for name in names if self.changes(name)]
                # Nothing cc or pkg-config reads for the script is left out of the identity.
                self.assertEqual([name for name in names if name not in keyed and not OWN_DECLARATION.fullmatch(name)], [])
                self.assertEqual(sorted(name for name in keyed if OWN_DECLARATION.fullmatch(name)), overlap)
                self.assertEqual((len(names), len(keyed)), counts)
                self.assertNotIn("/home/" + os.environ.get("USER", "runner") + "/", (BUILD_OUTPUTS / output).read_text())
        # ring archives with cc's metadata off: what cc reads for the archiver is declared by nobody.
        ring = set(declared("ring-0.17.14.output"))
        self.assertLessEqual({"CC", "HOST_CC", "CFLAGS", "HOST_CFLAGS", "CC_aarch64-unknown-linux-gnu", "CFLAGS_aarch64_unknown_linux_gnu"}, ring)
        self.assertFalse(ring & {"AR", "HOST_AR", "ARFLAGS", "HOST_ARFLAGS", "RANLIB", "RANLIBFLAGS"})
        for name in ("AR", "HOST_AR", "ARFLAGS", "HOST_ARFLAGS", "RANLIB", "RANLIBFLAGS", "AR_aarch64-unknown-linux-gnu"):
            self.assertTrue(self.changes(name), name)

    def test_read_set_is_that_of_the_locked_cc_cmake_and_pkg_config(self):
        lock = (ROOT / "Cargo.lock").read_text()
        self.assertEqual(locked_versions(lock, FLAG_CRATES), {name: [version] for name, version in FLAG_CRATES.items()}, (
            "Cargo.lock moved cc, cmake or pkg-config, or locks a second version of one. The flags identity keys the "
            "environment variables these three read, taken from the sources of the versions in FLAG_CRATES, because "
            "Cargo reruns a build script only for the variables it declared. Read the new version again in the Cargo "
            "registry: cc's src/lib.rs (Build::target_envs and every caller of get_env, get_env_boolean, "
            "get_env_overridable, getenv_with_target_prefixes and envflags), src/utilities.rs (the callers of "
            "cargo_env_var and cargo_env_var_os), src/command_helpers.rs and src/target/; cmake's src/lib.rs "
            "(getenv_target_os, getenv_os, getenv_unwrap and env::var_os); pkg-config's src/lib.rs (targeted_env_var, "
            "env_var_os and env::var). Then bring cache.FLAG_TARGETED, cache.FLAG_NAMES and cache.FLAG_FORMS and this "
            "file's FLAG_TARGETED, FLAG_SINGLE and FLAG_CRATES in step."))
        # The check sees an update and a second locked version.
        for name, version in FLAG_CRATES.items():
            stanza = f'name = "{name}"\nversion = "{version}"\n'
            self.assertEqual(lock.count(stanza), 1)
            moved = locked_versions(lock.replace(stanza, f'name = "{name}"\nversion = "{version}1"\n'), FLAG_CRATES)
            self.assertEqual(moved[name], [f"{version}1"])
            second = locked_versions(f'{lock}\n[[package]]\nname = "{name}"\nversion = "{version}1"\n', FLAG_CRATES)
            self.assertEqual(second[name], [version, f"{version}1"])
        # The reviewed list is the helper's, name for name.
        targeted = [name for names in FLAG_TARGETED.values() for name in names]
        self.assertEqual(cache.FLAG_TARGETED, tuple(targeted))
        single = {name for names in FLAG_SINGLE.values() for name in names}
        under_prefix = {name for name in single if name.startswith(("CC_", "PKG_CONFIG"))}
        self.assertEqual(under_prefix, {"CC_KNOWN_WRAPPER_CUSTOM", "CC_SHELL_ESCAPED_FLAGS", "CC_FORCE_DISABLE", "CC_ENABLE_DEBUG_OUTPUT",
                                        "PKG_CONFIG_PATH_FOR_TARGET", "PKG_CONFIG_ALL_STATIC", "PKG_CONFIG_ALL_DYNAMIC"})
        self.assertEqual(cache.FLAG_NAMES, {*(single - under_prefix), *FLAG_OTHERS,
                                            *(kind + name for name in targeted for kind in ("", "HOST_", "TARGET_"))})
        self.assertEqual(cache.FLAG_PREFIXES, (*(name + "_" for name in targeted), "CMAKE", "PKG_CONFIG",
                                               "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_"))
        self.assertEqual(cache.FLAG_FORMS.pattern, r"DEP_.+_ROOT|.+_(?:NO_PKG_CONFIG|STATIC|DYNAMIC)")
        self.assertEqual(cache.FLAG_EXCLUDED, {"CARGO_TARGET_DIR"})


class UbuntuKeyPreconditions(unittest.TestCase):
    """What must stay true around the action for the closure to be all a build reads."""

    def test_every_locked_prober_is_mapped_to_keyed_build_packages(self):
        lock = (ROOT / "Cargo.lock").read_text()
        self.assertEqual(lock_probers(lock), set(UBUNTU_PROBERS), (
            "The locked packages that depend on pkg-config, system-deps, cmake or bindgen changed, by name or by version. "
            "The Ubuntu cache key holds only the closure of cache.UBUNTU_BUILD_PACKAGES, so read the build script of the "
            "new package or version and give it a row in UBUNTU_PROBERS: the pkg-config names it asks for on Linux, each "
            "with the build package that ships its .pc file or UNSUPPLIED, the build packages behind what it runs and "
            "any command no build package ships, which must be a required tool of cache.UBUNTU_TOOLS; "
            "or the reason it asks for nothing there. A row is for one locked version because an update can ask for new "
            "names: aws-lc-sys's aws-lc and libcrypto-awslc arrived that way, unseen. If it probes a package the setup "
            "action does not install, add that package to the action's install list and to cache.UBUNTU_BUILD_PACKAGES; "
            "add a name no package ships to cache.UBUNTU_UNSUPPLIED_PROBES. Remove the row of a package or version that "
            "is gone."))
        build, rows = set(cache.UBUNTU_BUILD_PACKAGES), snapshot_rows()
        closure = {line.split(":", 1)[0] for line in cache.ubuntu_packages(UBUNTU_SNAPSHOT)[1]}
        unsupplied, ran = set(), set()
        for prober, asked in UBUNTU_PROBERS.items():
            with self.subTest(prober=prober):
                # Cargo's own spelling of a locked name and version, a pre-release or build suffix included.
                self.assertTrue(PROBER_ROW.fullmatch(prober), prober)
                if isinstance(asked, str):
                    self.assertRegex(asked, r"\w+ \w+ \w+")
                    continue
                modules, packages, *commands = asked
                self.assertTrue(modules)
                self.assertIn("pkg-config", packages)
                self.assertLessEqual(set(packages), build)
                # A command no build package ships is run from the image: it must be a
                # required keyed tool, which fails the step when the image has none.
                for command in (name for names in commands for name in names):
                    ran.add(command)
                    self.assertNotIn(command, build)
                    self.assertIn(command, cache.UBUNTU_TOOLS, f"{prober} runs {command}, which nothing keys")
                    self.assertNotIn(command, cache.UBUNTU_OPTIONAL_TOOLS)
                for module, supplier in modules.items():
                    if supplier == UNSUPPLIED:
                        unsupplied.add(module)
                        continue
                    package, owner = supplier if isinstance(supplier, tuple) else (supplier, supplier)
                    self.assertIn(package, build, module)
                    # A .pc file in a dependency of the build package: the edge is in the real snapshot.
                    depends = {name for names in cache.relation_groups(rows[f"{package}:arm64"][3], package) for name in names}
                    self.assertIn(owner, {package, *depends}, module)
                    self.assertIn(owner, closure, module)
        # In both directions: a name the table marks is keyed, and the key asks for no name the table does not mark.
        self.assertEqual(sorted(unsupplied), sorted(cache.UBUNTU_UNSUPPLIED_PROBES))
        # aws-lc-sys is the one locked crate that can run cmake, and its row says so:
        # the name is keyed as a build package or as a required tool, never by neither.
        self.assertEqual((ran, UBUNTU_PROBERS["aws-lc-sys 0.45.0"][2]), ({"cmake"}, ("cmake",)))
        self.assertTrue("cmake" in build or "cmake" in cache.UBUNTU_TOOLS)
        # A row is one locked version: an update has none until its build script is read again,
        # and neither has a second version locked beside the first.
        name, version = next(prober for prober, asked in UBUNTU_PROBERS.items()
                             if not isinstance(asked, str) and UNSUPPLIED in asked[0].values()).split(" ")
        stanza = f'name = "{name}"\nversion = "{version}"\n'
        self.assertEqual(lock.count(stanza), 1)
        updated = lock_probers(lock.replace(stanza, f'name = "{name}"\nversion = "{version}1"\n'))
        self.assertEqual((updated - set(UBUNTU_PROBERS), set(UBUNTU_PROBERS) - updated), ({f"{name} {version}1"}, {f"{name} {version}"}))
        second = f'{lock}\n[[package]]\nname = "{name}"\nversion = "{version}1"\ndependencies = [\n "cc",\n "pkg-config",\n]\n'
        self.assertEqual(lock_probers(second) - set(UBUNTU_PROBERS), {f"{name} {version}1"})
        # A dependency added later is seen, whichever of the four crates it uses.
        for crate in sorted(PROBING_CRATES):
            with self.subTest(crate=crate):
                added = f'{lock}\n[[package]]\nname = "libprobe-sys"\nversion = "1.0.0"\ndependencies = [\n "cc",\n "{crate} 0.3.0",\n]\n'
                self.assertEqual(lock_probers(added) - set(UBUNTU_PROBERS), {"libprobe-sys 1.0.0"})
        self.assertEqual(lock_probers(f'{lock}\n[[package]]\nname = "libplain"\nversion = "1.0.0"\ndependencies = [\n "cc",\n]\n'),
                         set(UBUNTU_PROBERS))

    def test_install_step_refuses_every_system_deps_variable_before_it_installs(self):
        # dav1d-sys would otherwise clone and build dav1d with git, meson and
        # ninja, none of which the key holds.
        text = (ROOT / ".github/actions/setup-rust/action.yml").read_text()
        steps = text.split("\n    - name: ")[1:]
        install = next(index for index, step in enumerate(steps) if "apt-get install" in step)
        self.assertLess(install, next(index for index, step in enumerate(steps) if "cache.py prepare" in step))
        body = steps[install].split("      run: |\n", 1)[1].split("\n")
        script = textwrap.dedent("\n".join(body[:next(index for index, line in enumerate(body)
                                                       if line.strip() and not line.startswith(" " * 8))]))
        (ROOT / ".local").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="cache-test-", dir=ROOT / ".local") as temp:
            base = Path(temp)
            calls = base / "calls"
            for name in ("sudo", "pkg-config"):
                (base / name).write_text(f"#!/bin/sh\nprintf '%s\\n' \"{name} $*\" >> {shlex.quote(str(calls))}\n")
                (base / name).chmod(0o755)
            environment = {name: value for name, value in os.environ.items() if not name.startswith("SYSTEM_DEPS_")}
            environment["PATH"] = f"{base}{os.pathsep}{environment['PATH']}"
            allowed = subprocess.run(["bash", "-c", script], env=environment, capture_output=True, text=True)
            self.assertEqual(allowed.returncode, 0, allowed.stderr)
            ran = calls.read_text().splitlines()
            self.assertEqual(ran[0], "sudo apt-get update")
            self.assertEqual(ran[1].split()[:5], ["sudo", "apt-get", "install", "-y", "--no-install-recommends"])
            self.assertEqual(set(ran[1].split()[5:]), {*cache.UBUNTU_BUILD_PACKAGES, *cache.UBUNTU_TOOL_PACKAGES})
            calls.unlink()
            for name, value in (("SYSTEM_DEPS_DAV1D_BUILD_INTERNAL", "always"), ("SYSTEM_DEPS_LINK", "static"), ("SYSTEM_DEPS_X", "")):
                with self.subTest(variable=name):
                    refused = subprocess.run(["bash", "-c", script], env={**environment, name: value}, capture_output=True, text=True)
                    self.assertEqual(refused.returncode, 1)
                    self.assertIn(f"::error::SYSTEM_DEPS_ variables must not reach the build: {name}", refused.stdout)
                    self.assertFalse(calls.exists())

    def test_no_job_installs_packages_after_the_setup_action_took_its_snapshot(self):
        workflows = {name: (ROOT / ".github/workflows" / name).read_text() for name in ("quality.yml", "release.yml")}
        self.assertEqual(sorted(setup_jobs(workflows["quality.yml"])), ["rust-arch", "rust-clippy", "rust-debug", "rust-release"])
        self.assertEqual(sorted(setup_jobs(workflows["release.yml"])), ["build"])
        for name, text in workflows.items():
            with self.subTest(workflow=name):
                self.assertEqual(later_installs(text), [], "a job that uses the setup action installs packages itself: "
                                 "the Ubuntu cache key would no longer describe what the build reads")
        # The scan sees an installation: Quality has one, in a job without the action.
        self.assertIn("sudo apt-get install", workflows["quality.yml"])
        sample = textwrap.dedent("""\
            name: Sample
            jobs:
              scan:
                steps:
                  - run: sudo apt-get install -y python3-pil
              build:
                steps:
                  - uses: ./.github/actions/setup-rust
                  - run: sudo apt-get install -y --no-install-recommends libpng-dev
                  - run: |
                      sudo apt install nasm
                      sudo dpkg -i tool.deb
                      sudo dpkg --install other.deb
                      dpkg-query -W nasm && echo adapt
              test:
                steps:
                  - uses: ./.github/actions/setup-rust
                    with:
                      profile: debug
                  - run: /usr/bin/apt-get -y install ninja-build
            """)
        self.assertEqual(sorted(setup_jobs(sample)), ["build", "test"])
        self.assertEqual(later_installs(sample), [
            ("build", "- run: sudo apt-get install -y --no-install-recommends libpng-dev"), ("build", "sudo apt install nasm"),
            ("build", "sudo dpkg -i tool.deb"), ("build", "sudo dpkg --install other.deb"),
            ("test", "- run: /usr/bin/apt-get -y install ninja-build")])


if __name__ == "__main__":
    unittest.main()
