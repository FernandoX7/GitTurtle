#!/usr/bin/env python3
"""Trusted release assembly; ordinary runs default to preparing reviewable bytes.

The workflow supplies the validated context through RELEASE_CONTEXT. No command
creates tags, chooses versions, consumes a PR artifact or retries remote writes.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


github = load("release_github", ROOT / "scripts/release/github.py")
identity = load("release_identity", ROOT / "scripts/release/identity.py")
packages = load("release_packages", ROOT / "scripts/ci/packages.py")
package_id = packages.identity
Error = github.Error
MAC = "aarch64-apple-darwin"
LINUX = "x86_64-unknown-linux-gnu"
PLATFORMS = {"linux": [LINUX], "macos": [MAC], "linux,macos": [LINUX, MAC]}


def write_json(path, value):
    with path.open("xb") as stream:
        stream.write(github.canonical(value))


def expected_package(directory, context, target):
    archives = [p for p in directory.iterdir() if p.name.endswith((".tar.gz", ".zip"))]
    if len(archives) != 1:
        raise Error("Expected exactly one platform archive")
    archive = archives[0]
    outer_file = Path(str(archive) + ".manifest.json")
    outer = package_id.read_json(outer_file)
    package = outer.get("package", {})
    return {"archive": archive.name, "archive_sha256": package_id.digest(archive),
            "manifest_sha256": package_id.digest(outer_file), "target": target,
            "revision": context["identity"]["commit"], "version": context["identity"]["version"],
            "input_binary_sha256": package.get("input_binary_sha256"),
            "cargo_lock_sha256": context["identity"]["cargo_lock_sha256"]}


def context_from_env(environ=None):
    env = os.environ if environ is None else environ
    raw = env.get("RELEASE_CONTEXT", "")
    if len(raw.encode()) > package_id.MAX_JSON:
        raise Error("Release context exceeds the bound")
    value = json.loads(raw, object_pairs_hook=github.unique_object)
    if not isinstance(value, dict) or value.get("format") != 1:
        raise Error("Unsupported release context")
    approved = approval_context(value)
    if value.get("approval_sha256") != hashlib.sha256(github.canonical(approved)).hexdigest():
        raise Error("Release context approval identity is inconsistent")
    github.validate_context(value, env)
    return value


def approval_context(context):
    # Run IDs are provenance, not the owner's enduring release choice. A retry
    # of the same approved source/context cannot silently change that choice.
    return {key: context[key] for key in ("identity", "macos_signing", "signing_identity", "quality")}


def api():
    return github.GitHub(os.environ.get("GH_TOKEN", ""))


def preflight(args):
    env = os.environ
    values = PLATFORMS.get(env.get("RELEASE_PLATFORMS"))
    if values is None:
        raise Error("Select an explicit supported platform set")
    signing = env.get("RELEASE_MACOS_SIGNING")
    if signing not in ("ad-hoc", "notarized") or (MAC not in values and signing != "ad-hoc"):
        raise Error("Notarization requires an explicitly promised macOS platform")
    selected = asdict(identity.verify_release_identity(ROOT, tag=env.get("RELEASE_TAG"),
        version=env.get("RELEASE_VERSION"), commit=env.get("RELEASE_COMMIT"), platforms=values,
        expected_tag_object=env.get("RELEASE_TAG_OBJECT")))
    selected["platforms"] = list(selected["platforms"])
    signing_identity = None
    if signing == "notarized":
        certificate = env.get("RELEASE_CERTIFICATE", "").upper()
        team = env.get("RELEASE_TEAM", "")
        if not re.fullmatch(r"[0-9A-F]{40}", certificate) or not re.fullmatch(r"[A-Z0-9]{10}", team):
            raise Error("Notarization requires the explicit Developer ID certificate fingerprint and Team ID")
        signing_identity = {"certificate_sha1": certificate, "team_id": team}
    elif env.get("RELEASE_CERTIFICATE") or env.get("RELEASE_TEAM"):
        raise Error("Certificate identity applies only to an explicitly notarized macOS release")
    context = {"format": 1, "identity": selected, "macos_signing": signing, "signing_identity": signing_identity,
               "run_id": github.positive(env.get("GITHUB_RUN_ID")),
               "run_attempt": github.positive(env.get("GITHUB_RUN_ATTEMPT"))}
    github.validate_context(context, env)
    service = api()
    github.remote_identity(service, selected)
    context["quality"] = github.quality_evidence(service, selected["commit"], env.get("RELEASE_QUALITY_RUN"))
    context["approval_sha256"] = hashlib.sha256(github.canonical(approval_context(context))).hexdigest()
    if env.get("RELEASE_PUBLISH") not in ("true", "false"):
        raise Error("Publication must be explicitly selected or disabled")
    # This output is bounded JSON, never a workflow expression or shell command.
    with Path(env["GITHUB_OUTPUT"]).open("a") as output:
        output.write("context=" + github.canonical(context).decode().strip() + "\n")
        output.write("linux=" + str(LINUX in values).lower() + "\n")
        output.write("macos=" + str(MAC in values).lower() + "\n")
        matrix = {"include": [{"target": target, "os": "macos-15" if target == MAC else "ubuntu-24.04"} for target in values]}
        output.write("matrix=" + github.canonical(matrix).decode().strip() + "\n")
    print("Release context approval SHA-256: " + context["approval_sha256"])
    print("Source preflight passed; package, signing and publication evidence remain separate.")


def select_xcode(_args=None, _context=None, *, applications=Path("/Applications"), environ=None):
    """Match the Quality lane's installed Xcode selection before cache identity.
    The selected toolchain applies through DEVELOPER_DIR; no shared host setting
    or developer directory is changed.
    """
    env = os.environ if environ is None else environ
    paths = sorted(applications.glob("Xcode*.app/Contents/Developer"))
    if len(paths) > 32:
        raise Error("Unexpected number of installed Xcode candidates")
    candidates = []
    for path in paths:
        if any(char in str(path) for char in "\r\n") or not path.is_dir():
            raise Error("Invalid installed Xcode path")
        status, text = packages.run(["/usr/bin/xcodebuild", "-version"],
            env={**env, "DEVELOPER_DIR": str(path)}, allowed=tuple(range(256)), timeout=20)
        match = re.search(r"^Xcode ([0-9]+(?:\.[0-9]+)*)$", text, re.MULTILINE)
        if status == 0 and match and int(match[1].split(".")[0]) >= 26:
            candidates.append((tuple(map(int, match[1].split("."))), path))
    if not candidates:
        raise Error("Packaging requires an installed full Xcode 26+ with Metal tools")
    selected = max(candidates)[1]
    with Path(env["GITHUB_ENV"]).open("a") as output:
        output.write("DEVELOPER_DIR=" + str(selected) + "\n")
    print("Selected " + selected.parent.parent.name)


def local_identity(context):
    wanted = context["identity"]
    actual = asdict(identity.verify_release_identity(ROOT, tag=wanted["tag"], version=wanted["version"],
        commit=wanted["commit"], platforms=wanted["platforms"], expected_tag_object=wanted["tag_object"]))
    actual["platforms"] = list(actual["platforms"])
    if actual != wanted:
        raise Error("Source checkout no longer matches the captured release context")


def prepare(args, context):
    """Package this job's fresh release build; strict notices fail before upload."""
    local_identity(context)
    if args.target not in context["identity"]["platforms"]:
        raise Error("Build target was not promised")
    args.directory.mkdir(mode=0o700)
    output = args.directory / "upload"
    output.mkdir(mode=0o700)
    binary = ROOT / "target" / args.target / "release/gitturtle"
    original = package_id.digest(binary)
    stem = (f"GitTurtle-{context['identity']['version']}-{context['identity']['commit'][:12]}-"
            f"{packages.TARGETS[args.target]}-release")
    command = [ROOT / f"scripts/package-{'linux' if args.target == LINUX else 'macos'}.sh",
               "--no-build", "--binary", binary, "--distribution",
               "--expected-revision", context["identity"]["commit"],
               "--expected-version", context["identity"]["version"], "--expected-sha256", original]
    if args.target == MAC:
        command.extend(("--archive-dir", output, args.directory / "GitTurtle.app"))
    else:
        command.append(args.directory / stem)
    packages.run(command, timeout=900)
    if args.target == LINUX:
        for suffix in (".tar.gz", ".tar.gz.sha256", ".tar.gz.manifest.json"):
            shutil.move(str(args.directory / (stem + suffix)), output)
    expected = expected_package(output, context, args.target)
    if expected["input_binary_sha256"] != original:
        raise Error("Package does not identify the fresh executable")
    payload, extracted, package = packages.verify_payload(output, expected, complete=True)
    packages.platform_checks(payload, extracted, package, args.directory)
    shutil.rmtree(output / "extracted")
    write_json(output / "receipt.json", {"format": 1, "context": context, "expected": expected,
        "producer": "fresh-release-build", "platform_checks": "passed",
        "native_interaction": "not-established-by-ci", "signing": package["signing"]})


