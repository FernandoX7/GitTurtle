#!/usr/bin/env python3
"""Build a verified local ad-hoc Apple Silicon package; never notarize or install it."""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import zipfile

_spec = importlib.util.spec_from_file_location("package_identity", Path(__file__).with_name("package-identity.py"))
identity = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(identity)
PackageError = identity.PackageError
TARGET = "aarch64-apple-darwin"
MAX_TREE = 2 * 1024**3
MAX_FILES = 30000
MAX_TOOL_OUTPUT = 8 * 1024**2


def diagnostic_excerpt(text):
    if len(text) <= 8192:
        return text
    notice = "\n... [diagnostic output truncated] ...\n"
    head = (8192 - len(notice)) // 2
    tail = 8192 - len(notice) - head
    return text[:head] + notice + text[-tail:]


def run(command, *, cwd=None, timeout=120, stdout_only=False):
    """Bound tools; structured stdout keeps stderr in separate diagnostics."""
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        child = subprocess.Popen([str(item) for item in command], cwd=cwd,
                                 stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                 start_new_session=True)
        try:
            deadline = time.monotonic() + timeout
            while child.poll() is None:
                if time.monotonic() >= deadline:
                    raise PackageError(f"{command[0]} timed out")
                if os.fstat(out.fileno()).st_size + os.fstat(err.fileno()).st_size > MAX_TOOL_OUTPUT:
                    raise PackageError(f"{command[0]} exceeded the diagnostic limit")
                time.sleep(0.05)
            if os.fstat(out.fileno()).st_size + os.fstat(err.fileno()).st_size > MAX_TOOL_OUTPUT:
                raise PackageError(f"{command[0]} exceeded the diagnostic limit")
            out.seek(0)
            err.seek(0)
            stdout = out.read().decode("utf-8", errors="replace")
            stderr = err.read().decode("utf-8", errors="replace")
            if child.returncode:
                detail = diagnostic_excerpt(stderr or stdout)
                raise PackageError(f"{command[0]} failed ({child.returncode}): {detail}")
            if stdout_only:
                sys.stderr.write(diagnostic_excerpt(stderr))
                return stdout
            return stdout + stderr
        finally:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()


def exists(path):
    return path.exists() or path.is_symlink()


def tree_digest(root):
    """Reject links/devices and bound payloads before copying or replacing them."""
    if root.is_symlink() or not root.is_dir():
        raise PackageError(f"Expected a real directory: {root}")
    digest = hashlib.sha256()
    size = count = 0
    for directory, directories, files in os.walk(root, followlinks=False):
        directories.sort()
        for name in sorted(directories + files):
            path = Path(directory) / name
            info = path.lstat()
            count += 1
            if count > MAX_FILES:
                raise PackageError("Package tree has too many entries")
            relative = path.relative_to(root).as_posix().encode()
            if stat.S_ISDIR(info.st_mode):
                entry = b"directory"
            elif stat.S_ISREG(info.st_mode):
                size += info.st_size
                if size > MAX_TREE:
                    raise PackageError("Package tree exceeds the size limit")
                entry = identity.digest(path).encode()
            else:
                raise PackageError(f"Package contains a link or special file: {path}")
            digest.update(relative + b"\0" + str(stat.S_IMODE(info.st_mode)).encode() + b"\0" + entry)
    return digest.hexdigest()


def load_plist(path):
    identity.regular(path, identity.MAX_JSON)
    value = plistlib.loads(path.read_bytes())
    if not isinstance(value, dict):
        raise PackageError("Expected an Info.plist dictionary")
    return value


def verify_bundle(bundle, version=None):
    tree_digest(bundle)
    if set(path.name for path in bundle.iterdir()) != {"Contents"}:
        raise PackageError("Existing output is not a conventional GitTurtle bundle")
    contents = bundle / "Contents"
    allowed = {"Info.plist", "MacOS", "Resources", "_CodeSignature", "PkgInfo"}
    if not set(path.name for path in contents.iterdir()) <= allowed:
        raise PackageError("Bundle contains unrecognized top-level code or data")
    info = load_plist(contents / "Info.plist")
    for key, value in {"CFBundleIdentifier": identity.APP_ID, "CFBundleExecutable": "gitturtle",
                       "CFBundlePackageType": "APPL"}.items():
        if info.get(key) != value:
            raise PackageError("Refusing an unrelated or malformed app bundle")
    if set(path.name for path in (contents / "MacOS").iterdir()) != {"gitturtle"}:
        raise PackageError("Unexpected nested executable")
    binary = contents / "MacOS/gitturtle"
    identity.verify_architecture(binary, TARGET)
    if not os.access(binary, os.X_OK):
        raise PackageError("Packaged executable is not executable")
    if not (contents / "Resources").is_dir():
        raise PackageError("Bundle resources are missing")
    if version is not None:
        for key in ("CFBundleVersion", "CFBundleShortVersionString"):
            if info.get(key) != version:
                raise PackageError("Bundle version does not match its executable")
        if info.get("CFBundleIconName") != "AppIcon" or info.get("CFBundleIconFile") not in ("AppIcon", "AppIcon.icns"):
            raise PackageError("Generated icon names are missing or unexpected")
        for resource in ("Assets.car", "AppIcon.icns"):
            path = contents / "Resources" / resource
            identity.regular(path, identity.MAX_BINARY)
            if path.stat().st_size == 0:
                raise PackageError("Compiled icon resource is empty")
    return info


