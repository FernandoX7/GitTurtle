#!/usr/bin/env python3
"""Lifecycle of an optional, bounded cache on fresh GitHub-hosted Rust runners."""
import argparse
import ast
from contextlib import contextmanager
import errno
import hashlib
import json
import grp
import os
import pwd
import re
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import time

PROFILES = {"debug", "release", "debug-release"}
MAX_FILES = 250_000
MAX_CLEAN_PACKAGES = 8
# Scheduling budget for starting cleanup work, then a separate allowance for
# source pruning, the verification scan and the whole-target fallback.
BUDGET_SECONDS = 90
FINALIZE_SECONDS = 90
# Logical pre-registration limits sized from hosted payloads and archive
# sizes (docs/ci.md); the combined mode is the sum of both.
PROFILE_LIMITS = {"debug": 7 * 1024**3, "release": 3584 * 1024**2, "debug-release": 10752 * 1024**2}
# Pinned upstream rust-cache SAVE_TARGETS: only library-kind target names name
# dependency artifacts. Test, example, bench, bin and build-script names do not.
LIBRARY_KINDS = {"lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"}
ARTIFACT_DIRECTORIES = {"deps", "build", ".fingerprint"}
LIBRARY_SUFFIXES = {".rlib", ".rmeta", ".a", ".so", ".dylib", ".dll"}
# Vendored path packages are the only local outputs kept: every tracked byte
# under this prefix is in the key. Their sources are rewound to Cargo's own
# deterministic registry timestamp (2006-07-24) before any Cargo command.
VENDOR_PREFIX = "vendor/"
VENDOR_MTIME_NS = 1_153_704_088 * 10**9
# The Arch container's inputs to compiled output, by pacman package: the C
# toolchain and its headers, then the libraries that build scripts probe or
# the test binaries link (aws-lc-sys probes openssl's pkg-config files, and
# dav1d-sys dav1d's). The rest of the rolling inventory stays out of the key,
# so an unrelated update does not discard the entry. Keep this in step with the
# job's install list (a test compares them).
ARCH_NATIVE_PACKAGES = (
    "binutils", "clang", "cmake", "gcc", "gcc-libs", "glibc", "linux-api-headers", "llvm-libs", "make", "pkgconf",
    "dav1d", "fontconfig", "freetype2", "libx11", "libxcb", "libxkbcommon", "libxkbcommon-x11", "openssl", "wayland",
)
# Used only at run time, by the tests or the job's pkg-config check (Vulkan and
# zstd) or as the MIME database the tests' GIO reads (shared-mime-info): no
# build script probes them and nothing links them at build time.
ARCH_RUNTIME_PACKAGES = ("shared-mime-info", "vulkan-headers", "vulkan-icd-loader", "zstd")
# The Ubuntu lanes' inputs to compiled output, by dpkg package: what a build
# compiles with, probes or links. They are the roots of the keyed dependency
# closure (ubuntu_packages), which reaches Ubuntu's version- and
# architecture-named toolchain packages without naming them. The hosted image's
# other packages, over a thousand, stay out of the key: the label is served by two
# clouds with different inventories, and a key over all of it gave one commit
# two entries. Two preconditions hold that width. A locked dependency that
# probes for a library, header or tool must find it in these packages, not in
# another package of the image. And dav1d-sys must link libdav1d-dev: its build
# script can instead clone and build dav1d with git, meson and ninja, none of
# them keyed, when a SYSTEM_DEPS_ variable asks for it, so the key relies on the
# install step's refusal of every such variable. Keep both tuples in step with
# the action's install list (a test compares them).
UBUNTU_BUILD_PACKAGES = (
    "build-essential", "clang", "pkg-config",
    "libfontconfig-dev", "libwayland-dev", "libxkbcommon-x11-dev", "libx11-xcb-dev",
    "libssl-dev", "libzstd-dev", "libvulkan-dev", "libdav1d-dev",
)
# apt's cmake is neither installed nor a root: every hosted image selects its
# own CMake under /usr/local, which the tool part keys. The closure therefore
# keys no dpkg CMake, so a snapshot that holds a package of this name, on any
# architecture and in any state, takes the complete inventory.
UBUNTU_UNKEYED_PACKAGES = ("cmake",)
# Run by the checkout, the tests or the packaging checks and never read into a
# cached dependency output: git is run only by crates/app/build.rs, a member
# whose output is never in an entry.
UBUNTU_TOOL_PACKAGES = ("git", "openssh-client", "ca-certificates", "python3", "python3-pil",
                        "desktop-file-utils", "xdg-utils")
# The one dpkg query behind the closure, the hash, the printed list and the
# fallback: no second query can see a different state.
DPKG_SNAPSHOT = ["dpkg-query", "-W", "-f=${Package}:${Architecture}\\t${Version}\\t${db:Status-Abbrev}"
                 "\\t${Pre-Depends}\\t${Depends}\\t${Provides}\\n"]