def receipt(directory, context, target, *, signed=False):
    value = package_id.read_json(directory / "receipt.json")
    expected = value.get("expected", {})
    if (value.get("format") != 1 or value.get("context") != context
            or expected.get("target") != target or value.get("platform_checks") != "passed"
            or value.get("producer") != ("signed-release-build" if signed else "fresh-release-build")):
        raise Error("Artifact receipt is not the expected same-run release producer")
    for key, wanted in {"revision": context["identity"]["commit"], "version": context["identity"]["version"],
                        "cargo_lock_sha256": context["identity"]["cargo_lock_sha256"]}.items():
        if expected.get(key) != wanted:
            raise Error("Artifact receipt has a different release identity")
    archive = expected.get("archive")
    if not isinstance(archive, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,199}", archive):
        raise Error("Invalid platform archive name")
    names = {archive, archive + ".sha256", archive + ".manifest.json", "receipt.json"}
    if signed:
        names.add("signing-result.json")
    if set(p.name for p in directory.iterdir()) != names:
        raise Error("Platform artifact contains missing or unexpected files")
    return value


def signing_input(args, context):
    local_identity(context)
    if context["macos_signing"] != "notarized" or MAC not in context["identity"]["platforms"]:
        raise Error("Notarized macOS was not selected")
    github.remote_identity(api(), context["identity"])
    github.verify_checks(api(), context)
    value = receipt(args.directory, context, MAC)
    payload, binary, package = packages.verify_payload(args.directory, value["expected"], complete=True)
    packages.platform_checks(payload, binary, package, args.directory)
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"app={payload}\n")
        output.write(f"binary_sha256={package_id.digest(binary)}\n")


