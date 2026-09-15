#!/usr/bin/env python3
"""Sign one trusted GitTurtle app; no build, download, release or ad-hoc fallback.

The caller owns trusted archive verification, complete notices and release
authorization. This helper's detached result is not a package manifest or C4
acceptance. See docs/releases.md before providing credentials.
"""

from __future__ import annotations

import argparse
import base64
import binascii
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import secrets
import selectors
import shlex
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid


MAX_OUTPUT = 2 * 1024 * 1024
TARGETS = {"aarch64-apple-darwin": "arm64"}
MACHO = {b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf",
         b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
         b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca"}
CREDENTIALS = ("GITTURTLE_SIGNING_P12_BASE64", "GITTURTLE_SIGNING_P12_PASSWORD",
               "GITTURTLE_NOTARY_KEY_BASE64", "GITTURTLE_NOTARY_KEY_ID",
               "GITTURTLE_NOTARY_ISSUER_ID")


class SigningError(Exception):
    """A safe, caller-facing failure; never interpolate subprocess output."""


class Cancelled(SigningError):
    pass


class CommandFailure(SigningError):
    """Captured output stays private; only validated fields may enter a report."""

    def __init__(self, label, code, stdout):
        super().__init__(f"{label} failed (exit {code})")
        self.stdout = stdout


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(data: bytes, label: str) -> dict:
    try:
        def pairs(items):
            result = {}
            for key, value in items:
                if key in result:
                    raise ValueError("duplicate key")
                result[key] = value
            return result
        result = json.loads(data, object_pairs_hook=pairs)
        if not isinstance(result, dict):
            raise ValueError("not an object")
        return result
    except (ValueError, UnicodeError):
        raise SigningError(f"Invalid {label} JSON") from None


def regular_file(path: Path, limit: int | None = None) -> bytes:
    mode = path.lstat().st_mode
    if not stat.S_ISREG(mode) or (limit is not None and path.stat().st_size > limit):
        raise SigningError("Missing, non-regular or oversized package input")
    return path.read_bytes() if limit is not None else b""


def no_symlink_components(path: Path) -> None:
    for component in (path, *path.parents):
        if component.is_symlink():
            raise SigningError("Symlinked input/output path is unsupported")


def validate_options(args) -> None:
    for value, pattern, label in (
        (args.source, r"[0-9a-f]{40}", "source revision"),
        (args.version, r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", "version"),
        (args.executable_sha256, r"[0-9a-f]{64}", "executable digest"),
        (args.identity, r"[0-9A-Fa-f]{40}", "certificate SHA-1 fingerprint"),
        (args.team_id, r"[A-Z0-9]{10}", "Team ID"),
    ):
        if not re.fullmatch(pattern, value):
            raise SigningError(f"Invalid expected {label}")
    if args.target not in TARGETS or not 60 <= args.notary_timeout_seconds <= 3600:
        raise SigningError("Unsupported target or notarization wait limit")
    if not args.app.is_absolute() or not args.output.is_absolute():
        raise SigningError("Input app and new output directory must use absolute paths")
    # Reject aliases before resolving so no symlink is hidden by normalization.
    no_symlink_components(args.app)
    no_symlink_components(args.output.parent)
    args.app = Path(os.path.abspath(args.app))
    args.output = Path(os.path.abspath(args.output))
    if args.app.suffix != ".app" or not args.app.is_dir():
        raise SigningError("Input must be an existing app directory")
    if os.path.lexists(args.output):
        raise SigningError("Output already exists; refusing to replace it")
    if args.output.is_relative_to(args.app):
        raise SigningError("Output cannot be inside the input app")


def validate_bundle(app: Path, args) -> Path:
    """Only the current one-executable bundle is supported, never unknown code."""
    executable = app / "Contents/MacOS/gitturtle"
    count = 0
    for directory, folders, files in os.walk(app, followlinks=False):
        for name in folders + files:
            path = Path(directory) / name
            mode = path.lstat().st_mode
            count += 1
            if count > 30000 or not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
                raise SigningError("Unsupported bundle entry or excessive entry count")
            if stat.S_ISDIR(mode):
                if path.suffix in {".app", ".framework", ".appex", ".xpc", ".bundle"}:
                    raise SigningError("Nested code bundles require an explicit signing plan")
            else:
                with path.open("rb") as stream:
                    magic = stream.read(4)
                if path != executable and (mode & 0o111 or magic in MACHO):
                    raise SigningError("Unexpected executable bundle content")
    regular_file(executable)
    with executable.open("rb") as stream:
        if stream.read(4) not in MACHO or not executable.stat().st_mode & 0o111:
            raise SigningError("Main executable is not an executable Mach-O file")
    try:
        info = plistlib.loads(regular_file(app / "Contents/Info.plist", 256 * 1024))
    except (ValueError, plistlib.InvalidFileException):
        raise SigningError("Invalid app property list") from None
    expected = {"CFBundleIdentifier": "com.gitturtle.desktop",
                "CFBundleExecutable": "gitturtle", "CFBundlePackageType": "APPL",
                "CFBundleShortVersionString": args.version, "CFBundleVersion": args.version}
    if not isinstance(info, dict) or any(info.get(k) != v for k, v in expected.items()):
        raise SigningError("App property list does not match the release identity")
    for resource in ("AppIcon.icns", "Assets.car"):
        regular_file(app / "Contents/Resources" / resource)
    return executable


def content_snapshot(app: Path) -> dict:
    """Bind the private copy to all input bytes/modes, not only its executable."""
    return {str(path.relative_to(app)): (stat.S_IMODE(path.stat().st_mode),
            digest(path) if path.is_file() else None)
            for path in app.rglob("*")}


def load_credentials(environ) -> dict:
    if any(not environ.get(name) for name in CREDENTIALS):
        raise SigningError("Required signing/notarization credentials are missing")
    result = {name: environ[name] for name in CREDENTIALS}
    if not re.fullmatch(r"[A-Z0-9]{10}", result[CREDENTIALS[3]]):
        raise SigningError("Invalid notarization key ID")
    try:
        uuid.UUID(result[CREDENTIALS[4]])
        for name in (CREDENTIALS[0], CREDENTIALS[2]):
            if len(result[name]) > 2 * 1024 * 1024:
                raise ValueError("large credential")
            decoded = base64.b64decode(result[name], validate=True)
            if not decoded:
                raise ValueError("empty credential")
            result[name] = decoded
        if not result[CREDENTIALS[2]].startswith(b"-----BEGIN PRIVATE KEY-----"):
            raise ValueError("not a PKCS8 key")
        if "\x00" in result[CREDENTIALS[1]] or len(result[CREDENTIALS[1]]) > 4096:
            raise ValueError("invalid import password")
    except (ValueError, binascii.Error):
        raise SigningError("Malformed signing/notarization credentials") from None
    return result


class Commands:
    """Bounded subprocesses with no shell, secret environment or command logging."""

    def __init__(self, report: dict):
        self.report = report
        self.env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "C", "LC_ALL": "C"}
        if os.environ.get("HOME"):
            self.env["HOME"] = os.environ["HOME"]

    def __call__(self, label: str, argv: list[str], timeout: int = 60) -> tuple[bytes, bytes]:
        started = time.monotonic()
        event = {"step": label, "status": "failed"}
        self.report["commands"].append(event)
        process = None
        try:
            process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, env=self.env, start_new_session=True)
            streams = {process.stdout: bytearray(), process.stderr: bytearray()}
            with selectors.DefaultSelector() as selector:
                for stream in streams:
                    selector.register(stream, selectors.EVENT_READ)
                total = 0
                while selector.get_map():
                    if time.monotonic() - started > timeout:
                        raise SigningError(f"{label} timed out; no automatic retry")
                    for key, _ in selector.select(timeout=0.1):
                        chunk = os.read(key.fileobj.fileno(), 65536)
                        if not chunk:
                            selector.unregister(key.fileobj)
                        else:
                            total += len(chunk)
                            if total > MAX_OUTPUT:
                                raise SigningError(f"{label} exceeded the output limit")
                            streams[key.fileobj].extend(chunk)
                remaining = max(0.1, timeout - (time.monotonic() - started))
                event["exit_code"] = process.wait(timeout=remaining)
            if process.returncode:
                raise CommandFailure(label, process.returncode, bytes(streams[process.stdout]))
            event["status"] = "passed"
            return bytes(streams[process.stdout]), bytes(streams[process.stderr])
        except (OSError, subprocess.TimeoutExpired):
            raise SigningError(f"{label} could not complete") from None
        finally:
            if process is not None:
                # Stop descendants on timeout/cancellation too; pipes may outlive their parent.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    raise SigningError(f"{label} could not be reaped after termination") from None
                finally:
                    process.stdout.close()
                    process.stderr.close()
            event["elapsed_seconds"] = round(time.monotonic() - started, 3)


