#!/usr/bin/env python3
"""Read-only release identity preflight (Python 3.11+).

Call verify_release_identity on an exclusively owned, clean checkout of the
chosen commit. Keep its tag_object and pass it as expected_tag_object on later
checks: an annotated tag can be rewritten without changing its peeled commit.
This is a local snapshot, not a lock or publication authorization. The caller
must separately verify the remote tag, required checks, trusted package bytes,
licenses and compiled --build-info before publication.

Platform identifiers are Rust target triples, not runner names. Only the
currently supported distribution targets are accepted. No version, tag or
platform is chosen by this module; tags must be the version or v + version.
"""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import hashlib
import os
from pathlib import Path, PurePosixPath
import re
import selectors
import signal
import stat
import subprocess
import sys
import time
import tomllib
import json
from collections.abc import Sequence


SUPPORTED_PLATFORMS = ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu")
MEMBERS = {
    "crates/app": "gitturtle",
    "crates/git-core": "gitturtle-core",
    "crates/preview": "gitturtle-preview",
}
MAX_OUTPUT_BYTES = 4 * 1024 * 1024
GIT_TIMEOUT_SECONDS = 15
MAX_SOURCE_BYTES = 2 * 1024 * 1024 * 1024
SOURCE_TIMEOUT_SECONDS = 60
_NUMBER = r"(?:0|[1-9][0-9]*)"
_PRERELEASE = rf"(?:{_NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
_VERSION = re.compile(
    rf"{_NUMBER}\.{_NUMBER}\.{_NUMBER}"
    rf"(?:-{_PRERELEASE}(?:\.{_PRERELEASE})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


class IdentityError(ValueError):
    """An identity could not be established; nothing was modified."""


@dataclass(frozen=True)
class ReleaseIdentity:
    tag: str
    tag_object: str
    commit: str
    version: str
    platforms: tuple[str, ...]
    cargo_lock_sha256: str


def _oid(value: str, label: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", value):
        raise IdentityError(f"{label} must be a full lowercase Git object ID")
    return value


def _platforms(values: Sequence[str], label: str) -> tuple[str, ...]:
    if isinstance(values, (str, bytes)) or not isinstance(values, Sequence):
        raise IdentityError(f"{label} must be an explicit platform list")
    if not values or len(values) > len(SUPPORTED_PLATFORMS):
        raise IdentityError(f"{label} must contain one or two supported platforms")
    if any(not isinstance(value, str) or value not in SUPPORTED_PLATFORMS for value in values):
        raise IdentityError(f"{label} contains an unsupported platform")
    if len(set(values)) != len(values):
        raise IdentityError(f"{label} contains a duplicate platform")
    return tuple(sorted(values))


def require_complete_platforms(expected: Sequence[str], available: Sequence[str]) -> None:
    """Compare independently verified package targets, never successful jobs alone."""
    promised = _platforms(expected, "promised platforms")
    actual = _platforms(available, "available platforms")
    missing = sorted(set(promised) - set(actual))
    extra = sorted(set(actual) - set(promised))
    if missing or extra:
        raise IdentityError(f"platform set mismatch: missing={missing}, unexpected={extra}")


class _Git:
    def __init__(self, repo: Path):
        self.repo = repo
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
            GIT_TERMINAL_PROMPT="0", GIT_OPTIONAL_LOCKS="0", GIT_NO_LAZY_FETCH="1",
            GIT_NO_REPLACE_OBJECTS="1", LC_ALL="C",
        )

    def read(self, args: Sequence[str], *, config: Sequence[str] = (), allow_missing: bool = False) -> bytes:
        argv = [
            "git", "--no-pager", "--no-optional-locks", "--literal-pathspecs",
            "-c", "core.fsmonitor=false", "-c", "core.hooksPath=" + os.devnull,
            "-c", "core.untrackedCache=false", "-c", "core.attributesFile=" + os.devnull,
            "-c", "core.trustctime=true", "-c", "core.checkStat=default",
            "-c", "core.filemode=true", "-c", "core.ignorestat=false",
            "-c", "protocol.allow=never",
        ]
        for setting in config:
            argv.extend(("-c", setting))
        argv.extend(args)
        # Bound both pipes while they are produced, not only after communicate().
        output = bytearray()
        count = 0
        process = None
        completed = False
        deadline = time.monotonic() + GIT_TIMEOUT_SECONDS
        try:
            process = subprocess.Popen(
                argv, cwd=self.repo, env=self.env, stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
            )
            with selectors.DefaultSelector() as selector:
                for pipe in (process.stdout, process.stderr):
                    os.set_blocking(pipe.fileno(), False)
                    selector.register(pipe, selectors.EVENT_READ)
                while selector.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise IdentityError("local Git read exceeded its time limit")
                    for key, _ in selector.select(remaining):
                        chunk = os.read(key.fd, 65536)
                        if not chunk:
                            selector.unregister(key.fileobj)
                            continue
                        count += len(chunk)
                        if count > MAX_OUTPUT_BYTES:
                            raise IdentityError("local Git read exceeded its output limit")
                        if key.fileobj is process.stdout:
                            output.extend(chunk)
            process.wait(timeout=max(0.001, deadline - time.monotonic()))
            completed = True
            if process.returncode and not (allow_missing and process.returncode == 1):
                # Git diagnostics can contain arbitrary config, paths or tag text.
                raise IdentityError(f"local Git {args[0]} failed; check repository and required objects")
            return bytes(output)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise IdentityError("local Git read failed or exceeded its time limit") from error
        finally:
            if process is not None:
                # Also close descendants holding pipes after the direct child exits.
                if not completed:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait()
                for pipe in (process.stdout, process.stderr):
                    if pipe is not None:
                        pipe.close()

    def resolve(self, expression: str) -> str:
        raw = self.read(("rev-parse", "--verify", "--end-of-options", expression))
        try:
            return _oid(raw.decode("ascii").strip(), "resolved object")
        except UnicodeDecodeError as error:
            raise IdentityError("Git returned an invalid object ID") from error

    def clean(self) -> None:
        # Status deliberately trusts these index flags; a release preflight must
        # not mistake hidden local edits or an incomplete sparse checkout for
        # the complete, clean source used to build an artifact.
        entries = self.read(("ls-files", "-v", "-z")).split(b"\0")
        if any(entry and not entry.startswith(b"H ") for entry in entries):
            raise IdentityError("release checkout has unsupported sparse/assumed index entries")
        # Even status can execute clean/process filters. Match core's passive
        # policy: inspect names only, then override every configured driver.
        raw = self.read(("config", "--includes", "--null", "--name-only", "--get-regexp",
                         r"^filter\..*\.(clean|process|required)$"), allow_missing=True)
        config = []
        for value in raw.split(b"\0"):
            if not value:
                continue
            try:
                key = value.decode("utf-8")
            except UnicodeDecodeError as error:
                raise IdentityError("unsupported Git filter configuration name") from error
            if any(character in key for character in "\n\r="):
                raise IdentityError("unsupported Git filter configuration name")
            config.append(key + ("=false" if key.endswith(".required") else "="))
        if self.read(("status", "--porcelain=v1", "-z", "--untracked-files=normal",
                      "--ignore-submodules=none"), config=config):
            raise IdentityError("release checkout is dirty; commit intended source before preflight")

    def document(self, commit: str, path: str) -> tuple[dict, bytes]:
        entry = self.read(("ls-tree", "-z", commit, "--", path))
        if not entry.startswith(b"100644 blob ") or not entry.endswith(b"\t" + path.encode() + b"\0"):
            raise IdentityError(f"{path} must be a committed regular file")
        data = self.read(("cat-file", "blob", f"{commit}:{path}"))
        try:
            return tomllib.loads(data.decode("utf-8")), data
        except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
            raise IdentityError(f"{path} is not valid UTF-8 TOML") from error


def _table(value: object, label: str) -> dict:
    if not isinstance(value, dict):
        raise IdentityError(f"{label} must be a TOML table")
    return value


def _verify_tracked_bytes(git: _Git, commit: str) -> None:
    """Do not trust a stat cache previously refreshed under permissive config.

    Release source must match raw committed bytes, including symlink targets
    and executable modes. Checkout transformations need a reviewed alternative;
    this deliberately never invokes configured filters to normalize them.
    """
    records = git.read(("ls-tree", "-r", "-z", commit)).split(b"\0")
    deadline = time.monotonic() + SOURCE_TIMEOUT_SECONDS
    remaining = MAX_SOURCE_BYTES
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    try:
        root = os.open(git.repo, directory_flags)
        try:
            for record in filter(None, records):
                metadata, path = record.split(b"\t", 1)
                mode, kind, expected = metadata.split(b" ")
                if kind != b"blob" or mode not in (b"100644", b"100755", b"120000"):
                    raise IdentityError("unsupported tracked source type (including submodules)")
                pieces = path.split(b"/")
                if any(piece in (b"", b".", b"..") for piece in pieces):
                    raise IdentityError("unsafe tracked source path")
                parent = os.dup(root)
                try:
                    for piece in pieces[:-1]:
                        child = os.open(piece, directory_flags, dir_fd=parent)
                        os.close(parent)
                        parent = child
                    digest = hashlib.new("sha1" if len(commit) == 40 else "sha256")
                    if mode == b"120000":
                        data = os.readlink(pieces[-1], dir_fd=parent)
                        digest.update(f"blob {len(data)}\0".encode())
                        digest.update(data)
                        remaining -= len(data)
                    else:
                        descriptor = os.open(pieces[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
                        with os.fdopen(descriptor, "rb") as stream:
                            info = os.fstat(stream.fileno())
                            if not stat.S_ISREG(info.st_mode) or bool(info.st_mode & 0o111) != (mode == b"100755"):
                                raise IdentityError("tracked source type or executable mode differs from the commit")
                            if info.st_size > remaining:
                                raise IdentityError("tracked source exceeds the release verification byte limit")
                            digest.update(f"blob {info.st_size}\0".encode())
                            while chunk := stream.read(65536):
                                remaining -= len(chunk)
                                if remaining < 0 or time.monotonic() >= deadline:
                                    raise IdentityError("tracked source exceeds release verification limits")
                                digest.update(chunk)
                    if remaining < 0 or time.monotonic() >= deadline:
                        raise IdentityError("tracked source exceeds release verification limits")
                    if digest.hexdigest().encode() != expected:
                        raise IdentityError("tracked source bytes differ from the selected commit")
                finally:
                    os.close(parent)
        finally:
            os.close(root)
    except (OSError, ValueError) as error:
        if isinstance(error, IdentityError):
            raise
        raise IdentityError("cannot verify complete raw tracked source without following symlinks") from error


def _check_local_dependencies(document: dict, base: str, excludes: list[str]) -> None:
    # Cargo may implicitly add path dependencies to a workspace. Refuse an
    # unreviewed member instead of silently checking only workspace.members.
    def visit(table: dict, depth: int = 0) -> None:
        if depth > 16:
            raise IdentityError("unsupported deeply nested dependency configuration")
        for key, value in table.items():
            if key == "path" and isinstance(value, str):
                path = PurePosixPath(os.path.normpath(str(PurePosixPath(base) / value)))
                if str(path) not in MEMBERS and not any(path == PurePosixPath(item) for item in excludes):
                    raise IdentityError("unsupported local dependency/workspace layout; review release version resolution")
            elif isinstance(value, dict):
                visit(value, depth + 1)
    for name in ("dependencies", "dev-dependencies", "build-dependencies", "target", "patch", "replace"):
        section = document.get(name, {})
        if not isinstance(section, dict):
            raise IdentityError(f"{name} must be a TOML table")
        visit(section)


def _versions(git: _Git, commit: str, expected: str) -> str:
    root, _ = git.document(commit, "Cargo.toml")
    workspace = _table(root.get("workspace"), "workspace")
    members = workspace.get("members")
    if "package" in root or not isinstance(members, list) or sorted(members, key=str) != sorted(MEMBERS):
        raise IdentityError("unsupported workspace membership; expected GitTurtle's three explicit members")
    package = _table(workspace.get("package"), "workspace.package")
    if package.get("version") != expected:
        raise IdentityError("workspace version does not match the explicit release version")
    excludes = workspace.get("exclude", [])
    if not isinstance(excludes, list) or any(not isinstance(item, str) for item in excludes):
        raise IdentityError("unsupported workspace exclusions")
    if workspace.get("dependencies"):
        raise IdentityError("workspace dependency inheritance requires reviewed release version resolution")
    _check_local_dependencies(root, ".", excludes)
    for member, name in MEMBERS.items():
        document, _ = git.document(commit, f"{member}/Cargo.toml")
        package = _table(document.get("package"), f"{member} package")
        if "workspace" in document or "workspace" in package or package.get("name") != name:
            raise IdentityError(f"unsupported package/workspace identity in {member}")
        version = package.get("version")
        if version == {"workspace": True} and type(version.get("workspace")) is bool:
            version = expected
        if version != expected:
            raise IdentityError(f"{name} version does not match the workspace/release version")
        _check_local_dependencies(document, member, excludes)
    lock, raw = git.document(commit, "Cargo.lock")
    if type(lock.get("version")) is not int or lock["version"] not in (3, 4):
        raise IdentityError("unsupported Cargo.lock format")
    packages = lock.get("package")
    if not isinstance(packages, list) or any(not isinstance(item, dict) for item in packages):
        raise IdentityError("Cargo.lock package inventory is malformed")
    for name in MEMBERS.values():
        entries = [item for item in packages if item.get("name") == name and "source" not in item]
        if len(entries) != 1 or entries[0].get("version") != expected:
            raise IdentityError(f"Cargo.lock must contain exactly one matching local {name} version")
    return hashlib.sha256(raw).hexdigest()


def verify_release_identity(
    repo: Path | str, *, tag: str, version: str, commit: str,
    platforms: Sequence[str], expected_tag_object: str | None = None,
) -> ReleaseIdentity:
    """Verify current local source, failing closed on unsupported version semantics.

    This intentionally understands the current virtual workspace's three
    explicit members and literal or workspace-inherited package versions.
    Changed membership/inheritance must be reviewed before extending it.
    Ignored build output does not make source dirty (matching --build-info).
    """
    commit = _oid(commit, "expected commit")
    if not isinstance(version, str) or len(version) > 128 or not _VERSION.fullmatch(version):
        raise IdentityError("release version must be an exact three-component SemVer")
    if not isinstance(tag, str) or tag not in (version, "v" + version):
        raise IdentityError("release tag must equal the version or v followed by the version")
    promised = _platforms(platforms, "promised platforms")
    if "aarch64-apple-darwin" in promised and ("-" in version or "+" in version):
        raise IdentityError("macOS requires a reviewed plist mapping for a suffixed Cargo version")
    if expected_tag_object is not None:
        _oid(expected_tag_object, "expected tag object")
    try:
        directory = Path(repo).resolve(strict=True)
    except (OSError, ValueError, TypeError) as error:
        raise IdentityError("release checkout is unavailable") from error
    git = _Git(directory)
    root = git.read(("rev-parse", "--show-toplevel")).rstrip(b"\n")
    if os.fsdecode(root) != str(directory):
        raise IdentityError("release checkout must be the repository root")
    reference = "refs/tags/" + tag
    tag_object = git.resolve(reference)
    if expected_tag_object is not None and tag_object != expected_tag_object:
        raise IdentityError("release tag object changed since the captured identity")
    if git.resolve(reference + "^{commit}") != commit:
        raise IdentityError("release tag does not peel to the expected commit")
    if git.resolve("HEAD^{commit}") != commit:
        raise IdentityError("release checkout HEAD does not match the expected commit")
    git.clean()
    lock_digest = _versions(git, commit, version)
    _verify_tracked_bytes(git, commit)
    git.clean()
    if git.resolve("HEAD^{commit}") != commit or git.resolve(reference) != tag_object:
        raise IdentityError("release source or tag changed during preflight")
    if git.resolve(reference + "^{commit}") != commit:
        raise IdentityError("release tag changed during preflight")
    return ReleaseIdentity(tag, tag_object, commit, version, promised, lock_digest)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--platform", action="append", required=True, choices=SUPPORTED_PLATFORMS)
    parser.add_argument("--expected-tag-object")
    parser.add_argument("--available-platform", action="append", choices=SUPPORTED_PLATFORMS,
                        help="Independently verified package target; repeat for the complete actual set")
    args = parser.parse_args(argv)
    try:
        identity = verify_release_identity(
            args.repo, tag=args.tag, version=args.version, commit=args.commit,
            platforms=args.platform, expected_tag_object=args.expected_tag_object,
        )
        if args.available_platform is not None:
            require_complete_platforms(identity.platforms, args.available_platform)
    except IdentityError as error:
        print(f"release identity refused: {error}", file=sys.stderr)
        return 2
    print(json.dumps(asdict(identity), sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