def signing_report(report, context, package=None):
    if (report.get("schema_version") != 1 or report.get("status") != "signed-notarized-stapled"
            or report.get("source_revision") != context["identity"]["commit"]
            or report.get("version") != context["identity"]["version"] or report.get("target") != MAC
            or report.get("staple_validation") != "passed"
            or report.get("credential_workspace_cleanup") != "passed"
            or report.get("keychain_cleanup", {}).get("status") != "passed"
            or report.get("notarization", {}).get("status") != "Accepted"):
        raise Error("Signing did not establish accepted, stapled bytes and completed cleanup")
    log = report.get("notarization", {}).get("log", {})
    if (log.get("status") != "Accepted" or log.get("status_code") != 0
            or log.get("sha256") != report.get("submitted_archive_sha256")
            or log.get("job_id") != report["notarization"].get("id")
            or log.get("issue_counts", {}).get("error") != 0):
        raise Error("Signing report has no matching accepted notarization log")
    for key in ("pre_sign_executable_sha256", "signed_executable_sha256", "post_staple_executable_sha256",
                "submitted_archive_sha256"):
        if not isinstance(report.get(key), str) or not re.fullmatch(r"[0-9a-f]{64}", report[key]):
            raise Error("Signing report is missing exact byte identity")
    expected_identity = dict(application="GitTurtle", source_revision=context["identity"]["commit"],
                             version=context["identity"]["version"], target=MAC, source_tree="clean", profile="release")
    signature = report.get("signature", {})
    if (context.get("signing_identity") is None
            or any(signature.get(key) != value for key, value in context["signing_identity"].items())
            or signature.get("type") != "Developer ID Application" or signature.get("hardened_runtime") is not True
            or signature.get("secure_timestamp") is not True or signature.get("entitlements") != {}
            or report.get("compiled_identity") != expected_identity):
        raise Error("Signing report does not match the approved certificate or compiled identity")
    if package is not None and (package.get("binary_sha256") != report["post_staple_executable_sha256"]
                               or any(package.get("compiled_identity", {}).get(k) != v for k, v in expected_identity.items())):
        raise Error("Signed package and signing report identify different executables")


