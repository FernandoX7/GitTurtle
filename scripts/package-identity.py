#!/usr/bin/env python3
"""Shared package provenance for locally built, trusted GitTurtle executables.

This verifies a supplied build against explicit expectations; it does not
authenticate an executable downloaded from an untrusted sender. Release tag and
clean-source validation belongs to the release preflight as well.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import sys
import tempfile
import time
import tomllib

APP_ID = "com.gitturtle.desktop"
TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin")
MAX_JSON = 8 * 1024 * 1024
MAX_PROBE = 16384
MAX_BINARY = 1024 * 1024 * 1024


class PackageError(ValueError):
    """A package input does not establish the expected identity."""


def regular(path: Path, limit: int) -> None:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
        raise PackageError(f"Expected a bounded regular file: {path.name}")


def digest(path: Path) -> str:
    regular(path, MAX_BINARY)
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise PackageError(f"Duplicate JSON field: {key}")
        result[key] = value
    return result


def read_json(path: Path) -> dict:
    regular(path, MAX_JSON)
    value = json.loads(path.read_bytes(), object_pairs_hook=unique_object)
    if not isinstance(value, dict):
        raise PackageError("Expected a JSON object")
    return value


def probe(binary: Path) -> dict:
    """Bound stdout/stderr and time; kill only our own process group on failure."""
    regular(binary, MAX_BINARY)
    if not os.access(binary, os.X_OK):
        raise PackageError("Expected an executable file")
    env = {key: value for key, value in os.environ.items()
           if key not in ("DISPLAY", "WAYLAND_DISPLAY")}
    env["ZED_HEADLESS"] = "1"
    # Darwin can return EPERM for a process group containing only zombies.
    # A live guard also reserves the group ID after the probe has been reaped,
    # so final cleanup cannot signal an unrelated, reused process group.
    # The parent alone holds its input pipe open, so a terminated caller also
    # releases the guard instead of leaving a long-lived sleeping helper.
    guard = subprocess.Popen([sys.executable, "-I", "-S", "-c", "import sys; sys.stdin.buffer.read()"],
                             stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, process_group=0)
    child = None
    data = {"out": bytearray(), "err": bytearray()}
    deadline = time.monotonic() + 5
    try:
        child = subprocess.Popen([str(binary.resolve()), "--build-info"], env=env,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, process_group=guard.pid)
        with selectors.DefaultSelector() as selector:
            for stream, name in ((child.stdout, "out"), (child.stderr, "err")):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise PackageError("Build identity probe timed out")
                for key, _ in selector.select(min(remaining, 0.1)):
                    chunk = os.read(key.fileobj.fileno(), MAX_PROBE + 1)
                    if not chunk:
                        selector.unregister(key.fileobj)
                    else:
                        data[key.data].extend(chunk)
                        if sum(map(len, data.values())) > MAX_PROBE:
                            raise PackageError("Build identity probe exceeded output limit")
            try:
                code = child.wait(timeout=max(0.01, deadline - time.monotonic()))
            except subprocess.TimeoutExpired as error:
                raise PackageError("Build identity probe timed out") from error
            if code:
                raise PackageError("Build identity probe failed")
        value = json.loads(data["out"], object_pairs_hook=unique_object)
        if not isinstance(value, dict):
            raise PackageError("Build identity must be a JSON object")
        return value
    finally:
        # Also stop descendants that retained a pipe or outlived the main probe.
        cleanup_error = None
        try:
            os.killpg(guard.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except OSError as error:
            cleanup_error = error
        # Still release our direct children and pipes if group signaling fails.
        # A live-group permission failure remains an error, never a successful
        # probe or a reason to wait indefinitely for an unkillable process.
        for process in (child, guard):
            if process is None:
                continue
            try:
                process.kill()
                process.wait(timeout=1)
            except (OSError, subprocess.SubprocessError) as error:
                cleanup_error = cleanup_error or error
            finally:
                for stream in (process.stdin, process.stdout, process.stderr):
                    if stream is not None:
                        stream.close()
        if cleanup_error is not None:
            raise cleanup_error


def verify_architecture(binary: Path, target: str) -> None:
    regular(binary, MAX_BINARY)
    with binary.open("rb") as stream:
        header = stream.read(32)
    if target == "x86_64-unknown-linux-gnu":
        valid = (len(header) == 32 and header[:6] == b"\x7fELF\x02\x01"
                 and int.from_bytes(header[18:20], "little") == 62)
    elif target == "aarch64-apple-darwin":
        valid = (len(header) == 32 and header[:4] == b"\xcf\xfa\xed\xfe"
                 and int.from_bytes(header[4:8], "little") == 0x0100000C)
    else:
        raise PackageError("Unsupported package target")
    if not valid:
        raise PackageError(f"Executable format/architecture does not match {target}")


def validate_identity(value: dict, *, revision: str, version: str, target: str,
                      profile: str, distribution: bool) -> None:
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise PackageError("Expected a full source commit SHA")
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?", version):
        raise PackageError("Unsupported project version")
    expected = dict(application="GitTurtle", source_revision=revision,
                    version=version, target=target, profile=profile)
    if target not in TARGETS or profile not in ("debug", "release"):
        raise PackageError("Unsupported target or profile")
    for key, wanted in expected.items():
        if value.get(key) != wanted:
            raise PackageError(f"Compiled {key} does not match the expected package input")
    if value.get("source_tree") not in ("clean", "modified"):
        raise PackageError("Compiled source tree identity is unavailable")
    if not isinstance(value.get("rustc"), str) or not value["rustc"].startswith("rustc "):
        raise PackageError("Compiled Rust toolchain identity is unavailable")
    if not isinstance(value.get("build_unix_seconds"), str) or not re.fullmatch(r"[0-9]{1,20}", value["build_unix_seconds"]):
        raise PackageError("Compiled build timestamp is unavailable")
    if distribution and (profile != "release" or value["source_tree"] != "clean"):
        raise PackageError("Distribution requires a clean release build")


def expectations(root: Path, revision: str | None, version: str | None) -> tuple[str, str, str, str]:
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_OPTIONAL_LOCKS="0", GIT_NO_LAZY_FETCH="1", GIT_NO_REPLACE_OBJECTS="1")
    command = ["git", "-C", str(root), "-c", "core.fsmonitor=false", "-c", "diff.external="]
    head = subprocess.check_output(command + ["rev-parse", "HEAD"], env=env, timeout=10).decode().strip()
    if revision is not None and revision != head:
        raise PackageError("Expected revision differs from the packaging checkout")
    regular(root / "Cargo.toml", MAX_JSON)
    document = tomllib.loads((root / "Cargo.toml").read_text())
    current_version = document["workspace"]["package"]["version"]
    if not isinstance(current_version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?", current_version):
        raise PackageError("Unsupported project version")
    if version is not None and version != current_version:
        raise PackageError("Expected version differs from the packaging checkout")
    with tempfile.TemporaryFile() as output:
        subprocess.run(command + ["status", "--porcelain", "--untracked-files=normal"],
                       env=env, timeout=10, stdout=output, check=True)
        output.seek(0)
        tree = "modified" if output.read(1) else "clean"
    return head, current_version, digest(root / "Cargo.lock"), tree


def create_manifest(*, binary: Path, licenses: Path, root: Path, target: str,
                    profile: str = "release", revision: str | None = None,
                    version: str | None = None, expected_sha256: str | None = None,
                    input_sha256: str | None = None, distribution: bool = False,
                    signing: str = "unsigned", built: bool = False) -> dict:
    revision, version, lock, tree = expectations(root, revision, version)
    if distribution and tree != "clean":
        raise PackageError("Distribution requires a clean packaging checkout")
    verify_architecture(binary, target)
    before = digest(binary)
    if expected_sha256 is not None and before != expected_sha256:
        raise PackageError("Executable SHA-256 differs from the expected input")
    identity = probe(binary)
    validate_identity(identity, revision=revision, version=version, target=target,
                      profile=profile, distribution=distribution)
    if digest(binary) != before:
        raise PackageError("Executable changed while its identity was inspected")
    inventory_path = licenses / "dependencies.json"
    inventory = read_json(inventory_path)
    if inventory.get("target") != target or inventory.get("cargo_lock_sha256") != lock:
        raise PackageError("License inventory target/lock does not match the package")
    reviews = inventory.get("review_required")
    dependencies = inventory.get("dependencies")
    if (not isinstance(reviews, list) or any(not isinstance(item, str) for item in reviews)
            or not isinstance(dependencies, list) or not dependencies):
        raise PackageError("License inventory is incomplete")
    for required in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        regular(licenses / required, MAX_JSON)
    if reviews:
        regular(licenses / "REVIEW_REQUIRED.md", MAX_JSON)
        if distribution:
            raise PackageError("License notices require review; public distribution is blocked")
    expected_signing = "unsigned" if target.endswith("linux-gnu") else "ad-hoc"
    if signing != expected_signing:
        raise PackageError("Unexpected platform signing status")
    if input_sha256 is not None and not re.fullmatch(r"[0-9a-f]{64}", input_sha256):
        raise PackageError("Invalid pre-signing executable digest")
    return dict(format=2, application="GitTurtle", bundle_id=APP_ID, target=target,
                packaged_from_revision=revision, packaging_tree_status=tree,
                release_built_by_packager=built,
                binary_sha256=before, input_binary_sha256=input_sha256 or before,
                compiled_identity=identity, cargo_lock_sha256=lock,
                license_inventory_sha256=digest(inventory_path),
                license_review_required=reviews,
                distribution="complete-notices" if distribution else "development",
                signing=signing)


def archive_manifest(archive: Path, package_info: Path) -> dict:
    """Keep the archive digest outside the archive to avoid a recursive hash."""
    package = read_json(package_info)
    if package.get("format") != 2 or package.get("application") != "GitTurtle":
        raise PackageError("Unsupported package manifest")
    return dict(format=1, archive=archive.name, archive_sha256=digest(archive),
                archive_bytes=archive.stat().st_size,
                package_manifest_sha256=digest(package_info), package=package)


def validate_manifest(value: dict, binary: Path, licenses: Path) -> None:
    """Verify an extracted payload before installation; no source checkout needed."""
    if (value.get("format") != 2 or value.get("application") != "GitTurtle"
            or value.get("bundle_id") != APP_ID
            or value.get("distribution") not in ("development", "complete-notices")):
        raise PackageError("Unsupported package manifest")
    target = value.get("target")
    info = value.get("compiled_identity")
    if not isinstance(info, dict):
        raise PackageError("Compiled package identity is missing")
    distribution = value["distribution"] == "complete-notices"
    validate_identity(info, revision=value.get("packaged_from_revision", ""),
                      version=info.get("version"), target=target,
                      profile="release" if target == TARGETS[0] else info.get("profile"),
                      distribution=distribution)
    expected_signing = "unsigned" if target == TARGETS[0] else "ad-hoc"
    if value.get("signing") != expected_signing:
        raise PackageError("Package signing status is inconsistent")
    if value.get("packaging_tree_status") not in ("clean", "modified"):
        raise PackageError("Packaging checkout identity is unavailable")
    if distribution and value["packaging_tree_status"] != "clean":
        raise PackageError("Distribution package came from a modified checkout")
    for field in ("binary_sha256", "input_binary_sha256", "cargo_lock_sha256", "license_inventory_sha256"):
        if not isinstance(value.get(field), str) or not re.fullmatch(r"[0-9a-f]{64}", value[field]):
            raise PackageError("Package digest metadata is incomplete")
    verify_architecture(binary, target)
    if digest(binary) != value["binary_sha256"]:
        raise PackageError("Executable does not match its package manifest")
    inventory_path = licenses / "dependencies.json"
    inventory = read_json(inventory_path)
    if (digest(inventory_path) != value["license_inventory_sha256"]
            or inventory.get("target") != target
            or inventory.get("cargo_lock_sha256") != value["cargo_lock_sha256"]
            or inventory.get("review_required") != value.get("license_review_required")):
        raise PackageError("License inventory does not match its package manifest")
    reviews = inventory.get("review_required")
    if not isinstance(reviews, list) or any(not isinstance(item, str) for item in reviews):
        raise PackageError("License review state is missing")
    if not isinstance(inventory.get("dependencies"), list) or not inventory["dependencies"]:
        raise PackageError("License dependency inventory is missing")
    if reviews:
        regular(licenses / "REVIEW_REQUIRED.md", MAX_JSON)
        if distribution:
            raise PackageError("Incomplete notices cannot be distributed")
    for required in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        regular(licenses / required, MAX_JSON)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--licenses", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--expected-revision")
    parser.add_argument("--expected-version")
    parser.add_argument("--expected-sha256")
    parser.add_argument("--input-sha256")
    parser.add_argument("--distribution", action="store_true")
    parser.add_argument("--built", action="store_true")
    parser.add_argument("--signing", choices=("unsigned", "ad-hoc"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        value = create_manifest(binary=args.binary, licenses=args.licenses, root=args.root,
                                target=args.target, profile=args.profile,
                                revision=args.expected_revision, version=args.expected_version,
                                expected_sha256=args.expected_sha256, input_sha256=args.input_sha256,
                                distribution=args.distribution, signing=args.signing, built=args.built)
        if (not args.built and args.expected_sha256 is None
                and (value["packaging_tree_status"] != "clean"
                     or value["compiled_identity"]["source_tree"] != "clean")):
            raise PackageError("Reusing modified source requires an explicit --expected-sha256 for the reviewed executable")
        with args.output.open("x") as stream:
            json.dump(value, stream, indent=2)
            stream.write("\n")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Package identity failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
