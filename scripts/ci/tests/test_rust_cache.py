"""Behavioral tests for cache identity, bounded cleanup and miss recovery."""
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("rust_cache", ROOT / ".github/actions/setup-rust/cache.py")
cache = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cache)
SPEC_TOOLCHAIN = cache.toolchain_identity
# `dpkg-query` in an ubuntu:24.04 arm64 container holding exactly the setup
# action's install list, taken on 2026-11-19 with cache.DPKG_SNAPSHOT's format:
# package:architecture, version, status, Pre-Depends, Depends and Provides,
# tab-separated. A build of the workspace's tests traced there read files of
# 75 packages, 71 of them in the closure this key takes.
UBUNTU_SNAPSHOT = (ROOT / "scripts/ci/tests/fixtures/ubuntu-24.04-arm64-dpkg.tsv").read_text()
# What build scripts and the cc and cmake crates run bare; cmake3 is optional.
TOOL_NAMES = ("cc", "c++", "ar", "pkg-config", "clang", "cmake")
# Tool answers for the tests that start no process.
UBUNTU_TOOLS ={**{name: {"run": f"/usr/bin/{name}", "path": f"/usr/bin/{name}-13", "matches": [f"/usr/bin/{name}"],
                          "version": f"{name} 13.0\n"} for name in TOOL_NAMES}, "cmake3": None}


def snapshot_rows(text=UBUNTU_SNAPSHOT):
    """A snapshot as {package:architecture: [version, status, Pre-Depends, Depends, Provides]}."""
    return {fields[0]: fields[1:] for fields in (line.split("\t") for line in text.splitlines())}


def snapshot_text(rows):
    return "".join("\t".join((identity, *fields)) + "\n" for identity, fields in rows.items())