def output_snapshot(bundle, sidecar):
    old = {}
    if exists(bundle):
        verify_bundle(bundle)
        old[bundle] = tree_digest(bundle)
    if exists(sidecar):
        if bundle not in old:
            raise PackageError("Refusing an unrelated existing package manifest")
        value = identity.read_json(sidecar)
        identity.validate_manifest(value, bundle / "Contents/MacOS/gitturtle",
                                   bundle / "Contents/Resources/licenses")
        old[sidecar] = identity.digest(sidecar)
    return old


def prerequisite_versions():
    xcode = run(["xcodebuild", "-version"])
    match = re.search(r"^Xcode (\d+)(?:\.[0-9]+)*$", xcode, re.MULTILINE)
    if match is None or int(match[1]) < 26:
        raise PackageError("Select full Xcode 26 or later for Icon Composer assets")
    tools = {"xcode": xcode.strip(), "developer_directory": run(["xcode-select", "-p"]).strip()}
    for name in ("actool", "metal", "assetutil"):
        tools[name + "_path"] = run(["xcrun", "--find", name]).strip()
    tools["metal"] = run(["xcrun", "metal", "--version"]).strip()
    tools["actool"] = run(["xcrun", "actool", "--version"]).strip()
    if not tools["metal"] or not tools["actool"]:
        raise PackageError("Xcode Metal/actool version information is missing")
    return tools


def inspect_catalog(path):
    value = json.loads(run(["xcrun", "assetutil", "--info", path], stdout_only=True))
    if not isinstance(value, list) or not value or not all(isinstance(item, dict) for item in value):
        raise PackageError("Compiled icon catalog could not be inspected")
    # Apple changes rendition schemas between Xcode releases. Retain the actual
    # decoded catalog, and verify appearance visually on the target macOS host.
    return value


def archive_name(version, revision, profile):
    return f"GitTurtle-{version}-{revision[:12]}-macos-arm64-{profile}.zip"


def inspect_zip(path):
    total = 0
    names = set()
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if len(entries) > MAX_FILES:
            raise PackageError("Archive has too many entries")
        for entry in entries:
            name = entry.filename
            pieces = name.rstrip("/").split("/")
            if (name in names or name.startswith("/") or "\\" in name or any(part in ("", ".", "..") for part in pieces)
                    or pieces[0] not in ("GitTurtle.app", "build-info.json", "__MACOSX")
                    or stat.S_ISLNK(entry.external_attr >> 16)):
                raise PackageError("Unsafe or unexpected ZIP entry")
            names.add(name)
            total += entry.file_size
            if total > MAX_TREE:
                raise PackageError("Expanded archive exceeds the size limit")
        if "build-info.json" not in names or "GitTurtle.app/Contents/MacOS/gitturtle" not in names:
            raise PackageError("Archive is missing its app or detached identity")
        if archive.testzip() is not None:
            raise PackageError("Archive CRC verification failed")


def fingerprint(path):
    return tree_digest(path) if path.is_dir() and not path.is_symlink() else identity.digest(path)


def rename_exclusive(source, destination):
    """Never replace an output created after preflight, including empty folders."""
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "darwin":
        function = libc.renamex_np
        function.argtypes = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
        result = function(os.fsencode(source), os.fsencode(destination), 0x00000004)  # RENAME_EXCL
    elif sys.platform.startswith("linux"):
        # Linux is used only by disposable source fixtures, not Mac packaging.
        function = libc.renameat2
        function.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
        result = function(-100, os.fsencode(source), -100, os.fsencode(destination), 1)  # RENAME_NOREPLACE
    else:
        raise PackageError("Exclusive rename is unsupported on this platform")
    if result:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), str(destination))


