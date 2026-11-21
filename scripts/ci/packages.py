#!/usr/bin/env python3
"""Prepare CI packages and verify an actual Actions download of one in a later job.

CI archives (especially PR builds) are test outputs, never trusted release inputs.
`prepare` executes only its own job's build after checking its digest. `verify`
runs in a later job of the same run: it executes the downloaded build only after
the SHA-256 of the downloaded provenance equals the one its release leg handed on
as a job output, and the provenance names this job's own target, revision and run.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
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
# Each declared target and the platform-architecture label in its archive and
# artifact names; the two Linux labels keep the two Linux legs' names apart.
TARGETS = {"x86_64-unknown-linux-gnu": "linux-x86_64", "aarch64-unknown-linux-gnu": "linux-aarch64",
           "aarch64-apple-darwin": "macos-arm64"}
# This records C0's known gaps, not license exemptions. Any remaining gap blocks
# upload; a new gap fails validation and requires explicit inventory review.
# Linux has had none since the license review of 2026-11-15, so any gap on
# either Linux target is new.
KNOWN_GAPS = {
    "x86_64-unknown-linux-gnu": set(),
    "aarch64-unknown-linux-gnu": set(),
    "aarch64-apple-darwin": {"block-0.1.6", "objc_exception-0.1.2", "leak-0.1.2", "leaky-cow-0.1.1"},
}
# The platforms whose verified complete-notice archive Quality uploads. Being
# listed here is the only way a platform uploads: no flag, environment variable
# or workflow input adds one, and a listed platform whose notices are incomplete
# uploads nothing either.
# Linux is listed (the owner's decision of 2026-11-15). Every main push, manual
# Quality run and product pull request therefore publishes, for each Linux
# target, a public Actions artifact kept UPLOAD_RETENTION_DAYS days: the release
# archive, its .sha256 and .manifest.json, and ci-provenance.json. It is a CI
# build, not a release. `prepare` writes `eligible=true`, which runs the upload
# and hand-off steps of quality.yml's release job; the legs of its later
# package-transfer job, one for each target of a listed platform, download and
# verify what was uploaded.
# macOS is not listed, so its archive never leaves the job, whatever its
# notices: it is still collected strictly, packaged and verified there. Adding
# "macos" would publish an ad-hoc signed, not notarized macOS archive as the
# same public artifact from the same events, from the day its notices are
# complete. That entry waits for the macOS distribution decision, which is made
# with signing.
HOSTED_UPLOAD_PLATFORMS = frozenset({"linux"})
PLATFORM_NAMES = {"linux": "Linux", "macos": "macOS"}
# What quality.yml's upload step asks of Actions; the step summary states it.
UPLOAD_RETENTION_DAYS = 7
# What a release leg hands to the later job, and the only forms that job accepts.
# The pinned download action reads an ID with parseInt, which takes "12abc" as 12
# and rounds a number above 2^53: fifteen digits without a leading zero are always
# the number that was written.
ARTIFACT_ID = re.compile(r"[1-9][0-9]{0,14}")
SHA256 = re.compile(r"[0-9a-f]{64}")
# The variable that carries `toJSON(needs.rust-release.outputs)` to `plan`.
RELEASE_OUTPUTS = "RELEASE_OUTPUTS_JSON"
MAX_OUTPUTS = 64 * 1024
# The later job's two answers, which the Quality gate pairs with `eligible`.
# `verify` alone writes the first, as its last act.
VERIFIED, HELD = "verified", "held"
# Printed by the step that runs only when the one download failed. Fixed words:
# no artifact, run or address is named.
DOWNLOAD_FAILED = ("The package artifact was not returned intact at this job's one download attempt, or it is past its "
                   f"{UPLOAD_RETENTION_DAYS} days of retention. A re-run is not the remedy: this is the owner's question "
                   "of the later job against no read-back in Quality.")
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


def platform_of(target):
    """The platform a declared target is packaged for: "linux" or "macos"."""
    if target in TARGETS and target in identity.LINUX_TARGETS:
        return "linux"
    if target in TARGETS and target.endswith("-apple-darwin"):
        return "macos"
    raise Error("Target has no declared package platform")


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
    if platform_of(target) == "linux":
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
    if platform_of(package["target"]) == "linux":
        run([sys.executable, ROOT / "scripts/test-install-linux.py", "--bundle", payload])
        home = directory / "install-home"
        home.mkdir()
        env = dict(HOME=str(home), XDG_DATA_HOME=str(home / ".local/share"), PATH="/usr/bin:/bin")
        run([sys.executable, payload / "install.py"], cwd=directory, env=env)
        installed = home / ".local/bin/gitturtle"
        if identity.digest(installed) != identity.digest(binary) or identity.probe(installed) != package["compiled_identity"]:
            raise Error("Installed executable differs from the downloaded package")
        run(["desktop-file-validate", home / f".local/share/applications/{identity.APP_ID}.desktop"])
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
    # The one read of the upload set: it decides the upload, never how the package is built or checked.
    platform = platform_of(args.target)
    listed = platform in HOSTED_UPLOAD_PLATFORMS
    upload = complete and listed
    stem = f"GitTurtle-{version}-{revision[:12]}-{TARGETS[args.target]}-release"
    prepared = output / "prepared"
    prepared.mkdir()
    command = [ROOT / f"scripts/package-{platform}.sh",
               "--no-build", "--binary", binary, "--expected-revision", revision,
               "--expected-version", version, "--expected-sha256", original]
    if complete:
        command.append("--distribution")
    if platform == "linux":
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
                    upload_eligible=upload, license_review_required=inventory["review_required"],
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
    # What this leg hands to the later job under its own label: its answer and, for
    # an upload, the SHA-256 of the provenance file as uploaded, which that job
    # requires of the file it downloads before it reads it.
    label = TARGETS[args.target]
    handed = f"{label}-eligible={str(upload).lower()}\n"
    if upload:
        publish = output / "publish"
        publish.mkdir()
        for suffix in ("", ".sha256", ".manifest.json"):
            shutil.copyfile(Path(str(archive) + suffix), publish / (archive.name + suffix))
        write_json(publish / "ci-provenance.json", expected)
        handed += f"{label}-provenance-sha256={identity.digest(publish / 'ci-provenance.json')}\n"
    if args.output:
        with args.output.open("a") as stream:
            stream.write(f"eligible={str(upload).lower()}\nartifact-name={expected['artifact_name']}\n{handed}")
    summary = (f"### {label} CI package\n\n"
               f"Source `{revision}`, version `{version}`, release profile. "
               f"Archive SHA-256 `{expected['archive_sha256']}`.\n\n"
               "Local archive/executable/package checks passed. Native desktop verification remains separate.\n\n")
    held = f"the {PLATFORM_NAMES[platform]} upload is held until {PLATFORM_NAMES[platform]} distribution is decided"
    if upload:
        summary += (f"Complete notices verified. This job uploads the archive next as the Actions artifact "
                    f"`{expected['artifact_name']}`, kept {UPLOAD_RETENTION_DAYS} days. The later job "
                    "`Package transfer` downloads and verifies that artifact: this job's green does not say that it "
                    "did. It is an early-development CI build, not a release.\n")
    elif complete:
        summary += f"Complete notices verified. **Download unavailable: {held}; C3 hosted transfer remains open.** "
        summary += "The verified complete-notice archive stays in this job and is not uploaded.\n"
    else:
        summary += "**Download unavailable: C0 license notices are incomplete; C3 hosted transfer remains open.** "
        summary += "The development archive is withheld from public upload. Verified strict refusal: " + ", ".join(f"`{item}`" for item in inventory["review_required"]) + "."
        # Completing the notices alone would not change this for a platform that is not listed.
        summary += "\n" if listed else f" Separately, {held}.\n"
    if args.summary:
        with args.summary.open("a") as stream:
            stream.write(summary)
    print(summary)


def hand_off(args):
    """After the upload: write the artifact's ID under this leg's label for the later job."""
    label = TARGETS[args.target]
    if not ARTIFACT_ID.fullmatch(args.artifact_id):
        raise Error(f"{label}: the upload returned no artifact ID of the expected form")
    with args.output.open("a") as stream:
        stream.write(f"{label}-artifact-id={args.artifact_id}\n")
    print(f"{label}: the uploaded artifact's ID is handed to the later job, which downloads and verifies it")


def transfer_plan(raw, target):
    """What the later job does with what its release leg handed on: the artifact ID and
    the provenance SHA-256 to verify with, or None for a package the leg held.

    Exactly those two shapes are accepted. A refusal names the field, never its value.
    """
    label = TARGETS[target]
    try:
        outputs = json.loads(raw, object_pairs_hook=identity.unique_object) if len(raw) <= MAX_OUTPUTS else None
    except ValueError:
        outputs = None
    if not isinstance(outputs, dict) or any(not isinstance(value, str) for value in outputs.values()):
        raise Error("The release job's outputs are missing or malformed")
    # An output that was never set and one set to nothing are the same to a reader.
    eligible, artifact, digest = (outputs.get(f"{label}-{name}", "") for name in
                                  ("eligible", "artifact-id", "provenance-sha256"))
    if eligible == "true":
        if not ARTIFACT_ID.fullmatch(artifact):
            raise Error(f"{label}: the release leg's artifact ID is missing or malformed")
        if not SHA256.fullmatch(digest):
            raise Error(f"{label}: the release leg's provenance SHA-256 is missing or malformed")
        return artifact, digest
    if eligible == "false":
        if artifact or digest:
            raise Error(f"{label}: the release leg held its package, yet handed on an artifact ID or a provenance SHA-256")
        return None
    raise Error(f"{label}: the release leg's `eligible` answer is missing or is neither `true` nor `false`")


def plan(args):
    """The later job's first act, before any action runs: accept or refuse what was handed on."""
    label = TARGETS[args.target]
    accepted = transfer_plan(os.environ.get(RELEASE_OUTPUTS, ""), args.target)
    if accepted:
        artifact, digest = accepted
        outputs = f"verify=true\nartifact-id={artifact}\nprovenance-sha256={digest}\n"
        message = ("The release leg handed on an artifact ID and a provenance SHA-256 of the expected form; "
                   "this job downloads that artifact once and verifies it.")
    else:
        outputs = f"verify=false\n{label}-transfer={HELD}\n"
        message = ("The release leg held this package: nothing was uploaded, so nothing is downloaded or "
                   "verified here.")
    with args.output.open("a") as stream:
        stream.write(outputs)
    if args.summary and not accepted:
        with args.summary.open("a") as stream:
            stream.write(f"### {label} package transfer\n\n{message}\n")
    print(f"{label}: {message}")


