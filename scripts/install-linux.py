#!/usr/bin/env python3
"""Install an extracted GitTurtle Linux bundle for the current user."""

import hashlib
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import stat
import sys
import tempfile
import time


APP_ID = "com.gitturtle.desktop"
ICON_SIZES = (16, 24, 32, 48, 64, 128, 256, 512)


def fail(message):
    raise RuntimeError(message)


def owned_directory(data, relative):
    """The selected data root may be a symlink; managed descendants may not."""
    path = data
    for part in Path(relative).parts:
        path = path / part
        if path.is_symlink() or (path.exists() and not path.is_dir()):
            fail(f"Refusing an invalid installation directory: {path}.")
    return path


def package_tools(bundle):
    spec = importlib.util.spec_from_file_location("gitturtle_package_identity", bundle / "package_identity.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def data_path():
    path = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    if not path.is_absolute():
        fail("XDG_DATA_HOME must be an absolute path (or unset).")
    return path


def desktop_string(value):
    return str(value).replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")


def desktop_command(path):
    value = str(path)
    # Desktop Exec is not a shell command. Its grammar forbids '=' in the
    # executable path; field-code expansion inside quotes is unspecified.
    if any(character in value for character in "\n\r\t=%"):
        fail("The installation path cannot contain a newline, tab, '=' or '%' (desktop Exec restrictions).")
    quoted = "".join("\\" + char if char in '\\"`$' else char for char in value)
    return desktop_string('"' + quoted + '"')


def replace_file(source, destination, mode):
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".gitturtle-", dir=destination.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            with source.open("rb") as input_file:
                shutil.copyfileobj(input_file, output)
            output.flush()
            os.fchmod(output.fileno(), mode)
            os.fsync(output.fileno())
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def digest_file(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def write_json(value, destination):
    with tempfile.TemporaryDirectory(prefix="gitturtle-metadata-") as temporary:
        source = Path(temporary) / "metadata.json"
        source.write_text(json.dumps(value, indent=2) + "\n")
        replace_file(source, destination, 0o600)


def refuse_running(binary, proc=Path("/proc")):
    """Match executable identity/path, never process-name text or kill a process."""
    if not binary.exists():
        return
    identity = binary.stat()
    try:
        processes = list(proc.iterdir())
    except OSError as error:
        fail(f"Cannot check running GitTurtle processes: {error}. Installation was not changed.")
    running = []
    for process in processes:
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid != os.getuid():
                continue
            executable = process / "exe"
            target = os.readlink(executable)
            actual = executable.stat()
        except (FileNotFoundError, ProcessLookupError):
            continue
        except PermissionError:
            # A process can disable /proc access (for example a sandboxed
            # browser). It cannot be classified as this executable by name.
            continue
        if ((actual.st_dev, actual.st_ino) == (identity.st_dev, identity.st_ino)
                or target.removesuffix(" (deleted)") == str(binary)):
            running.append(process.name)
    if running:
        fail("GitTurtle is still running (PID " + ", ".join(running) + "). "
             "Let any Git operation finish, then quit GitTurtle and rerun the installer. "
             "No process was stopped and the installation was not changed.")


def owned_target(base, name, home, data):
    relative = Path(name)
    if relative.is_absolute() or ".." in relative.parts:
        fail("Invalid path in installation backup.")
    allowed = (
        base == "home" and name == ".local/bin/gitturtle"
        or base == "data" and (
            name in {f"icons/{APP_ID}.png", f"applications/{APP_ID}.desktop", "gitturtle/build-info.json"}
            or name.startswith("gitturtle/licenses/")
            or name in {f"icons/hicolor/{size}x{size}/apps/{APP_ID}.png" for size in ICON_SIZES}
        )
    )
    if not allowed:
        fail("Backup contains a path outside GitTurtle's installed files.")
    # HOME and XDG_DATA_HOME select the installation roots and may themselves
    # be symlinks. A backup entry must not redirect any component below those
    # roots: checking only the final file lets a replaced license directory
    # send rollback writes into unrelated files.
    destination = home if base == "home" else data
    for part in relative.parts:
        destination = destination / part
        if destination.is_symlink():
            fail(f"Refusing to replace a path through a symlink: {destination}.")
    if destination.exists() and not destination.is_file():
        fail(f"Expected an installed file: {destination}.")
    return destination


def installed_targets(home, data):
    targets = {("home", ".local/bin/gitturtle"), ("data", "gitturtle/build-info.json"),
               ("data", f"icons/{APP_ID}.png"), ("data", f"applications/{APP_ID}.desktop")}
    targets.update(("data", f"icons/hicolor/{size}x{size}/apps/{APP_ID}.png") for size in ICON_SIZES)
    licenses = data / "gitturtle/licenses"
    if licenses.exists():
        targets.update(("data", str(path.relative_to(data))) for path in licenses.rglob("*")
                       if path.is_file() or path.is_symlink())
    return targets


def save_backup(targets, home, data):
    root = owned_directory(data, "gitturtle/install-backups")
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    directory = Path(tempfile.mkdtemp(prefix=f"{time.time_ns()}-", dir=root))
    entries = []
    try:
        for index, (base, name) in enumerate(sorted(targets)):
            source = owned_target(base, name, home, data)
            entry = {"base": base, "path": name, "stored": None}
            if source.exists():
                stored = directory / f"files/{index}"
                mode = source.stat().st_mode & 0o777
                replace_file(source, stored, mode)
                entry.update(stored=f"files/{index}", mode=mode, sha256=digest_file(stored))
            entries.append(entry)
        write_json({"format": 1, "files": entries}, directory / "manifest.json")
        return directory
    except BaseException:
        shutil.rmtree(directory)
        raise


def backup_entries(directory, home, data):
    if directory.is_symlink() or not directory.is_dir():
        fail("Invalid installation backup directory.")
    manifest = directory / "manifest.json"
    if manifest.is_symlink() or manifest.stat().st_size > 8 * 1024 * 1024:
        fail("Invalid installation backup manifest.")
    document = json.loads(manifest.read_text())
    if document.get("format") != 1 or not isinstance(document.get("files"), list):
        fail("Unsupported installation backup format.")
    entries, seen = [], set()
    for entry in document["files"]:
        destination = owned_target(entry["base"], entry["path"], home, data)
        if destination in seen:
            fail("Duplicate installation backup entry.")
        seen.add(destination)
        source = None
        if entry["stored"] is not None:
            source = directory / entry["stored"]
            if (source.is_symlink() or not source.resolve().is_relative_to(directory.resolve())
                    or digest_file(source) != entry["sha256"]):
                fail("Installation backup checksum mismatch; nothing was restored.")
            if entry["mode"] not in range(0o1000):
                fail("Invalid file mode in installation backup.")
        entries.append((source, destination, entry.get("mode", 0o644)))
    return entries


def restore_backup(directory, home, data):
    entries = backup_entries(directory, home, data)  # Check everything first.
    for source, destination, mode in entries:
        if source is None:
            destination.unlink(missing_ok=True)
        else:
            replace_file(source, destination, mode)


def refresh_desktop(data):
    commands = ["update-desktop-database", "gtk-update-icon-cache"]
    arguments = [[str(data / "applications")],
                 ["--force", "--ignore-theme-index", str(data / "icons/hicolor")]]
    for command, args in zip(commands, arguments):
        if shutil.which(command):
            result = subprocess.run([command, *args], capture_output=True, text=True)
            if result.returncode:
                print(f"Installed files are ready; {command} could not refresh the desktop cache. "
                      "Log out and back in if the application menu is stale.", file=sys.stderr)


def rollback(home, data):
    marker = data / "gitturtle/previous-installation.json"
    if not marker.is_file():
        fail("There is no previous installation recorded by this installer.")
    if marker.is_symlink() or marker.stat().st_size > 4096:
        fail("Invalid previous-installation record.")
    name = json.loads(marker.read_text())["backup"]
    if not isinstance(name, str) or Path(name).name != name or name in {"", ".", ".."}:
        fail("Invalid previous-installation backup name.")
    backup = owned_directory(data, "gitturtle/install-backups") / name
    entries = backup_entries(backup, home, data)
    refuse_running(home / ".local/bin/gitturtle")
    targets = installed_targets(home, data)
    targets.update(("home" if destination == home / ".local/bin/gitturtle" else "data",
                    ".local/bin/gitturtle" if destination == home / ".local/bin/gitturtle"
                    else str(destination.relative_to(data))) for _, destination, _ in entries)
    current = save_backup(targets, home, data)
    refuse_running(home / ".local/bin/gitturtle")
    # Publish the recovery point before changing files, including if this
    # process is forcibly interrupted between atomic replacements.
    write_json({"backup": current.name}, marker)
    try:
        restore_backup(backup, home, data)
    except BaseException:
        restore_backup(current, home, data)
        raise
    refresh_desktop(data)
    print("Restored the previous GitTurtle installation. Settings and drafts were preserved.\n"
          "Run the same --rollback command again to return to the installation just replaced.")


def install():
    if sys.argv[1:] not in ([], ["--rollback"]):
        fail("Usage: python3 install.py [--rollback] (current user only; run without sudo)")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        fail("This bundle targets Linux x86-64. Build from source for another platform.")
    home, data = Path.home(), data_path()
    if not home.is_absolute():
        fail("HOME must be an absolute path.")
    support = owned_directory(data, "gitturtle")
    support.mkdir(parents=True, exist_ok=True)
    lock_path = support / "install.lock"
    if lock_path.is_symlink() or (lock_path.exists() and not lock_path.is_file()):
        fail("Invalid installation lock file.")
    descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            fail("Another GitTurtle installation is in progress; let it finish first.")
        if sys.argv[1:] == ["--rollback"]:
            rollback(home, data)
        else:
            install_bundle(home, data)


def install_bundle(home, data):
    bundle = Path(__file__).resolve().parent
    manifest = bundle / "SHA256SUMS"
    if not manifest.is_file() or manifest.is_symlink():
        fail("Extract the complete bundle first; SHA256SUMS is missing.")
    if manifest.stat().st_size > 2 * 1024 * 1024:
        fail("Bundle checksum manifest exceeds its size limit.")
    required = {"bin/gitturtle", "install.py", "package_identity.py", "README.md", "build-info.json", "icons/app-icon.png"}
    required.update({"licenses/LICENSE", "licenses/THIRD_PARTY_NOTICES.md", "licenses/dependencies.json"})
    required.update(f"icons/hicolor/{size}x{size}/apps/{APP_ID}.png" for size in ICON_SIZES)
    checked = set()
    for line in manifest.read_text().splitlines():
        digest, name = line.split("  ", 1)
        if (not re.fullmatch(r"[0-9a-f]{64}", digest) or not name
                or any(part in ("", ".", "..") for part in name.split("/"))
                or Path(name).is_absolute() or "\\" in name or len(checked) >= 10000):
            fail("Invalid bundle checksum entry.")
        if name in checked:
            fail(f"Duplicate bundle checksum entry: {name}.")
        source = bundle
        for part in Path(name).parts:
            source = source / part
            if source.is_symlink():
                fail(f"Bundle contains a symlink: {name}.")
        if (not stat.S_ISREG(source.stat().st_mode) or source.stat().st_size > 1024**3
                or digest_file(source) != digest):
            fail(f"Bundle checksum mismatch: {name}. Extract a fresh copy.")
        checked.add(name)
    if required - checked:
        fail("Bundle checksum manifest is incomplete. Extract a fresh copy.")
    license_files = {str(path.relative_to(bundle)) for path in (bundle / "licenses").rglob("*") if path.is_file()}
    if license_files - checked:
        fail("Bundle contains unchecked license files. Extract a fresh copy.")
    tools = package_tools(bundle)
    info = tools.read_json(bundle / "build-info.json")
    tools.validate_manifest(info, bundle / "bin/gitturtle", bundle / "licenses")
    for command in ("git", "ldd", "desktop-file-validate", "update-desktop-database"):
        if not shutil.which(command):
            fail(f"Missing {command}; install the runtime packages listed in README.md.")
    binary = bundle / "bin/gitturtle"
    # The bundle contains our locally built executable. ldd is used only after
    # its recorded checksum has been checked, never on repository preview bytes.
    dependencies = subprocess.run(["ldd", str(binary)], text=True, capture_output=True)
    if dependencies.returncode or "not found" in dependencies.stdout:
        fail("Runtime library check failed; install the README.md runtime packages.\n"
             + dependencies.stdout + dependencies.stderr)
    if tools.probe(binary) != info["compiled_identity"]:
        fail("Executable build identity differs from its package manifest.")

    binary_destination = home / ".local/bin/gitturtle"
    refuse_running(binary_destination)
    icon = data / f"icons/{APP_ID}.png"
    launcher = data / f"applications/{APP_ID}.desktop"
    entry = (
        "[Desktop Entry]\nType=Application\nName=GitTurtle\n"
        "Comment=Browse Git history and manage local repositories\n"
        f"Exec={desktop_command(binary_destination)} %f\n"
        f"Icon={desktop_string(icon)}\nTerminal=false\n"
        "Categories=Development;RevisionControl;\nKeywords=Git;History;Diff;\n"
        f"StartupWMClass={APP_ID}\n"
    )
    with tempfile.TemporaryDirectory(prefix="gitturtle-install-") as temporary:
        entry_source = Path(temporary) / launcher.name
        entry_source.write_text(entry)
        subprocess.run(["desktop-file-validate", str(entry_source)], check=True)
        payload = [(bundle / name, data / "gitturtle" / name, 0o644) for name in sorted(license_files)]
        payload.extend([(binary, binary_destination, 0o755),
                        (bundle / "build-info.json", data / "gitturtle/build-info.json", 0o644),
                        (bundle / "icons/app-icon.png", icon, 0o644), (entry_source, launcher, 0o644)])
        # Install only the owned icons required by the checksum manifest. A
        # wildcard here also copied unchecked files over unrelated app icons
        # without including those destinations in the recovery manifest.
        payload.extend((bundle / name, data / name, 0o644)
                       for size in ICON_SIZES
                       for name in [f"icons/hicolor/{size}x{size}/apps/{APP_ID}.png"])
        targets = installed_targets(home, data)
        targets.update(("data", "gitturtle/" + name) for name in license_files)
        had_installation = binary_destination.exists()
        backup = save_backup(targets, home, data)
        refuse_running(binary_destination)
        if had_installation:
            write_json({"backup": backup.name}, data / "gitturtle/previous-installation.json")
        try:
            # Preserve a local recovery command after the extracted bundle is removed.
            replace_file(bundle / "install.py", data / "gitturtle/install.py", 0o755)
            for source, destination, mode in payload:
                replace_file(source, destination, mode)
        except BaseException:
            restore_backup(backup, home, data)
            raise
        if had_installation:
            print(f"Previous installation: {backup}\n"
                  f"To restore it after quitting GitTurtle: python3 \"{data / 'gitturtle/install.py'}\" --rollback")
        else:
            shutil.rmtree(backup)
    refresh_desktop(data)
    print(f"Installed {binary_destination}\nLauncher: {launcher}\n"
          "Open GitTurtle from Applications, or run the installed executable with a repository path.\n"
          "The extracted bundle and source checkout are no longer needed to run it.")


if __name__ == "__main__":
    try:
        install()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"GitTurtle installation failed: {error}", file=sys.stderr)
        sys.exit(1)