def publish(outputs, old, stage):
    """Rollback ordinary failures; retain backups if another process changed output."""
    for destination in outputs:
        if exists(destination):
            if destination not in old or fingerprint(destination) != old[destination]:
                raise PackageError(f"Output appeared or changed during packaging: {destination}")
        elif destination in old:
            raise PackageError(f"Existing output disappeared during packaging: {destination}")
    write_json(stage / "publication.json", {
        "format": 1,
        "outputs": [{"destination": str(destination), "prepared": str(source),
                     "prepared_sha256": fingerprint(source)} for destination, source in outputs.items()],
        "previous": [{"destination": str(destination), "backup": str(stage / f"previous-{number}"),
                      "sha256": old[destination]} for number, destination in enumerate(old)],
    })
    backups = {}
    published = {}
    try:
        for number, destination in enumerate(old):
            backup = stage / f"previous-{number}"
            rename_exclusive(destination, backup)
            backups[destination] = backup
            if fingerprint(backup) != old[destination]:
                raise PackageError(f"Prior output changed during backup capture: {destination}")
        for destination, source in outputs.items():
            expected = fingerprint(source)
            if source.is_dir():
                if exists(destination):
                    raise PackageError(f"Output appeared during packaging: {destination}")
                rename_exclusive(source, destination)
            else:
                # link is atomic and fails rather than replacing a concurrent file.
                os.link(source, destination)
                source.unlink()
            published[destination] = expected
        # A process with an open handle can still change captured files. Check
        # again after publication before successful staging cleanup discards them.
        for destination, backup in backups.items():
            if fingerprint(backup) != old[destination]:
                raise PackageError(f"Prior output changed after backup capture: {destination}")
    except BaseException as original:
        errors = []
        for destination, expected in reversed(list(published.items())):
            try:
                if fingerprint(destination) != expected:
                    raise PackageError("new output was changed externally")
                if destination.is_dir():
                    shutil.rmtree(destination)
                else:
                    destination.unlink()
            except (OSError, ValueError) as error:
                errors.append(f"{destination}: {error}")
        for destination, backup in backups.items():
            try:
                if exists(destination):
                    raise PackageError("destination occupied; prior version retained")
                rename_exclusive(backup, destination)
            except (OSError, ValueError) as error:
                errors.append(f"{destination}: {error}")
        if errors:
            raise PackageError(f"Publication failed; recovery files retained at {stage}: {'; '.join(errors)}") from original
        raise