class UbuntuRunner:
    """A PATH of stand-ins for everything the Ubuntu key runs.

    `dpkg-query` answers with a snapshot file and records its arguments; each
    tool is a link in a PATH directory to its own real file, as the image's
    alternatives are, and answers `--version` from a file of its own.
    """

    def __init__(self, base, snapshot=UBUNTU_SNAPSHOT):
        self.bin, self.real, self.state, self.pc = (base / name for name in ("bin", "real", "state", "pkgconfig"))
        for directory in (self.bin, self.real, self.state, self.pc):
            directory.mkdir(parents=True)
        self.snapshot = self.state / "snapshot"
        self.snapshot.write_text(snapshot)
        self.queries = self.state / "queries"
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
        answer = self.state / f"{name}-{variant}.version"
        answer.write_text(version or f"{name} (Ubuntu) {variant}.0\nconfigured for the fixture\n")
        real = self.real / f"{name}-{variant}"
        self.script(real, f'case "$*" in\n  --version) exec /bin/cat {shlex.quote(str(answer))} ;;\n'
                          f'  "--variable pc_path pkg-config") exec /bin/cat {shlex.quote(str(self.pc_path))} ;;\nesac\nexit 64')
        link = (directory or self.bin) / name
        link.unlink(missing_ok=True)
        link.symlink_to(real)
        return real

    def environment(self, *earlier):
        return patch.dict(os.environ, {"PATH": os.pathsep.join(str(directory) for directory in (*earlier, self.bin))})

    def native(self, *earlier):
        """The native digest and the lines the step prints, with `earlier` directories first on PATH."""
        with self.environment(*earlier), patch("builtins.print") as shown:
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
        with patch.dict(os.environ, settings), patch.object(cache, "native_identity", return_value="native"):
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

    def test_profiles_have_distinct_identity_and_private_paths(self):
        self.init_git()
        identities = []
        for profile in sorted(cache.PROFILES):
            with patch.dict(os.environ, {"CACHE_PROFILE": profile, "RUNNER_OS": "Linux"}), patch.object(cache, "native_identity", return_value="native"):
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
        inputs = {"tools": UBUNTU_TOOLS, "pc_path": "/usr/lib/pkgconfig", "pc_files": {}, **changed}
        return cache.ubuntu_identity(snapshot_text(self.rows) if text is None else text, **inputs)[0]

    def depend(self, identity, term):
        """One more Depends group on a real package."""
        self.rows[identity][3] = ", ".join(filter(None, (self.rows[identity][3], term)))

    def test_closure_of_the_real_snapshot_is_what_a_traced_build_read_and_no_tool(self):
        mode, keyed, count, installed, others, cause = self.packages()
        self.assertEqual((mode, count, installed, others, cause), ("closure", 195, 278, 0, None))
        self.assertEqual(keyed, sorted(keyed))
        self.assertLessEqual(set(keyed), {f"{identity}={fields[0]}" for identity, fields in self.rows.items()})
        names = {line.split(":", 1)[0] for line in keyed}
        self.assertLessEqual(set(cache.UBUNTU_BUILD_PACKAGES), names)
        # Version- and architecture-named toolchain packages no list would name,
        # and the kernel headers libc6-dev includes.
        self.assertLessEqual({"gcc-13", "gcc-13-aarch64-linux-gnu", "cpp-13", "g++-13", "binutils-aarch64-linux-gnu",
                              "libc6", "libc6-dev", "linux-libc-dev", "libgcc-13-dev", "libstdc++-13-dev", "pkgconf-bin",
                              "libclang-common-18-dev", "libllvm18", "cmake-data", "libdav1d7", "libssl3t64"}, names)
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
        self.assertEqual(len(keys), 196)
        self.assertEqual(self.key(), baseline)

    def test_packages_outside_the_closure_never_change_the_key(self):
        baseline = self.key()
        outside = sorted(set(self.rows) - {line.split("=", 1)[0] for line in self.keyed()})
        self.assertEqual(len(outside), 83)
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
        self.assertEqual(self.packages()[2:4], (195, 280))

    def test_new_dependency_edge_brings_its_package_into_the_key(self):
        baseline = self.key()
        self.rows["libprobe1:arm64"] = ["1.0-1", "ii ", "", "libprobe-data", ""]
        self.rows["libprobe-data:all"] = ["1.0-1", "ii ", "", "", ""]
        self.assertEqual(self.key(), baseline)
        self.depend("libssl-dev:arm64", "libprobe1 (= 1.0-1)")
        edged = self.key()
        self.assertNotEqual(edged, baseline)
        self.assertLessEqual({"libprobe1:arm64=1.0-1", "libprobe-data:all=1.0-1"}, set(self.keyed()))
        self.assertEqual(len(self.keyed()), 197)
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

    def fallbacks(self):
        """Every cause that sends the key to the inventory: label -> (snapshot, the cause the warning names)."""
        def edited(edit):
            self.rows = snapshot_rows()
            edit()
            return snapshot_text(self.rows)

        whole = snapshot_text(snapshot_rows())
        return {
            "a line without six fields": (edited(lambda: self.rows["zlib1g:arm64"].pop()), "snapshot line 277 has 5 fields, not six"),
            "a line with a seventh field": (edited(lambda: self.rows["adduser:all"].append("extra")), "snapshot line 1 has 7 fields, not six"),
            "an empty line": (whole.replace("\n", "\n\n", 1), "snapshot line 2 has 1 fields, not six"),
            "a package listed twice": (whole + whole.splitlines()[0] + "\n", "snapshot line 279 does not name one package and its version"),
            "a dependency outside the grammar": (edited(lambda: self.depend("libssl-dev:arm64", "libprobe1 [amd64]")),
                                                 "libssl-dev:arm64 has a dependency outside dpkg's grammar: 'libprobe1 [amd64]'"),
            "a substitution left in a dependency": (edited(lambda: self.depend("cmake:arm64", "${shlibs:Depends}")),
                                                    "cmake:arm64 has a dependency outside dpkg's grammar: '${shlibs:Depends}'"),
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
                key, shown = cache.ubuntu_identity(text, UBUNTU_TOOLS, "/usr/lib/pkgconfig", {})
                self.assertEqual(shown[:2], [f"::warning::Ubuntu cache key fell back to the complete package inventory: {cause}",
                                             "Ubuntu native key mode: inventory"])
                self.assertNotEqual(key, closure)
                # Any package's version changes the fallback key, in the closure or
                # not: every line for the four causes a real image could show, a
                # sample of them for the rest.
                every = label in {"a line without six fields", "a dependency outside the grammar",
                                  "a group in which no alternative resolves", "a closure member that is only unpacked"}
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
        self.assertEqual(len(self.rows), 195)
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
        self.assertEqual(held[2:5], (194, 194, 1))
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
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": [["openssl.pc", 291]]}))
        keys.add(self.key(pc_files={"/usr/local/lib/pkgconfig": [["openssl.pc", 292]]}))
        self.assertEqual(len(keys), 1 + 2 * len(TOOL_NAMES) + 5)

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
        self.assertEqual(len(cache.UBUNTU_BUILD_PACKAGES), 12)


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
        self.assertEqual(lines[:start], ["Ubuntu native key mode: closure", "Keyed Ubuntu packages: 195 of 278 installed"])
        keyed = lines[start + 1:end]
        self.assertEqual(keyed, cache.ubuntu_packages(UBUNTU_SNAPSHOT)[1])
        self.assertEqual((len(keyed), keyed[0], keyed[-1]), (195, "binutils-aarch64-linux-gnu:arm64=2.42-4ubuntu2.10", "zlib1g:arm64=1:1.3.dfsg-3.1ubuntu2.3"))
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
                                           f"Keyed pkg-config files in {self.runner.pc}: none", f"Ubuntu native digest: {native}"])
        # The digest is the mode, then exactly the values the lines show.
        hashed = {"packages": keyed, "tools": tools, "pc_path": pc_path, "pc_files": {str(self.runner.pc): []}}
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
        # Later on PATH it is shown and not run, so the key is the dpkg CMake's.
        with patch.dict(os.environ, {"PATH": os.pathsep.join((str(self.runner.bin), str(local)))}), patch("builtins.print") as shown:
            self.assertEqual(cache.native_identity("Linux"), baseline)
        self.assertIn(f"on PATH: {self.runner.bin / 'cmake'}, {local / 'cmake'};", shown.call_args.args[0])

    def test_pc_file_outside_the_dpkg_directories_changes_the_key(self):
        baseline, lines = self.runner.native()
        self.assertEqual([line for line in lines if line.startswith("Keyed pkg-config files in ")],
                         [f"Keyed pkg-config files in {self.runner.pc}: none"])
        # Only .pc files count, by name and size.
        (self.runner.pc / "notes.txt").write_text("not a module")
        self.assertEqual(self.runner.native()[0], baseline)
        (self.runner.pc / "openssl.pc").write_text("Name: OpenSSL\n")
        shadowed, lines = self.runner.native()
        self.assertNotEqual(shadowed, baseline)
        self.assertIn(f"Keyed pkg-config files in {self.runner.pc}: openssl.pc 14", lines)
        (self.runner.pc / "openssl.pc").write_text("Name: OpenSSL\nVersion: 4\n")
        (self.runner.pc / "dav1d.pc").symlink_to("missing.pc")
        grown, lines = self.runner.native()
        self.assertNotIn(grown, {baseline, shadowed})
        self.assertIn(f"Keyed pkg-config files in {self.runner.pc}: dav1d.pc -1, openssl.pc 25", lines)
        # The search order is keyed, and a directory dpkg owns is not listed.
        other = self.base / "elsewhere"
        self.runner.pc_path.write_text(f"/usr/lib/pkgconfig:/usr/share/pkgconfig:{self.runner.pc}:{other}\n")
        reordered, lines = self.runner.native()
        self.assertNotIn(reordered, {baseline, shadowed, grown})
        self.assertEqual([line.split(":")[0] for line in lines if line.startswith("Keyed pkg-config files in ")],
                         [f"Keyed pkg-config files in {self.runner.pc}", f"Keyed pkg-config files in {other}"])

    def test_abnormal_snapshot_warns_first_and_keys_the_whole_inventory(self):
        closure, _ = self.runner.native()
        rows = snapshot_rows()
        rows["libssl3t64:arm64"][1] = "iU "
        self.runner.snapshot.write_text(snapshot_text(rows))
        native, lines = self.runner.native()
        self.assertNotEqual(native, closure)
        self.assertEqual(lines[:4], [
            "::warning::Ubuntu cache key fell back to the complete package inventory: libssl3t64:arm64 is in dpkg state 'iU ', not installed",
            "Ubuntu native key mode: inventory", "Keyed Ubuntu packages: 277 of 277 installed, and 1 other snapshot lines",
            "::group::Keyed Ubuntu packages"])
        self.assertEqual(lines.index("::endgroup::"), 4 + 278)
        self.assertIn("libssl3t64:arm64=3.0.13-0ubuntu3.16 [iU ]", lines)
        self.assertIn("git:arm64=" + rows["git:arm64"][0], lines)
        # A snapshot that cannot be taken fails the step, as a failing command always did.
        self.runner.script(self.runner.bin / "dpkg-query", "exit 2")
        with self.assertRaises(subprocess.CalledProcessError):
            self.runner.native()


if __name__ == "__main__":
    unittest.main()
