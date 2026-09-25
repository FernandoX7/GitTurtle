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
# Keep each independent required phase explicit. A successful debug matrix
# cannot compensate for a missing/cancelled release matrix or formatter.
JOBS = {"rust-format": "product", "rust-debug": "product", "rust-release": "product",
        "agent-tooling": "tooling", "website": "website"}
OID = re.compile(r"[0-9a-f]{40}\Z")
DOC_NAMES = {
    "README.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "SECURITY.md",
    "DESIGN.md", "CHANGELOG.md",
}
DOC_SUFFIXES = {".md", ".png", ".jpg", ".jpeg", ".svg", ".webp"}


class PolicyError(ValueError):
    """Input cannot establish the requested policy result."""


def full_plan(reason: str) -> dict:
    return {"version": 1, "product": True, "tooling": True, "website": True,
            "reason": reason, "path_count": None}


def classify_paths(paths: list[bytes]) -> dict:
    if not paths or len(paths) > MAX_PATHS:
        return full_plan("empty-or-oversized-comparison")
    selected = set()
    for raw in paths:
        try:
            path = raw.decode("utf-8", errors="strict")
        except UnicodeError:
            return full_plan("unrecognized-path")
        parts = path.split("/")
        if (not path or any(part in ("", ".", "..") for part in parts)
                or "\\" in path or any(ord(c) < 32 or ord(c) == 127 for c in path)):
            return full_plan("unrecognized-path")
        if path == "scripts/ci/packages.py":
            return full_plan("package-delivery")
        if path.startswith(".github/"):
            return full_plan("workflow-or-repository-policy")
        if (path.startswith(("scripts/ci/", "scripts/agent_loop/", "scripts/native_qa/", ".agents/", ".codex/",
                             "docs/development/"))
                or path in ("scripts/agent-loop.py", "scripts/check-agent-guidance.py")
                or parts[-1] == "AGENTS.md"):
            selected.add("tooling")
        elif path.startswith("website/"):
            selected.add("website")
        elif path in DOC_NAMES or (path.startswith("docs/")
                                  and PurePosixPath(path).suffix in DOC_SUFFIXES):
            pass  # Guidance and CI policy checks run in the mandatory changes job.
        else:
            # Includes crates, manifests, toolchain, native assets, vendor, build
            # and package scripts, root configuration, and unknown future inputs.
            return full_plan("product-or-unrecognized-input")
    return {"version": 1, **{lane: lane in selected for lane in LANES},
            "reason": "classified-paths", "path_count": len(paths)}


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


def classify_checkout(repository: Path, event_name: str, event: dict, checkout_sha: str) -> dict:
    if event_name == "workflow_dispatch":
        return full_plan("manual-full-validation")
    try:
        expected = valid_oid(checkout_sha)
        actual = git_read(repository, "rev-parse", "HEAD").decode("ascii").strip()
        if actual != expected:
            raise PolicyError("checkout-identity-mismatch")
        if event_name == "pull_request":
            pr = event["pull_request"]
            base, head = valid_oid(pr["base"]["sha"]), valid_oid(pr["head"]["sha"])
            parents = git_read(repository, "rev-list", "--parents", "-n", "1", expected).decode("ascii").split()
            if parents != [expected, base, head]:
                raise PolicyError("merge-identity-mismatch")
            merge_base = git_read(repository, "merge-base", base, head).decode("ascii").strip()
            valid_oid(merge_base)
            comparisons = [(merge_base, head), (base, expected)]
        elif event_name == "push":
            valid_oid(event["before"])
            head = valid_oid(event["after"])
            if (head != expected or event.get("deleted") is not False
                    or event.get("ref") != "refs/heads/main"):
                raise PolicyError("push-identity-mismatch")
            # Main establishes both package coverage and a trusted cache seed.
            # Cheap changed-path routing belongs to contribution PRs only.
            return full_plan("main-full-validation")
        else:
            raise PolicyError("unsupported-event")
        paths = set()
        for before, after in comparisons:
            paths.update(parse_paths(git_read(repository, "diff", "--no-ext-diff", "--no-textconv",
                                             "--name-only", "-z", "--no-renames", before, after, "--")))
        return classify_paths(sorted(paths))
    except (PolicyError, KeyError, TypeError, UnicodeError):
        return full_plan("comparison-unavailable-or-inconsistent")


def validate_plan(plan: object) -> dict:
    if not isinstance(plan, dict) or set(plan) != {"version", *LANES, "reason", "path_count"}:
        raise PolicyError("missing or malformed classification")
    if type(plan["version"]) is not int or plan["version"] != 1:
        raise PolicyError("unknown classification version")
    if any(type(plan[lane]) is not bool for lane in LANES):
        raise PolicyError("classification flags must be booleans")
    count = plan["path_count"]
    if plan["reason"] == "classified-paths":
        if type(count) is not int or not 1 <= count <= MAX_PATHS:
            raise PolicyError("classified paths lack a complete comparison")
    elif count is not None or not all(plan[lane] for lane in LANES):
        raise PolicyError("fallback must require all validation lanes")
    if not isinstance(plan["reason"], str) or not re.fullmatch(r"[a-z-]{1,64}", plan["reason"]):
        raise PolicyError("invalid classification reason")
    # Product inputs require development tooling as well as both Rust platforms.
    if plan["product"] and not plan["tooling"]:
        raise PolicyError("product validation requires tooling")
    return plan


def gate(needs: object) -> list[str]:
    if not isinstance(needs, dict) or set(needs) != {"changes", *JOBS}:
        raise PolicyError("missing or unexpected job results")
    changes = needs["changes"]
    if not isinstance(changes, dict) or changes.get("result") != "success":
        raise PolicyError("change classification did not succeed")
    try:
        outputs = changes["outputs"]
        plan = validate_plan(json.loads(outputs["plan"]))
        if any(outputs.get(lane) != str(plan[lane]).lower() for lane in LANES):
            raise PolicyError("classification outputs disagree")
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        raise PolicyError("missing or malformed classification outputs") from error
    results = []
    for job, lane in JOBS.items():
        entry = needs[job]
        result = entry.get("result") if isinstance(entry, dict) else None
        if result == "success":
            results.append(f"{job}: success")
        elif result == "skipped" and not plan[lane]:
            results.append(f"{job}: not needed by classification")
        else:
            raise PolicyError(f"{job}: required result was not successful ({result!r})")
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