def write_json(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def package(args, root):
    profile = "debug" if args.debug else "release"
    if args.binary and not args.no_build:
        raise PackageError("--binary requires --no-build")
    if args.distribution and (args.debug or not args.archive_dir):
        raise PackageError("Distribution requires release profile and --archive-dir")
    revision, version, _lock, tree = identity.expectations(root, args.expected_revision, args.expected_version)
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise PackageError("macOS plist versions require three numeric components; prerelease mapping is not configured")
    if args.distribution and tree != "clean":
        raise PackageError("Distribution requires a clean packaging checkout")
    if args.expected_sha256 is not None and not re.fullmatch(r"[0-9a-f]{64}", args.expected_sha256):
        raise PackageError("Expected SHA-256 must contain 64 lowercase hex digits")
    bundle = Path(os.path.abspath(args.bundle or root / "dist/GitTurtle.app"))
    if bundle.suffix != ".app":
        raise PackageError("The output path must end in .app")
    bundle.parent.mkdir(parents=True, exist_ok=True)
    # Resolve the explicitly selected parent (macOS /tmp itself is a symlink),
    # but never resolve or follow an existing bundle/sidecar.
    bundle = bundle.parent.resolve() / bundle.name
    sidecar = bundle.with_name(bundle.name + ".build-info.json")
    lock = bundle.with_name("." + bundle.name + ".package-lock")
    try:
        lock.mkdir(mode=0o700)
    except FileExistsError as error:
        raise PackageError(f"Package output is locked: {lock}; inspect any interrupted run before removing its lock") from error
    stage = None
    archive_stage = None
    try:
        old = output_snapshot(bundle, sidecar)
        archive_dir = Path(args.archive_dir).absolute() if args.archive_dir else None
        archive_paths = []
        if archive_dir is not None:
            archive_dir.mkdir(parents=True, exist_ok=True)
            archive_dir = archive_dir.resolve()
            archive = archive_dir / archive_name(version, revision, profile)
            archive_paths = [archive, Path(str(archive) + ".sha256"), Path(str(archive) + ".manifest.json")]
            if any(exists(path) for path in archive_paths):
                raise PackageError("Archive or archive sidecar exists; choose a new archive directory")
        tools = prerequisite_versions()
        binary = Path(args.binary).absolute() if args.binary else root / f"target/{TARGET}/{profile}/gitturtle"
        if not args.no_build:
            command = ["cargo", "build", "--locked", "-p", "gitturtle", "--target", TARGET, "--target-dir", root / "target"]
            if profile == "release":
                command.append("--release")
            run(command, cwd=root, timeout=3600)
        identity.verify_architecture(binary, TARGET)
        input_hash = identity.digest(binary)
        if args.expected_sha256 is not None and input_hash != args.expected_sha256:
            raise PackageError("Executable SHA-256 differs from the expected input")
        compiled = identity.probe(binary)
        identity.validate_identity(compiled, revision=revision, version=version, target=TARGET,
                                   profile=profile, distribution=args.distribution)
        if args.no_build and (tree != "clean" or compiled["source_tree"] != "clean") and args.expected_sha256 is None:
            raise PackageError("Reusing modified source requires an explicit --expected-sha256 for the reviewed executable")
        stage = Path(tempfile.mkdtemp(prefix=".gitturtle-package-", dir=bundle.parent))
        app = stage / "GitTurtle.app"
        resources = app / "Contents/Resources"
        resources.mkdir(parents=True)
        (app / "Contents/MacOS").mkdir()
        final_binary = app / "Contents/MacOS/gitturtle"
        shutil.copyfile(binary, final_binary)
        final_binary.chmod(0o755)
        if identity.digest(final_binary) != input_hash or identity.digest(binary) != input_hash:
            raise PackageError("Executable changed while packaging inputs were captured")
        command = [sys.executable, root / "scripts/collect-third-party-licenses.py", "--target", TARGET]
        if args.distribution:
            command.append("--require-complete")
        run(command + [resources / "licenses"], cwd=root, timeout=600)
        # Before icon/signature work, validate the actual notices and staged
        # executable. Signing status below records the package's intended mode;
        # this temporary manifest is never published as completed output.
        identity.create_manifest(binary=final_binary, licenses=resources / "licenses", root=root,
                                 target=TARGET, profile=profile, revision=revision, version=version,
                                 expected_sha256=input_hash, distribution=args.distribution, signing="ad-hoc")
        icon_source = root / "assets/AppIcon.icon"
        tree_digest(icon_source)
        identity.regular(icon_source / "icon.json", identity.MAX_JSON)
        icon_dir = stage / "icon"
        icon_dir.mkdir()
        run(["xcrun", "actool", icon_source, "--compile", icon_dir, "--platform", "macosx",
             "--minimum-deployment-target", "11.0", "--app-icon", "AppIcon",
             "--output-partial-info-plist", icon_dir / "icon-info.plist",
             "--output-format", "human-readable-text", "--warnings", "--notices"])
        generated = load_plist(icon_dir / "icon-info.plist")
        # Do not let generated tool output override executable, identity/version,
        # document handlers or runtime permissions.
        icon_keys = {"CFBundleIconFile", "CFBundleIconName"}
        if set(generated) - icon_keys:
            raise PackageError("actool emitted unexpected bundle metadata; review it before packaging")
        if generated.get("CFBundleIconName") != "AppIcon" or generated.get("CFBundleIconFile") not in ("AppIcon", "AppIcon.icns"):
            raise PackageError("actool did not produce the expected AppIcon names")
        for name in ("AppIcon.icns", "Assets.car"):
            identity.regular(icon_dir / name, identity.MAX_BINARY)
            shutil.copyfile(icon_dir / name, resources / name)
            (resources / name).chmod(0o644)
        tools["icon_catalog"] = inspect_catalog(resources / "Assets.car")
        info = dict(CFBundleName="GitTurtle", CFBundleDisplayName="GitTurtle", CFBundleExecutable="gitturtle",
                    CFBundleIdentifier=identity.APP_ID, CFBundlePackageType="APPL", CFBundleShortVersionString=version,
                    CFBundleVersion=version, NSPrincipalClass="NSApplication", NSHighResolutionCapable=True,
                    NSSupportsAutomaticGraphicsSwitching=True, LSMinimumSystemVersion="11.0", **generated)
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        verify_bundle(app, version)
        run(["plutil", "-lint", app / "Contents/Info.plist"])
        # This bundle has one executable and no nested code; seal resources once.
        run(["codesign", "--force", "--sign", "-", app])
        run(["codesign", "--verify", "--deep", "--strict", app])
        signature = run(["codesign", "--display", "--verbose=4", app])
        if not re.search(r"^Signature=adhoc$", signature, re.MULTILINE):
            raise PackageError("Final bundle does not report an ad-hoc signature")
        verify_bundle(app, version)
        manifest = identity.create_manifest(binary=final_binary, licenses=resources / "licenses", root=root,
                                            target=TARGET, profile=profile, revision=revision, version=version,
                                            input_sha256=input_hash, distribution=args.distribution,
                                            signing="ad-hoc", built=not args.no_build)
        if manifest["compiled_identity"] != compiled:
            raise PackageError("Compiled identity changed after bundle signing")
        manifest["macos_package"] = dict(minimum_deployment_target="11.0", toolchain=tools,
                                          signature="\n".join(line for line in signature.splitlines() if line.startswith(("Identifier=", "Format=", "CodeDirectory ", "Signature=", "CDHash=", "TeamIdentifier="))),
                                          notarization="not-performed",
                                          icon_sha256={name: identity.digest(resources / name) for name in ("Assets.car", "AppIcon.icns")})
        package_info = stage / "build-info.json"
        write_json(package_info, manifest)
        outputs = {bundle: app, sidecar: package_info}
        if archive_dir is not None:
            archive_stage = Path(tempfile.mkdtemp(prefix=".gitturtle-archive-", dir=archive_dir))
            archive_root = archive_stage / "payload"
            archive_root.mkdir()
            run(["ditto", app, archive_root / "GitTurtle.app"])
            shutil.copyfile(package_info, archive_root / "build-info.json")
            zipped = archive_stage / archive_paths[0].name
            run(["ditto", "-c", "-k", "--sequesterRsrc", archive_root, zipped])
            inspect_zip(zipped)
            extracted = archive_stage / "extracted"
            run(["ditto", "-x", "-k", zipped, extracted])
            extracted_app = extracted / "GitTurtle.app"
            verify_bundle(extracted_app, version)
            if tree_digest(extracted_app) != tree_digest(app) or identity.digest(extracted / "build-info.json") != identity.digest(package_info):
                raise PackageError("Extracted archive differs from the verified bundle")
            identity.validate_manifest(identity.read_json(extracted / "build-info.json"),
                                       extracted_app / "Contents/MacOS/gitturtle", extracted_app / "Contents/Resources/licenses")
            run(["codesign", "--verify", "--deep", "--strict", extracted_app])
            checksum = archive_stage / archive_paths[1].name
            checksum.write_text(f"{identity.digest(zipped)}  {zipped.name}\n")
            archive_info = archive_stage / archive_paths[2].name
            write_json(archive_info, identity.archive_manifest(zipped, package_info))
            outputs.update(zip(archive_paths, (zipped, checksum, archive_info)))
        publish(outputs, old, stage)
        for completed_stage in (stage, archive_stage):
            if completed_stage:
                try:
                    shutil.rmtree(completed_stage)
                except OSError as error:
                    print(f"Verified output was published; staging cleanup needs attention at {completed_stage}: {error}", file=sys.stderr)
        return outputs
    except BaseException as error:
        if stage and stage.exists():
            (stage / "failure.txt").write_text(f"Incomplete package; not verified output.\n{type(error).__name__}: {error}\n")
            print(f"Failed-package diagnostics retained at {stage}", file=sys.stderr)
        if archive_stage and archive_stage.exists():
            print(f"Incomplete archive retained at {archive_stage}", file=sys.stderr)
        raise
    finally:
        lock.rmdir()


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--debug", action="store_true")
    result.add_argument("--no-build", action="store_true")
    result.add_argument("--binary", type=Path, help="Trusted executable to reuse; requires --no-build")
    result.add_argument("--expected-revision")
    result.add_argument("--expected-version")
    result.add_argument("--expected-sha256", help="SHA-256 of the executable before ad-hoc signing")
    result.add_argument("--distribution", action="store_true", help="Require clean release identity and complete notices; still ad-hoc")
    result.add_argument("--archive-dir", type=Path, help="New versioned ZIP and checksum/manifest outputs")
    result.add_argument("bundle", nargs="?", type=Path)
    return result


def interrupted(_signal, _frame):
    raise KeyboardInterrupt


def main():
    arguments = parser()
    args = arguments.parse_args()
    os.umask(0o077)
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        arguments.exit(1, "macOS packages require an Apple Silicon macOS host; Intel/universal output is unsupported.\n")
    signal.signal(signal.SIGTERM, interrupted)
    try:
        outputs = package(args, Path(__file__).resolve().parent.parent)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        arguments.exit(1, f"macOS packaging failed: {error}\n")
    except KeyboardInterrupt:
        arguments.exit(130, "macOS packaging interrupted; inspect any retained staging diagnostics.\n")
    print("Prepared local ad-hoc package (not notarized):")
    for output in outputs:
        print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