@contextmanager
def cancellation_handler():
    def cancel(signum, _frame):
        raise Cancelled(f"Cancelled by signal {signum}; no automatic retry")
    previous = {sig: signal.signal(sig, cancel) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        yield
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


@contextmanager
def signing_keychain(command, private: Path, credentials: dict, report: dict):
    keychain = private / "signing.keychain-db"
    password = secrets.token_urlsafe(32)
    p12 = private / "identity.p12"
    p12.write_bytes(credentials[CREDENTIALS[0]])
    p12.chmod(0o600)
    previous, _ = command("read-keychain-search-list", ["/usr/bin/security", "list-keychains", "-d", "user"])
    try:
        search_list = shlex.split(previous.decode("utf-8"))
    except (ValueError, UnicodeError):
        raise SigningError("Invalid keychain search list") from None
    if any(not Path(path).is_absolute() for path in search_list):
        raise SigningError("Invalid keychain search list")
    attempted = False
    try:
        attempted = True  # Creation can mutate the list even if its reply is lost.
        command("create-signing-keychain", ["/usr/bin/security", "create-keychain", "-p", password, str(keychain)])
        command("configure-signing-keychain", ["/usr/bin/security", "set-keychain-settings", "-lut", "7200", str(keychain)])
        command("unlock-signing-keychain", ["/usr/bin/security", "unlock-keychain", "-p", password, str(keychain)])
        command("import-signing-identity", ["/usr/bin/security", "import", str(p12), "-k", str(keychain),
                "-P", credentials[CREDENTIALS[1]], "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"])
        p12.unlink()
        command("authorize-signing-key", ["/usr/bin/security", "set-key-partition-list", "-S", "apple-tool:,apple:",
                "-s", "-k", password, str(keychain)])
        yield keychain
    finally:
        # A second ordinary cancellation must not interrupt bounded cleanup.
        previous_handlers = {sig: signal.signal(sig, signal.SIG_IGN) for sig in (signal.SIGINT, signal.SIGTERM)}
        errors = []
        try:
            if attempted:
                try:
                    command("restore-keychain-search-list", ["/usr/bin/security", "list-keychains", "-d", "user", "-s", *search_list], 20)
                except SigningError:
                    errors.append("restore-keychain-search-list")
                try:
                    command("delete-signing-keychain", ["/usr/bin/security", "delete-keychain", str(keychain)], 20)
                except SigningError:
                    errors.append("delete-signing-keychain")
            report["keychain_cleanup"] = {"status": "failed" if errors else "passed", "failed_steps": errors}
        finally:
            for sig, handler in previous_handlers.items():
                signal.signal(sig, handler)
        if errors:
            raise SigningError("Keychain cleanup failed; release output withheld")


def verify_identity(command, executable: Path, args) -> dict:
    output, _ = command("verify-compiled-identity", [str(executable), "--build-info"], 15)
    identity = read_json(output, "compiled identity")
    expected = {"application": "GitTurtle", "source_revision": args.source, "version": args.version,
                "target": args.target, "source_tree": "clean", "profile": "release"}
    if any(identity.get(key) != value for key, value in expected.items()):
        raise SigningError("Compiled identity does not match the clean release input")
    output, _ = command("verify-mach-o-architecture", ["/usr/bin/lipo", "-archs", str(executable)])
    if output.decode("ascii", errors="replace").split() != [TARGETS[args.target]]:
        raise SigningError("Mach-O architecture does not match the expected single target")
    return expected


def verify_signature(command, app: Path, args) -> dict:
    command("verify-strict-signature", ["/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)])
    _, output = command("inspect-signature", ["/usr/bin/codesign", "--display", "--verbose=4", str(app)])
    lines = output.decode("utf-8", errors="replace").splitlines()
    fields = {}
    for line in lines:
        key, separator, value = line.partition("=")
        if separator:
            fields.setdefault(key, []).append(value)
    if fields.get("TeamIdentifier") != [args.team_id] or fields.get("Identifier") != ["com.gitturtle.desktop"]:
        raise SigningError("Signature identity mismatch")
    authority = fields.get("Authority", [""])[0]
    if not authority.startswith("Developer ID Application: ") or not authority.endswith(f"({args.team_id})"):
        raise SigningError("Signature is not the expected Developer ID Application")
    timestamp = fields.get("Timestamp", [])
    if len(timestamp) != 1 or not timestamp[0] or timestamp[0].lower() == "none":
        raise SigningError("Signature lacks a secure timestamp")
    if not any(re.search(r"flags=0x[0-9a-fA-F]+\([^)]*\bruntime\b", line) for line in lines):
        raise SigningError("Signature lacks hardened runtime")
    entitlements, _ = command("inspect-entitlements", ["/usr/bin/codesign", "--display", "--entitlements", "-", "--xml", str(app)])
    try:
        if entitlements.strip() and plistlib.loads(entitlements) != {}:
            raise SigningError("Unexpected entitlement; review before distributing")
    except (ValueError, plistlib.InvalidFileException):
        raise SigningError("Malformed signed entitlements") from None
    return {"type": "Developer ID Application", "team_id": args.team_id,
            "certificate_sha1": args.identity.upper(), "hardened_runtime": True,
            "entitlements": {}, "secure_timestamp": True}


def submission_id(response: dict) -> str:
    value = response.get("id")
    try:
        return str(uuid.UUID(value))
    except (ValueError, TypeError, AttributeError):
        raise SigningError("Notary response has no valid submission ID; do not resubmit blindly") from None


def notary_log_summary(data: dict, identifier: str, archive_digest: str) -> dict:
    if data.get("jobId") != identifier or data.get("sha256") != archive_digest:
        raise SigningError("Notarization log does not identify the submitted archive")
    status = data.get("status")
    code = data.get("statusCode")
    if status not in {"Accepted", "Invalid", "Rejected"} or type(code) is not int:
        raise SigningError("Malformed notarization log status")
    issues = data.get("issues")
    if issues is None:
        issues = []
    if not isinstance(issues, list) or len(issues) > 10000:
        raise SigningError("Malformed notarization log issues")
    # Deliberately omit free-form messages/paths: service diagnostics can echo
    # private upload paths. The submission ID permits an authorized log lookup.
    counts = {"warning": 0, "error": 0}
    codes = set()
    for issue in issues:
        if not isinstance(issue, dict) or issue.get("severity") not in counts:
            raise SigningError("Malformed notarization issue")
        counts[issue["severity"]] += 1
        if type(issue.get("code")) is int:
            codes.add(issue["code"])
    return {"job_id": identifier, "sha256": archive_digest, "status": status,
            "status_code": code, "issue_counts": counts, "issue_codes": sorted(codes)}


def write_report(output: Path, report: dict) -> None:
    temporary = output / ".signing-result.json.tmp"
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2, sort_keys=True)
        stream.write("\n")
    temporary.replace(output / "signing-result.json")


def sign(args, command_factory=Commands, environ=None, platform=None) -> dict:
    if (platform or sys.platform) != "darwin":
        raise SigningError("Developer ID signing requires macOS")
    validate_options(args)
    credentials = load_credentials(os.environ if environ is None else environ)
    executable = validate_bundle(args.app, args)
    if digest(executable) != args.executable_sha256:
        raise SigningError("Pre-sign executable digest mismatch")
    source_contents = content_snapshot(args.app)
    args.output.mkdir(mode=0o700)
    report = {"schema_version": 1, "status": "failed", "commands": [],
              "source_revision": args.source, "version": args.version, "target": args.target,
              "pre_sign_executable_sha256": args.executable_sha256,
              "downloaded_package_gatekeeper_and_launch": "pending"}
    command = command_factory(report)
    submitted = args.output / "notarization-input.zip"
    partial = args.output / ".final.zip.partial"
    private = None
    final_name = f"GitTurtle-{args.version}-{args.target}-{args.source[:12]}-notarized.zip"
    final = args.output / final_name
    try:
        with tempfile.TemporaryDirectory(prefix="gitturtle-sign-") as temporary:
            private = Path(temporary)
            app = private / "GitTurtle.app"
            command("copy-trusted-app", ["/usr/bin/ditto", str(args.app), str(app)])
            staged_executable = validate_bundle(app, args)
            if content_snapshot(app) != source_contents or content_snapshot(args.app) != source_contents:
                raise SigningError("Input app or copied content changed during preparation")
            report["compiled_identity"] = verify_identity(command, staged_executable, args)
            key = private / "AuthKey.p8"
            key.write_bytes(credentials[CREDENTIALS[2]])
            key.chmod(0o600)
            entitlements = private / "empty-entitlements.plist"
            entitlements.write_bytes(plistlib.dumps({}, fmt=plistlib.FMT_XML))
            with signing_keychain(command, private, credentials, report) as keychain:
                identities, _ = command("verify-imported-identity", ["/usr/bin/security", "find-identity", "-v", "-p", "codesigning", str(keychain)])
                matches = re.findall(r'\b([0-9A-Fa-f]{40}) "([^"\r\n]+)"', identities.decode("utf-8", errors="replace"))
                if len(matches) != 1 or matches[0][0].upper() != args.identity.upper() or not (
                    matches[0][1].startswith("Developer ID Application: ") and matches[0][1].endswith(f"({args.team_id})")
                ):
                    raise SigningError("Imported identity does not match the expected certificate and team")
                # Current package has one main executable and no nested code.
                # Signing the app seals that executable and its resources together.
                command("sign-app", ["/usr/bin/codesign", "--force", "--sign", args.identity,
                        "--keychain", str(keychain), "--timestamp", "--options", "runtime",
                        "--entitlements", str(entitlements), str(app)], 180)
                report["signature"] = verify_signature(command, app, args)
                if verify_identity(command, staged_executable, args) != report["compiled_identity"]:
                    raise SigningError("Signed executable identity changed")
                report["signed_executable_sha256"] = digest(staged_executable)
                command("archive-notarization-input", ["/usr/bin/ditto", "-c", "-k", "--keepParent", str(app), str(submitted)], 300)
                regular_file(submitted)
                report["submitted_archive_sha256"] = digest(submitted)
                authentication = ["--key", str(key), "--key-id", credentials[CREDENTIALS[3]], "--issuer", credentials[CREDENTIALS[4]]]
                try:
                    response, _ = command("submit-notarization", ["/usr/bin/xcrun", "notarytool", "submit", str(submitted), *authentication, "--output-format", "json"], 900)
                except CommandFailure as error:
                    # A failed upload can still have allocated a submission ID.
                    # Preserve a valid ID without interpreting it as acceptance.
                    try:
                        identifier = submission_id(read_json(error.stdout, "notarization submission"))
                        report["notarization"] = {"id": identifier, "status": "submission-uncertain"}
                        write_report(args.output, report)
                    except SigningError:
                        pass
                    raise
                identifier = submission_id(read_json(response, "notarization submission"))
                report["notarization"] = {"id": identifier, "status": "submitted"}
                write_report(args.output, report)
                wait_error = None
                try:
                    response, _ = command("wait-for-notarization", ["/usr/bin/xcrun", "notarytool", "wait", identifier, *authentication,
                            "--timeout", f"{args.notary_timeout_seconds}s", "--output-format", "json"], args.notary_timeout_seconds + 30)
                except CommandFailure as error:
                    # notarytool can return nonzero for a rejected submission.
                    # Read only its verified result, then retrieve the diagnostic log.
                    response = error.stdout
                    wait_error = error
                result = read_json(response, "notarization result")
                if submission_id(result) != identifier or result.get("status") not in {"Accepted", "Invalid", "Rejected"}:
                    raise SigningError("Notarization has no matching terminal result")
                report["notarization"]["status"] = result["status"]
                log_path = private / "notary-log.json"
                command("retrieve-notarization-log", ["/usr/bin/xcrun", "notarytool", "log", identifier, *authentication, str(log_path)], 120)
                report["notarization"]["log"] = notary_log_summary(read_json(regular_file(log_path, MAX_OUTPUT), "notarization log"), identifier, report["submitted_archive_sha256"])
                log = report["notarization"]["log"]
                if result["status"] != "Accepted" or log["status"] != "Accepted" or log["status_code"] != 0 or log["issue_counts"]["error"]:
                    raise SigningError("Apple notarization did not accept the submitted archive")
                if wait_error is not None:
                    raise SigningError("Notarization wait failed despite its reported status; release output withheld")
                command("staple-ticket", ["/usr/bin/xcrun", "stapler", "staple", str(app)], 300)
                command("validate-stapled-ticket", ["/usr/bin/xcrun", "stapler", "validate", str(app)], 120)
                verify_signature(command, app, args)
                validate_bundle(app, args)
                verify_identity(command, staged_executable, args)
                report["post_staple_executable_sha256"] = digest(staged_executable)
                report["staple_validation"] = "passed"
                command("archive-stapled-app", ["/usr/bin/ditto", "-c", "-k", "--keepParent", str(app), str(partial)], 300)
                regular_file(partial)
                report["final_archive"] = {"name": final_name, "sha256": digest(partial), "bytes": partial.stat().st_size}
        report["credential_workspace_cleanup"] = "passed"
        # Only completed cleanup can expose the final artifact name.
        partial.rename(final)
        (args.output / "SHA256SUMS").write_text(f"{report['final_archive']['sha256']}  {final_name}\n", encoding="utf-8")
        submitted.unlink()
        report["status"] = "signed-notarized-stapled"
        write_report(args.output, report)
        return report
    except BaseException as error:
        partial.unlink(missing_ok=True)
        final.unlink(missing_ok=True)
        (args.output / "SHA256SUMS").unlink(missing_ok=True)
        report["status"] = "failed"
        if private is not None:
            report["credential_workspace_cleanup"] = "passed" if not private.exists() else "failed"
        report["failure"] = str(error) if isinstance(error, SigningError) else "Unexpected local failure; release output withheld"
        write_report(args.output, report)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True, help="Trusted extracted app; preserved unchanged")
    parser.add_argument("--output", type=Path, required=True, help="New private directory; never replaces outputs")
    parser.add_argument("--source", required=True, help="Expected full lowercase commit SHA")
    parser.add_argument("--version", required=True, help="Expected numeric major.minor.patch version")
    parser.add_argument("--target", required=True, choices=sorted(TARGETS))
    parser.add_argument("--executable-sha256", required=True, help="Expected pre-sign executable SHA-256")
    parser.add_argument("--identity", required=True, help="Expected Developer ID Application certificate SHA-1 fingerprint")
    parser.add_argument("--team-id", required=True)
    parser.add_argument("--notary-timeout-seconds", type=int, default=1800)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        with cancellation_handler():
            sign(args)
    except (SigningError, OSError, ValueError) as error:
        reason = str(error) if isinstance(error, SigningError) else "Unexpected local failure"
        print(f"Signing did not complete: {reason}. Inspect signing-result.json if the new output directory was created; do not automatically resubmit or publish.", file=sys.stderr)
        return 1
    print("Signed, notarized and stapled archive prepared. Downloaded-package Gatekeeper and native launch checks remain required.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
