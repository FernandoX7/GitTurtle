"""Pinned cargo-nextest installation and the Quality tests-step policy; no network required."""

import contextlib
import email.message
import importlib.util
import hashlib
import io
import os
from pathlib import Path
import re
import socket
import subprocess
import tarfile
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest import mock
import urllib.error
import urllib.parse
import urllib.request
import urllib.response


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("ci_tools", ROOT / "scripts/ci/tools.py")
tools = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tools)


def archive(members):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for name, data, kind in members:
            info = tarfile.TarInfo(name)
            info.type = kind
            if kind == tarfile.SYMTYPE:
                info.linkname = "/bin/sh"
            else:
                info.size = len(data)
            tar.addfile(info, io.BytesIO(data) if kind != tarfile.SYMTYPE else None)
    return buffer.getvalue()


RELEASE_HOST = "github.com"
ASSET_HOST = "release-assets.githubusercontent.com"
# What a release download is answered with: a redirect to a signed asset URL.
SIGNED_ASSET = f"https://{ASSET_HOST}/github-production-release-asset/1/2?sp=r&sig=SIGNED&skoid=KEY"
REDIRECT = (302, {"Location": SIGNED_ASSET}, b"")


class Transport:
    """Every HTTPS request `download` makes, answered by host with no network: (status, headers, body) or an error."""

    def __init__(self, answers):
        self.answers, self.requests = answers, []

    def __call__(self, request):
        self.requests.append(request)
        answer = self.answers[urllib.parse.urlsplit(request.full_url).hostname]
        if isinstance(answer, BaseException):
            raise answer
        status, headers, body = answer
        message = email.message.Message()
        for name, value in headers.items():
            message[name] = value
        response = urllib.response.addinfourl(io.BytesIO(body), message, request.full_url, status)
        response.msg = "status"
        return response

    def hosts(self):
        return [urllib.parse.urlsplit(request.full_url).hostname for request in self.requests]

    @contextlib.contextmanager
    def installed(self):
        with mock.patch.object(urllib.request.HTTPSHandler, "https_open", self), \
                mock.patch.object(urllib.request, "getproxies", return_value={}):
            yield self


def quality_job_steps(name):
    text = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
    job = re.split(r"\n  [a-z][a-z0-9-]*:\n", text.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]
    return job, job.split("\n      - ")[1:]


def rust_debug_steps():
    return quality_job_steps("rust-debug")


def job_header(job, key):
    """A job-level block (`if`, `env`, `strategy`) up to the next job-level key."""
    return re.search(rf"^    {key}:.*\n(?:      .*\n)*", job, re.M).group(0)


def step_index(steps, markers):
    return {name: next(i for i, step in enumerate(steps) if marker in step) for name, marker in markers.items()}


def matrix_axis(job, key):
    """One matrix axis, written as an inline list, as its items' text."""
    found = re.findall(rf"^        {key}: \[(.*)\]$", job_header(job, "strategy"), re.M)
    if len(found) != 1:
        raise AssertionError(f"matrix axis {key} is not one inline list")
    return [item.strip() for item in found[0].split(",")]


def leg_names(job):
    """The names GitHub gives the job's legs: its name with each matrix axis expanded, the first axis slowest."""
    names = [re.search(r"^    name: (.+)$", job, re.M).group(1)]
    for axis in dict.fromkeys(re.findall(r"\$\{\{ matrix\.([a-z-]+) \}\}", names[0])):
        names = [name.replace(f"${{{{ matrix.{axis} }}}}", value) for name in names for value in matrix_axis(job, axis)]
    return names


def step_conditions(steps):
    """Each conditional step's `if`, by the step's name."""
    conditions = {}
    for step in steps:
        found = re.findall(r"^        if: (.*)$", step, re.M)
        if found:
            (conditions[re.match(r"name: (.*)", step).group(1)],) = found
    return conditions


# The two test jobs and the legs their matrices produce: two shards per platform.
TEST_LEGS = {
    "rust-debug": ["Rust tests · macos-15 · 1/2", "Rust tests · macos-15 · 2/2",
                   "Rust tests · ubuntu-24.04 · 1/2", "Rust tests · ubuntu-24.04 · 2/2"],
    "rust-arch": ["Rust tests · archlinux · 1/2", "Rust tests · archlinux · 2/2"],
}
TEST_AXES = {"rust-debug": ["os", "shard", "shards"], "rust-arch": ["shard", "shards"]}
PARTITIONED_TESTS = ('python3 scripts/ci/metrics.py measure --name tests --directory "$RUNNER_TEMP/ci-metrics" -- '
                     'cargo nextest run --locked --workspace -P ci --no-fail-fast --timings '
                     '--partition "slice:$SHARD/$SHARDS"')
