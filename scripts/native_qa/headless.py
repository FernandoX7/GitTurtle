"""Opt-in headless Linux Xvfb + Lavapipe (software Vulkan) launch helpers.

Headless runs never claim native Wayland/XWayland or GPU parity: `Session` logs
them under `BACKEND_LABEL` ("Xvfb + Lavapipe (headless software)") so software
regression frames stay explicitly distinguished from native desktop evidence.
This module never sets `ZED_HEADLESS` (which is an upstream GPUI test-platform
switch unrelated to GitTurtle).
"""

from __future__ import annotations

import os
from pathlib import Path
import platform
import select
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

from . import runenv

BACKEND_LABEL = "Xvfb + Lavapipe (headless software)"
DEFAULT_SCREEN_WIDTH = 1600
DEFAULT_SCREEN_HEIGHT = 1000
XVFB_READY_TIMEOUT = 10.0

DEFAULT_ICD_DIRS = (
    Path("/usr/share/vulkan/icd.d"),
    Path("/etc/vulkan/icd.d"),
    Path("/usr/local/share/vulkan/icd.d"),
)


def _arch_icd_names(machine: str | None = None) -> tuple[str, ...]:
    arch = (machine or platform.machine()).lower()
    aliases = {
        "arm64": "aarch64",
        "amd64": "x86_64",
    }
    canonical = aliases.get(arch, arch)
    names = [f"lvp_icd.{canonical}.json"]
    if arch != canonical:
        names.append(f"lvp_icd.{arch}.json")
    names.append("lvp_icd.json")
    return tuple(names)


def find_lavapipe_icd(
    search_dirs: tuple[Path, ...] | list[Path] | None = None,
    *,
    machine: str | None = None,
) -> Path | None:
    """Locate Mesa's Lavapipe Vulkan ICD manifest for this host architecture."""
    dirs = DEFAULT_ICD_DIRS if search_dirs is None else tuple(search_dirs)
    preferred = _arch_icd_names(machine)
    for directory in dirs:
        if directory.is_symlink() or not directory.is_dir():
            continue
        for name in preferred:
            candidate = directory / name
            if not candidate.is_symlink() and candidate.is_file():
                return candidate.resolve()
        for candidate in sorted(directory.glob("lvp_icd*.json")):
            if not candidate.is_symlink() and candidate.is_file():
                return candidate.resolve()
    return None


def probe_headless(
    *,
    icd_dirs: tuple[Path, ...] | list[Path] | None = None,
    which=shutil.which,
    machine: str | None = None,
    sys_platform: str | None = None,
) -> dict:
    """Read-only probe checking whether Xvfb and Lavapipe ICD are available."""
    plat = sys.platform if sys_platform is None else sys_platform
    issues: list[str] = []
    if not plat.startswith("linux"):
        issues.append(f"headless Xvfb + Lavapipe QA is Linux-only (host is {plat})")
    xvfb = which("Xvfb")
    if not xvfb:
        issues.append("Xvfb is not on PATH (install xorg-server-xvfb / xvfb)")
    icd = find_lavapipe_icd(icd_dirs, machine=machine)
    if icd is None:
        issues.append("Mesa Lavapipe ICD manifest (lvp_icd*.json) not found in Vulkan icd.d directories")
    return {
        "backend": BACKEND_LABEL,
        "platform": plat,
        "xvfb": xvfb,
        "lavapipe_icd": str(icd) if icd is not None else None,
        "ready": not issues,
        "issues": issues,
    }


def headless_env(icd_path: Path, extra: dict[str, str] | None = None) -> dict[str, str]:
    """Build extra launch environment variables forcing Lavapipe software Vulkan."""
    checked = runenv.absolute(icd_path, "Lavapipe ICD")
    try:
        info = os.lstat(checked)
    except OSError:
        raise runenv.Refusal(f"Lavapipe ICD manifest {checked} does not exist") from None
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISREG(info.st_mode):
        raise runenv.Refusal(f"Lavapipe ICD manifest {checked} must be a regular non-symlink file")
    resolved = checked.resolve()
    merged: dict[str, str] = {}
    if extra:
        if "ZED_HEADLESS" in extra:
            raise runenv.Refusal("ZED_HEADLESS is not a GitTurtle launch variable")
        runenv.check_extra(extra)
        merged.update(extra)
    merged["VK_ICD_FILENAMES"] = str(resolved)
    merged["VK_DRIVER_FILES"] = str(resolved)
    merged["MESA_LOADER_DRIVER_OVERRIDE"] = "llvmpipe"
    merged["LIBGL_ALWAYS_SOFTWARE"] = "1"
    return merged


