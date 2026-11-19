#!/usr/bin/env python3
"""Install pinned CI tools from checksum-verified release archives (stdlib only)."""

import argparse
import errno
import hashlib
import http.client
import io
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tarfile
import urllib.error
import urllib.parse
import urllib.request


# cargo-nextest 0.9.152 (2026-11-13). Its Starting, result and Summary lines have
# the shapes of 0.9.146's, which scripts/ci/executed_tests.py reads: checked on
# real output of both. Digests match the release's published `.sha256` assets and
# GitHub's asset digests; see docs/ci.md. The pin is part of the debug cache key.
NEXTEST_VERSION = "0.9.152"
NEXTEST_RELEASE = f"https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-{NEXTEST_VERSION}/"
NEXTEST_ARCHIVES = {
    ("Linux", "X64"): ("cargo-nextest-0.9.152-x86_64-unknown-linux-gnu.tar.gz",
                       "d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a"),
    # Upstream publishes one universal (arm64 and x86_64) macOS archive.
    ("macOS", "ARM64"): ("cargo-nextest-0.9.152-universal-apple-darwin.tar.gz",
                         "2fadf57df4243e691f088ee3bff4f4ab9af16da0c95a1db84b5cb1685e098a76"),
}
NEXTEST_MEMBER = "cargo-nextest"
MAX_ARCHIVE = 64 << 20
MAX_EXECUTABLE = 128 << 20
HOSTNAME = re.compile(r"[A-Za-z0-9.-]{1,253}")


class ToolError(Exception):
    pass


class Hops(urllib.request.HTTPRedirectHandler):
    """The host of every request one download makes, so a failure can name the hop that failed."""

    def __init__(self, url):
        self.hosts = [hostname(url)]

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        request = super().redirect_request(req, fp, code, msg, headers, newurl)
        if request is not None:
            self.hosts.append(hostname(newurl))
        return request


def hostname(url):
    """A URL's host alone, for a log line: never its path or query, which can be signed."""
    try:
        host = urllib.parse.urlsplit(url or "").hostname or ""
    except ValueError:
        host = ""
    return host if HOSTNAME.fullmatch(host) else "an unnamed host"


def download(url, limit=MAX_ARCHIVE):
    """Fetch at most `limit` bytes over HTTPS in one anonymous attempt.

    Nothing is retried: a failed request fails the step. The failure names the
    host of the hop that failed with the HTTP status or the error's class, and
    no other part of a URL. The release redirects to a signed asset URL, an
    HTTPError's text names no host, and its `url` carries the signed query.
    """
    hops = Hops(url)
    try:
        with urllib.request.build_opener(hops).open(url, timeout=60) as response:
            if not response.geturl().startswith("https://"):
                raise ToolError("tool download left HTTPS")
            data = response.read(limit + 1)
    except urllib.error.HTTPError as error:
        error.close()
        host = hostname(error.url) if error.url else hops.hosts[-1]
        raise ToolError(f"tool download failed: HTTP {error.code} from {host}") from None
    except (urllib.error.URLError, http.client.HTTPException, TimeoutError, ConnectionError) as error:
        reason = getattr(error, "reason", None)
        kind = type(reason if isinstance(reason, BaseException) else error).__name__
        raise ToolError(f"tool download failed: {kind} from {hops.hosts[-1]}") from None
    if len(data) > limit:
        raise ToolError("tool archive exceeds its size limit")
    return data


def check_digest(data, expected_sha256, what):
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected_sha256:
        raise ToolError(f"checksum mismatch for {what}: expected {expected_sha256}, got {actual}")


def archive_directory(directory, *, create=False):
    """A descriptor of the cache directory itself, never of a link's target; None when there is none."""
    if create:
        Path(directory).mkdir(parents=True, exist_ok=True)
    try:
        return os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    except FileNotFoundError:
        return None
    except OSError as error:
        raise ToolError(f"archive cache is not a directory of its own: {errno.errorcode.get(error.errno, error.errno)}") from None


def cached_archive(directory, name, expected_sha256):
    """The pinned archive from the restored cache directory, or None when it holds none.

    The cache is not trusted: the file is opened without following a link, must
    be a regular file no larger than MAX_ARCHIVE, and is hashed against the pin
    before anything parses it. Whatever else is there fails the step by name and
    stays as it is: it is never replaced by a fetch.
    """
    parent = archive_directory(directory)
    if parent is None:
        return None
    try:
        try:
            descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        except FileNotFoundError:
            return None
        except OSError as error:
            raise ToolError(f"cached archive {name} is a link or cannot be opened: "
                            f"{errno.errorcode.get(error.errno, error.errno)}") from None
        try:
            info = os.fstat(descriptor)
            if not stat.S_ISREG(info.st_mode):
                raise ToolError(f"cached archive {name} is not a regular file")
            if info.st_size > MAX_ARCHIVE:
                raise ToolError(f"cached archive {name} exceeds its size limit")
            with os.fdopen(descriptor, "rb", closefd=False) as stream:
                data = stream.read(MAX_ARCHIVE + 1)
        finally:
            os.close(descriptor)
    finally:
        os.close(parent)
    if len(data) > MAX_ARCHIVE:
        raise ToolError(f"cached archive {name} exceeds its size limit")
    check_digest(data, expected_sha256, f"cached archive {name}, which is left in place")
    return data