SHARD_ARTIFACT = ("ci-diagnostics-${{ github.job }}-${{ runner.os }}-${{ runner.arch }}-shard-${{ matrix.shard }}-"
                  "${{ github.run_id }}-${{ github.run_attempt }}")


class InstallerTests(unittest.TestCase):
    def test_pins_are_exact_versioned_release_archives(self):
        self.assertRegex(tools.NEXTEST_VERSION, r"^\d+\.\d+\.\d+$")
        self.assertEqual(set(tools.NEXTEST_ARCHIVES), {("Linux", "X64"), ("macOS", "ARM64")})
        self.assertTrue(tools.NEXTEST_RELEASE.startswith("https://github.com/nextest-rs/nextest/releases/download/"))
        self.assertIn(f"cargo-nextest-{tools.NEXTEST_VERSION}/", tools.NEXTEST_RELEASE)
        for name, sha256 in tools.NEXTEST_ARCHIVES.values():
            self.assertTrue(name.startswith(f"cargo-nextest-{tools.NEXTEST_VERSION}-") and name.endswith(".tar.gz"))
            self.assertRegex(sha256, r"^[0-9a-f]{64}$")

    def test_verified_member_returns_the_executable_only_for_the_pinned_digest(self):
        data = archive([("cargo-nextest", b"binary", tarfile.REGTYPE)])
        digest = hashlib.sha256(data).hexdigest()
        self.assertEqual(tools.verified_member(data, digest, "cargo-nextest"), b"binary")
        with self.assertRaisesRegex(tools.ToolError, "checksum mismatch"):
            tools.verified_member(data + b"x", digest, "cargo-nextest")

    def test_missing_or_linked_member_is_refused(self):
        for members in ([("other", b"x", tarfile.REGTYPE)], [("cargo-nextest", b"", tarfile.SYMTYPE)]):
            data = archive(members)
            with self.subTest(members=members[0][0]), self.assertRaises(tools.ToolError):
                tools.verified_member(data, hashlib.sha256(data).hexdigest(), "cargo-nextest")
        with self.assertRaises(tools.ToolError):
            tools.verified_member(b"not a tarball", hashlib.sha256(b"not a tarball").hexdigest(), "cargo-nextest")

    def test_download_is_one_anonymous_request_and_an_oversized_body_is_refused(self):
        url = tools.NEXTEST_RELEASE + "archive.tar.gz"
        with Transport({RELEASE_HOST: REDIRECT, ASSET_HOST: (200, {}, b"abc")}).installed() as transport:
            self.assertEqual(tools.download(url, limit=3), b"abc")
        self.assertEqual(transport.hosts(), [RELEASE_HOST, ASSET_HOST])
        # No token and no header of ours on either hop.
        for request in transport.requests:
            self.assertEqual({name.lower() for name in {**request.headers, **request.unredirected_hdrs}} - {"host", "user-agent"}, set())
        with Transport({RELEASE_HOST: (200, {}, b"abcd")}).installed() as transport:
            with self.assertRaisesRegex(tools.ToolError, "size limit"):
                tools.download(url, limit=3)
        self.assertEqual(len(transport.requests), 1)
        self.assertFalse(hasattr(tools, "ATTEMPTS"))

    def test_one_failed_request_is_not_followed_by_a_second(self):
        url = tools.NEXTEST_RELEASE + "archive.tar.gz"
        for answers, hosts in (
                ({RELEASE_HOST: (504, {}, b"")}, [RELEASE_HOST]),
                ({RELEASE_HOST: urllib.error.URLError(ConnectionResetError("reset"))}, [RELEASE_HOST]),
                ({RELEASE_HOST: TimeoutError("timed out")}, [RELEASE_HOST]),
                # One request a hop: the redirect's target fails and nothing is asked again.
                ({RELEASE_HOST: REDIRECT, ASSET_HOST: (504, {}, b"")}, [RELEASE_HOST, ASSET_HOST]),
                ({RELEASE_HOST: REDIRECT, ASSET_HOST: (500, {}, b"")}, [RELEASE_HOST, ASSET_HOST])):
            with self.subTest(answers=answers), Transport(answers).installed() as transport:
                with self.assertRaises(tools.ToolError):
                    tools.download(url)
                self.assertEqual(transport.hosts(), hosts)

    def test_a_failure_names_the_failing_hops_host_and_status_or_error_class_and_never_a_query(self):
        url = tools.NEXTEST_RELEASE + "archive.tar.gz"
        unresolved = urllib.error.URLError(socket.gaierror(-2, "Name or service not known"))
        for answers, message in (
                ({RELEASE_HOST: REDIRECT, ASSET_HOST: (504, {}, b"")}, f"tool download failed: HTTP 504 from {ASSET_HOST}"),
                ({RELEASE_HOST: (504, {}, b"")}, f"tool download failed: HTTP 504 from {RELEASE_HOST}"),
                ({RELEASE_HOST: REDIRECT, ASSET_HOST: unresolved}, f"tool download failed: gaierror from {ASSET_HOST}"),
                ({RELEASE_HOST: unresolved}, f"tool download failed: gaierror from {RELEASE_HOST}"),
                ({RELEASE_HOST: REDIRECT, ASSET_HOST: TimeoutError("timed out")}, f"tool download failed: TimeoutError from {ASSET_HOST}"),
                # A reason that is text and not an error: the error's own class.
                ({RELEASE_HOST: urllib.error.URLError(f"unknown url type: {SIGNED_ASSET}")}, f"tool download failed: URLError from {RELEASE_HOST}"),
                # A host that is no hostname is not printed either.
                ({RELEASE_HOST: (302, {"Location": "https://bad_host/x?sig=SIGNED"}, b""), "bad_host": (504, {}, b"")},
                 "tool download failed: HTTP 504 from an unnamed host")):
            with self.subTest(message=message), Transport(answers).installed():
                with self.assertRaises(tools.ToolError) as failed:
                    tools.download(url)
                self.assertEqual(str(failed.exception), message)
                for leaked in ("?", "SIGNED", "sig=", "github-production-release-asset"):
                    self.assertNotIn(leaked, str(failed.exception))
        # The step's own line is that message and nothing more.
        with Transport({RELEASE_HOST: REDIRECT, ASSET_HOST: (504, {}, b"")}).installed(), \
                contextlib.redirect_stdout(io.StringIO()) as output, contextlib.redirect_stderr(io.StringIO()) as errors:
            status = tools.main(["install-nextest", "--directory", "unused", "--os", "Linux", "--arch", "X64"])
        self.assertEqual((status, output.getvalue(), errors.getvalue()),
                         (1, "", f"error: tool download failed: HTTP 504 from {ASSET_HOST}\n"))

    ARCHIVE = "cargo-nextest-test.tar.gz"
    VERSION_LINE = f"cargo-nextest {tools.NEXTEST_VERSION} (abc 2026-09-21)"

    ARCHIVE_BYTES = archive([("cargo-nextest", b"#!/bin/sh\n", tarfile.REGTYPE)])

    def archive_bytes(self):
        return self.ARCHIVE_BYTES

    def install(self, directory, version_output, platform=("Linux", "X64"), returncode=0, *, cache=None, fetched=None):
        """One install with the pin set to a small archive; `cache` is the --archive-cache, `fetched` what a fetch returns."""
        data = self.archive_bytes()
        pins = {platform: (self.ARCHIVE, hashlib.sha256(data).hexdigest())}
        completed = subprocess.CompletedProcess([], returncode, stdout=version_output, stderr="")
        path_file = Path(directory) / "github-path"
        arguments = [] if cache is None else ["--archive-cache", str(cache)]
        with mock.patch.object(tools, "NEXTEST_ARCHIVES", pins), \
                mock.patch.object(tools, "download", return_value=data if fetched is None else fetched) as download, \
                mock.patch.object(tools.subprocess, "run", return_value=completed) as run, \
                contextlib.redirect_stdout(io.StringIO()) as output, contextlib.redirect_stderr(io.StringIO()) as errors:
            status = tools.main(["install-nextest", "--directory", str(Path(directory) / "bin"),
                                 "--path-file", str(path_file), "--os", "Linux", "--arch", "X64", *arguments])
        return SimpleNamespace(status=status, path_file=path_file, run=run, download=download, output=output.getvalue(),
                               errors=errors.getvalue(), data=data, sha256=pins[platform][1])

    def tree(self, directory):
        """Every path under a directory, with a link's target or a file's bytes."""
        return {str(path.relative_to(directory)): (os.readlink(path) if path.is_symlink() else path.read_bytes() if path.is_file() else None)
                for path in sorted(Path(directory).rglob("*"))}

    def test_a_restored_archive_is_verified_and_used_with_no_request(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            cache = Path(directory) / "cargo/registry/archives"
            cache.mkdir(parents=True)
            (cache / self.ARCHIVE).write_bytes(self.archive_bytes())
            before = self.tree(cache)
            result = self.install(directory, self.VERSION_LINE + "\n", cache=cache)
            self.assertEqual((result.status, result.errors), (0, ""))
            result.download.assert_not_called()
            self.assertEqual(result.output, f"{self.VERSION_LINE}; {self.ARCHIVE} sha256 {result.sha256}; archive from: cache\n")
            # The executable is unpacked where --directory says, and the cache still holds the archive alone.
            self.assertEqual((Path(directory) / "bin/cargo-nextest").read_bytes(), b"#!/bin/sh\n")
            self.assertEqual(self.tree(cache), before)
            self.assertEqual(list(before), [self.ARCHIVE])

    def test_a_miss_is_one_fetch_whose_verified_archive_is_stored_by_rename_for_the_next_run(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            cache = Path(directory) / "cargo/registry/archives"
            with mock.patch.object(tools.os, "rename", wraps=os.rename) as rename:
                first = self.install(directory, self.VERSION_LINE + "\n", cache=cache)
            self.assertEqual((first.status, first.errors), (0, ""))
            first.download.assert_called_once_with(tools.NEXTEST_RELEASE + self.ARCHIVE)
            self.assertEqual(first.output, f"{self.VERSION_LINE}; {self.ARCHIVE} sha256 {first.sha256}; archive from: fetch\n")
            # The archive and nothing else: no executable and no temporary file.
            self.assertEqual(self.tree(cache), {self.ARCHIVE: first.data})
            self.assertEqual([call.args for call in rename.call_args_list], [(f".{self.ARCHIVE}.partial", self.ARCHIVE)])
            self.assertTrue(all(call.kwargs["src_dir_fd"] == call.kwargs["dst_dir_fd"] for call in rename.call_args_list))
            second = self.install(directory, self.VERSION_LINE + "\n", cache=cache)
            second.download.assert_not_called()
            self.assertTrue(second.output.endswith("; archive from: cache\n"))
            # A temporary file that a killed run left is replaced, never trusted.
            (cache / self.ARCHIVE).unlink()
            (cache / f".{self.ARCHIVE}.partial").write_bytes(b"half")
            self.assertEqual(self.install(directory, self.VERSION_LINE + "\n", cache=cache).status, 0)
            self.assertEqual(self.tree(cache), {self.ARCHIVE: first.data})

    def test_a_fetched_archive_with_another_digest_is_never_stored(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            cache = Path(directory) / "cargo/registry/archives"
            result = self.install(directory, self.VERSION_LINE + "\n", cache=cache, fetched=self.archive_bytes() + b"x")
            self.assertEqual(result.status, 1)
            self.assertRegex(result.errors, rf"^error: checksum mismatch for fetched archive {re.escape(self.ARCHIVE)}: "
                                            rf"expected {result.sha256}, got [0-9a-f]{{64}}\n$")
            self.assertEqual(self.tree(directory), {})
            # Without a cache directory nothing is stored anywhere, as before.
            plain = self.install(directory, self.VERSION_LINE + "\n")
            self.assertEqual(plain.status, 0)
            plain.download.assert_called_once()
            self.assertTrue(plain.output.endswith("; archive from: fetch\n"))
            self.assertEqual(sorted(self.tree(directory)), ["bin", "bin/cargo-nextest", "github-path"])

    def test_a_cached_archive_that_is_not_the_pinned_regular_file_fails_by_name_and_is_left_as_it_is(self):
        good = self.archive_bytes()

        def wrong_digest(path, base):
            path.write_bytes(good + b"x")

        def link(path, base):
            (base / "elsewhere.tar.gz").write_bytes(good)
            path.symlink_to(base / "elsewhere.tar.gz")

        def dangling(path, base):
            path.symlink_to(base / "missing.tar.gz")

        def directory_there(path, base):
            path.mkdir()

        def oversized(path, base):
            path.write_bytes(good)

        name = re.escape(self.ARCHIVE)
        cases = {
            "another digest": (wrong_digest, rf"checksum mismatch for cached archive {name}, which is left in place: "
                                             r"expected [0-9a-f]{64}, got [0-9a-f]{64}", None),
            "a link to the pinned archive": (link, rf"cached archive {name} is a link or cannot be opened: ELOOP", None),
            "a dangling link": (dangling, rf"cached archive {name} is a link or cannot be opened: ELOOP", None),
            "a directory": (directory_there, rf"cached archive {name} is not a regular file", None),
            "an oversized file": (oversized, rf"cached archive {name} exceeds its size limit", len(good) - 1),
        }
        for label, (place, message, limit) in cases.items():
            with self.subTest(case=label), tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
                base = Path(directory)
                cache = base / "cargo/registry/archives"
                cache.mkdir(parents=True)
                place(cache / self.ARCHIVE, base)
                before = self.tree(base)
                with mock.patch.object(tools, "MAX_ARCHIVE", limit or tools.MAX_ARCHIVE), \
                        mock.patch.object(tools.tarfile, "open") as parsed, \
                        mock.patch.object(tools.hashlib, "sha256", wraps=hashlib.sha256) as hashed:
                    result = self.install(directory, self.VERSION_LINE + "\n", cache=cache)
                self.assertEqual(result.status, 1)
                self.assertRegex(result.errors, rf"^error: {message}\n$")
                # Not replaced by a fetch, not parsed, and nothing written or removed.
                result.download.assert_not_called()
                parsed.assert_not_called()
                self.assertEqual(self.tree(base), before)
                self.assertFalse(result.path_file.exists())
                # Only the regular file within the bound was read as far as its hash.
                self.assertEqual(hashed.call_count > 1, label == "another digest")

    def test_the_archive_cache_must_be_a_named_directory_of_its_own(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            base = Path(directory)
            (base / "real").mkdir()
            (base / "real" / self.ARCHIVE).write_bytes(self.archive_bytes())
            (base / "alias").symlink_to(base / "real")
            result = self.install(directory, self.VERSION_LINE + "\n", cache=base / "alias")
            # Linux answers ENOTDIR for a link opened as a directory without following it, macOS ELOOP.
            self.assertEqual(result.status, 1)
            self.assertRegex(result.errors, r"^error: archive cache is not a directory of its own: (ENOTDIR|ELOOP)\n$")
            result.download.assert_not_called()
            # An empty value is a setup that exported no directory: it never means "fetch every time".
            result = self.install(directory, self.VERSION_LINE + "\n", cache="")
            self.assertEqual(result.status, 1)
            self.assertIn("archive cache directory is empty", result.errors)
            result.download.assert_not_called()

    def test_install_adds_the_directory_to_path_only_after_cargo_resolves_the_pin(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            result = self.install(directory, f"cargo-nextest {tools.NEXTEST_VERSION} (abc 2026-09-21)\n")
            self.assertEqual(result.status, 0)
            binary = Path(directory) / "bin/cargo-nextest"
            self.assertEqual(binary.read_bytes(), b"#!/bin/sh\n")
            self.assertTrue(binary.stat().st_mode & 0o111)
            self.assertEqual(result.path_file.read_text(), f"{binary.parent.resolve()}\n")
            self.assertEqual(result.run.call_args.args[0], ["cargo", "nextest", "--version"])
            self.assertTrue(result.run.call_args.kwargs["env"]["PATH"].startswith(str(binary.parent.resolve())))

    def test_a_different_resolved_version_or_platform_fails_without_touching_path(self):
        with tempfile.TemporaryDirectory(dir=ROOT / "scripts/ci/tests") as directory:
            result = self.install(directory, "cargo-nextest 0.9.100 (old)\n")
            self.assertEqual(result.status, 1)
            self.assertFalse(result.path_file.exists())
            result = self.install(directory, "", returncode=101)
            self.assertEqual(result.status, 1)
            result = self.install(directory, "", platform=("Linux", "ARM64"))
            self.assertEqual(result.status, 1)
            self.assertFalse(result.path_file.exists())


class WorkflowPolicyTests(unittest.TestCase):
    def test_tests_run_with_pinned_nextest_then_doctests_before_finish(self):
        job, steps = rust_debug_steps()
        index = {name: next(i for i, step in enumerate(steps) if marker in step) for name, marker in {
            "refs": "git pack-refs --all --no-prune",
            "setup": "uses: ./.github/actions/setup-rust\n        with:\n          profile: debug",
            "install": "scripts/ci/tools.py install-nextest",
            "tests": "cargo nextest run",
            "doctests": "--name doctests ",
            "finish": "phase: finish",
        }.items()}
        self.assertEqual(sorted(index, key=index.get), ["refs", "setup", "install", "tests", "doctests", "finish"])
        # Clippy has its own job, so the test job's path is the build and the tests.
        self.assertNotIn("cargo clippy", job)
        self.assertTrue(steps[index["tests"]].rstrip().endswith(PARTITIONED_TESTS))
        # Default selection reuses the nextest build; the filter matches only
        # doctest names (`src/lib.rs - item (line 3)`), never a test path.
        self.assertTrue(steps[index["doctests"]].rstrip().endswith(
            '-- cargo test --locked --workspace --timings -- "(line "'))
        self.assertIn('--path-file "$GITHUB_PATH"', steps[index["install"]])
        # The only other Cargo test invocation in the job runs doctests.
        self.assertEqual(job.count("cargo test "), 1)
        # Every shard installs nextest and runs its tests; only shard 1 runs the doctests.
        for name in ("install", "tests"):
            self.assertNotIn("\n        if:", steps[index[name]])
        self.assertIn("\n        if: matrix.shard == 1\n", steps[index["doctests"]])
        self.assertNotIn("permissions:", job)

    def test_clippy_runs_beside_the_tests_and_only_restores_their_debug_cache(self):
        job, steps = quality_job_steps("rust-clippy")
        debug, _ = rust_debug_steps()
        # Every product change, on the test job's platforms, with its environment:
        # the cache key reads build flags from the environment, so a different
        # one would restore nothing.
        for key in ("if", "env"):
            self.assertEqual(job_header(job, key), job_header(debug, key), key)
        # The same platforms; the test job's matrix also has its shards.
        self.assertEqual(matrix_axis(job, "os"), matrix_axis(debug, "os"))
        self.assertEqual(job_header(job, "strategy"),
                         "    strategy:\n      fail-fast: false\n      max-parallel: 2\n      matrix:\n"
                         "        os: [macos-15, ubuntu-24.04]\n")
        self.assertEqual({use.split("@")[0] for use in re.findall(r"\buses: (\S+)", job)},
                         {"actions/checkout", "actions/upload-artifact", "./.github/actions/setup-rust"})
        setup = [step for step in steps if "uses: ./.github/actions/setup-rust" in step]
        self.assertEqual(len(setup), 1)
        self.assertEqual({line.strip() for line in setup[0].split("\n        with:\n", 1)[1].splitlines()},
                         {"profile: debug", "vendor-reuse: true"})
        # No finish phase, so no save: the test job alone writes the debug entry
        # it restores, and this job's checked dependencies never replace its build.
        self.assertNotIn("phase: finish", job)
        self.assertNotIn("Swatinem/rust-cache", job)
        index = step_index(steps, {
            "checkout": "uses: actions/checkout@",
            "setup": "uses: ./.github/actions/setup-rust",
            "clippy": "cargo clippy",
            "summary": "metrics.py summary",
        })
        self.assertEqual(sorted(index, key=index.get), list(index))
        self.assertTrue(steps[index["clippy"]].rstrip().endswith(
            '--name clippy --directory "$RUNNER_TEMP/ci-metrics" -- '
            'cargo clippy --locked --workspace --all-targets --timings -- -D warnings'))
        self.assertNotIn("\n        if:", steps[index["clippy"]])
        self.assertEqual(job.count("cargo "), 1)
        self.assertNotIn("permissions:", job)
        self.assertRegex(job, r"\n    timeout-minutes: \d+\n")

    def test_rust_test_jobs_keep_their_60_minute_limit(self):
        # Runners have been seen 1.8 times slower, which takes the Arch job's
        # 28 minutes to about 50: past 45 (docs/ci.md#test-execution-with-nextest).
        # Sharding leaves the limit where it was: a shard still builds the
        # whole workspace, and no step carries a limit of its own.
        for name in ("rust-debug", "rust-arch"):
            job, _ = quality_job_steps(name)
            self.assertEqual(re.findall(r"^    timeout-minutes: (.*)$", job, re.M), ["60"], name)
            self.assertEqual(len(re.findall(r"^ *timeout-minutes:", job, re.M)), 1, name)

    def test_each_test_job_gives_every_shard_of_its_matrix_one_nextest_slice(self):
        for name in TEST_LEGS:
            with self.subTest(job=name):
                job, steps = quality_job_steps(name)
                (tests,) = [step for step in steps if "cargo nextest run" in step]
                # The unsharded command and the partition alone: no filter,
                # package or target selection, other profile or retry option.
                self.assertEqual(re.findall(r"^        run: (.*)$", tests, re.M), [PARTITIONED_TESTS])
                self.assertIn("\n        env:\n          SHARD: ${{ matrix.shard }}\n"
                              "          SHARDS: ${{ matrix.shards }}\n        run: ", tests)
                self.assertNotIn("\n        if:", tests)
                self.assertEqual((job.count("--partition"), job.count("cargo nextest ")), (1, 1))
                # The total is written once, and the shards are exactly 1 to
                # that total: a shorter list would leave a slice's tests unrun.
                (total,) = matrix_axis(job, "shards")
                self.assertEqual(matrix_axis(job, "shard"), [str(shard) for shard in range(1, int(total) + 1)])
                self.assertEqual(re.findall(r"^        ([a-z-]+):", job_header(job, "strategy"), re.M), TEST_AXES[name])

    def test_the_test_legs_are_two_shards_per_platform_and_all_start_at_once(self):
        for name, legs in TEST_LEGS.items():
            with self.subTest(job=name):
                job, _ = quality_job_steps(name)
                self.assertEqual(leg_names(job), legs)
                strategy = job_header(job, "strategy")
                self.assertIn("\n      fail-fast: false\n", strategy)
                self.assertIn(f"\n      max-parallel: {len(legs)}\n", strategy)

    def test_shard_1_alone_runs_the_doctests_and_the_cache_save_and_every_linux_shard_the_libdav1d_records(self):
        # A condition without a status function also requires every earlier
        # step to have succeeded, so the save still follows shard 1's tests.
        once = {"Locked workspace doctests": "matrix.shard == 1",
                "Prepare bounded successful-main debug cache save": "matrix.shard == 1"}
        always = {"Summarize CI measurements": "always()",
                  "Preserve CI measurements and sanitized diagnostics": "always()"}
        # Each shard's runner or container installs its own libdav1d, and the
        # helper tests run in whichever shard the slice puts them: a shard
        # condition would leave a log without the records of the library its
        # tests met. Only the macOS legs skip the step; the Arch job has none.
        records = {"rust-debug": {"Print the libdav1d link and initializers": "runner.os == 'Linux'"}, "rust-arch": {}}
        for name in TEST_LEGS:
            with self.subTest(job=name):
                job, steps = quality_job_steps(name)
                # Every other step, the tests among them, runs in every shard.
                self.assertEqual(step_conditions(steps), {**records[name], **once, **always})
                index = step_index(steps, {"records": "readelf -d -W", "doctests": "--name doctests ",
                                           "finish": "phase: finish"})
                self.assertEqual([steps[position].split("\n", 1)[0] for position in index.values()],
                                 ["name: Print the libdav1d link and initializers", "name: Locked workspace doctests",
                                  "name: Prepare bounded successful-main debug cache save"])
                # One finish phase, so one writer of the platform's debug entry:
                # the restore in the setup phase never saves (`save-if: false`).
                self.assertEqual((job.count("phase: finish"), job.count("uses: ./.github/actions/setup-rust")), (1, 2))
                self.assertNotIn("Swatinem/rust-cache", job)
                self.assertNotIn("actions/cache", job)

    def test_no_shard_value_reaches_the_rust_cache_key(self):
        # cache.py keys an entry on the profile, the container, tracked build
        # inputs, native packages, rustc and compiler-flag environment, and
        # rust-cache adds the environment under its prefixes. A shard number in
        # the job's environment or the setup action's inputs could give one
        # platform's shards different keys from each other and from Clippy's.
        for name in TEST_LEGS:
            with self.subTest(job=name):
                job, steps = quality_job_steps(name)
                self.assertNotRegex(job_header(job, "env"), r"matrix\.|strategy\.|SHARD")
                setup = [step.split("\n      #")[0] for step in steps if "uses: ./.github/actions/setup-rust" in step]
                self.assertEqual(len(setup), 2)
                for step in setup:
                    self.assertNotIn("${{", step)
                    self.assertNotIn("env:", step)
                self.assertEqual([step.split("\n", 1)[0] for step in steps if "SHARD" in step],
                                 ["name: Locked workspace tests with nextest"])

    def test_each_shard_uploads_its_diagnostics_under_its_own_artifact_name(self):
        # Both shards of a platform share the job id, the OS and the
        # architecture, and the upload action refuses a name already taken.
        for name in TEST_LEGS:
            with self.subTest(job=name):
                job, _ = quality_job_steps(name)
                self.assertEqual(re.findall(r"^          name: (.*)$", job, re.M), [SHARD_ARTIFACT])

    def test_a_failed_or_dropped_shard_cannot_leave_its_matrix_successful(self):
        # Quality gate reads one result per needed job id (scripts/ci/changes.py
        # `gate`), and a matrix's is success only when every leg succeeded. Each
        # of these would break that: a leg allowed to fail, a leg removed from
        # the matrix, or a condition that skips the job for a product change.
        for name in TEST_LEGS:
            with self.subTest(job=name):
                job, steps = quality_job_steps(name)
                self.assertEqual(re.findall(r"^    if: (.*)$", job, re.M), ["needs.changes.outputs.product == 'true'"])
                for unwanted in ("continue-on-error", "include:", "exclude:"):
                    self.assertNotIn(unwanted, job)
        gate, _ = quality_job_steps("quality")
        self.assertIn("\n    if: always()\n", gate)
        needs = re.search(r"^    needs: \[(.*)\]$", gate, re.M).group(1).split(", ")
        self.assertLessEqual(set(TEST_LEGS), set(needs))
        self.assertIn("          NEEDS_JSON: ${{ toJSON(needs) }}\n        run: python3 scripts/ci/changes.py gate\n", gate)

    def test_arch_job_runs_the_debug_test_phases_unprivileged_in_a_pinned_image_with_the_shared_cache(self):
        job, steps = quality_job_steps("rust-arch")
        _, debug_steps = rust_debug_steps()
        self.assertRegex(job, r"\n    container:\n      image: archlinux:base-devel@sha256:[0-9a-f]{64}\n")
        # The cache goes only through setup-rust, whose save is main-only.
        self.assertEqual({use.split("@")[0] for use in re.findall(r"\buses: (\S+)", job)},
                         {"actions/checkout", "actions/upload-artifact", "./.github/actions/setup-rust"})
        setup = [step for step in steps if "uses: ./.github/actions/setup-rust" in step]
        self.assertEqual(len(setup), 2)
        for step in setup:
            options = {line.strip() for line in step.splitlines()}
            self.assertLessEqual({"profile: debug", "container: archlinux", "run-as: builder"}, options)
        self.assertIn("          vendor-reuse: true", setup[0].splitlines())
        self.assertIn("          phase: finish", setup[1].splitlines())
        # The action owns CARGO_HOME; the job keeps only rustup's home.
        self.assertNotIn("CARGO_HOME=", job)
        self.assertRegex(job, r"\n    timeout-minutes: \d+\n")
        # Root bypasses file permissions, which permission-refusal tests rely on;
        # only package installation and ownership setup keep the root shell.
        self.assertIn("\n        shell: setpriv --reuid=builder --regid=builder --init-groups -- bash ", job)
        root = [step for step in steps if "\n        shell: " in step]
        self.assertEqual(len(root), 3)
        for step in root:
            self.assertTrue(any(command in step for command in ("pacman -Su", "useradd ", "chown -R builder")), step)
        index = step_index(steps, {
            "packages": "pacman -Su",
            "user": "useradd ",
            "checkout": "uses: actions/checkout@",
            "owner": "chown -R builder",
            "refs": "git pack-refs --all --no-prune",
            "toolchain": "rustup toolchain install",
            "setup": "vendor-reuse: true",
            "install": "scripts/ci/tools.py install-nextest",
            "tests": "cargo nextest run",
            "doctests": "--name doctests ",
            "finish": "phase: finish",
            "summary": "metrics.py summary",
        })
        self.assertEqual(sorted(index, key=index.get), list(index))
        # Git must be installed before checkout, or checkout downloads no .git.
        self.assertRegex(steps[index["packages"]], r"\bgit\b")
        self.assertIn("persist-credentials: false", steps[index["checkout"]])
        debug = step_index(debug_steps, {
            "install": "scripts/ci/tools.py install-nextest",
            "tests": "cargo nextest run",
            "doctests": "--name doctests ",
        })
        # The same commands as the Ubuntu and macOS test phases; a comment that
        # introduces the next step is not part of this one.
        for name, position in debug.items():
            self.assertEqual(steps[index[name]].split("\n      #")[0].rstrip(),
                             debug_steps[position].split("\n      #")[0].rstrip(), name)

    def test_quality_keeps_read_only_permissions_and_no_third_party_installer(self):
        text = (ROOT / ".github/workflows/quality.yml").read_text(encoding="utf-8")
        blocks = re.findall(r"\n( *)permissions:\n((?:\1  .*\n)+)", text)
        self.assertTrue(blocks)
        for _, grants in blocks:
            self.assertEqual(grants.split(), ["contents:", "read"])
        self.assertNotRegex(text, r"uses: (?!actions/|\./)\S+")

    def test_ci_profile_reports_every_test_and_never_retries_or_stops_early(self):
        config = tomllib.loads((ROOT / ".config/nextest.toml").read_text(encoding="utf-8"))["profile"]["ci"]
        self.assertEqual((config["fail-fast"], config["retries"]), (False, 0))
        self.assertEqual((config["failure-output"], config["status-level"]), ("immediate", "skip"))


if __name__ == "__main__":
    unittest.main()