def parse_displayfd_output(raw: bytes) -> str:
    """Validate the numeric display printed by `Xvfb -displayfd`."""
    try:
        text = raw.decode("ascii", errors="strict").strip()
    except UnicodeDecodeError:
        raise runenv.Refusal(f"Xvfb -displayfd returned non-ASCII bytes {raw!r}") from None
    if not text.isdigit():
        raise runenv.Refusal(f"Xvfb -displayfd returned unexpected display token {raw!r}")
    number = int(text)
    if not (0 <= number <= 99999):
        raise runenv.Refusal(f"Xvfb display number out of range: {number}")
    return f":{number}"


class XvfbSession:
    """Own a private Xvfb server using `-displayfd` so it never touches the operator's display."""

    def __init__(
        self,
        *,
        width: int = DEFAULT_SCREEN_WIDTH,
        height: int = DEFAULT_SCREEN_HEIGHT,
        xvfb_bin: str | None = None,
        ready_timeout: float = XVFB_READY_TIMEOUT,
    ) -> None:
        if (
            type(width) is not int
            or type(height) is not int
            or width < 320
            or height < 240
            or width > 8192
            or height > 8192
        ):
            raise runenv.Refusal(f"invalid Xvfb screen geometry {width!r}x{height!r}")
        self.width = width
        self.height = height
        self.xvfb_bin = xvfb_bin or shutil.which("Xvfb")
        if not self.xvfb_bin:
            raise runenv.Refusal("Xvfb is not installed on PATH")
        self.ready_timeout = ready_timeout
        self.proc: subprocess.Popen | None = None
        self._stderr_file = None
        self.display: str | None = None

    def __enter__(self) -> str:
        read_fd: int | None = None
        write_fd: int | None = None
        stderr_file = tempfile.TemporaryFile()
        try:
            read_fd, write_fd = os.pipe()
            argv = [
                self.xvfb_bin,
                "-displayfd",
                str(write_fd),
                "-screen",
                "0",
                f"{self.width}x{self.height}x24",
                "-nolisten",
                "tcp",
                "-noreset",
            ]
            try:
                self.proc = subprocess.Popen(
                    argv,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=stderr_file,
                    pass_fds=(write_fd,),
                )
            except OSError as error:
                raise runenv.Refusal(f"failed to launch Xvfb: {error}") from None
            self._stderr_file = stderr_file
            stderr_file = None

            os.close(write_fd)
            write_fd = None

            data = bytearray()
            deadline = time.monotonic() + self.ready_timeout
            while b"\n" not in data:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise runenv.Refusal(f"Xvfb did not report a display within {self.ready_timeout:.1f}s")
                readable, _, _ = select.select([read_fd], [], [], min(0.25, remaining))
                if readable:
                    chunk = os.read(read_fd, 64)
                    if not chunk:
                        break
                    data.extend(chunk)
                elif self.proc.poll() is not None:
                    break
            if not data:
                snippet = ""
                if self._stderr_file is not None:
                    try:
                        self._stderr_file.seek(0)
                        snippet = self._stderr_file.read(512).decode("utf-8", errors="replace").strip()
                    except OSError:
                        snippet = ""
                raise runenv.Refusal(
                    f"Xvfb exited before reporting a display (code {self.proc.poll()}): {snippet}"
                )
            self.display = parse_displayfd_output(bytes(data))
            return self.display
        except BaseException:
            self.close()
            raise
        finally:
            if write_fd is not None:
                os.close(write_fd)
            if read_fd is not None:
                os.close(read_fd)
            if stderr_file is not None:
                stderr_file.close()

    def close(self) -> None:
        proc = self.proc
        stderr_file = self._stderr_file
        self.proc = None
        self._stderr_file = None
        try:
            if proc is not None and proc.poll() is None:
                try:
                    proc.send_signal(signal.SIGTERM)
                    proc.wait(timeout=5.0)
                except subprocess.TimeoutExpired:
                    try:
                        proc.kill()
                        proc.wait(timeout=2.0)
                    except (OSError, subprocess.TimeoutExpired):
                        pass
                except OSError:
                    pass
        finally:
            if stderr_file is not None:
                try:
                    stderr_file.close()
                except OSError:
                    pass

    def __exit__(self, *_exc) -> None:
        self.close()
