#!/usr/bin/env python3
"""Install pinned CI tools from checksum-verified release archives (stdlib only)."""

import argparse
import hashlib
import http.client
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request


# cargo-nextest 0.9.146 (2026-09-21) changes only yanked dependency versions from
# 0.9.145, whose status-line format scripts/gate.py parses. Digests match the
# release's published `.sha256` files and GitHub's asset digests; see docs/ci.md.
NEXTEST_VERSION = "0.9.146"
NEXTEST_RELEASE = f"https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-{NEXTEST_VERSION}/"
NEXTEST_ARCHIVES = {
    ("Linux", "X64"): ("cargo-nextest-0.9.146-x86_64-unknown-linux-gnu.tar.gz",
                       "682c21b777c333e96fd532e114d3a5a894e0729ab88d94c0a9f20f8419695428"),
    # Upstream publishes one universal (arm64 and x86_64) macOS archive.
    ("macOS", "ARM64"): ("cargo-nextest-0.9.146-universal-apple-darwin.tar.gz",
                         "39785160b3c2f6ed9a765049cf4fa79f3b39aa02eb7598a5a0e2a1a0b9ffb9a8"),
}
NEXTEST_MEMBER = "cargo-nextest"
MAX_ARCHIVE = 64 << 20
MAX_EXECUTABLE = 128 << 20
ATTEMPTS = 3


class ToolError(Exception):
    pass


def download(url, limit=MAX_ARCHIVE, attempts=ATTEMPTS):
    """Fetch at most `limit` bytes over HTTPS, retrying transport failures only."""
    for attempt in range(1, attempts + 1):
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                if not response.geturl().startswith("https://"):
                    raise ToolError("tool download left HTTPS")
                data = response.read(limit + 1)
            if len(data) > limit:
                raise ToolError("tool archive exceeds its size limit")
            return data
        except (urllib.error.URLError, http.client.HTTPException, TimeoutError, ConnectionError) as error:
            if attempt == attempts:
                raise ToolError(f"tool download failed after {attempts} attempts: {error}") from None
            time.sleep(2 * attempt)
    raise AssertionError("unreachable")


def verified_member(data, expected_sha256, member):
    """Return one regular file's bytes from a gzip tarball whose digest matches."""
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected_sha256:
        raise ToolError(f"checksum mismatch: expected {expected_sha256}, got {actual}")
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
    executable = verified_member(download(NEXTEST_RELEASE + name), sha256, NEXTEST_MEMBER)
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
    print(f"{first}; {name} sha256 {sha256}")
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
