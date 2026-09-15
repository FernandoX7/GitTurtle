#!/usr/bin/env python3
"""One manual actool observation; never build Rust, package, sign or retry.

The workflow runs each Xcode/method pair on a fresh macos-15 runner. Only the
evidence directory may be uploaded; compiled icons remain in the work directory.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import itertools
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys
import time
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
CAP = 8 * 1024 * 1024
EXPECTED_ICON = {
    "Assets/turtle.png": "73262e257f69c3b474ad02356dc42d15e916183c539ebeb2ba5bb9c1e70bb9a1",
    "icon.json": "4d095aa7727d47013e2ddaa50a4df035ab6272c1e961804450c7203c659cf7d8",
}


def digest(path):
    if path.stat().st_size > 64 * 1024 * 1024:
        raise RuntimeError("Diagnostic file exceeds the 64MiB hashing bound")
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write_json(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def capture_log(source, evidence, name):
    """Retain full output up to CAP plus independent head/tail excerpts."""
    size = source.stat().st_size
    with source.open("rb") as stream:
        full = stream.read(CAP)
        stream.seek(max(0, size - 8192))
        tail = stream.read(8192)
    for suffix, data in (("full.log", full), ("head.log", full[:8192]), ("tail.log", tail)):
        (evidence / f"{name}.{suffix}").write_bytes(data)
    return {"original_bytes": size, "retained_bytes": len(full), "truncated": size > CAP,
            "full_sha256": digest(evidence / f"{name}.full.log")}


def direct(command, work, *, timeout=120):
    """Match direct shell execution: child inherits this process's session."""
    started = time.monotonic()
    with (work / "stdout.raw").open("xb") as out, (work / "stderr.raw").open("xb") as err:
        child = subprocess.Popen(command, cwd=ROOT, stdin=subprocess.DEVNULL,
                                 stdout=out, stderr=err, start_new_session=False)
        reason = None
        try:
            while child.poll() is None:
                if time.monotonic() - started > timeout:
                    reason = "timeout"
                    break
                if os.fstat(out.fileno()).st_size + os.fstat(err.fileno()).st_size > CAP:
                    reason = "diagnostic limit"
                    break
                time.sleep(0.05)
        finally:
            if child.poll() is None:
                # Do not signal the inherited process group or shared Apple
                # services. GitHub cleans remaining job-owned processes.
                child.kill()
            child.wait()
    return {"returncode": child.returncode, "error": reason,
            "elapsed_seconds": time.monotonic() - started,
            "start_new_session": False,
            "cleanup_scope": "direct child only; no killall/shared-service signals"}