def signed_validator(report, context):
    def validate(package, binary, licenses):
        signing_report(report, context, package)
        if package.get("signing") != "developer-id-notarized":
            raise Error("Signed package does not declare its actual signing status")
        # Only the signature enum differs in the shared local package contract.
        # All final file digests, compiled identity and complete notices remain
        # subject to its ordinary validation; the signed contract is checked above.
        package_id.validate_manifest({**package, "signing": "ad-hoc"}, binary, licenses)
    return validate


def app_snapshot(app):
    result = {}
    if not app.is_dir() or app.is_symlink():
        raise Error("Expected a regular app directory")
    for directory, dirs, files in os.walk(app, followlinks=False):
        for name in dirs + files:
            path = Path(directory) / name
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode) or not (stat.S_ISDIR(mode) or stat.S_ISREG(mode)):
                raise Error("Restored app contains a link or special file")
            if len(result) >= packages.MAX_ENTRIES:
                raise Error("Restored app exceeds the entry bound")
            result[path.relative_to(app).as_posix()] = ("directory" if stat.S_ISDIR(mode)
                else (package_id.digest(path), bool(mode & 0o111)))
    return result


def finish_signing(args, context):
    """Repack only after the signing helper has removed credentials. Add detached
    provenance outside the sealed app, then hash the actual final download.
    """
    local_identity(context)
    report = package_id.read_json(args.signing / "signing-result.json")
    signing_report(report, context)
    before = package_id.read_json(args.directory / "receipt.json")
    if before.get("context") != context or before.get("expected", {}).get("target") != MAC:
        raise Error("Signing input receipt has a different source")
    outer = package_id.read_json(args.directory / (before["expected"]["archive"] + ".manifest.json"))
    package = outer["package"]
    if report["pre_sign_executable_sha256"] != package["binary_sha256"]:
        raise Error("Signing input does not match the verified ad-hoc package")
    name = report.get("final_archive", {}).get("name")
    if not isinstance(name, str) or not re.fullmatch(r"GitTurtle-[A-Za-z0-9._+-]+\.zip", name):
        raise Error("Invalid signed archive filename")
    archive = args.signing / name
    if github.file_record(archive) != {"name": name, "sha256": report["final_archive"].get("sha256"),
                                    "bytes": report["final_archive"].get("bytes")}:
        raise Error("Signing helper archive bytes have changed")
    stage = args.signing / "verified"
    packages.extract(archive, stage)
    if not set(p.name for p in stage.iterdir()) <= {"GitTurtle.app", "__MACOSX"} or not (stage / "GitTurtle.app").is_dir():
        raise Error("Unexpected signed archive root")
    app = stage / "GitTurtle.app"
    binary = app / "Contents/MacOS/gitturtle"
    final_package = {**package, "signing": "developer-id-notarized",
                     "binary_sha256": report["post_staple_executable_sha256"]}
    signed_validator(report, context)(final_package, binary, app / "Contents/Resources/licenses")
    if package_id.probe(binary) != final_package["compiled_identity"]:
        raise Error("Signed executable does not report the expected compiled identity")
    write_json(stage / "build-info.json", final_package)
    args.output.mkdir(mode=0o700)
    final = args.output / name
    # Preserve every signed app entry, AppleDouble metadata and ticket from the
    # helper's ZIP. Add only detached provenance; never rebuild the sealed app.
    shutil.copyfile(archive, final)
    with zipfile.ZipFile(final, "a", compression=zipfile.ZIP_DEFLATED) as output:
        output.write(stage / "build-info.json", "build-info.json")
    write_json(Path(str(final) + ".manifest.json"), package_id.archive_manifest(final, stage / "build-info.json"))
    Path(str(final) + ".sha256").write_text(f"{package_id.digest(final)}  {final.name}\n")
    report = {**report, "helper_final_archive_role": "stapled-app-before-detached-package-provenance",
              "release_archive": github.file_record(final)}
    write_json(args.output / "signing-result.json", report)
    expected = expected_package(args.output, context, MAC)
    payload, _, _ = packages.verify_payload(args.output, expected, complete=True,
                                            manifest_validator=signed_validator(report, context))
    # The bounded parser establishes safe ordinary entries first. ditto then
    # restores platform metadata into a separate new tree for Apple validation.
    # Compare the actual app's regular bytes/executable intent before use.
    restored = args.signing / "restored-final"
    restored.mkdir(mode=0o700)
    packages.run(["/usr/bin/ditto", "-x", "-k", final, restored])
    if app_snapshot(restored / "GitTurtle.app") != app_snapshot(payload):
        raise Error("Native archive extraction differs from the verified app bytes")
    packages.run(["codesign", "--verify", "--deep", "--strict", restored / "GitTurtle.app"])
    packages.run(["xcrun", "stapler", "validate", restored / "GitTurtle.app"])
    shutil.rmtree(args.output / "extracted")
    write_json(args.output / "receipt.json", {"format": 1, "context": context, "expected": expected,
        "producer": "signed-release-build", "platform_checks": "passed",
        "native_interaction": "pending-user-verification", "signing": "developer-id-notarized",
        "signing_report_sha256": package_id.digest(args.output / "signing-result.json")})