DPKG_INSTALLED = "ii "
# dpkg's own spelling of a binary relation: a name, an optional qualifier
# (`perl:any`) and an optional version constraint. Anything else is not guessed
# at; it sends the key to the complete inventory.
DPKG_NAME = r"[a-z0-9][a-z0-9+.-]+"
DPKG_VERSION = re.compile(r"[A-Za-z0-9.+~:-]+")
DPKG_PACKAGE = re.compile(rf"({DPKG_NAME}):[a-z0-9-]+")
DPKG_DEPENDENCY = re.compile(rf"({DPKG_NAME})(?::[a-z0-9-]+)?(?: \((?:<<|<=|=|>=|>>) {DPKG_VERSION.pattern}\))?")
DPKG_PROVIDED = re.compile(rf"({DPKG_NAME})(?: \(= {DPKG_VERSION.pattern}\))?")
# The names a build script or the cc and cmake crates run bare, and the linker,
# the assembler and the make that gcc and CMake take from PATH in their turn.
# Versions alone do not say which binary PATH and the alternatives select. The
# hosted image's CMake under /usr/local is no dpkg package and the action
# installs none, so this is all that keys it, and an image without one fails
# the step by name. cmake3 is the cmake crate's other name for it and may be
# absent.
UBUNTU_TOOLS = ("cc", "c++", "ar", "ld", "as", "make", "pkg-config", "clang", "cmake")
UBUNTU_OPTIONAL_TOOLS = ("cmake3",)
# pkg-config directories whose files belong to dpkg packages; a .pc file in any
# other directory of the search order could shadow a keyed library's.
DPKG_PC_ROOTS = ("/usr/lib", "/usr/share")
# Where gcc looks for headers before the system's and the linker for libraries,
# and where no file belongs to a dpkg package. Only the names at their top
# level are keyed: a tripwire for a changed image, not a key over what the
# directories hold.
UBUNTU_LOCAL_DIRECTORIES = ("/usr/local/include", "/usr/local/lib")
# pkg-config names a locked build script asks for that no build package ships.
# aws-lc-sys asks for openssl, aws-lc, libcrypto and libcrypto-awslc and links a
# system AWS-LC when one of them resolves to one; libssl-dev ships the first and
# the third. pkgconf answers a name it has no file for from any .pc whose
# Provides names it, so these two could resolve on one image and not on the
# next, and Cargo would not rerun the build script. What each resolves to is
# keyed, and one that resolves sends the key to the complete inventory: the
# closure is then not all a build reads. A test holds this tuple equal to the
# unsupplied names of the lock's probers.
UBUNTU_UNSUPPLIED_PROBES = ("aws-lc", "libcrypto-awslc")
# Variables that send pkg-config to directories of the job's own choosing: the
# pkg-config crate hands the tool the first set form of each (FLAG_TARGETED's
# forms) under the plain name. The .pc files of the directories the first two
# name are keyed by content. Any of the three that is set also sends the key to
# the complete inventory: a digest keys a .pc file's text, not the headers and
# libraries it leads to. PKG_CONFIG_TOP_BUILD_DIR only fills a variable of a
# .pc file and stays keyed by value, in the flags identity.
PKG_CONFIG_DIRECTORIES = ("PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR")
PKG_CONFIG_SYSROOT = "PKG_CONFIG_SYSROOT_DIR"
# Environment that changes what rustc, build scripts, the compilers or the cc,
# cmake and pkg-config crates emit. Each selected variable is hashed by name
# and value, and the step prints the names alone. Paths the action fixes itself
# (CARGO_HOME, CARGO_TARGET_DIR) are not flags.
#
# Cargo reruns a build script only for variables the script declared with
# rerun-if-env-changed. ring 0.17.14 archives with cc's metadata off and the
# cmake crate declares nothing, so for what they read this identity is all that
# keeps a restored object from being used under another setting. The read set
# is that of the locked cc 1.4.5, cmake 0.1.58 and pkg-config 0.3.34, taken
# from their sources; a test pins the three versions to this list.
#
# Each of the three looks a FLAG_TARGETED name up as <VAR>_<target>, as
# <VAR>_<target with underscores>, as HOST_<VAR> or TARGET_<VAR>, and as <VAR>
# (cc: Build::target_envs; cmake: getenv_target_os; pkg-config:
# targeted_env_var). The first two forms are keyed by the prefix <VAR>_, the
# others as exact names: never by a bare HOST_ or TARGET_ prefix, under which a
# runner's own variables would split the key between images.
FLAG_TARGETED = (
    "AR", "ARFLAGS", "CC", "CFLAGS", "CXX", "CXXFLAGS", "CXXSTDLIB", "NVCC", "RANLIB", "RANLIBFLAGS",
    "CMAKE", "CMAKE_GENERATOR", "CMAKE_PREFIX_PATH", "CMAKE_TOOLCHAIN_FILE", "EMCMAKE", "EMMAKE",
    "PKG_CONFIG", "PKG_CONFIG_ALLOW_CROSS", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_PATH", "PKG_CONFIG_SYSROOT_DIR",
)
# Then the names each reads in one form (cc's own CC_ settings are under the
# CC_ prefix and pkg-config's under PKG_CONFIG), what the compilers and the
# linker read themselves and nothing declares (CPATH, C_INCLUDE_PATH,
# CPLUS_INCLUDE_PATH, LIBRARY_PATH), and rustc's and Cargo's own settings.
#
# Out, by decision: what Cargo sets itself for every build script, so that the
# job's value never reaches one (TARGET, HOST, OUT_DIR, OPT_LEVEL, DEBUG,
# PROFILE, NUM_JOBS, CARGO_MANIFEST_DIR, CARGO_MAKEFLAGS, CARGO_PKG_NAME); PATH,
# whose selected tools the native identity keys; what cc reads for MSVC targets
# alone; and the variables a build script declares itself, for which Cargo
# reruns it: AWS_LC_SYS_*, bindgen's LIBCLANG_PATH and BINDGEN_EXTRA_CLANG_ARGS,
# a name it hands to cc's flags_from_environment, and SYSTEM_DEPS_*, which the
# install step refuses anyway.
FLAG_NAMES = {"CPPFLAGS", "LDFLAGS", "LIBRARY_PATH", "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH",
              "RUSTC", "RUSTC_WRAPPER", "RUSTC_BOOTSTRAP", "RUSTC_LINKER", "RUSTFLAGS", "RUSTDOCFLAGS",
              "CARGO_ENCODED_RUSTFLAGS", "CARGO_INCREMENTAL", "CARGO_TRIM_PATHS_REMAP", "CARGO_TRIM_PATHS_SCOPE",
              "CARGO_CFG_TARGET_ABI", "CARGO_CFG_TARGET_ARCH", "CARGO_CFG_TARGET_ENV", "CARGO_CFG_TARGET_FEATURE",
              "CARGO_CFG_TARGET_OS", "CARGO_CFG_TARGET_VENDOR",
              "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "IPHONEOS_DEPLOYMENT_TARGET", "TVOS_DEPLOYMENT_TARGET",
              "WATCHOS_DEPLOYMENT_TARGET", "XROS_DEPLOYMENT_TARGET",
              "CRATE_CC_NO_DEFAULTS", "CROSS_COMPILE", "PAUTHTEST_RESOURCE_DIR", "PAUTHTEST_SYSROOT",
              "WASI_SDK_PATH", "WASI_SYSROOT", "WASM_MUSL_SYSROOT", "SYSROOT",
              *(kind + name for name in FLAG_TARGETED for kind in ("", "HOST_", "TARGET_"))}
FLAG_PREFIXES = (*(name + "_" for name in FLAG_TARGETED), "CMAKE", "PKG_CONFIG",
                 "CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_")
# The forms built around a caller's name: the cmake crate's DEP_<dependency>_ROOT
# and the pkg-config crate's <LIBRARY>_NO_PKG_CONFIG, <LIBRARY>_STATIC and
# <LIBRARY>_DYNAMIC. A build script's own name of such a form
# (AWS_LC_SYS_STATIC) is keyed with them: one name too many never makes a key stale.
FLAG_FORMS = re.compile(r"DEP_.+_ROOT|.+_(?:NO_PKG_CONFIG|STATIC|DYNAMIC)")
FLAG_EXCLUDED = {"CARGO_TARGET_DIR"}
# The identity prepare keyed, kept beside the job for the finish phase: the
# comparison names what changed from it. Its verdict is the one word that
# lets the bound phase save; everything else refuses.
IDENTITY_STATE = "gitturtle-cache-identity.json"
IDENTITY_UNCHANGED = "unchanged"
IDENTITY_REFUSAL = "cache identity changed or was not confirmed since setup"
TRIPLE = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}")
ACCOUNT = re.compile(r"[a-z_][a-z0-9_-]{0,31}")
# The pinned rust-cache hashes `rustc -vV` of every toolchain that `rustup
# toolchain list` names into its part of the key. A hosted image ships a
# default Rust of its own, which no build here runs and which differs between
# image releases, so one platform on two images had two keys and built cold.
# The setup phase therefore leaves rustup the active toolchain alone. A name is
# checked against this pattern before it reaches a command line, where it is
# one argument that cannot be read as an option.
TOOLCHAIN = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")
TOOLCHAIN_CHANNEL = re.compile(r'^channel[ \t]*=[ \t]*"([^"\n]*)"[ \t]*(?:#.*)?$', re.MULTILINE)
# The test jobs' pinned cargo-nextest archive rides in the entry they restore, so
# a restored job makes no request for it. It lives beside Cargo's own registry
# directories: the pinned rust-cache saves `registry` whole and cleans only its
# `index`, `cache` and `src`, and the bound phase prunes only `src`. The cache
# holds the release archive, never the executable, and the installer hashes it
# against the pin before anything reads it (scripts/ci/tools.py). The profiles
# whose lanes install it key the pin, read from the installer's own literals,
# so a pin move is a new key and never a mismatch in a restored entry. The
# identity keys the pin and never what the directory holds.
TOOL_ARCHIVES = "gitturtle-tool-archives"
TOOL_PROFILES = {"debug"}
TOOL_INSTALLER = "scripts/ci/tools.py"
TOOL_VERSION = re.compile(r"\d+\.\d+\.\d+")
TOOL_ARCHIVE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")
TOOL_DIGEST = re.compile(r"[0-9a-f]{64}")


def profile_limit(profile):
    if profile not in PROFILE_LIMITS:
        raise ValueError("unsupported cache profile")
    return PROFILE_LIMITS[profile]


def run(command, *, cwd=None, timeout=120):
    result = subprocess.run(command, cwd=cwd, check=True, capture_output=True, timeout=timeout)
    if len(result.stdout) > 16 * 1024**2:
        raise ValueError("cache metadata exceeded its byte limit")
    return result.stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def keyed_inputs(root):
    """Tracked build inputs whose actual bytes form the cache identity, sorted."""
    names = run(["git", "ls-files", "-z"], cwd=root).split(b"\0")
    inputs = []
    for raw in sorted(filter(None, names)):
        name = raw.decode("utf-8", errors="surrogateescape")
        path = Path(name)
        if (name.startswith(VENDOR_PREFIX) or name.startswith(".github/actions/setup-rust/")
                or path.name in {"Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml"}
                or name in {".cargo/config", ".cargo/config.toml"}):
            inputs.append((raw, path))
    return inputs


def source_identity(root, inputs=None):
    """Hash actual tracked build inputs, including local patches, not their names."""
    hasher = hashlib.sha256()
    for raw, path in keyed_inputs(root) if inputs is None else inputs:
        source = root / path
        hasher.update(raw + b"\0")
        if source.is_symlink():
            hasher.update(b"symlink\0" + os.fsencode(os.readlink(source)))
        else:
            with source.open("rb") as stream:
                while chunk := stream.read(1024 * 1024):
                    hasher.update(chunk)
        hasher.update(b"\0")
    return hasher.hexdigest()