def wrapped(command, work, *, timeout=120):
    """Call the unchanged production run() with retained temporary-file backing.

    Only the temporary-file factory is observed. The actual Popen arguments,
    new-session behavior, timeout, output bound and process-group cleanup remain
    the production helper's implementation. These are diagnostic observations,
    not a new packaging implementation.
    """
    spec = importlib.util.spec_from_file_location("diagnostic_package_macos", ROOT / "scripts/package-macos.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    names = iter(("stdout.raw", "stderr.raw"))

    def retained_file(*args, **kwargs):
        return (work / next(names)).open("x+b")

    started = time.monotonic()
    result = {"start_new_session": True, "production_wrapper_sha256": digest(ROOT / "scripts/package-macos.py"),
              "observation": "TemporaryFile backing retained; execution and cleanup are unchanged"}
    with patch.object(module.tempfile, "TemporaryFile", retained_file):
        try:
            module.run(command, cwd=ROOT, timeout=timeout)
            result.update(returncode=0, error=None)
        except Exception as error:
            # Production run() exposes failures as PackageError rather than
            # returning the child code. Preserve that distinction accurately.
            result.update(returncode=None, error_type=type(error).__name__, error=str(error))
    result["elapsed_seconds"] = time.monotonic() - started
    return result


def metadata(command, evidence, name):
    folder = evidence.parent / ("metadata-" + name)
    folder.mkdir()
    result = direct(command, folder, timeout=20)
    result["argv"] = command
    result["stdout"] = capture_log(folder / "stdout.raw", evidence, name + "-stdout")
    result["stderr"] = capture_log(folder / "stderr.raw", evidence, name + "-stderr")
    return result


def crash_snapshot():
    found = {}
    directory = Path.home() / "Library/Logs/DiagnosticReports"
    if directory.is_dir():
        for path in itertools.islice(directory.iterdir(), 2000):
            if path.name.lower().startswith(("actool", "ibtoold", "assetcatalog")):
                info = path.lstat()
                if stat.S_ISREG(info.st_mode):
                    found[str(path)] = (info.st_mtime_ns, info.st_size)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xcode", choices=("26.2", "26.3"), required=True)
    parser.add_argument("--method", choices=("direct", "wrapper"), required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    args.directory.mkdir(mode=0o700)
    evidence = args.directory / "evidence"
    evidence.mkdir()
    work = args.directory / "work"
    work.mkdir()
    result = {"format": 1, "xcode": args.xcode, "method": args.method, "started_at_unix": time.time(),
              "scope": "Manual diagnostic only. No Rust build, package, signing, notarization or native verification.",
              "context": {name: os.environ.get(name) for name in
                          ("GITHUB_SHA", "GITHUB_REF", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "RUNNER_OS", "RUNNER_ARCH", "ImageVersion")}}
    code = 2
    try:
        if platform.system() != "Darwin" or platform.machine() != "arm64":
            raise RuntimeError("Diagnostic requires the specified Apple Silicon macOS runner")
        developer = Path(f"/Applications/Xcode_{args.xcode}.app/Contents/Developer")
        if not developer.is_dir():
            raise RuntimeError("Requested installed Xcode is unavailable; no fallback")
        os.environ["DEVELOPER_DIR"] = str(developer)
        result["developer_directory"] = str(developer)
        icon = ROOT / "assets/AppIcon.icon"
        observed = {}
        for count, path in enumerate(icon.rglob("*")):
            if count >= 20:
                raise RuntimeError("Unexpected icon input entry count")
            mode = path.lstat().st_mode
            if stat.S_ISREG(mode):
                observed[str(path.relative_to(icon))] = digest(path)
            elif not stat.S_ISDIR(mode):
                raise RuntimeError("Unexpected icon input entry")
        result["icon_sha256"] = observed
        if observed != EXPECTED_ICON:
            raise RuntimeError("Icon bytes differ from the failed hosted candidate; stop for review")
        result["versions"] = {}
        for name, argv in (
            ("source", ["git", "rev-parse", "HEAD"]),
            ("os", ["sw_vers"]),
            ("xcode", ["xcodebuild", "-version"]),
            ("sdk-version", ["xcrun", "--sdk", "macosx", "--show-sdk-version"]),
            ("sdk-path", ["xcrun", "--sdk", "macosx", "--show-sdk-path"]),
            ("actool-path", ["xcrun", "--find", "actool"]),
            ("metal-version", ["xcrun", "metal", "--version"]),
            ("actool-version", ["xcrun", "actool", "--version"]),
            ("displays", ["system_profiler", "SPDisplaysDataType", "-json"]),
        ):
            result["versions"][name] = metadata(argv, evidence, name)
            if name != "displays" and result["versions"][name]["returncode"] != 0:
                raise RuntimeError(f"Required diagnostic prerequisite failed: {name}")
        if (evidence / "xcode-stdout.full.log").read_text().splitlines()[0] != f"Xcode {args.xcode}":
            raise RuntimeError("Selected Xcode reports a different version; no fallback")
        output = work / "icon"
        output.mkdir()
        command = ["xcrun", "actool", str(icon), "--compile", str(output), "--platform", "macosx",
                   "--minimum-deployment-target", "11.0", "--app-icon", "AppIcon",
                   "--output-partial-info-plist", str(output / "icon-info.plist"),
                   "--output-format", "human-readable-text", "--warnings", "--notices"]
        result["argv"] = command
        before = crash_snapshot()
        result["command"] = (direct if args.method == "direct" else wrapped)(command, work)
        result["stdout"] = capture_log(work / "stdout.raw", evidence, "actool-stdout")
        result["stderr"] = capture_log(work / "stderr.raw", evidence, "actool-stderr")
        # Give CrashReporter a bounded opportunity to finish writing its report.
        time.sleep(3)
        crashes = []
        for path, identity in crash_snapshot().items():
            if before.get(path) == identity or len(crashes) >= 8:
                continue
            record = {"name": Path(path).name, "original_bytes": identity[1]}
            if identity[1] <= 1024 * 1024:
                record["capture"] = capture_log(Path(path), evidence, f"crash-{len(crashes)}")
            else:
                record["omitted"] = "Crash report exceeds the 1MiB per-file bound"
            crashes.append(record)
        result["new_crash_reports"] = crashes
        result["compiled_output"] = []
        for count, path in enumerate(output.rglob("*")):
            if count >= 100:
                raise RuntimeError("Compiled icon output exceeds the entry bound")
            if path.is_symlink():
                raise RuntimeError("Unexpected compiled icon link")
            if path.is_file():
                result["compiled_output"].append({"name": str(path.relative_to(output)),
                                                   "bytes": path.stat().st_size, "sha256": digest(path)})
        result["output_upload"] = "Compiled assets deliberately excluded; only evidence is uploaded"
        code = 0 if result["command"].get("returncode") == 0 else 1
    except Exception as error:
        result["diagnostic_error"] = {"type": type(error).__name__, "message": str(error)}
    finally:
        result["exit_code"] = code
        result["finished_at_unix"] = time.time()
        write_json(evidence / "result.json", result)
        print(json.dumps({"xcode": args.xcode, "method": args.method, "exit_code": code,
                          "evidence": str(evidence)}, indent=2))
    return code


if __name__ == "__main__":
    sys.exit(main())
