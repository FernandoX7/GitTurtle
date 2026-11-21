#!/usr/bin/env python3
"""Template-scan the raster images a pull request or main push adds or changes.

`plan` lists those images and records whether they can be scanned. The pixel
templates arrive only through the optional NATIVE_QA_PRIVACY_TEMPLATES
repository secret (written by `qa.py privacy pack`), which GitHub does not pass
to fork pull requests. Without it the plan is `unavailable` and a notice says
the images were not scanned; nothing reports a pass. `scan` writes the secret
to a private file under the runner's temporary directory, runs the redacted
scan in scripts/native_qa, and removes the file.

The matching is brute force, so the workflow runs the job as a matrix of
SHARDS shards. Every shard makes the same plan, and `slice` gives it every
SHARDS-th image of the sorted list: the slices are disjoint and together are
the list. A shard whose slice is empty has nothing to scan and never sees the
secret. A matrix job's output is the last shard's to set it, so a shard writes
`scanned` only when its whole slice is clean (an empty slice of a list other
shards scan included): with every shard successful, `scanned` is true of the
whole list.

The only workflow outputs are fixed status words and a count. Image paths
travel NUL-delimited in a file. Workflow commands are stopped before the
scanner starts, and each line it prints is passed on as it arrives, so no path
is interpreted as a command and a cancelled scan shows the images it finished.
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
# The image-privacy matrix in .github/workflows/quality.yml has this many
# shards. `slice` refuses any other count: a matrix of fewer jobs cutting the
# list this many ways would leave slices that no job scans.
SHARDS = 8


def comparisons(repository: Path, event_name: str, event: dict, checkout_sha: str) -> list[tuple[str, str]]:
    """The comparisons that define added or changed images; empty when the event has none."""
    if event_name in ("workflow_dispatch", "schedule"):
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


def slice_of(images: list[bytes], shard: int, shards: int) -> list[bytes]:
    """Shard `shard`'s images: every `shards`-th one of the sorted list, from index `shard - 1`.

    A pure function of the list, the shard's number and the count, so jobs that
    share nothing but the plan cut the same partition: each image is in exactly
    one slice, in list order, and neighbours in the list (one directory's large
    or small frames) go to different shards.
    """
    if type(shard) is not int or type(shards) is not int or shards != SHARDS or not 1 <= shard <= shards:
        raise changes.PolicyError(f"shard {shard!r} of {shards!r} is not one of the workflow's {SHARDS} shards")
    if any(first >= second for first, second in zip(images, images[1:])):
        raise changes.PolicyError("the image list is not sorted and unique, so its slices are undefined")
    return images[shard - 1::shards]


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


def run_slice(args) -> int:
    try:
        images = [path for path in args.list.read_bytes().split(b"\0") if path]
        mine = slice_of(images, args.shard, args.shards)
    except (changes.PolicyError, OSError) as error:
        print(f"::error title=Image privacy::cannot take this shard's slice of the image list: {error}")
        return 2
    if not images:
        # `scanned` would claim a scan nobody made: the plan reports an empty list itself.
        print("::error title=Image privacy::the slice step received no images")
        return 2
    args.slice.parent.mkdir(parents=True, exist_ok=True)
    args.slice.write_bytes(b"".join(path + b"\0" for path in mine))
    print(json.dumps({"shard": args.shard, "shards": args.shards, "images": len(images), "slice": len(mine)},
                     sort_keys=True))
    if mine:
        line = f"shard {args.shard} of {args.shards} scans {len(mine)} of {len(images)} added or changed image(s)."
    else:
        line = (f"shard {args.shard} of {args.shards} has none of the {len(images)} added or changed image(s) "
                "to scan; the other shards scan them.")
    if args.summary:
        with args.summary.open("a", encoding="utf-8") as summary:
            summary.write(f"**Image privacy:** {line}\n")
    if args.output:
        with args.output.open("a", encoding="utf-8") as output:
            # An empty slice is wholly scanned: the scan step, the only one that
            # is given the secret, does not run for it.
            output.write("slice=scan\n" if mine else "slice=empty\nstatus=scanned\n")
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
    # Workflow commands stop before the scanner starts and resume whatever
    # happens to it. Each of its lines is passed on, flushed, as it is read, so
    # the log shows every image whose scan has ended, also when the job is
    # cancelled, and no line (they begin with a path) is taken for a command.
    token = secrets.token_hex(16)
    print(f"::stop-commands::{token}")
    try:
        with subprocess.Popen(
                [sys.executable, "-B", str(QA), "privacy", "scan", "--redacted", "--jobs", str(args.jobs),
                 "--packed", str(templates), "--", *images],
                cwd=args.repository, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, errors="replace") as child:
            try:
                ended = True
                for line in child.stdout:
                    ended = line.endswith("\n")
                    print(line, end="", flush=True)
                if not ended:
                    print()
            except BaseException:
                # An interrupted or failed pass-through, Ctrl-C included, must not leave the scanner
                # running with the templates loaded: it is killed and gone before the exception continues.
                child.kill()
                child.wait()
                raise
        returncode = child.returncode
    finally:
        templates.unlink(missing_ok=True)
        print(f"::{token}::")
    if returncode == 0:
        if args.output:
            with args.output.open("a", encoding="utf-8") as output:
                output.write("status=scanned\n")
        verdict = f"{len(images)} added or changed image(s) template-scanned; no match."
    elif returncode == 1:
        verdict = "an added or changed image matched a private-string template; retake or crop it."
        print(f"::error title=Image privacy::{verdict}")
    else:
        verdict = f"the scan could not complete (exit {returncode})."
        print(f"::error title=Image privacy::{verdict}")
    if args.summary:
        with args.summary.open("a", encoding="utf-8") as summary:
            summary.write(f"**Image privacy:** {verdict}\n")
    return returncode


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("plan", help="list added or changed images for this Actions event")
    p.add_argument("--list", type=Path, required=True, help="write the NUL-delimited image list here")
    p.add_argument("--output", type=Path, help="append status= and images= to GITHUB_OUTPUT")
    p.add_argument("--summary", type=Path, help="append a skip notice to GITHUB_STEP_SUMMARY")
    p.add_argument("--repository", type=Path, default=Path("."))
    p.set_defaults(func=run_plan)
    c = sub.add_parser("slice", help="write one shard's share of the planned list; needs no secret")
    c.add_argument("--list", type=Path, required=True, help="the NUL-delimited list `plan` wrote")
    c.add_argument("--slice", type=Path, required=True, help="write this shard's NUL-delimited images here")
    c.add_argument("--shard", type=int, required=True, help=f"this job's number, 1 to {SHARDS}")
    c.add_argument("--shards", type=int, required=True, help=f"the matrix's job count; anything but {SHARDS} is refused")
    c.add_argument("--output", type=Path, help="append slice= and, for an empty slice, status=scanned to GITHUB_OUTPUT")
    c.add_argument("--summary", type=Path, help="append this shard's share to GITHUB_STEP_SUMMARY")
    c.set_defaults(func=run_slice)
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