def vendor_paths(root, inputs):
    """Vendored files and their directories up to vendor/, all inside the key.

    Refuses symlinks and anything that is not a regular file or directory, so a
    rewound timestamp can never reach a file outside the hashed set.
    """
    vendor = root / VENDOR_PREFIX.rstrip("/")
    paths = set()
    for raw, path in inputs:
        if not raw.startswith(VENDOR_PREFIX.encode()):
            continue
        source = root / path
        mode = os.lstat(source).st_mode
        if not stat.S_ISREG(mode):
            raise ValueError("nonregular vendored input refused")
        paths.add(source)
        parent = source.parent
        while parent != root:
            if not stat.S_ISDIR(os.lstat(parent).st_mode):
                raise ValueError("nondirectory vendored parent refused")
            paths.add(parent)
            if parent == vendor:
                break
            parent = parent.parent
    return sorted(paths)


def normalize_vendor_mtimes(root, inputs):
    """Rewind vendored sources to one fixed time so restored outputs stay fresh.

    Checkout gives every file the current time, and Cargo rebuilds a path
    package whose sources are newer than its dep-info. This is safe only
    because each rewound file's bytes are in the cache key and a non-exact
    restore is discarded: equal times can never hide a content change. Never
    apply it to crates/, whose sources the key does not hash.
    """
    paths = vendor_paths(root, inputs)
    for path in paths:
        os.utime(path, ns=(VENDOR_MTIME_NS, VENDOR_MTIME_NS), follow_symlinks=False)
    return len(paths)


class InventoryFallback(Exception):
    """A snapshot the closure cannot be trusted on; its message names the cause."""


def relation_groups(field, owner):
    """A Pre-Depends or Depends field as groups of alternative package names."""
    groups = []
    for group in field.split(",") if field else ():
        names = []
        for term in group.split("|"):
            match = DPKG_DEPENDENCY.fullmatch(term.strip(" "))
            if not match:
                raise InventoryFallback(f"{owner} has a dependency outside dpkg's grammar: {term.strip(' ')!r}")
            names.append(match.group(1))
        groups.append(names)
    return groups


def ubuntu_closure(lines):
    """Installed packages a build can read: UBUNTU_BUILD_PACKAGES and all they depend on.

    Follows Pre-Depends and Depends, never Recommends or Suggests (the action
    installs without them). A name resolves to every package of that name on
    any architecture and to every package that Provides it, and every
    alternative that resolves is followed, so the result is a superset of what
    apt needed. Only state `ii ` is installed. A root with no installed package
    raises, as `pacman -Q` fails on Arch, whatever else is wrong with the
    snapshot. Any other abnormality raises InventoryFallback, a reached package
    in another state included (unpacked, half installed, or removed with its
    configuration left): a dependency is never skipped and the closure is never
    partial. So does a package of UBUNTU_UNKEYED_PACKAGES, in any state.
    """
    # The roots are checked first, on a reading that refuses no line, so a line
    # the closure cannot be trusted on never turns a missing root into a fallback.
    installed = {match.group(1) for fields in (line.split("\t") for line in lines)
                 if fields[2:3] == [DPKG_INSTALLED] and (match := DPKG_PACKAGE.fullmatch(fields[0]))}
    for root in UBUNTU_BUILD_PACKAGES:
        if root not in installed:
            raise ValueError(f"Ubuntu build package {root} is not installed")
    packages, by_name, providers = {}, {}, {}
    for number, line in enumerate(lines, 1):
        fields = line.split("\t")
        if len(fields) != 6:
            raise InventoryFallback(f"snapshot line {number} has {len(fields)} fields, not six")
        identity, version, status, pre_depends, depends, provides = fields
        match = DPKG_PACKAGE.fullmatch(identity)
        if not match or not DPKG_VERSION.fullmatch(version) or identity in packages:
            raise InventoryFallback(f"snapshot line {number} does not name one package and its version")
        packages[identity] = (version, status, (pre_depends, depends))
        by_name.setdefault(match.group(1), []).append(identity)
        for term in provides.split(",") if provides else ():
            provided = DPKG_PROVIDED.fullmatch(term.strip(" "))
            if not provided:
                raise InventoryFallback(f"{identity} provides a name outside dpkg's grammar: {term.strip(' ')!r}")
            providers.setdefault(provided.group(1), []).append(identity)
    for name in UBUNTU_UNKEYED_PACKAGES:
        if name in by_name:
            raise InventoryFallback(f"the snapshot holds {', '.join(by_name[name])}, which no build package keys")
    closure = {}
    pending = [identity for root in UBUNTU_BUILD_PACKAGES for identity in by_name[root]]
    while pending:
        identity = pending.pop()
        if identity in closure:
            continue
        version, status, relations = packages[identity]
        if status != DPKG_INSTALLED:
            raise InventoryFallback(f"{identity} is in dpkg state {status!r}, not installed")
        closure[identity] = version
        for field in relations:
            for names in relation_groups(field, identity):
                resolved = [found for name in names for found in (*by_name.get(name, ()), *providers.get(name, ()))]
                if not resolved:
                    raise InventoryFallback(f"nothing installed satisfies {' | '.join(names)!r} of {identity}")
                pending.extend(resolved)
    return sorted(f"{identity}={version}" for identity, version in closure.items())


def ubuntu_packages(snapshot, resolved=(), directed=()):
    """One dpkg snapshot as (mode, keyed lines, keyed installed, installed, other lines, fallback cause).

    `closure` keys `name:arch=version` of the closure and nothing else from the
    inventory. `inventory` is the fallback: every line of the snapshot with its
    package, architecture, version and status. `resolved` names the
    UBUNTU_UNSUPPLIED_PROBES that pkg-config resolves and `directed` the set
    variables that send pkg-config elsewhere: any of them is a cause.
    """
    lines = snapshot.split("\n")
    if lines[-1] == "":
        lines.pop()
    installed = sum(line.split("\t")[2:3] == [DPKG_INSTALLED] for line in lines)
    try:
        keyed = ubuntu_closure(lines)
        causes = []
        if directed:
            causes.append(f"the environment sets {', '.join(directed)}, and a digest keys a .pc file's text, not what it leads to")
        if resolved:
            causes.append(f"pkg-config resolves {', '.join(resolved)}, which no build package ships")
        if causes:
            raise InventoryFallback("; ".join(causes))
        return "closure", keyed, len(keyed), installed, 0, None
    except InventoryFallback as fallback:
        keyed = []
        for line in lines:
            fields = line.split("\t")
            if len(fields) >= 3 and DPKG_PACKAGE.fullmatch(fields[0]) and DPKG_VERSION.fullmatch(fields[1]):
                keyed.append(f"{fields[0]}={fields[1]}" + ("" if fields[2] == DPKG_INSTALLED else f" [{fields[2]}]"))
            else:
                keyed.append("unparsed: " + line)
        return "inventory", sorted(keyed), installed, installed, len(lines) - installed, str(fallback)


def tool_identity(name, *, required=True):
    """Which binary a bare `name` runs for a build script, and what it says it is."""
    path = shutil.which(name)
    if path is None:
        if required:
            raise ValueError(f"build tool {name} is not on PATH")
        return None
    # Every match in PATH order, as `type -a` shows it: printed, not hashed.
    matches = [found for directory in os.environ.get("PATH", "").split(os.pathsep)
               if (found := shutil.which(name, path=directory))]
    return {"run": path, "path": os.path.realpath(path), "matches": list(dict.fromkeys([path, *matches])),
            "version": run([path, "--version"]).decode("utf-8", "surrogateescape")}


def unreadable(error):
    """How something that is there and could not be read is keyed and shown: never as empty."""
    return f"unreadable {errno.errorcode.get(error.errno, error.errno)}"


def variable_forms(environ, bases):
    """The set variables of `environ` that are a keyed form of one of `bases`, as sorted (name, value) pairs."""
    return sorted((name, value) for name, value in environ.items() if value and any(
        name in (base, "HOST_" + base, "TARGET_" + base) or name.startswith(base + "_") for base in bases))


