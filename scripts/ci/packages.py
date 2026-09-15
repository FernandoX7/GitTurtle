#!/usr/bin/env python3
"""Prepare CI packages and verify an actual Actions download against local evidence.

CI archives (especially PR builds) are test outputs, never trusted release inputs.
This helper executes only the current job's own build after checking its digest.
"""
from __future__ import annotations

import argparse
import gzip
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import signal
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location("ci_package_identity", ROOT / "scripts/package-identity.py")
identity = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(identity)
Error = identity.PackageError
TARGETS = {"x86_64-unknown-linux-gnu": "linux-x86_64", "aarch64-apple-darwin": "macos-arm64"}
# This records C0's known gaps, not license exemptions. Any remaining gap blocks
# upload; a new gap fails validation and requires explicit inventory review.
KNOWN_GAPS = {
    "x86_64-unknown-linux-gnu": {"mac-0.1.1", "ufbx-0.11.3"},
    "aarch64-apple-darwin": {"mac-0.1.1", "ufbx-0.11.3", "block-0.1.6", "objc_exception-0.1.2", "leak-0.1.2", "leaky-cow-0.1.1"},
}
MAX_TREE = 2 * 1024**3
MAX_ENTRIES = 30000
MAX_LOG = 8 * 1024**2


def write_json(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def run(command, *, cwd=ROOT, env=None, allowed=(0,), timeout=600):
    """Bound time/output and preserve the first and last failure diagnostics."""
    with tempfile.TemporaryFile() as output:
        child = subprocess.Popen([str(arg) for arg in command], cwd=cwd, env=env,
                                 stdin=subprocess.DEVNULL, stdout=output,
                                 stderr=subprocess.STDOUT, start_new_session=True)
        try:
            deadline = time.monotonic() + timeout
            while child.poll() is None:
                if time.monotonic() > deadline or output.tell() > MAX_LOG:
                    raise Error(f"{Path(command[0]).name} exceeded its time/output bound")
                time.sleep(0.05)
            if output.tell() > MAX_LOG:
                raise Error("Command exceeded its diagnostic bound")
            output.seek(0)
            text = output.read().decode("utf-8", errors="replace")
            if child.returncode not in allowed:
                detail = text
                if len(detail) > 8192:
                    notice = "\n... [diagnostic output truncated] ...\n"
                    head = (8192 - len(notice)) // 2
                    tail = 8192 - len(notice) - head
                    detail = detail[:head] + notice + detail[-tail:]
                raise Error(f"{Path(command[0]).name} failed ({child.returncode}): {detail}")
            return child.returncode, text
        finally:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()


def notice_state(code, inventory, target, lock):
    if (inventory.get("target") != target or inventory.get("cargo_lock_sha256") != lock
            or not isinstance(inventory.get("dependencies"), list) or not inventory["dependencies"]):
        raise Error("Strict notice collection returned an inconsistent inventory")
    missing = inventory.get("review_required")
    if not isinstance(missing, list) or any(not isinstance(item, str) for item in missing):
        raise Error("Strict notice collection did not establish review state")
    if code == 0 and not missing:
        return True
    if code != 2 or not missing or not set(missing) <= KNOWN_GAPS[target]:
        raise Error("Notice collection failed or found new gaps; review C0 before continuing")
    return False


def entry_path(name, seen):
    if not isinstance(name, str) or len(name.encode("utf-8")) > 1024:
        raise Error("Invalid archive path")
    path = PurePosixPath(name.rstrip("/"))
    parts = name.rstrip("/").split("/")
    if (path.is_absolute() or "\\" in name or any(part in ("", ".", "..") for part in parts)
            or any(ord(char) < 32 or ord(char) == 127 for char in name)):
        raise Error("Unsafe archive path")
    # Detect collisions on the default case-insensitive macOS filesystem too.
    key = name.rstrip("/").casefold()
    if key in seen:
        raise Error("Duplicate archive entry")
    seen.add(key)
    if len(seen) > MAX_ENTRIES:
        raise Error("Archive entry count exceeds the bound")
    return path


class TarReader:
    """Bound tar metadata reads before tarfile can allocate a claimed PAX body."""
    def __init__(self, stream):
        self.stream = stream
        self.bytes_read = 0
        self.deadline = time.monotonic() + 120

    def read(self, size):
        if size < 0 or size > MAX_LOG or time.monotonic() > self.deadline:
            raise Error("Tar metadata read exceeds its bound")
        value = self.stream.read(size)
        self.bytes_read += len(value)
        if self.bytes_read > MAX_TREE + MAX_LOG:
            raise Error("Tar stream exceeds its expansion bound")
        return value

    def seek(self, offset, whence=0):
        if whence != 0 or offset < self.tell() or offset > MAX_TREE + MAX_LOG:
            raise Error("Unsafe tar stream seek")
        # Read forward in bounded chunks; gzip.seek would skip decompression
        # without checking our expanded-byte and time budgets.
        remaining = offset - self.tell()
        while remaining:
            chunk = self.read(min(remaining, 1024 * 1024))
            if not chunk:
                raise Error("Truncated tar stream")
            remaining -= len(chunk)
        return self.tell()

    def tell(self):
        return self.stream.tell()


def zip_metadata_bound(path):
    # ZipFile materializes its entire central directory on open. Reject excessive
    # metadata first; our sub-GiB, <30k-entry package contract needs no ZIP64.
    with path.open("rb") as stream:
        size = stream.seek(0, os.SEEK_END)
        # Include the locator before even a maximum-length EOCD comment.
        stream.seek(max(0, size - 65557 - 20))
        tail = stream.read(65557 + 20)
    tail_start = max(0, size - 65557 - 20)
    offset = tail.rfind(b"PK\x05\x06")
    if offset < 0 or len(tail) - offset < 22:
        raise Error("ZIP end record is missing")
    # Python honors this locator even when the legacy record has no ZIP64
    # sentinel values. Refuse it before ZipFile reads the effective directory.
    if offset >= 20 and tail[offset - 20:offset - 16] == b"PK\x06\x07":
        raise Error("ZIP64 archives are outside the supported package contract")
    _, disk, directory_disk, disk_count, total, metadata, start, comment = struct.unpack(
        "<4s4H2IH", tail[offset:offset + 22])
    if (disk or directory_disk or disk_count != total or total > MAX_ENTRIES
            or metadata > MAX_LOG or start + metadata != tail_start + offset
            or len(tail) - offset != 22 + comment):
        raise Error("ZIP directory exceeds the supported metadata bound")
    # The declared count is not authoritative: ZipFile iterates the actual
    # directory bytes. Count bounded records first so an understated legacy
    # count cannot allocate an excessive ZipInfo list either.
    with path.open("rb") as stream:
        stream.seek(start)
        directory = stream.read(metadata)
    cursor = count = 0
    while cursor < len(directory):
        if directory[cursor:cursor + 4] != b"PK\x01\x02" or len(directory) - cursor < 46:
            raise Error("Malformed ZIP central directory")
        name_size, extra_size, comment_size = struct.unpack_from("<HHH", directory, cursor + 28)
        cursor += 46 + name_size + extra_size + comment_size
        count += 1
        if cursor > len(directory) or count > MAX_ENTRIES:
            raise Error("ZIP directory exceeds the supported entry bound")
    if count != total:
        raise Error("ZIP directory count disagrees with its entries")


def extract(archive, destination):
    """Extract only ordinary files/directories into a new, privately owned tree."""
    identity.regular(archive, identity.MAX_BINARY)
    destination.mkdir(mode=0o700)  # Existing outputs, including links, are refused.
    seen = set()
    expanded = 0
    deadline = time.monotonic() + 120

    def copy(name, size, mode, directory, source):
        nonlocal expanded
        relative = entry_path(name, seen)
        if size < 0 or size > identity.MAX_BINARY:
            raise Error("Archive member exceeds the bound")
        expanded += size
        if expanded > MAX_TREE or time.monotonic() > deadline:
            raise Error("Archive expansion exceeds the bound")
        target = destination.joinpath(*relative.parts)
        target.parent.mkdir(parents=True, exist_ok=True)
        if directory:
            target.mkdir(exist_ok=True)
            return
        with target.open("xb") as out:
            remaining = size
            while remaining:
                chunk = source.read(min(remaining, 1024 * 1024))
                if not chunk or time.monotonic() > deadline:
                    raise Error("Archive payload is truncated or extraction timed out")
                out.write(chunk)
                remaining -= len(chunk)
        # Retain executable intent; discard archive ownership and special bits.
        target.chmod(0o755 if mode & 0o111 else 0o644)

    if archive.name.endswith(".tar.gz"):
        with gzip.open(archive, "rb") as compressed, tarfile.open(fileobj=TarReader(compressed), mode="r:") as source:
            for member in source:
                if not (member.isfile() or member.isdir()) or member.issparse():
                    raise Error("Archive links/special/sparse entries are forbidden")
                stream = source.extractfile(member) if member.isfile() else None
                try:
                    copy(member.name, member.size, member.mode, member.isdir(), stream)
                finally:
                    if stream:
                        stream.close()
    elif archive.name.endswith(".zip"):
        zip_metadata_bound(archive)
        with zipfile.ZipFile(archive) as source:
            if len(source.infolist()) > MAX_ENTRIES:
                raise Error("Archive entry count exceeds the bound")
            for member in source.infolist():
                mode = member.external_attr >> 16
                kind = stat.S_IFMT(mode)
                if kind not in (0, stat.S_IFREG, stat.S_IFDIR) or member.flag_bits & 1:
                    raise Error("Archive links/special/encrypted entries are forbidden")
                with source.open(member) as stream:
                    copy(member.filename, member.file_size, mode, member.is_dir(), stream)
    else:
        raise Error("Unsupported archive format")


def verify_notices(licenses, inventory, *, complete=True):
    """Check the full notice files named by the inventory, not only its digest."""
    for dependency in inventory["dependencies"]:
        notices = dependency.get("notices")
        if not isinstance(notices, list) or (complete and not notices):
            raise Error("Dependency notices are missing")
        for notice in notices:
            name = notice.get("path")
            relative = entry_path(name, set())
            path = licenses.joinpath(*relative.parts)
            if identity.digest(path) != notice.get("sha256"):
                raise Error("A complete license notice differs from its inventory")


def verify_payload(directory, expected, *, complete, probe=True, manifest_validator=None):
    """Verify bytes from a trusted expected record before any executable probe.

    Release orchestration may provide its stricter signed-manifest validator;
    the CI CLI always uses the standard unsigned/ad-hoc identity contract.
    probe=False is structural verification only (for cross-platform assembly).
    """
    archive = directory / expected["archive"]
    if identity.digest(archive) != expected["archive_sha256"]:
        raise Error("Downloaded archive differs from the prepared bytes")
    outer_path = Path(str(archive) + ".manifest.json")
    if identity.digest(outer_path) != expected["manifest_sha256"]:
        raise Error("Downloaded provenance differs from the prepared bytes")
    outer = identity.read_json(outer_path)
    if (outer.get("format") != 1 or outer.get("archive") != archive.name
            or outer.get("archive_sha256") != expected["archive_sha256"]
            or outer.get("archive_bytes") != archive.stat().st_size):
        raise Error("Archive metadata does not match its bytes")
    checksum = Path(str(archive) + ".sha256")
    identity.regular(checksum, 2048)
    if checksum.read_text() != f"{expected['archive_sha256']}  {archive.name}\n":
        raise Error("Detached checksum does not match the archive")
    unpacked = directory / "extracted"
    extract(archive, unpacked)
    target = expected["target"]
    if target == "x86_64-unknown-linux-gnu":
        payload = unpacked / archive.name.removesuffix(".tar.gz")
        if set(p.name for p in unpacked.iterdir()) != {payload.name}:
            raise Error("Unexpected Linux archive root")
        info = payload / "build-info.json"
        binary, licenses = payload / "bin/gitturtle", payload / "licenses"
    else:
        if not set(p.name for p in unpacked.iterdir()) <= {"GitTurtle.app", "build-info.json", "__MACOSX"}:
            raise Error("Unexpected macOS archive root")
        payload = unpacked / "GitTurtle.app"
        info = unpacked / "build-info.json"
        binary, licenses = payload / "Contents/MacOS/gitturtle", payload / "Contents/Resources/licenses"
    if identity.digest(info) != outer.get("package_manifest_sha256"):
        raise Error("Extracted package manifest differs from the archive metadata")
    package = identity.read_json(info)
    if package != outer.get("package"):
        raise Error("Inner and outer package manifests disagree")
    (manifest_validator or identity.validate_manifest)(package, binary, licenses)
    identity.validate_identity(package["compiled_identity"], revision=expected["revision"],
                               version=expected["version"], target=target, profile="release",
                               distribution=complete)
    if (package["target"] != target or package["cargo_lock_sha256"] != expected["cargo_lock_sha256"]
            or package["input_binary_sha256"] != expected["input_binary_sha256"]
            or package["distribution"] != ("complete-notices" if complete else "development")):
        raise Error("Package does not match the prepared build inputs")
    inventory = identity.read_json(licenses / "dependencies.json")
    verify_notices(licenses, inventory, complete=complete)
    if probe and identity.probe(binary) != package["compiled_identity"]:
        raise Error("Extracted executable identity does not match its manifest")
    return payload, binary, package


def platform_checks(payload, binary, package, directory):
    if package["target"] == "x86_64-unknown-linux-gnu":
        run([sys.executable, ROOT / "scripts/test-install-linux.py", "--bundle", payload])
        home = directory / "install-home"
        home.mkdir()
        env = dict(HOME=str(home), XDG_DATA_HOME=str(home / ".local/share"), PATH="/usr/bin:/bin")
        run([sys.executable, payload / "install.py"], cwd=directory, env=env)
        installed = home / ".local/bin/gitturtle"
        if identity.digest(installed) != identity.digest(binary) or identity.probe(installed) != package["compiled_identity"]:
            raise Error("Installed executable differs from the downloaded package")
        run(["desktop-file-validate", home / ".local/share/applications/com.gitturtle.desktop.desktop"])
        _, linked = run(["ldd", installed])
        if "not found" in linked:
            raise Error("Installed package has unresolved native libraries")
        status, diagnostic = run([installed], cwd=directory, env=env, allowed=(1,), timeout=10)
        if status != 1 or "GitTurtle needs a Wayland or X11 desktop session" not in diagnostic:
            raise Error("No-display startup did not fail with the expected explanation")
    else:
        spec = importlib.util.spec_from_file_location("ci_macos_package", ROOT / "scripts/package-macos.py")
        macos = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(macos)
        macos.verify_bundle(payload, package["compiled_identity"]["version"])
        run(["plutil", "-lint", payload / "Contents/Info.plist"])
        run(["codesign", "--verify", "--deep", "--strict", payload])
        _, signature = run(["codesign", "--display", "--verbose=4", payload])
        if not re.search(r"^Signature=adhoc$", signature, re.MULTILINE):
            raise Error("CI macOS archive no longer has the declared ad-hoc signature")


def prepare(args):
    revision, version, lock, tree = identity.expectations(ROOT, args.revision, None)
    if tree != "clean":
        raise Error("CI packaging requires a clean source checkout")
    output = args.directory.resolve()
    output.mkdir(mode=0o700)  # A rerun requires a new output directory.
    binary = ROOT / "target" / args.target / "release/gitturtle"
    original = identity.digest(binary)
    identity.validate_identity(identity.probe(binary), revision=revision, version=version,
                               target=args.target, profile="release", distribution=True)
    code, _ = run([sys.executable, ROOT / "scripts/collect-third-party-licenses.py", "--target", args.target,
                   "--require-complete", output / "strict-notices"], allowed=(0, 2))
    inventory = identity.read_json(output / "strict-notices/dependencies.json")
    complete = notice_state(code, inventory, args.target, lock)
    stem = f"GitTurtle-{version}-{revision[:12]}-{TARGETS[args.target]}-release"
    prepared = output / "prepared"
    prepared.mkdir()
    command = [ROOT / f"scripts/package-{'linux' if args.target.endswith('linux-gnu') else 'macos'}.sh",
               "--no-build", "--binary", binary, "--expected-revision", revision,
               "--expected-version", version, "--expected-sha256", original]
    if complete:
        command.append("--distribution")
    if args.target.endswith("linux-gnu"):
        command.append(prepared / stem)
        archive = prepared / (stem + ".tar.gz")
    else:
        command += ["--archive-dir", prepared, output / "GitTurtle.app"]
        archive = prepared / (stem + ".zip")
    run(command)
    if identity.digest(binary) != original:
        raise Error("Packaging changed the compiled input executable")
    if not re.fullmatch(r"[0-9]+", args.run_id) or not re.fullmatch(r"[0-9]+", args.attempt):
        raise Error("Expected numeric GitHub run identity")
    trust = "untrusted-pr" if args.event == "pull_request" else "ci-build"
    expected = dict(format=1, target=args.target, revision=revision, version=version,
                    cargo_lock_sha256=lock, input_binary_sha256=original, archive=archive.name,
                    archive_sha256=identity.digest(archive),
                    manifest_sha256=identity.digest(Path(str(archive) + ".manifest.json")),
                    upload_eligible=complete, license_review_required=inventory["review_required"],
                    artifact_name=f"{stem}-{trust}-{args.run_id}-{args.attempt}",
                    event=args.event, run_id=args.run_id, run_attempt=args.attempt,
                    release_input=False, native_smoke="not-performed", hosted_transfer="not-performed")
    payload, extracted_binary, package = verify_payload(prepared, expected, complete=complete)
    platform_checks(payload, extracted_binary, package, prepared)
    expected["local_archive_checks"] = "passed"
    expected["signing"] = package["signing"]
    write_json(output / "prepared.json", expected)
    if args.report:
        write_json(args.report, expected)
    if complete:
        publish = output / "publish"
        publish.mkdir()
        for suffix in ("", ".sha256", ".manifest.json"):
            shutil.copyfile(Path(str(archive) + suffix), publish / (archive.name + suffix))
        write_json(publish / "ci-provenance.json", expected)
    if args.output:
        with args.output.open("a") as stream:
            stream.write(f"eligible={str(complete).lower()}\nartifact-name={expected['artifact_name']}\n")
    summary = (f"### {TARGETS[args.target]} CI package\n\n"
               f"Source `{revision}`, version `{version}`, release profile. "
               f"Archive SHA-256 `{expected['archive_sha256']}`.\n\n"
               "Local archive/executable/package checks passed. Native desktop verification remains separate.\n\n")
    if complete:
        summary += "Complete notices verified. Upload and download verification follow; this is an early-development CI build, not a release.\n"
    else:
        summary += "**Download unavailable: C0 license notices are incomplete; C3 hosted transfer remains open.** "
        summary += "The development archive is withheld from public upload. Verified strict refusal: " + ", ".join(f"`{item}`" for item in inventory["review_required"]) + ".\n"
    if args.summary:
        with args.summary.open("a") as stream:
            stream.write(summary)
    print(summary)


def verify(args):
    expected = identity.read_json(args.expected)
    if expected.get("format") != 1 or expected.get("upload_eligible") is not True or expected.get("release_input") is not False:
        raise Error("Expected a prepared, complete-notice CI package; CI outputs cannot be release inputs")
    # Exact names prevent unrelated assets being silently treated as verified.
    names = {expected["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")} | {"ci-provenance.json"}
    if set(path.name for path in args.directory.iterdir()) != names:
        raise Error("Downloaded artifact contains missing or unexpected files")
    if identity.read_json(args.directory / "ci-provenance.json") != expected:
        raise Error("Downloaded CI provenance differs from this job's prepared evidence")
    payload, binary, package = verify_payload(args.directory, expected, complete=True)
    platform_checks(payload, binary, package, args.directory)
    report = {**expected, "hosted_transfer": "passed", "artifact_id": args.artifact_id,
              "native_smoke": "not-performed"}
    if not re.fullmatch(r"[0-9]+", args.artifact_id):
        raise Error("Expected uploaded artifact ID")
    write_json(args.report, report)
    if args.summary:
        with args.summary.open("a") as stream:
            stream.write(f"\nActual Actions artifact `{args.artifact_id}` downloaded and verified: `{expected['archive_sha256']}`. "
                         "Package checks passed; native desktop acceptance remains separate.\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepared = commands.add_parser("prepare")
    prepared.add_argument("--target", choices=TARGETS, required=True)
    prepared.add_argument("--revision", required=True)
    prepared.add_argument("--directory", type=Path, required=True)
    prepared.add_argument("--event", choices=("push", "pull_request", "workflow_dispatch"), required=True)
    prepared.add_argument("--run-id", required=True)
    prepared.add_argument("--attempt", required=True)
    prepared.add_argument("--output", type=Path)
    prepared.add_argument("--summary", type=Path)
    prepared.add_argument("--report", type=Path)
    downloaded = commands.add_parser("verify")
    downloaded.add_argument("--expected", type=Path, required=True)
    downloaded.add_argument("--directory", type=Path, required=True)
    downloaded.add_argument("--artifact-id", required=True)
    downloaded.add_argument("--report", type=Path, required=True)
    downloaded.add_argument("--summary", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        (prepare if args.command == "prepare" else verify)(args)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, tarfile.TarError, zipfile.BadZipFile) as error:
        parser.exit(1, f"CI package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