def store_archive(directory, name, data):
    """Keep a verified archive for the cache save: a temporary name in the directory, then a rename."""
    parent = archive_directory(directory, create=True)
    if parent is None:
        raise ToolError("archive cache directory could not be created")
    temporary = f".{name}.partial"
    try:
        try:
            os.unlink(temporary, dir_fd=parent)
        except FileNotFoundError:
            pass
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644, dir_fd=parent)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        os.rename(temporary, name, src_dir_fd=parent, dst_dir_fd=parent)
    finally:
        os.close(parent)


def pinned_archive(url, name, expected_sha256, directory):
    """The pinned archive's bytes and where they came from: `cache`, with no request, or one `fetch`.

    A fetched archive enters the cache directory only after its digest matched.
    """
    if directory is not None:
        data = cached_archive(directory, name, expected_sha256)
        if data is not None:
            return data, "cache"
    data = download(url)
    check_digest(data, expected_sha256, f"fetched archive {name}")
    if directory is not None:
        store_archive(directory, name, data)
    return data, "fetch"


def verified_member(data, expected_sha256, member):
    """Return one regular file's bytes from a gzip tarball whose digest matches."""
    check_digest(data, expected_sha256, "the archive")
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            info = archive.getmember(member)
            if not info.isfile() or info.size > MAX_EXECUTABLE:
                raise ToolError(f"{member} is not a bounded regular file in the archive")
            return archive.extractfile(info).read()
    except (KeyError, tarfile.TarError) as error:
        raise ToolError(f"archive does not contain a readable {member}: {error}") from None


def install_nextest(args):
    platform = (args.os or os.environ.get("RUNNER_OS", ""), args.arch or os.environ.get("RUNNER_ARCH", ""))
    if platform not in NEXTEST_ARCHIVES:
        raise ToolError(f"no pinned cargo-nextest archive for {platform[0] or '?'} {platform[1] or '?'}")
    name, sha256 = NEXTEST_ARCHIVES[platform]
    if args.archive_cache == "":
        raise ToolError("the archive cache directory is empty: the setup action exports it for the debug profile")
    # The cache holds the archive alone. The executable is unpacked into
    # --directory, outside every cached path, from bytes whose digest matched.
    data, source = pinned_archive(NEXTEST_RELEASE + name, name, sha256, args.archive_cache)
    executable = verified_member(data, sha256, NEXTEST_MEMBER)
    directory = Path(args.directory)
    directory.mkdir(parents=True, exist_ok=True)
    staged = directory / f".{NEXTEST_MEMBER}.partial"
    staged.write_bytes(executable)
    staged.chmod(0o755)
    target = directory / NEXTEST_MEMBER
    os.replace(staged, target)
    # Resolve through Cargo's subcommand lookup with the directory first on PATH,
    # as later steps will, so a different cargo-nextest cannot shadow the pin.
    env = {**os.environ, "PATH": f"{directory.resolve()}{os.pathsep}{os.environ.get('PATH', '')}"}
    try:
        version = subprocess.run(["cargo", "nextest", "--version"], env=env, capture_output=True,
                                 text=True, timeout=60, check=False)
    except (OSError, subprocess.TimeoutExpired):
        raise ToolError("cargo nextest --version could not run") from None
    first = version.stdout.splitlines()[0] if version.stdout else ""
    if version.returncode != 0 or not first.startswith(f"cargo-nextest {NEXTEST_VERSION} "):
        raise ToolError(f"cargo nextest resolves to {first or 'nothing'}, not {NEXTEST_VERSION}")
    print(f"{first}; {name} sha256 {sha256}; archive from: {source}")
    if args.path_file:
        with open(args.path_file, "a", encoding="utf-8") as stream:
            stream.write(f"{directory.resolve()}\n")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    nextest = commands.add_parser("install-nextest", help=f"install cargo-nextest {NEXTEST_VERSION} for this runner")
    nextest.add_argument("--directory", required=True, help="fresh directory that receives the executable")
    nextest.add_argument("--path-file", help="append the directory to this file (GITHUB_PATH)")
    nextest.add_argument("--archive-cache", help="directory the Rust dependency cache restores and saves; it keeps "
                                                 "the verified archive, so a restored one needs no request")
    nextest.add_argument("--os", help="runner OS (default: RUNNER_OS)")
    nextest.add_argument("--arch", help="runner architecture (default: RUNNER_ARCH)")
    args = parser.parse_args(argv)
    try:
        return install_nextest(args)
    except (ToolError, OSError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