def pkg_config_view(pkg_config, environ):
    """What pkg-config can find outside dpkg's files: (built-in path, its .pc files, the variables' .pc files).

    The built-in path is `pc_path`; of its directories, those no dpkg package
    owns are listed. Then every directory a set form of PKG_CONFIG_DIRECTORIES
    names, under its variable. The three are not one search order:
    PKG_CONFIG_LIBDIR replaces the built-in path, and `pc_path` still prints it.
    Each file is keyed by a digest of its content. Only a directory that does
    not exist is empty; one that cannot be listed is keyed as unreadable.
    """
    order = run([pkg_config, "--variable", "pc_path", "pkg-config"]).decode("utf-8", "surrogateescape").strip()
    files = {}
    for directory in filter(None, order.split(":")):
        # The real path decides, by whole components: a link from under
        # /usr/lib to another place is listed, and /usr/lib64 is not /usr/lib.
        real = os.path.realpath(directory)
        if any(real == root or real.startswith(root + "/") for root in DPKG_PC_ROOTS):
            continue
        files[directory] = pc_listing(directory)
    added = {name: {directory: pc_listing(directory) for directory in filter(None, value.split(os.pathsep))}
             for name, value in variable_forms(environ, PKG_CONFIG_DIRECTORIES)}
    return order, files, added


def pc_listing(directory):
    """The .pc files of one directory, each with a digest of its content, or how the directory could not be listed."""
    try:
        names = sorted(name for name in os.listdir(directory) if name.endswith(".pc"))
    except FileNotFoundError:
        names = []
    except OSError as error:
        return unreadable(error)
    return [[name, pc_digest(os.path.join(directory, name))] for name in names]


def pc_digest(path):
    """A digest of one .pc file's content, or how it could not be read.

    A dangling link is unreadable and still occupies the name pkg-config looks
    up. Nothing but a regular file is read: a pipe or a device need not end.
    """
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    except OSError as error:
        return unreadable(error)
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            return "not a regular file"
        hasher = hashlib.sha256()
        while chunk := os.read(descriptor, 1024 * 1024):
            hasher.update(chunk)
        return hasher.hexdigest()
    except OSError as error:
        return unreadable(error)
    finally:
        os.close(descriptor)


def local_listing(directories):
    """The top-level entries of each directory, by name: a tripwire, not a key over content.

    A regular file is listed with its size, a directory with a trailing `/`
    and a link with its target; nothing is opened and nothing below the top
    level is read. A directory that does not exist is `missing`, and one that
    cannot be listed is unreadable, never empty.
    """
    listing = {}
    for directory in directories:
        try:
            names = sorted(os.listdir(directory))
        except FileNotFoundError:
            listing[directory] = "missing"
        except OSError as error:
            listing[directory] = unreadable(error)
        else:
            listing[directory] = [local_entry(os.path.join(directory, name), name) for name in names]
    return listing


def local_entry(path, name):
    try:
        info = os.lstat(path)
        if stat.S_ISLNK(info.st_mode):
            return f"{name} -> {os.readlink(path)}"
    except OSError as error:
        return f"{name} {unreadable(error)}"
    if stat.S_ISDIR(info.st_mode):
        return f"{name}/"
    return f"{name} {info.st_size}" if stat.S_ISREG(info.st_mode) else f"{name} (not a file, a directory or a link)"


def pkg_config_probe(pkg_config, name):
    """What the selected pkg-config resolves `name` to: its version, or None for nothing.

    pkg-config exits with status 1 for a name it cannot resolve, which is the
    answer here and no fault. Any other status says it could not tell (a
    wrapper that cannot run it answers 126 or 127): that, like a command that
    cannot start, is killed or runs out of time, still fails the step.
    """
    try:
        output = run([pkg_config, "--modversion", name])
    except subprocess.CalledProcessError as error:
        if error.returncode == 1:
            return None
        raise
    return output.decode("utf-8", "surrogateescape").removesuffix("\n")


def shown(text):
    """Command output as one printable log line that starts no workflow command.

    Every character outside printable ASCII is escaped, so a value cannot begin
    a line of its own, and the runner's older command spelling `##[`, which it
    reads anywhere in a line, is broken up.
    """
    return "".join(character if " " <= character <= "~" else character.encode("unicode_escape").decode("ascii")
                   for character in text).replace("##[", "#\\x23[")


def ubuntu_identity(snapshot, tools, pc_path, pc_files, probes, local, pc_added=None, directed=()):
    """The Ubuntu native digest and the log lines that show the values in it.

    The digest's input starts with the mode, so a fallback key can never equal
    a closure key. The lines are built from the hashed values themselves, so
    two hosted runs can be compared line by line. Two things are shown without
    being hashed: each tool's other matches on PATH and the count of installed
    packages. Of a tool's version output, all of it hashed, the line shows a
    short digest and the first line. `pc_added` and `directed` are what the
    PKG_CONFIG variables add, and with none set neither the digest's input nor
    a line differs from a key without them.
    """
    mode, keyed, count, installed, others, cause = ubuntu_packages(
        snapshot, [name for name, version in probes.items() if version is not None], directed)
    hashed = {"packages": keyed, "pc_path": pc_path, "pc_files": pc_files, "probes": probes, "local": local,
              "tools": {name: tool and {"path": tool["path"], "version": tool["version"]} for name, tool in tools.items()}}
    if pc_added:
        hashed["pc_added"] = pc_added
    native = digest(mode.encode() + b"\n" + json.dumps(hashed, sort_keys=True).encode())
    lines = []
    if cause:
        lines.append(f"::warning::Ubuntu cache key fell back to the complete package inventory: {shown(cause)}")
    lines += [f"Ubuntu native key mode: {mode}",
              f"Keyed Ubuntu packages: {count} of {installed} installed" + (f", and {others} other snapshot lines" if others else ""),
              "::group::Keyed Ubuntu packages", *map(shown, keyed), "::endgroup::"]
    for name, tool in tools.items():
        if tool is None:
            lines.append(f"Keyed tool {name}: absent")
            continue
        first = tool["version"].split("\n", 1)[0]
        lines.append(f"Keyed tool {name}: {shown(tool['path'])}; on PATH: {shown(', '.join(tool['matches']))}; "
                     f"version {digest(tool['version'].encode('utf-8', 'surrogateescape'))[:12]}: {shown(first)}")
    lines.append(f"Keyed pkg-config pc_path: {shown(pc_path)}")
    for directory, found in pc_files.items():
        listing = found if isinstance(found, str) else ", ".join(f"{name} {state}" for name, state in found) or "none"
        lines.append(f"Keyed pkg-config files in {shown(directory)}: {shown(listing)}")
    for variable, directories in (pc_added or {}).items():
        for directory, found in directories.items():
            listing = found if isinstance(found, str) else ", ".join(f"{name} {state}" for name, state in found) or "none"
            lines.append(f"Keyed pkg-config files in {shown(directory)}, which {shown(variable)} names: {shown(listing)}")
    for name, version in probes.items():
        lines.append(f"Keyed pkg-config probe {name}: " + ("absent" if version is None else f"version {shown(version)}"))
    for directory, found in local.items():
        listing = found if isinstance(found, str) else ", ".join(found) or "empty"
        lines.append(f"Keyed top level of {shown(directory)}: {shown(listing)}")
    lines.append(f"Ubuntu native digest: {native}")
    return native, lines


def native_identity(platform):
    """The platform's native digest and the lines that show what is in it; nothing is printed here."""
    if platform == "Linux":
        snapshot = run(DPKG_SNAPSHOT).decode("utf-8", "surrogateescape")
        tools = {name: tool_identity(name) for name in UBUNTU_TOOLS}
        tools.update((name, tool_identity(name, required=False)) for name in UBUNTU_OPTIONAL_TOOLS)
        pkg_config = tools["pkg-config"]["run"]
        probes = {name: pkg_config_probe(pkg_config, name) for name in UBUNTU_UNSUPPLIED_PROBES}
        pc_path, pc_files, pc_added = pkg_config_view(pkg_config, os.environ)
        directed = [name for name, _ in variable_forms(os.environ, (*PKG_CONFIG_DIRECTORIES, PKG_CONFIG_SYSROOT))]
        return ubuntu_identity(snapshot, tools, pc_path, pc_files, probes,
                               local_listing(UBUNTU_LOCAL_DIRECTORIES), pc_added, directed)
    if platform == "macOS":
        commands = [["xcodebuild", "-version"], ["xcrun", "--sdk", "macosx", "--show-sdk-version"],
                    ["xcrun", "--sdk", "macosx", "--show-sdk-build-version"],
                    ["xcrun", "clang", "--version"], ["sw_vers", "-buildVersion"]]
    elif platform == "archlinux":
        # Named packages only: `pacman -Q` fails the step if one is missing,
        # rather than keying on whatever happened to be installed.
        commands = [["pacman", "-Q", *sorted(ARCH_NATIVE_PACKAGES)]]
    else:
        raise ValueError("Rust cache supports only the native Quality platforms")
    outputs = [run(command) for command in commands]
    # The day's rolling versions this key was built against, for comparing runs.
    lines = ["Keyed Arch packages: " + " ".join(outputs[0].decode("utf-8", "replace").split())] if platform == "archlinux" else []
    return digest(b"\0".join(outputs)), lines