def assemble(args, context):
    local_identity(context)
    args.output.mkdir(mode=0o700)
    actual = []
    platform_records = []
    for target in context["identity"]["platforms"]:
        signed = target == MAC and context["macos_signing"] == "notarized"
        artifact = ("release-signed-" if signed else "release-package-") + target
        directory = args.directory / f"{artifact}-{context['run_id']}-{context['run_attempt']}"
        record = receipt(directory, context, target, signed=signed)
        validator = None
        if signed:
            report_file = directory / "signing-result.json"
            if package_id.digest(report_file) != record.get("signing_report_sha256"):
                raise Error("Signing record differs from its same-run receipt")
            report = package_id.read_json(report_file)
            if report.get("release_archive") != github.file_record(directory / record["expected"]["archive"]):
                raise Error("Signing provenance identifies different final release archive bytes")
            validator = signed_validator(report, context)
        _, _, package = packages.verify_payload(directory, record["expected"], complete=True,
                                                 probe=False, manifest_validator=validator)
        actual.append(package["target"])
        platform_records.append({"target": target, "signing": package["signing"],
                                 "expected": record["expected"],
                                 "native_interaction": record["native_interaction"]})
        names = [record["expected"]["archive"] + suffix for suffix in ("", ".sha256", ".manifest.json")]
        for name in names:
            shutil.copyfile(directory / name, args.output / name)
        if signed:
            shutil.copyfile(directory / "signing-result.json", args.output / "macos-signing-result.json")
    identity.require_complete_platforms(context["identity"]["platforms"], actual)
    required_artifacts = {f"{'release-signed-' if target == MAC and context['macos_signing'] == 'notarized' else 'release-package-'}{target}-{context['run_id']}-{context['run_attempt']}"
                          for target in context["identity"]["platforms"]}
    # The original Mac package may be present beside its signed transformation.
    allowed = required_artifacts | ({f"release-package-{MAC}-{context['run_id']}-{context['run_attempt']}"}
                                    if MAC in actual and context["macos_signing"] == "notarized" else set())
    if not set(p.name for p in args.directory.iterdir()) <= allowed:
        raise Error("Unexpected platform artifacts were downloaded")
    source = context["identity"]
    notes = [f"# GitTurtle {source['version']}", "", f"Source: `{source['commit']}`",
             f"Tag object: `{source['tag_object']}`", f"Release context: `{context['approval_sha256']}`", "",
             "## Downloads", ""]
    for item in platform_records:
        notes.append(f"- `{item['expected']['archive']}` — `{item['target']}`, {item['signing']}.")
    notes += ["", "Every archive includes complete target-specific license notices. Verify its SHA-256 before extraction.",
              "Provenance and checksums identify these exact bytes; automated package checks do not establish a desktop launch."]
    if MAC in actual:
        notes += ["", "macOS Gatekeeper, quarantine and native launch evidence remains a separate release-owner requirement.",
                  "Ad-hoc packages are developer test builds and are not Developer ID signed or notarized."]
    (args.output / "RELEASE_NOTES.md").write_text("\n".join(notes) + "\n")
    assets = [github.file_record(p) for p in sorted(args.output.iterdir())]
    (args.output / "SHA256SUMS").write_text("".join(f"{item['sha256']}  {item['name']}\n" for item in assets))
    assets.append(github.file_record(args.output / "SHA256SUMS"))
    write_json(args.output / "release-manifest.json", {"format": 1, "context": context,
               "platforms": platform_records, "assets": assets})
    print("Assembled release manifest SHA-256: " + package_id.digest(args.output / "release-manifest.json"))
    print("Assembly is ready for review; publication and downloaded native verification are separate actions.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("preflight")
    sub.add_parser("select-xcode")
    p = sub.add_parser("prepare")
    p.add_argument("--target", choices=(MAC, LINUX), required=True)
    p.add_argument("--directory", type=Path, required=True)
    p = sub.add_parser("signing-input")
    p.add_argument("--directory", type=Path, required=True)
    p = sub.add_parser("finish-signing")
    for name in ("directory", "signing", "output"):
        p.add_argument("--" + name, type=Path, required=True)
    p = sub.add_parser("assemble")
    p.add_argument("--directory", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p = sub.add_parser("activate-signing")
    p = sub.add_parser("publish")
    p.add_argument("--directory", type=Path, required=True)
    p.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        if args.command == "preflight":
            preflight(args)
        else:
            context = context_from_env()
            if args.command == "activate-signing":
                github.require_activation(context, os.environ, "GITTURTLE_SIGNING_CONTEXT")
            elif args.command == "publish":
                if os.environ.get("RELEASE_PUBLISH") != "true":
                    raise Error("Publication was not explicitly requested")
                github.require_activation(context, os.environ, "GITTURTLE_RELEASE_CONTEXT")
                local_identity(context)
                result = github.publish(api(), context, args.directory, args.report)
                print("Release publication status: " + result["status"])
            else:
                globals()[args.command.replace("-", "_")](args, context)
    except (Error, identity.IdentityError, package_id.PackageError, OSError, ValueError, KeyError) as error:
        # Never print raw network response bodies or environment values.
        reason = str(error) if isinstance(error, (Error, identity.IdentityError, package_id.PackageError)) else "Malformed or unavailable release input"
        print("Release operation refused: " + reason, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
