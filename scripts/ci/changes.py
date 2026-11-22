#!/usr/bin/env python3
"""Conservative Quality routing and its required-result boundary.

No network calls or contributor-provided shell fragments. Missing comparison
information selects all lanes; malformed or missing gate information fails.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile

MAX_INPUT_BYTES = 4 * 1024 * 1024
MAX_PATHS = 10000
LANES = ("product", "tooling", "website")
# Keep each independent required phase explicit. A successful debug (test)
# matrix cannot compensate for a missing/cancelled Clippy matrix, release
# matrix, formatter or the Arch Linux container job, and a successful release
# matrix cannot compensate for the later job that downloads and verifies what
# its Linux legs uploaded.
JOBS = {"rust-format": "product", "rust-debug": "product", "rust-clippy": "product",
        "rust-release": "product", "package-transfer": "product", "rust-arch": "product",
        "agent-tooling": "tooling", "website": "website"}
# Runs unconditionally in parallel with other lanes once `changes` finishes,
# taking the full CI unit test suite off the serial `changes` critical path.
CI_TESTS_JOB = "ci-tests"
# A Linux release leg hands on `<label>-eligible` and its leg of the later job
# `<label>-transfer` (scripts/ci/packages.py writes both). A package counts only
# as one of these pairs, each with the line the gate prints for it. The labels
# are those of the platforms packages.py uploads; a test holds them equal.
RELEASE_JOB, TRANSFER_JOB = "rust-release", "package-transfer"
TRANSFER_LABELS = ("linux-x86_64", "linux-aarch64")
TRANSFER_PAIRS = {
    ("true", "verified"): "uploaded by its release leg; downloaded and verified by the later job",
    ("false", "held"): "held by its release leg; nothing uploaded and nothing verified",
}
# The image privacy job runs whenever added or changed raster images exist (or
# when comparison fails closed). Only "scanned" is a scan; the other results
# succeed without claiming one, and the gate says so.
PRIVACY_JOB = "image-privacy"
PRIVACY_RESULTS = {
    "scanned": f"{PRIVACY_JOB}: added or changed images template-scanned; no match",
    "no-images": f"{PRIVACY_JOB}: no added or changed images",
    "unavailable": f"{PRIVACY_JOB}: NOT scanned; the templates secret is unavailable to this run "
                   "(fork pull request or unconfigured repository)",
    "no-comparison": f"{PRIVACY_JOB}: NOT scanned; this event has no comparison",
}
IMAGE_SUFFIXES = (b".png", b".jpg", b".jpeg", b".gif", b".webp")
ZERO_OID = "0" * 40
OID = re.compile(r"[0-9a-f]{40}\Z")
DOC_NAMES = {
    "README.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md",
    "DESIGN.md", "SUPPORT.md",
}
DOC_SUFFIXES = {".md", ".png", ".jpg", ".jpeg", ".svg", ".webp"}
BENCHMARK_DATA_SUFFIXES = {".json", ".csv", ".tsv"}
PURE_PRODUCT_PREFIXES = ("crates/", "vendor/", "assets/", ".cargo/")
PURE_PRODUCT_FILES = {
    "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rustfmt.toml",
    "clippy.toml", "deny.toml",
}


class PolicyError(ValueError):
    """Input cannot establish the requested policy result."""


def paths_have_images(paths: list[bytes]) -> bool:
    return any(path.lower().endswith(IMAGE_SUFFIXES) for path in paths)


def full_plan(reason: str, *, has_images: bool = True) -> dict:
    return {"version": 1, "product": True, "tooling": True, "website": True,
            "has_images": has_images, "reason": reason, "path_count": None}


def classify_paths(paths: list[bytes], *, has_images: bool | None = None) -> dict:
    images = paths_have_images(paths) if has_images is None else has_images
    if not paths or len(paths) > MAX_PATHS:
        return full_plan("empty-or-oversized-comparison", has_images=True)
    decoded: list[tuple[str, list[str]]] = []
    for raw in paths:
        try:
            path = raw.decode("utf-8", errors="strict")
        except UnicodeError:
            return full_plan("unrecognized-path", has_images=True)
        parts = path.split("/")
        if (not path or any(part in ("", ".", "..") for part in parts)
                or "\\" in path or any(ord(c) < 32 or ord(c) == 127 for c in path)):
            return full_plan("unrecognized-path", has_images=True)
        decoded.append((path, parts))
    selected = set()
    for path, parts in decoded:
        if path == "scripts/ci/packages.py":
            return full_plan("package-delivery", has_images=images)
        # The pinned tool installer only runs in the Rust jobs, so a pin change
        # must exercise them before it reaches main.
        if path == "scripts/ci/tools.py":
            return full_plan("pinned-ci-tool", has_images=images)
        # The package installation helper runs only inside the Rust jobs' setup,
        # the Image privacy job and the package transfer job, so a change of it
        # must exercise them before it reaches main.
        if path == "scripts/ci/apt_install.py":
            return full_plan("package-installation-helper", has_images=images)
        # Every Rust job runs its commands under the collector, and the Rust
        # cache action records with it, so a change of it must run them before
        # it reaches main.
        if path == "scripts/ci/metrics.py":
            return full_plan("ci-collector", has_images=images)
        # Linux Rust test shards run this ELF inspector directly.
        if path == "scripts/ci/libdav1d_elf.py":
            return full_plan("elf-link-check", has_images=images)
        if path == ".github/copilot-instructions.md":
            selected.add("tooling")
            continue
        if path.startswith(".github/"):
            return full_plan("workflow-or-repository-policy", has_images=images)
        # Development guidance, hooks, tooling scripts and dev helpers are
        # validated by the guidance check and the tooling suite.
        if (path.startswith(("scripts/ci/", "scripts/dev/", "scripts/native_qa/",
                             ".agents/", ".codex/", ".claude/", ".githooks/",
                             "docs/development/"))
                or path in ("scripts/check-agent-guidance.py", "scripts/check-docs-sync.py",
                            "docs/doc-manifest.json",
                            "scripts/gate.py", "scripts/test_gate.py",
                            "scripts/test_claude_rm_hook.py")
                or parts[-1] in ("AGENTS.md", "CLAUDE.md", "GEMINI.md")):
            selected.add("tooling")
        elif path.startswith("website/"):
            selected.add("website")
        elif (path in DOC_NAMES
              or (path.startswith("docs/")
                  and not path.startswith("docs/licenses/")
                  and PurePosixPath(path).suffix in DOC_SUFFIXES)
              or (path.startswith("docs/benchmarks/")
                  and PurePosixPath(path).suffix in BENCHMARK_DATA_SUFFIXES)
              or (path.startswith("crates/app/docs/") and PurePosixPath(path).suffix == ".md")
              or path in ("crates/git-core/README.md", "packaging/aur/README.md")):
            pass  # Guidance and CI policy checks run in the mandatory changes job.
        elif path.startswith(PURE_PRODUCT_PREFIXES) or path in PURE_PRODUCT_FILES:
            # Pure Rust/app/vendor/manifest/asset edits run all product Rust
            # checks without holding a macOS slot on unrelated agent-tooling
            # or website jobs (ci-tests still runs in parallel on Ubuntu).
            selected.add("product")
        else:
            # Includes packaging/release scripts, root configuration, and
            # unknown future inputs: fail closed to full validation.
            return full_plan("product-or-unrecognized-input", has_images=images)
    return {"version": 1, **{lane: lane in selected for lane in LANES},
            "has_images": images, "reason": "classified-paths", "path_count": len(paths)}


def parse_paths(raw: bytes) -> list[bytes]:
    # --no-renames presents both sides of a move. NUL delimiters preserve spaces,
    # tabs and non-UTF-8 names without interpreting them as shell/log syntax.
    if len(raw) > MAX_INPUT_BYTES or (raw and not raw.endswith(b"\0")):
        raise PolicyError("invalid-or-oversized-diff")
    paths = raw[:-1].split(b"\0") if raw else []
    if len(paths) > MAX_PATHS or any(not path for path in paths):
        raise PolicyError("invalid-or-oversized-diff")
    return paths


def git_read(repository: Path, *arguments: str) -> bytes:
    env = os.environ.copy()
    env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
                "GIT_OPTIONAL_LOCKS": "0", "GIT_NO_LAZY_FETCH": "1",
                "GIT_TERMINAL_PROMPT": "0"})
    with tempfile.TemporaryFile() as output:
        try:
            subprocess.run(["git", "-c", "core.fsmonitor=false", "-c", "diff.external=",
                            *arguments], cwd=repository, env=env, stdout=output,
                           stderr=subprocess.DEVNULL, check=True, timeout=60)
        except (OSError, subprocess.SubprocessError) as error:
            raise PolicyError("comparison-unavailable") from error
        output.seek(0)
        result = output.read(MAX_INPUT_BYTES + 1)
    if len(result) > MAX_INPUT_BYTES:
        raise PolicyError("comparison-too-large")
    return result


def valid_oid(value: object) -> str:
    if not isinstance(value, str) or not OID.fullmatch(value) or set(value) == {"0"}:
        raise PolicyError("missing-comparison-identity")
    return value


def checkout_identity(repository: Path, checkout_sha: str) -> str:
    expected = valid_oid(checkout_sha)
    actual = git_read(repository, "rev-parse", "HEAD").decode("ascii").strip()
    if actual != expected:
        raise PolicyError("checkout-identity-mismatch")
    return expected


def pull_request_comparisons(repository: Path, event: dict, expected: str) -> list[tuple[str, str]]:
    """The PR head against its merge-base, and the base against the exercised merge commit."""
    pr = event["pull_request"]
    base, head = valid_oid(pr["base"]["sha"]), valid_oid(pr["head"]["sha"])
    parents = git_read(repository, "rev-list", "--parents", "-n", "1", expected).decode("ascii").split()
    if parents != [expected, base, head]:
        raise PolicyError("merge-identity-mismatch")
    merge_base = git_read(repository, "merge-base", base, head).decode("ascii").strip()
    valid_oid(merge_base)
    return [(merge_base, head), (base, expected)]


def compared_paths(repository: Path, comparisons: list[tuple[str, str]], diff_filter: str = "") -> list[bytes]:
    paths = set()
    for before, after in comparisons:
        paths.update(parse_paths(git_read(repository, "diff", "--no-ext-diff", "--no-textconv",
                                         "--name-only", "-z", "--no-renames",
                                         *([f"--diff-filter={diff_filter}"] if diff_filter else []),
                                         before, after, "--")))
    return sorted(paths)


def classify_checkout(repository: Path, event_name: str, event: dict, checkout_sha: str) -> dict:
    if event_name == "workflow_dispatch":
        return full_plan("manual-full-validation", has_images=False)
    if event_name == "schedule":
        return full_plan("scheduled-cache-seed", has_images=False)
    try:
        expected = checkout_identity(repository, checkout_sha)
        if event_name == "pull_request":
            comparisons = pull_request_comparisons(repository, event, expected)
            paths = compared_paths(repository, comparisons)
            non_deleted = compared_paths(repository, comparisons, "d")
            return classify_paths(paths, has_images=paths_have_images(non_deleted))
        elif event_name == "push":
            head = valid_oid(event["after"])
            if (head != expected or event.get("deleted") is not False
                    or event.get("ref") != "refs/heads/main"):
                raise PolicyError("push-identity-mismatch")
            before = event.get("before")
            if before == ZERO_OID:
                return full_plan("main-no-comparison", has_images=False)
            before_oid = valid_oid(before)
            comparisons = [(before_oid, expected)]
            paths = compared_paths(repository, comparisons)
            non_deleted = compared_paths(repository, comparisons, "d")
            classified = classify_paths(paths, has_images=paths_have_images(non_deleted))
            if classified["product"]:
                # Any code, manifest, workflow, or unrecognized change on main
                # runs full validation and refreshes trusted main caches.
                return full_plan("main-full-validation", has_images=classified["has_images"])
            # Pure docs/records/website/tooling merges on main leave the built
            # product and Rust cache keys unchanged; run only affected lanes.
            return classified
        else:
            raise PolicyError("unsupported-event")
    except (PolicyError, KeyError, TypeError, UnicodeError):
        return full_plan("comparison-unavailable-or-inconsistent", has_images=True)


def validate_plan(plan: object) -> dict:
    if not isinstance(plan, dict) or set(plan) != {"version", *LANES, "has_images", "reason", "path_count"}:
        raise PolicyError("missing or malformed classification")
    if type(plan["version"]) is not int or plan["version"] != 1:
        raise PolicyError("unknown classification version")
    if any(type(plan[lane]) is not bool for lane in LANES) or type(plan["has_images"]) is not bool:
        raise PolicyError("classification flags must be booleans")
    count = plan["path_count"]
    if plan["reason"] == "classified-paths":
        if type(count) is not int or not 1 <= count <= MAX_PATHS:
            raise PolicyError("classified paths lack a complete comparison")
    elif count is not None or not all(plan[lane] for lane in LANES):
        raise PolicyError("fallback must require all validation lanes")
    if not isinstance(plan["reason"], str) or not re.fullmatch(r"[a-z-]{1,64}", plan["reason"]):
        raise PolicyError("invalid classification reason")
    return plan


def transfer_results(needs: dict) -> list[str]:
    """A line for each Linux package, from the pair of its release leg's answer and the later job's.

    Called once both jobs' results are a success or a justified skip.
    """
    ran = [needs[job]["result"] == "success" for job in (RELEASE_JOB, TRANSFER_JOB)]
    if not any(ran):
        return []  # The product lane did not run, so no package was built.
    if not all(ran):
        raise PolicyError(f"{TRANSFER_JOB}: the release job and the later job did not both run")

    def word(job: str, name: str, side: int) -> str:
        """The word a job handed on under a name when it is one a pair knows; otherwise how a
        refusal names what it found, since a value that is no such word is never printed."""
        outputs = needs[job].get("outputs")
        value = outputs.get(name) if isinstance(outputs, dict) else None
        if isinstance(value, str) and value in {pair[side] for pair in TRANSFER_PAIRS}:
            return value
        return "missing" if value is None or value == "" else "unexpected"

    results = []
    for label in TRANSFER_LABELS:
        pair = word(RELEASE_JOB, f"{label}-eligible", 0), word(TRANSFER_JOB, f"{label}-transfer", 1)
        if pair not in TRANSFER_PAIRS:
            raise PolicyError(f"{TRANSFER_JOB}: {label} is neither an uploaded and verified package nor a held one "
                              f"(eligible: {pair[0]}; transfer: {pair[1]})")
        results.append(f"{TRANSFER_JOB}: {label} {TRANSFER_PAIRS[pair]}")
    return results


def gate(needs: object) -> list[str]:
    if not isinstance(needs, dict) or set(needs) != {"changes", CI_TESTS_JOB, PRIVACY_JOB, *JOBS}:
        raise PolicyError("missing or unexpected job results")
    changes = needs["changes"]
    if not isinstance(changes, dict) or changes.get("result") != "success":
        raise PolicyError("change classification did not succeed")
    try:
        outputs = changes["outputs"]
        plan = validate_plan(json.loads(outputs["plan"]))
        if (any(outputs.get(lane) != str(plan[lane]).lower() for lane in LANES)
                or outputs.get("has-images") != str(plan["has_images"]).lower()):
            raise PolicyError("classification outputs disagree")
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        raise PolicyError("missing or malformed classification outputs") from error
    ci_tests = needs[CI_TESTS_JOB]
    if not isinstance(ci_tests, dict) or ci_tests.get("result") != "success":
        raise PolicyError(f"{CI_TESTS_JOB}: required result was not successful ({ci_tests.get('result') if isinstance(ci_tests, dict) else None!r})")
    results = [f"{CI_TESTS_JOB}: success"]
    for job, lane in JOBS.items():
        entry = needs[job]
        result = entry.get("result") if isinstance(entry, dict) else None
        if result == "success":
            results.append(f"{job}: success")
        elif result == "skipped" and not plan[lane]:
            results.append(f"{job}: not needed by classification")
        else:
            raise PolicyError(f"{job}: required result was not successful ({result!r})")
    results += transfer_results(needs)
    entry = needs[PRIVACY_JOB]
    result = entry.get("result") if isinstance(entry, dict) else None
    outputs = entry.get("outputs") if isinstance(entry, dict) else None
    status = outputs.get("status") if isinstance(outputs, dict) else None
    if result == "skipped" and not plan["has_images"]:
        skipped_status = (
            "no-comparison"
            if plan["reason"] in ("manual-full-validation", "scheduled-cache-seed", "main-no-comparison")
            else "no-images"
        )
        results.append(PRIVACY_RESULTS[skipped_status])
    elif result == "success" and isinstance(status, str) and status in PRIVACY_RESULTS:
        results.append(PRIVACY_RESULTS[status])
    else:
        raise PolicyError(f"{PRIVACY_JOB}: required result was not successful ({result!r}, {status!r})")
    return results


def load_json(path: Path) -> object:
    with path.open("rb") as stream:
        raw = stream.read(MAX_INPUT_BYTES + 1)
    if len(raw) > MAX_INPUT_BYTES:
        raise PolicyError("JSON input exceeds limit")
    return json.loads(raw)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    classify = sub.add_parser("classify", help="classify the checked-out Actions event; uncertainty runs everything")
    classify.add_argument("--repository", type=Path, default=Path("."))
    classify.add_argument("--output", type=Path, help="write fixed boolean flags and plan to GITHUB_OUTPUT")
    sub.add_parser("gate", help="validate JSON from the NEEDS_JSON environment variable")
    args = parser.parse_args()
    try:
        if args.command == "classify":
            try:
                event = load_json(Path(os.environ["GITHUB_EVENT_PATH"]))
                plan = classify_checkout(args.repository, os.environ.get("GITHUB_EVENT_NAME", ""),
                                         event, os.environ.get("GITHUB_SHA", ""))
            except (KeyError, OSError, ValueError):
                plan = full_plan("event-unavailable")
            serialized = json.dumps(plan, separators=(",", ":"), sort_keys=True)
            print(serialized)
            if args.output:
                with args.output.open("a", encoding="utf-8") as output:
                    for lane in LANES:
                        output.write(f"{lane}={str(plan[lane]).lower()}\n")
                    output.write(f"has-images={str(plan['has_images']).lower()}\n")
                    output.write(f"plan={serialized}\n")
        else:
            raw = os.environ.get("NEEDS_JSON", "")
            if len(raw.encode("utf-8")) > MAX_INPUT_BYTES:
                raise PolicyError("job results exceed limit")
            for result in gate(json.loads(raw)):
                print(result)
    except (PolicyError, OSError, ValueError) as error:
        print(f"CI policy failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