def toolchain_identity():
    """The active compiler's full `rustc -vV` and the target triple it builds for.

    The Quality jobs that key a cache compile for the host, so the host line is
    the target.
    """
    output = run(["rustc", "-vV"])
    hosts = [line.split(b":", 1)[1].strip().decode() for line in output.splitlines() if line.startswith(b"host:")]
    if len(hosts) != 1 or not TRIPLE.fullmatch(hosts[0]):
        raise ValueError("rustc reported no single host target")
    return {"rustc": digest(output), "target": hosts[0]}


def listed_toolchains(listing):
    """The toolchain names of a `rustup toolchain list` text, one a line, in its order.

    A line's first word is the name, with or without rustup's `(active)` and
    `(default)` notes. A name outside TOOLCHAIN raises: it is never guessed at.
    """
    names = []
    for line in listing.splitlines():
        words = line.split()
        if not words:
            continue
        if not TOOLCHAIN.fullmatch(words[0]):
            raise ValueError(f"rustup lists a toolchain whose name is not supported: {words[0]!r}")
        names.append(words[0])
    return names


def active_toolchain(active, pinned):
    """The active toolchain's name, which must be the one rust-toolchain.toml selects.

    `active` is the output of `rustup show active-toolchain` in the checkout,
    whose first word is the name, and `pinned` the text of rust-toolchain.toml.
    rustup names the toolchain after the file's channel and the host, so
    another name says something else selected it (RUSTUP_TOOLCHAIN does).
    """
    words = active.split()
    if not words or not TOOLCHAIN.fullmatch(words[0]):
        raise ValueError("rustup names no supported active Rust toolchain")
    channels = TOOLCHAIN_CHANNEL.findall(pinned)
    if len(channels) != 1 or not TOOLCHAIN.fullmatch(channels[0]):
        raise ValueError("rust-toolchain.toml names no single supported channel")
    if words[0] != channels[0] and not words[0].startswith(channels[0] + "-"):
        raise ValueError(f"the active Rust toolchain {words[0]} is not the {channels[0]} that rust-toolchain.toml selects")
    return words[0]


def other_toolchains(listing, active):
    """The listed toolchains that are not the active one: what the setup phase uninstalls."""
    names = listed_toolchains(listing)
    if active not in names:
        raise ValueError(f"rustup does not list the active Rust toolchain {active}; it lists: {', '.join(names) or 'none'}")
    return [name for name in names if name != active]


def only_toolchain(listing, active):
    """Raise, naming what is listed, unless rustup lists the active toolchain and no other."""
    names = listed_toolchains(listing)
    if names != [active]:
        raise ValueError(f"rustup must list the active Rust toolchain {active} alone; it lists: {', '.join(names) or 'none'}")


def rustup(root, *arguments):
    """What a rustup command prints, run in the checkout, where rust-toolchain.toml selects the toolchain."""
    return run(["rustup", *arguments], cwd=root).decode("utf-8", "surrogateescape")


def selected_toolchain(root):
    """The checkout's active toolchain, which must be the one its rust-toolchain.toml selects."""
    return active_toolchain(rustup(root, "show", "active-toolchain"), (root / "rust-toolchain.toml").read_text(encoding="utf-8"))


def toolchains():
    """Uninstall every toolchain but the active one, then fail unless rustup lists it alone.

    The listing is the command the pinned rust-cache hashes. A container job
    uninstalls nothing: its account's RUSTUP_HOME holds only the toolchain the
    job installed from rust-toolchain.toml, and the check keeps that true.
    """
    started = time.monotonic()
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    container = os.environ.get("CACHE_CONTAINER", "")
    if container not in {"", "archlinux"}:
        raise ValueError("unsupported cache container")
    active = selected_toolchain(root)
    if not container:
        for name in other_toolchains(rustup(root, "toolchain", "list", "--quiet"), active):
            rustup(root, "toolchain", "uninstall", name)
            print(f"Uninstalled Rust toolchain {name}, {time.monotonic() - started:.1f} s into the step")
    only_toolchain(rustup(root, "toolchain", "list", "--quiet"), active)
    print(f"rustup lists the active Rust toolchain alone: {active} ({time.monotonic() - started:.1f} s)")


def command_failure(error):
    """What failed and how, as one line, for a command that exited with a failure, timed out or could not be started.

    The command line is the helper's own fixed words and, for an uninstall, a
    name that was validated first. A failure's last line of standard error is
    the tool's own reason, cut to 200 characters; the caller escapes the line.
    """
    if isinstance(error, subprocess.CalledProcessError):
        said = reversed((error.stderr or b"").decode("utf-8", "replace").splitlines())
        reason = next((line.strip()[:200] for line in said if line.strip()), "")
        return f"{' '.join(error.cmd)} failed with exit status {error.returncode}" + (f": {reason}" if reason else "")
    if isinstance(error, subprocess.TimeoutExpired):
        return f"{' '.join(error.cmd)} did not finish in {error.timeout:g} s"
    if isinstance(error, OSError):
        return f"{error.filename or 'a file or command'} could not be used: {error.strerror or type(error).__name__}"
    return f"a command failed: {type(error).__name__}"


def toolchains_since_setup(root):
    """None while rustup still lists the pinned toolchain alone, else the refusing verdict and its one warning.

    The save step is a second run of the pinned rust-cache, which hashes the
    list again. A toolchain that came back since setup (a `rustc` run outside
    the checkout downloads the default one) would store the entry under a
    suffix no setup phase computes, so the finish phase asks what setup asked,
    with the same functions. It only reads: nothing is uninstalled once the
    job's steps have run. A list that differs is named, as setup names it; a
    rustup that cannot be run or does not answer is never read as unchanged.
    """
    try:
        active = selected_toolchain(root)
        only_toolchain(rustup(root, "toolchain", "list", "--quiet"), active)
    except ValueError as error:
        return "changed", "::warning::Rust toolchains changed since setup, so this job saves no cache entry: " + shown(str(error))
    except (OSError, subprocess.SubprocessError) as error:
        return "unconfirmed", ("::warning::Rust toolchains could not be read from rustup, so this job saves no cache entry: "
                               + shown(type(error).__name__))
    return None


def tool_pins(source):
    """The cargo-nextest pin of the installer's source text: its version and every archive's name and digest.

    Read from the two literal assignments without running the file, so the
    identity has no side effect. Anything but one valid literal of each raises.
    """
    found = {}
    for node in ast.parse(source).body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name) \
                and node.targets[0].id in ("NEXTEST_VERSION", "NEXTEST_ARCHIVES"):
            if node.targets[0].id in found:
                raise ValueError("the cargo-nextest pin is assigned twice")
            found[node.targets[0].id] = ast.literal_eval(node.value)
    version, archives = found.get("NEXTEST_VERSION"), found.get("NEXTEST_ARCHIVES")
    if not isinstance(version, str) or not TOOL_VERSION.fullmatch(version) or not isinstance(archives, dict) or not archives:
        raise ValueError("the cargo-nextest pin is not one version and its archives")
    pinned = []
    for platform, archive in archives.items():
        if not (isinstance(platform, tuple) and len(platform) == 2 and all(isinstance(part, str) for part in platform)
                and isinstance(archive, tuple) and len(archive) == 2 and all(isinstance(part, str) for part in archive)
                and TOOL_ARCHIVE.fullmatch(archive[0]) and TOOL_DIGEST.fullmatch(archive[1])):
            raise ValueError("the cargo-nextest pin has an archive without a name and a SHA-256")
        pinned.append([*platform, *archive])
    return {"nextest": {"version": version, "archives": sorted(pinned)}}


def keyed_flags(environ):
    """The set variables of the environment that are flags, as sorted (name, value) pairs."""
    return sorted((name, value) for name, value in environ.items()
                  if value and name not in FLAG_EXCLUDED
                  and (name in FLAG_NAMES or name.startswith(FLAG_PREFIXES) or FLAG_FORMS.fullmatch(name)))


def flags_identity(environ):
    """Compiler, linker and Cargo profile settings from the environment, by name."""
    return digest(json.dumps(keyed_flags(environ)).encode())


