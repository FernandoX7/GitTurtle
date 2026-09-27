#!/usr/bin/env python3
"""Template-scan the raster images a pull request or main push adds or changes.

`plan` lists those images and records whether they can be scanned. The pixel
templates arrive only through the optional NATIVE_QA_PRIVACY_TEMPLATES
repository secret (written by `qa.py privacy pack`), which GitHub does not pass
to fork pull requests. Without it the plan is `unavailable` and a notice says
the images were not scanned; nothing reports a pass. `scan` writes the secret
to a private file under the runner's temporary directory, runs the redacted
scan in scripts/native_qa, and removes the file.

The only workflow outputs are a fixed status word and a count. Image paths
travel NUL-delimited in a file, and scan output is printed with workflow
commands stopped, so no path is interpreted as a command.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import changes  # noqa: E402

SECRET = "NATIVE_QA_PRIVACY_TEMPLATES"
QA = HERE.parent / "native_qa" / "qa.py"
SUFFIXES = (b".png", b".jpg", b".jpeg", b".gif", b".webp")
ZERO = "0" * 40


def comparisons(repository: Path, event_name: str, event: dict, checkout_sha: str) -> list[tuple[str, str]]:
    """The comparisons that define added or changed images; empty when the event has none."""
    if event_name == "workflow_dispatch":
        return []
    expected = changes.checkout_identity(repository, checkout_sha)
    if event_name == "pull_request":
        return changes.pull_request_comparisons(repository, event, expected)
    if event_name == "push":
        if (changes.valid_oid(event["after"]) != expected or event.get("deleted") is not False
                or event.get("ref") != "refs/heads/main"):
            raise changes.PolicyError("push-identity-mismatch")
        if event.get("before") == ZERO:
            return []
        return [(changes.valid_oid(event["before"]), expected)]
    raise changes.PolicyError("unsupported-event")


def readable(path: bytes) -> bool:
    try:
        text = path.decode("utf-8", errors="strict")
    except UnicodeError:
        return False
    return text.isprintable()


def plan(repository: Path, event_name: str, event: dict, checkout_sha: str, configured: bool
         ) -> tuple[str, list[bytes]]:
    """(status, images): scan, no-images, unavailable or no-comparison."""
    try:
        pairs = comparisons(repository, event_name, event, checkout_sha)
        images = [path for path in changes.compared_paths(repository, pairs, "d") if path.lower().endswith(SUFFIXES)]
    except (changes.PolicyError, KeyError, TypeError, UnicodeError):
        if not configured:
            return "unavailable", []
        raise changes.PolicyError("comparison-unavailable-or-inconsistent") from None
    if not pairs:
        return "no-comparison", []
    images = [path for path in images
              if (repository / os.fsdecode(path)).is_file() and not (repository / os.fsdecode(path)).is_symlink()]
    if not images:
        return "no-images", []
    if not configured:
        return "unavailable", images
    if not all(readable(path) for path in images):
        raise changes.PolicyError("an added or changed image has an unprintable or ambiguous name; rename it")
    return "scan", images


NOTICES = {
    "unavailable": "{count} added or changed image(s) were NOT template-scanned: the {secret} secret is not "
                   "available to this run (a fork pull request or an unconfigured repository). This is not a pass; "
                   "scan them with `qa.py privacy scan` where the templates are available.",
    "no-comparison": "Added or changed images were NOT template-scanned: this event has no comparison. "
                     "This is not a pass.",
}


def run_plan(args) -> int:
    try:
        event = changes.load_json(Path(os.environ["GITHUB_EVENT_PATH"]))
        status, images = plan(args.repository, os.environ.get("GITHUB_EVENT_NAME", ""), event,
                              os.environ.get("GITHUB_SHA", ""), os.environ.get("TEMPLATES_CONFIGURED") == "true")
    except (changes.PolicyError, KeyError, OSError, ValueError) as error:
        print(f"::error title=Image privacy::cannot list added or changed images: {error}")
        return 1
    args.list.parent.mkdir(parents=True, exist_ok=True)
    args.list.write_bytes(b"".join(path + b"\0" for path in images))
    count = len(images)
    print(json.dumps({"status": status, "images": count}, sort_keys=True))
    if status in NOTICES:
        notice = NOTICES[status].format(count=count or "Any", secret=SECRET)
        print(f"::notice title=Image privacy scan skipped::{notice}")
        if args.summary:
            with args.summary.open("a", encoding="utf-8") as summary:
                summary.write(f"**Image privacy:** {notice}\n")
    if args.output:
        with args.output.open("a", encoding="utf-8") as output:
            output.write(f"status={status}\nimages={count}\n")
    return 0


def run_scan(args) -> int:
    packed = os.environ.get(SECRET, "")
    if not packed.strip():
        print(f"::error title=Image privacy::the {SECRET} secret is empty in the scan step")
        return 2
    images = [os.fsdecode(path) for path in args.list.read_bytes().split(b"\0") if path]
    if not images:
        print("::error title=Image privacy::the scan step received no images")
        return 2
    scratch = args.directory / "scan"
    scratch.mkdir(mode=0o700, parents=True, exist_ok=True)
    templates = scratch / "templates.b64"
    descriptor = os.open(templates, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w", encoding="ascii", errors="replace") as stream:
        stream.write(packed)
    # The child needs neither the secret nor the default temporary directory:
    # the C engine's per-template scratch files stay under the runner's temp.
    environment = {key: value for key, value in os.environ.items() if key != SECRET}
    environment["TMPDIR"] = str(scratch)
    try:
        completed = subprocess.run(
            [sys.executable, "-B", str(QA), "privacy", "scan", "--redacted", "--jobs", str(args.jobs),
             "--packed", str(templates), "--", *images],
            cwd=args.repository, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="replace", check=False)
    finally:
        templates.unlink(missing_ok=True)
    token = secrets.token_hex(16)
    print(f"::stop-commands::{token}")
    print(completed.stdout, end="" if completed.stdout.endswith("\n") else "\n")
    print(f"::{token}::")
    if completed.returncode == 0:
        if args.output:
            with args.output.open("a", encoding="utf-8") as output:
                output.write("status=scanned\n")
        verdict = f"{len(images)} added or changed image(s) template-scanned; no match."
    elif completed.returncode == 1:
        verdict = "an added or changed image matched a private-string template; retake or crop it."
        print(f"::error title=Image privacy::{verdict}")
    else:
        verdict = f"the scan could not complete (exit {completed.returncode})."
        print(f"::error title=Image privacy::{verdict}")
    if args.summary:
        with args.summary.open("a", encoding="utf-8") as summary:
            summary.write(f"**Image privacy:** {verdict}\n")
    return completed.returncode


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("plan", help="list added or changed images for this Actions event")
    p.add_argument("--list", type=Path, required=True, help="write the NUL-delimited image list here")
    p.add_argument("--output", type=Path, help="append status= and images= to GITHUB_OUTPUT")
    p.add_argument("--summary", type=Path, help="append a skip notice to GITHUB_STEP_SUMMARY")
    p.add_argument("--repository", type=Path, default=Path("."))
    p.set_defaults(func=run_plan)
    s = sub.add_parser("scan", help=f"redacted template scan of the listed images, templates from ${SECRET}")
    s.add_argument("--list", type=Path, required=True)
    s.add_argument("--directory", type=Path, required=True, help="private scratch directory under RUNNER_TEMP")
    s.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    s.add_argument("--output", type=Path, help="append status=scanned to GITHUB_OUTPUT on a clean scan")
    s.add_argument("--summary", type=Path, help="append the verdict to GITHUB_STEP_SUMMARY")
    s.add_argument("--repository", type=Path, default=Path("."))
    s.set_defaults(func=run_scan)
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
