#!/usr/bin/env python3
"""Install an extracted GitTurtle Linux bundle for the current user."""

import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile


APP_ID = "com.gitturtle.desktop"
ICON_SIZES = (16, 24, 32, 48, 64, 128, 256, 512)


def fail(message):
    raise RuntimeError(message)


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
            output.write(source.read_bytes())
            output.flush()
            os.fchmod(output.fileno(), mode)
            os.fsync(output.fileno())
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def install():
    if len(sys.argv) != 1:
        fail("Usage: python3 install.py (installs for the current user; run without sudo)")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        fail("This bundle targets Linux x86-64. Build from source for another platform.")
    bundle = Path(__file__).resolve().parent
    manifest = bundle / "SHA256SUMS"
    if not manifest.is_file():
        fail("Extract the complete bundle first; SHA256SUMS is missing.")
    required = {"bin/gitturtle", "install.py", "README.md", "build-info.json", "icons/app-icon.png"}
    required.update(f"icons/hicolor/{size}x{size}/apps/{APP_ID}.png" for size in ICON_SIZES)
    checked = set()
    for line in manifest.read_text().splitlines():
        digest, name = line.split("  ", 1)
        if name in checked:
            fail(f"Duplicate bundle checksum entry: {name}.")
        source = (bundle / name).resolve()
        if not source.is_relative_to(bundle) or hashlib.sha256(source.read_bytes()).hexdigest() != digest:
            fail(f"Bundle checksum mismatch: {name}. Extract a fresh copy.")
        checked.add(name)
    if required - checked:
        fail("Bundle checksum manifest is incomplete. Extract a fresh copy.")
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

    home = Path.home()
    if not home.is_absolute():
        fail("HOME must be an absolute path.")
    binary_destination = home / ".local/bin/gitturtle"
    data = data_path()
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
        replace_file(binary, binary_destination, 0o755)
        replace_file(bundle / "icons/app-icon.png", icon, 0o644)
        for source in sorted((bundle / "icons/hicolor").glob("*/apps/*.png")):
            replace_file(source, data / "icons/hicolor" / source.relative_to(bundle / "icons/hicolor"), 0o644)
        replace_file(entry_source, launcher, 0o644)
    subprocess.run(["update-desktop-database", str(launcher.parent)], check=True)
    # Absolute Icon works even before a theme cache update; the standard-size
    # hicolor copies also serve window/app-ID lookup by desktop shells.
    if shutil.which("gtk-update-icon-cache"):
        subprocess.run(["gtk-update-icon-cache", "--force", "--ignore-theme-index", str(data / "icons/hicolor")], check=True)
    print(f"Installed {binary_destination}\nLauncher: {launcher}\n"
          "Open GitTurtle from Applications, or run the installed executable with a repository path.\n"
          "The extracted bundle and source checkout are no longer needed to run it.")


if __name__ == "__main__":
    try:
        install()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"GitTurtle installation failed: {error}", file=sys.stderr)
        sys.exit(1)