def run_as(account):
    """Become the job's unprivileged account before any Cargo storage is touched.

    A container job starts as root. Cargo run as root would leave root-owned
    registry, lock and target files that the build account cannot update, and
    permission-refusal tests pass only for an unprivileged user. Changing every
    user ID from root also clears the effective capabilities.
    """
    if not ACCOUNT.fullmatch(account):
        raise ValueError("unsupported cache account name")
    entry = pwd.getpwnam(account)
    if entry.pw_uid == 0:
        raise ValueError("cache helper refuses to run as root")
    # setpriv and chown name the group after the account; they must agree.
    if grp.getgrnam(account).gr_gid != entry.pw_gid:
        raise ValueError("cache account's primary group must share its name")
    if os.geteuid() == 0:
        os.initgroups(account, entry.pw_gid)
        os.setgid(entry.pw_gid)
        os.setuid(entry.pw_uid)
    if (os.getuid(), os.geteuid(), os.getgid(), os.getegid()) != (entry.pw_uid, entry.pw_uid, entry.pw_gid, entry.pw_gid):
        raise ValueError("cache helper is not running as its account")
    status = Path("/proc/self/status")
    if status.exists() and not re.search(r"^CapEff:\s+0+$", status.read_text(), re.MULTILINE):
        raise ValueError("cache helper kept effective capabilities")


def output_file(name, fields):
    with Path(os.environ[name]).open("a", encoding="utf-8") as stream:
        for key, value in fields.items():
            if "\n" in str(value) or "\r" in str(value):
                raise ValueError("multiline action output refused")
            stream.write(f"{key}={value}\n")


def roots():
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    cargo = temp / "gitturtle-cargo"
    if os.environ.get("CARGO_HOME") != str(cargo):
        raise ValueError("cache cleanup requires the action's isolated Cargo home")
    target = root / "target"
    if os.environ.get("CARGO_TARGET_DIR") != str(target):
        raise ValueError("cache cleanup requires the action's workspace target")
    paths = (target, cargo / "registry", cargo / "git")
    for path in (cargo, *paths):
        if path.is_symlink():
            raise ValueError("symlinked cache roots refused")
    return root, temp, paths


def entries(paths, *, deadline=None, clock=time.monotonic):
    """Conservative payload accounting: logical bytes + 4 KiB per entry.

    Counts aliases separately, and never follows directory/file symlinks. This is
    an upper bound before upstream dependency cleanup, not a compressed archive.
    Validate excluded source trees too; projection never bypasses these guards.
    """
    result = []
    for path in paths:
        if path.is_symlink():
            raise ValueError("symlinked cache root refused")
        if not path.exists():
            continue
        if not path.is_dir():
            raise ValueError("nondirectory cache root refused")

        def refused(error):
            raise error

        for directory, dirs, files in os.walk(path, followlinks=False, onerror=refused):
            if deadline is not None and clock() >= deadline:
                raise TimeoutError("cache accounting exceeded its time limit")
            for name in dirs + files:
                child = os.path.join(directory, name)
                info = os.lstat(child)
                mode = info.st_mode
                if not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                    raise ValueError("nonregular cache payload refused")
                result.append((Path(child), (info.st_size if stat.S_ISREG(mode) else 0) + 4096))
                if len(result) > MAX_FILES:
                    raise ValueError("cache payload exceeded its entry limit")
    return result


def clear(paths):
    for path in paths:
        if path.is_symlink():
            raise ValueError("symlinked cache root refused")
        if path.exists():
            shutil.rmtree(path)


def suffix(name):
    index = name.rfind(".")
    return name[index:] if 0 < index < len(name) - 1 else ""


def package_artifact_bytes(packages, target_entries, root):
    """Attribute target bytes to dependency packages from structured locations only.

    Relative to the target root, the first `deps` component names the artifact
    file, and the first `build` or `.fingerprint` component names the package
    directory whose whole subtree belongs to that package. Absolute components
    above the root and unstructured target members never participate. Candidate
    names are the package name and its library-kind target names; a dependency
    test or example named `debug` or `build` must not claim a whole profile.
    """
    by_name = {}
    for package in packages:
        names = {package["name"]}
        for target in package.get("targets", []):
            if LIBRARY_KINDS.isdisjoint(target.get("kind", ())):
                continue
            normalized = target["name"].replace("-", "_")
            names.update((normalized, "lib" + normalized))
        for name in names:
            by_name.setdefault(name, set()).add(package["id"])
    prefix = root.parts
    depth = len(prefix)
    sizes = {}
    for path, size in target_entries:
        parts = path.parts
        if parts[:depth] != prefix:
            continue
        relative = parts[depth:]
        for index, component in enumerate(relative[:-1]):
            if component in ARTIFACT_DIRECTORIES:
                # Scores choose packages to pass to Cargo; Cargo owns actual
                # artifact cleanup (including fingerprints and build-script outputs).
                for package_id in by_name.get(relative[index + 1].rsplit("-", 1)[0], ()):
                    sizes[package_id] = sizes.get(package_id, 0) + size
                break
    return sizes


def largest_packages(packages, target_entries, root):
    sizes = package_artifact_bytes(packages, target_entries, root)
    return sorted(sizes, key=lambda package_id: (-sizes[package_id], package_id))


def removable_sources(packages, paths, measured):
    """Match only pinned rust-cache cleanRegistry's direct source directories.

    Current *-sys sources retain timestamps needed by native build scripts. Git
    databases and checkouts are never projected away. Keep extracted sources in
    place until the last Cargo command, which can otherwise recreate them.
    """
    keep = {f"{p['name']}-{p['version']}" for p in packages if p["name"].endswith("-sys")}
    prefix = (paths[1] / "src").parts
    depth = len(prefix) + 2
    removable = set()
    for path, _ in measured:
        parts = path.parts
        if len(parts) == depth and parts[:-2] == prefix and parts[-1] not in keep and path.is_dir():
            removable.add(path)
    return removable


def payload_snapshot(paths, measured, removable, stage):
    buckets = {name: {"logical_bytes": 0, "entry_overhead_bytes": 0, "entries": 0}
               for name in ("target", "registry_src_retained", "registry_src_removable",
                            "registry_cache", "registry_index", "registry_other",
                            "git_db", "git_checkouts", "git_other")}
    compiled = {name: {"files": 0, "logical_bytes": 0}
                for name in ("libraries", "native_generated", "fingerprints")}
    (target, target_depth), (registry, registry_depth), (git, git_depth) = ((root.parts, len(root.parts)) for root in paths)
    removable_keys = {path.parts for path in removable}
    for path, size in measured:
        parts = path.parts
        kind = None
        if parts[:target_depth] == target:
            name = "target"
            relative = parts[target_depth:]
            if "deps" in relative and suffix(parts[-1]) in LIBRARY_SUFFIXES:
                kind = "libraries"
            elif "build" in relative and "out" in relative:
                kind = "native_generated"
            elif ".fingerprint" in relative:
                kind = "fingerprints"
        elif parts[:registry_depth] == registry:
            relative = parts[registry_depth:]
            if relative[0] == "src":
                name = "registry_src_removable" if parts[:registry_depth + 3] in removable_keys else "registry_src_retained"
            else:
                name = "registry_" + relative[0] if relative[0] in {"cache", "index"} else "registry_other"
        elif parts[:git_depth] == git:
            relative = parts[git_depth:]
            name = "git_" + relative[0] if relative[0] in {"db", "checkouts"} else "git_other"
        else:
            raise ValueError("cache entry outside its roots refused")
        bucket = buckets[name]
        bucket["logical_bytes"] += size - 4096
        bucket["entry_overhead_bytes"] += 4096
        bucket["entries"] += 1
        if kind and path.is_file():
            compiled[kind]["files"] += 1
            compiled[kind]["logical_bytes"] += size - 4096
    logical = sum(value["logical_bytes"] for value in buckets.values())
    overhead = sum(value["entry_overhead_bytes"] for value in buckets.values())
    excluded = buckets["registry_src_removable"]
    return {"stage": stage, "logical_bytes": logical, "entry_overhead_bytes": overhead,
            "entries": len(measured), "bytes": logical + overhead,
            "projected_bytes": logical + overhead - excluded["logical_bytes"] - excluded["entry_overhead_bytes"],
            "subtrees": buckets, "compiled": compiled}