def download_failed(args):
    """Say what a failed download is. The workflow runs this only when its one download step failed."""
    if args.summary:
        with args.summary.open("a") as stream:
            stream.write(f"\n**{DOWNLOAD_FAILED}**\n")
    print(f"::error::{DOWNLOAD_FAILED}")


def bound_provenance(directory, expected_sha256):
    """The downloaded provenance record, read once and believed only when those bytes
    hash to what the release leg handed on. Nothing is parsed before the comparison."""
    path = directory / "ci-provenance.json"
    identity.regular(path, identity.MAX_JSON)
    with path.open("rb") as stream:
        raw = stream.read(identity.MAX_JSON + 1)
    if len(raw) > identity.MAX_JSON or hashlib.sha256(raw).hexdigest() != expected_sha256:
        raise Error("Downloaded CI provenance differs from what the release leg handed on")
    value = json.loads(raw, object_pairs_hook=identity.unique_object)
    if not isinstance(value, dict):
        raise Error("Expected a JSON object")
    return value


def verify(args):
    label = TARGETS[args.target]
    if not ARTIFACT_ID.fullmatch(args.artifact_id):
        raise Error("Expected uploaded artifact ID")
    if not SHA256.fullmatch(args.expected_sha256):
        raise Error("Expected the SHA-256 of the release leg's provenance")
    if not re.fullmatch(r"[0-9]+", args.run_id) or not re.fullmatch(r"[0-9]+", args.attempt):
        raise Error("Expected numeric GitHub run identity")
    expected = bound_provenance(args.directory, args.expected_sha256)
    if expected.get("format") != 1 or expected.get("upload_eligible") is not True or expected.get("release_input") is not False:
        raise Error("Expected a prepared, upload-eligible CI package; CI outputs cannot be release inputs")
    # The record is of this job's own target, source and run. A re-run of this job
    # alone may read an earlier attempt's upload, never a later one's.
    uploaded = expected.get("run_attempt")
    if expected.get("target") != args.target:
        raise Error("Downloaded CI provenance is for another target than this job's")
    if expected.get("revision") != args.revision:
        raise Error("Downloaded CI provenance is for another revision than this job's")
    if expected.get("run_id") != args.run_id:
        raise Error("Downloaded CI provenance is from another run than this job's")
    if not isinstance(uploaded, str) or not re.fullmatch(r"[0-9]+", uploaded) or int(uploaded) > int(args.attempt):
        raise Error("Downloaded CI provenance is from no numbered attempt or from a later one than this job's")
    # Exact names prevent unrelated assets being silently treated as verified.
    names = {expected["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")} | {"ci-provenance.json"}
    if set(path.name for path in args.directory.iterdir()) != names:
        raise Error("Downloaded artifact contains missing or unexpected files")
    payload, binary, package = verify_payload(args.directory, expected, complete=True)
    platform_checks(payload, binary, package, args.directory)
    report = {**expected, "hosted_transfer": "passed", "artifact_id": args.artifact_id,
              "native_smoke": "not-performed", "provenance_sha256": args.expected_sha256,
              "verified_run_attempt": args.attempt}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    write_json(args.report, report)
    if args.summary:
        with args.summary.open("a") as stream:
            stream.write(f"### {label} package transfer\n\n"
                         f"Actual Actions artifact `{args.artifact_id}`, uploaded by attempt {uploaded} of this run, "
                         f"downloaded once and verified in attempt {args.attempt}: `{expected['archive_sha256']}`. "
                         "Package checks passed; native desktop acceptance remains separate.\n")
    # The last act, after every check, the report and the summary: the one place
    # that writes the word the Quality gate requires of an uploaded package.
    if args.output:
        with args.output.open("a") as stream:
            stream.write(f"{label}-transfer={VERIFIED}\n")


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
    handed = commands.add_parser("hand-off")
    handed.add_argument("--target", choices=TARGETS, required=True)
    handed.add_argument("--artifact-id", required=True)
    handed.add_argument("--output", type=Path, required=True)
    planned = commands.add_parser("plan", description=f"Reads the release job's outputs as JSON from {RELEASE_OUTPUTS}.")
    planned.add_argument("--target", choices=TARGETS, required=True)
    planned.add_argument("--output", type=Path, required=True)
    planned.add_argument("--summary", type=Path)
    failed = commands.add_parser("download-failed")
    failed.add_argument("--summary", type=Path)
    downloaded = commands.add_parser("verify")
    downloaded.add_argument("--directory", type=Path, required=True)
    downloaded.add_argument("--expected-sha256", required=True)
    downloaded.add_argument("--target", choices=TARGETS, required=True)
    downloaded.add_argument("--revision", required=True)
    downloaded.add_argument("--run-id", required=True)
    downloaded.add_argument("--attempt", required=True)
    downloaded.add_argument("--artifact-id", required=True)
    downloaded.add_argument("--report", type=Path, required=True)
    downloaded.add_argument("--summary", type=Path)
    downloaded.add_argument("--output", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        {"prepare": prepare, "hand-off": hand_off, "plan": plan, "download-failed": download_failed,
         "verify": verify}[args.command](args)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, tarfile.TarError, zipfile.BadZipFile) as error:
        parser.exit(1, f"CI package verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