def bound_payload(root, paths, profile, *, execute=run, snapshots=None, diagnostics=None, clock=time.monotonic, target=None):
    started = clock()
    deadline = started + BUDGET_SECONDS
    limit = profile_limit(profile)
    clean_profiles = {"debug": ("dev",), "release": ("release",), "debug-release": ("dev", "release")}[profile]
    snapshots = [] if snapshots is None else snapshots
    diagnostics = {} if diagnostics is None else diagnostics
    diagnostics["limit_bytes"] = limit
    timings = diagnostics["stage_seconds"] = []
    evictions = diagnostics["evicted_packages"] = []
    observed = {"cleanup": 0.0, "measurement": 0.0}

    @contextmanager
    def stage(name):
        diagnostics["stage"] = name
        start = clock()
        try:
            yield
        finally:
            timings.append({"stage": name, "elapsed_seconds": round(clock() - start, 6)})

    with stage("metadata"):
        metadata = json.loads(execute(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1"], cwd=root, timeout=30))
        packages = metadata["packages"]

    def measure(name, until):
        with stage(name):
            measured = entries(paths, deadline=until, clock=clock)
            removable = removable_sources(packages, paths, measured)
            snapshot = payload_snapshot(paths, measured, removable, name)
            snapshots.append(snapshot)
        observed["measurement"] = timings[-1]["elapsed_seconds"]
        return measured, removable, snapshot

    def clean(package_ids, name):
        # Package-scoped Cargo clean otherwise defaults to dev, silently leaving
        # release artifacts intact, and without --target it touches only the
        # host layout. A job compiling with --target keeps its dependency
        # artifacts under target/<triple>/<profile>, so each selected group is
        # cleaned in both layouts; the triple never enters the fixed stage names.
        first = len(timings)
        layouts = [("", ()), *([("_target", ("--target", target))] if target else [])]
        for clean_profile in clean_profiles:
            for suffix, layout in layouts:
                with stage(f"{name}_{clean_profile}{suffix}"):
                    if clock() >= deadline:
                        raise TimeoutError("cache cleanup exceeded its time limit")
                    execute(["cargo", "clean", "--locked", "--profile", clean_profile, *layout,
                             *[part for package_id in package_ids for part in ("--package", package_id)]], cwd=root, timeout=30)
        observed["cleanup"] = sum(row["elapsed_seconds"] for row in timings[first:])

    measure("before_local_cleanup", deadline)
    # Members rebuild after every checkout (their sources are not in the key),
    # so only their outputs are cleaned. Vendored path packages are kept and
    # never evicted: their bytes are keyed and their sources rewound (prepare).
    vendor = root / VENDOR_PREFIX.rstrip("/")
    vendored = [package for package in packages if package.get("source") is None
                and package.get("manifest_path") and Path(package["manifest_path"]).is_relative_to(vendor)]
    kept = {package["id"] for package in vendored}
    local = [package["id"] for package in packages if package.get("source") is None and package["id"] not in kept]
    if local:
        clean(local, "local_cleanup")
    measured, removable, snapshot = measure("after_local_cleanup", deadline)
    before = snapshot["bytes"]
    removed = 0
    with stage("dependency_selection"):
        ordered = largest_packages([p for p in packages if p.get("source") is not None], measured, paths[0])
    for package_id in ordered[:MAX_CLEAN_PACKAGES]:
        if snapshot["projected_bytes"] <= limit:
            break
        # Another eviction, its verifying measurement and one more measurement
        # of headroom for scan variance must fit the remaining scheduling
        # budget, judged by the last observed durations. Mandatory finalization
        # work below never inherits an exhausted loop deadline.
        remaining = deadline - clock()
        if remaining <= 0 or remaining < observed["cleanup"] + 2 * observed["measurement"]:
            break
        index, package = next((index, package) for index, package in enumerate(packages) if package["id"] == package_id)
        eviction = {"metadata_index": index, "cleanup_completed": False}
        # Never expose package IDs: Git IDs can include a source URL or path.
        for field in ("name", "version"):
            value = package.get(field)
            if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}", value):
                eviction[field] = value
        evictions.append(eviction)
        clean([package_id], f"dependency_cleanup_{removed + 1}")
        eviction["cleanup_completed"] = True
        removed += 1
        measured, removable, snapshot = measure(f"after_dependency_cleanup_{removed}", deadline)
    # Upstream repeats metadata discovery and this precise source pruning in its
    # post action. This physical rescan verifies the predicted payload before we
    # register that save. Upstream catches cleanup errors, so its actual archive
    # size still needs hosted evidence; this is a pre-registration bound. Never
    # prune before a Cargo command or split an OUT_DIR group.
    projected = snapshot["projected_bytes"]
    finalize_deadline = clock() + FINALIZE_SECONDS
    with stage("source_pruning"):
        clear(sorted(removable))
    measured, _, snapshot = measure("after_source_pruning", finalize_deadline)
    with stage("projection_verification"):
        if snapshot["bytes"] != projected:
            raise ValueError("cache source projection did not match physical pruning")
    dropped_target = snapshot["bytes"] > limit
    if dropped_target:
        with stage("target_fallback"):
            clear(paths[:1])
        measured, _, snapshot = measure("after_target_fallback", finalize_deadline)
    retained = snapshot["bytes"]
    return {"limit_bytes": limit, "before_bytes": before, "retained_bytes": retained,
            "retained_vendor_packages": len(vendored),
            "retained_vendor_bytes": sum(package_artifact_bytes(vendored, measured, paths[0]).values()),
            "removed_dependency_packages": removed, "removed_source_directories": len(removable),
            "dropped_target": dropped_target, "save": retained <= limit,
            "evicted_packages": evictions, "snapshots": snapshots, "stage_seconds": timings}


def record(name, elapsed, *, cache=None, details=None):
    root = Path(os.environ["GITHUB_WORKSPACE"])
    sys.path.insert(0, str(root / "scripts/ci"))
    import metrics
    directory = Path(os.environ["RUNNER_TEMP"]) / "ci-metrics"
    directory.mkdir(parents=True, exist_ok=True)
    value = {"schema_version": 1, "name": name, "context": metrics.runner_context(),
             "elapsed_seconds": round(elapsed, 6), "exit_code": 0,
             "cache": metrics.cache_measurement(cache), "details": details}
    (directory / f"{name}.json").write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def cache_identity():
    """The cache key and its parts, from the environment and the machine as they are now.

    No side effect: nothing is created, rewound, exported or printed, so
    prepare and the finish phase's comparison compute the identity alike.
    CARGO_INCREMENTAL is keyed by its effective value: prepare exports 0 when
    the job set none, as the restore action does after it, so the variable is
    the same in both phases by construction and needs no exemption. A profile
    of TOOL_PROFILES has a sixth part, the pinned tools, whatever the job goes
    on to install: Clippy restores the entry the test job saves.
    """
    profile = os.environ["CACHE_PROFILE"]
    if profile not in PROFILES:
        raise ValueError("unsupported cache profile")
    container = os.environ.get("CACHE_CONTAINER", "")
    if container not in {"", "archlinux"}:
        raise ValueError("unsupported cache container")
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    inputs = keyed_inputs(root)
    flags = keyed_flags({**os.environ, "CARGO_INCREMENTAL": os.environ.get("CARGO_INCREMENTAL") or "0"})
    native, lines = native_identity(container or os.environ["RUNNER_OS"])
    parts = {"profile": profile, "source": source_identity(root, inputs), "native": native,
             "toolchain": toolchain_identity(), "flags": digest(json.dumps(flags).encode())}
    if profile in TOOL_PROFILES:
        parts["tools"] = tool_pins((root / TOOL_INSTALLER).read_text(encoding="utf-8"))
    key = "-".join(filter(None, (profile, container, digest(json.dumps(parts, sort_keys=True).encode()))))
    return {"key": key, "parts": parts, "native_lines": lines, "flags": flags, "inputs": inputs}


def compared():
    """Whether the identity is still the one setup keyed: (verdict, lines to print).

    The verdict compares the recomputed key with the CI_RUST_CACHE_KEY the save
    step reads. What prepare kept only names the difference: the parts, the
    native part's differing keyed lines, and the flags by name, never by value.
    A missing key or state raises, which the caller turns into a refusal.
    Unchanged also takes rustup listing the pinned toolchain alone, as it did
    at setup: the save step keys that list, which is no part of this key.
    """
    expected = os.environ["CI_RUST_CACHE_KEY"]
    state = json.loads((Path(os.environ["RUNNER_TEMP"]) / IDENTITY_STATE).read_text(encoding="utf-8"))
    kept = {"parts": dict(state["parts"]), "native_lines": [str(line) for line in state["native_lines"]],
            "flags": {str(name): str(value) for name, value in state["flags"].items()}}
    current = cache_identity()
    listed = toolchains_since_setup(Path(os.environ["GITHUB_WORKSPACE"]).resolve())
    if current["key"] == expected:
        if listed is None:
            return IDENTITY_UNCHANGED, [f"Cache identity unchanged since setup: {shown(current['key'])}"]
        return listed[0], [listed[1]]
    parts = [name for name in current["parts"] if kept["parts"].get(name) != current["parts"][name]] or ["key"]
    lines = [f"::warning::Cache identity changed since setup, so this job saves no cache entry: {shown(', '.join(parts))}"]
    if "native" in parts:
        before, after = kept["native_lines"], current["native_lines"]
        lines += [f"- {shown(line)}" for line in before if line not in after]
        lines += [f"+ {shown(line)}" for line in after if line not in before]
    if "flags" in parts:
        flags = {name: digest(value.encode("utf-8", "surrogateescape")) for name, value in current["flags"]}
        differing = sorted(name for name in {*kept["flags"], *flags} if kept["flags"].get(name) != flags.get(name))
        lines.append(f"Flag variables that differ: {shown(', '.join(differing))}")
    return "changed", lines + ([listed[1]] if listed else [])


def prepare():
    vendor_reuse = os.environ.get("CACHE_VENDOR_REUSE", "false")
    if vendor_reuse not in {"true", "false"}:
        raise ValueError("unsupported vendor reuse setting")
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    identity = cache_identity()
    profile, inputs = identity["parts"]["profile"], identity["inputs"]
    cargo = temp / "gitturtle-cargo"
    cargo.mkdir(exist_ok=True)
    if cargo.is_symlink() or (root / "target").is_symlink():
        raise ValueError("symlinked cache roots refused")
    # The names alone: a value can hold a path or a token.
    print("\n".join([*identity["native_lines"],
                     "Keyed flag variables: " + shown(", ".join(name for name, _ in identity["flags"]))]))
    (temp / IDENTITY_STATE).write_text(json.dumps({
        "key": identity["key"], "parts": identity["parts"], "native_lines": identity["native_lines"],
        "flags": {name: digest(value.encode("utf-8", "surrogateescape")) for name, value in identity["flags"]}}), encoding="utf-8")
    if vendor_reuse == "false":
        # Release builds must not link vendored objects from an earlier run:
        # checkout times leave any restored vendored output dirty, so Cargo
        # compiles those crates from this checkout.
        vendor_mtimes = "disabled"
    else:
        try:
            vendor_mtimes = str(normalize_vendor_mtimes(root, inputs))
        except (OSError, ValueError):
            # A refusal rewinds nothing; a later I/O error may leave some sources
            # at checkout time. Either way those packages rebuild: slower, never stale.
            vendor_mtimes = "refused"
    # The container name keeps its entries apart from the Ubuntu job's in the
    # cache list; both run on Linux x64 runners.
    exported = {"CI_RUST_CACHE_KEY": identity["key"],
                "CI_RUST_CACHE_PROFILE": profile,
                "CI_RUST_CACHE_VENDOR_MTIMES": vendor_mtimes,
                "CI_RUST_CACHE_LOCK": digest((root / "Cargo.lock").read_bytes()),
                "CARGO_HOME": cargo, "CARGO_TARGET_DIR": root / "target"}
    if not os.environ.get("CARGO_INCREMENTAL"):
        # The value the identity keyed, exported before the restore action exports the same.
        exported["CARGO_INCREMENTAL"] = "0"
    if profile in TOOL_PROFILES:
        # Only where the key holds the pin: no other profile's entry may carry an archive.
        exported["CI_RUST_CACHE_TOOL_ARCHIVES"] = cargo / "registry" / TOOL_ARCHIVES
    output_file("GITHUB_ENV", exported)


def restored():
    root, temp, paths = roots()
    started = float((temp / "gitturtle-cache-start").read_text())
    hit = os.environ.get("CACHE_HIT") == "true" and os.environ.get("CACHE_OUTCOME") == "success"
    # Upstream reports false for misses, partial compatibility matches and
    # download/extraction errors. Discard all such payloads, including partially
    # extracted archives, before Cargo is allowed to use them.
    if not hit:
        clear(paths)
    if digest((root / "Cargo.lock").read_bytes()) != os.environ["CI_RUST_CACHE_LOCK"]:
        raise ValueError("cache metadata unexpectedly changed the locked dependencies")
    elapsed = time.monotonic() - started
    record("rust-cache-restore", elapsed, cache={"hit": hit, "restore_seconds": elapsed},
           details={"key_prefix": os.environ["CI_RUST_CACHE_KEY"],
                    "vendor_mtimes": os.environ.get("CI_RUST_CACHE_VENDOR_MTIMES"),
                    "boundary": "restore action plus dispatch and recovery; not transfer-only"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("toolchains", "prepare", "start", "restored", "compare", "bound"))
    args = parser.parse_args()
    account = os.environ.get("CACHE_RUN_AS")
    if account and args.phase not in {"compare", "bound"}:
        run_as(account)
    if args.phase == "compare":
        # On every event, after validation passed: nothing here fails the job.
        # Whatever goes wrong refuses the save and is never read as unchanged.
        try:
            if account:
                run_as(account)
            verdict, lines = compared()
        except Exception as error:  # noqa: BLE001
            verdict = "unconfirmed"
            lines = ["::warning::Cache identity could not be compared with setup, so this job saves no cache entry: "
                     + shown(type(error).__name__)]
        try:
            # The verdict before its line: "unchanged" is printed only once the bound phase can read it.
            output_file("GITHUB_OUTPUT", {"identity": verdict})
        except Exception:  # noqa: BLE001
            # No verdict reaches the bound phase, which then refuses, so nothing here may say unchanged.
            lines = [*(lines if verdict != IDENTITY_UNCHANGED else []),
                     "::warning::Cache identity verdict could not be recorded, so this job saves no cache entry"]
        try:
            print("\n".join(lines))
        except Exception:  # noqa: BLE001
            # A line that cannot be printed fails no job either.
            pass
    elif args.phase == "toolchains":
        try:
            toolchains()
        except ValueError as error:
            # By name, as an annotation: the message holds what rustup lists.
            print(f"::error::{shown(str(error))}")
            return 1
        except (OSError, subprocess.SubprocessError) as error:
            # A rustup that fails, does not answer or is missing: the command and how it ended, not a traceback.
            print(f"::error::{shown(command_failure(error))}")
            return 1
    elif args.phase == "prepare":
        prepare()
    elif args.phase == "start":
        (Path(os.environ["RUNNER_TEMP"]) / "gitturtle-cache-start").write_text(str(time.monotonic()))
    elif args.phase == "restored":
        restored()
    else:
        start = time.monotonic()
        root, _, paths = roots()
        if os.environ["CACHE_PROFILE"] != os.environ["CI_RUST_CACHE_PROFILE"]:
            raise ValueError("finish must match its setup profile")
        snapshots = []
        diagnostics = {}
        target = os.environ.get("CACHE_TARGET", "")
        confirmed = os.environ.get("CACHE_IDENTITY") == IDENTITY_UNCHANGED
        try:
            if not confirmed:
                raise ValueError(IDENTITY_REFUSAL)
            # Inside the guard: after validation passed, an account failure
            # only refuses the save.
            if account:
                run_as(account)
            if target and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}", target):
                raise ValueError("unsupported cache target triple")
            result = bound_payload(root, paths, os.environ["CACHE_PROFILE"], snapshots=snapshots, diagnostics=diagnostics,
                                   target=target or None)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
            # Cache failure cannot turn already-passed validation into a failure;
            # retain a fixed diagnostic without leaking Cargo output or paths.
            result = {"save": False, "reason": "cache budget preparation unavailable" if confirmed else IDENTITY_REFUSAL,
                      "snapshots": snapshots, "limit_bytes": diagnostics.get("limit_bytes"),
                      "failure_stage": diagnostics.get("stage", "unavailable" if confirmed else "identity"),
                      "stage_seconds": diagnostics.get("stage_seconds", []),
                      "evicted_packages": diagnostics.get("evicted_packages", [])}
        output_file("GITHUB_OUTPUT", {"save": str(result["save"]).lower()})
        record("rust-cache-budget", time.monotonic() - start, details=result)
        print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
