#!/usr/bin/env python3
"""NEVER MERGE: one disposable hosted debug-cache invalidation diagnostic.

Each debug lane calls the unchanged production setup action twice with no Cargo
build, test or fetch, no finish phase and no cache save. The first restore must
hit the trusted main seed exactly and contain compiled dependencies. One fixed
vendor comment then changes the real source identity; the second restore must
report a miss that the cache service confirms, and production recovery must
leave no target, registry or Git payload behind. Observations are written before
expectations are checked, so every failure keeps its evidence.

Main may move while a candidate waits. Preflight reads only the current main
SHA from the API and compares locally in the full-history checkout: the bound
main commit must be current main or one of its ancestors, and the commits and
file list main gained since the binding are recorded so the coordinator can
verify that no cache input changed; the identity computed at the bound main and
at the seed must equal the checkout's identity. The candidate commit must sit
directly on the bound main and differ from it by exactly the owned files,
whether the checkout is that commit or a pull request merge commit.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.parse

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = "vendor/gpui-component/src/label.rs"
COMMENT = b"\n// Disposable cache invalidation diagnostic; never integrate this fixture.\n"
OWNED = {".github/workflows/quality.yml", "scripts/ci/cache_diagnostic.py",
         "scripts/ci/tests/test_cache_diagnostic.py"}
PRODUCTION = (".github/actions/setup-rust/action.yml", ".github/actions/setup-rust/cache.py")
EVENTS = {"pull_request", "workflow_dispatch"}
MAX_SAMPLES = 3
MAX_SAMPLE_BYTES = 64 * 1024**2
ARCHIVE_MAGIC = b"!<arch>\n"
FILE_STATUS = {"A": "added", "D": "removed", "M": "modified", "T": "changed"}
SERVICE_OUTCOME = ("rust-cache reports cache-hit=false for a real miss and for a download or "
                   "extraction error alike. The main-scoped service listing shows whether an "
                   "entry exists for the prepared key; the hosted restore log shows which "
                   "outcome occurred (an informational no-cache line versus a warning "
                   "annotation). A green job alone does not establish invalidation.")


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cache = load("cache_diagnostic_production", ROOT / PRODUCTION[1])
github = load("cache_diagnostic_github", ROOT / "scripts/release/github.py")


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def git(*arguments, cwd=None):
    return cache.run(["git", *arguments], cwd=ROOT if cwd is None else cwd)


def save(directory, phase, value):
    (directory / f"{phase}.json").write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def prefix_key(action):
    """The pinned restore prefix comes from the production action, not a private copy."""
    values = set(re.findall(r"^\s*prefix-key:\s*(\S+)\s*$", action.read_text(encoding="utf-8"), re.M))
    if len(values) != 1:
        raise ValueError("Production setup action must declare exactly one cache prefix key")
    return values.pop()


def service_entries(api, prefix):
    """Read-only main-scoped cache service listing for one exact key prefix."""
    route = (f"/repos/{github.REPOSITORY}/actions/caches?key={urllib.parse.quote(prefix, safe='')}"
             f"&ref={urllib.parse.quote('refs/heads/main', safe='')}")
    rows = []
    for entry in api.pages(route, "actions_caches"):
        if not isinstance(entry, dict) or not isinstance(entry.get("key"), str):
            raise ValueError("Malformed cache service entry")
        if not entry["key"].startswith(prefix):
            raise ValueError("Cache service returned an entry outside the requested prefix")
        rows.append({field: entry.get(field) for field in
                     ("id", "key", "ref", "version", "size_in_bytes", "created_at", "last_accessed_at")})
    return rows


def tree_source_identity(commit, name):
    """Compute one commit's identity with the production helper on a disposable worktree."""
    tree = Path(os.environ["RUNNER_TEMP"]).resolve() / f"cache-diagnostic-{name}"
    if tree.exists() or tree.is_symlink():
        raise ValueError("Diagnostic worktree path already exists")
    git("worktree", "add", "--detach", "--quiet", str(tree), commit)
    try:
        return cache.source_identity(tree)
    finally:
        git("worktree", "remove", "--force", str(tree))


def changed_paths(base, head):
    return set(filter(None, git("diff", "--name-only", "-z", "--no-renames", base, head).decode().split("\0")))


def main_compare(main, current):
    """Bound main must be current main or one of its ancestors; record what main gained since.

    Computed locally in the fetch-depth-0 checkout from the API main SHA: the
    production transport refuses routes containing "..", so the compare API is
    never used.
    """
    try:
        git("cat-file", "-e", current + "^{commit}")
    except subprocess.CalledProcessError:
        raise ValueError("Current main commit is not present in the checkout; refresh this diagnostic") from None
    try:
        git("merge-base", "--is-ancestor", main, current)
    except subprocess.CalledProcessError:
        raise ValueError("Bound main commit is neither current main nor one of its ancestors; rebind this diagnostic") from None
    ahead = int(git("rev-list", "--count", f"{main}..{current}").decode().strip())
    behind = int(git("rev-list", "--count", f"{current}..{main}").decode().strip())
    if behind != 0:
        raise ValueError("Bound main commit is neither current main nor one of its ancestors; rebind this diagnostic")
    fields = git("diff", "--name-status", "--no-renames", "-z", f"{main}...{current}").decode().split("\0")
    files = [{"filename": name, "status": FILE_STATUS.get(status[:1], status)}
             for status, name in zip(fields[0::2], fields[1::2])]
    return {"status": "identical" if main == current else "ahead", "ahead_by": ahead, "behind_by": behind,
            "total_commits": ahead, "files": files}


def checkout_shape(main):
    """The candidate commit itself, or a pull request merge of it into a newer main."""
    parents = git("rev-list", "--parents", "-n", "1", "HEAD").decode().split()
    if len(parents) == 2:
        head, base, candidate = parents[0], parents[1], parents[0]
    elif len(parents) == 3:
        head, base, candidate = parents[0], parents[1], parents[2]
    else:
        raise ValueError("Unsupported checkout shape: expected a candidate commit or a pull request merge commit")
    if git("rev-list", "--parents", "-n", "1", candidate).decode().split()[1:] != [main]:
        raise ValueError("Candidate commit must sit directly on the bound main commit")
    try:
        git("merge-base", "--is-ancestor", main, base)
    except subprocess.CalledProcessError:
        raise ValueError("Bound main commit must be the checkout base or one of its ancestors") from None
    return {"head_commit": head, "base_commit": base, "candidate_commit": candidate, "parents": len(parents) - 1}


def preflight(api):
    seed = github.oid(os.environ.get("DIAGNOSTIC_SEED_COMMIT"))
    main = github.oid(os.environ.get("DIAGNOSTIC_MAIN_COMMIT"))
    run_id = github.positive(os.environ.get("DIAGNOSTIC_SEED_RUN"))
    if os.environ["GITHUB_EVENT_NAME"] == "pull_request" and os.environ.get("GITHUB_BASE_REF") != "main":
        raise ValueError("A pull request diagnostic must target main to read its caches")
    run = api.request("GET", f"/repos/{github.REPOSITORY}/actions/runs/{run_id}")
    if run.get("event") != "push":
        raise ValueError("Seed must be a successful main push: manual and PR runs cannot save caches")
    quality = github.quality_evidence(api, seed, run_id)
    current = github.oid(api.request("GET", f"/repos/{github.REPOSITORY}/git/ref/heads/main").get("object", {}).get("sha"))
    compare = main_compare(main, current)
    if git("status", "--porcelain", "-z"):
        raise ValueError("Diagnostic checkout must start clean")
    try:
        git("merge-base", "--is-ancestor", seed, main)
    except subprocess.CalledProcessError:
        raise ValueError("Seed commit must be the bound main commit or one of its ancestors") from None
    shape = checkout_shape(main)
    candidate_changed = changed_paths(main, shape["candidate_commit"])
    if candidate_changed != OWNED:
        raise ValueError("Only the diagnostic workflow, helper and test may differ from the bound main commit")
    head_changed = changed_paths(shape["base_commit"], "HEAD")
    if head_changed != OWNED:
        raise ValueError("The checkout must differ from its base by exactly the diagnostic workflow, helper and test")
    identity = cache.source_identity(ROOT)
    if identity != tree_source_identity(main, "main"):
        raise ValueError("Cache build inputs differ between the checkout and the bound main commit")
    if identity != tree_source_identity(seed, "seed"):
        raise ValueError("Cache build inputs changed after the seed; the exact key cannot hit")
    return {"event": os.environ["GITHUB_EVENT_NAME"], "github_ref": os.environ.get("GITHUB_REF"),
            "base_ref": os.environ.get("GITHUB_BASE_REF"), "github_sha": os.environ.get("GITHUB_SHA"),
            "seed_commit": seed, "seed_run": quality, "main_commit": main,
            "api_main_commit": current, "main_compare": compare, "checkout": shape,
            "candidate_changed_paths": sorted(candidate_changed), "head_changed_paths": sorted(head_changed),
            "source_identity": identity, "fixture": FIXTURE, "fixture_sha256": sha(ROOT / FIXTURE),
            "production_sha256": {name: sha(ROOT / name) for name in PRODUCTION},
            "cache_prefix_key": prefix_key(ROOT / PRODUCTION[0]),
            "profile": "debug", "runner_os": os.environ["RUNNER_OS"],
            "runner_arch": os.environ.get("RUNNER_ARCH"),
            "image_os": os.environ.get("ImageOS"), "image_version": os.environ.get("ImageVersion"),
            "cache_write": False, "rust_build": False}


def inventory(paths):
    rows = cache.entries(paths)  # Production entry/type bounds, with no symlink traversal.
    target = paths[0]
    libraries = []
    for path, size in rows:
        if (path.is_relative_to(target) and "deps" in path.relative_to(target).parts
                and path.name.startswith("lib") and path.suffix == ".rlib" and path.is_file()):
            libraries.append((path, size - 4096))
    samples = []
    for path, size in sorted(libraries):
        if size > MAX_SAMPLE_BYTES:
            continue
        with path.open("rb") as stream:
            if stream.read(len(ARCHIVE_MAGIC)) != ARCHIVE_MAGIC:
                continue
        samples.append({"path": path.relative_to(target).as_posix(), "bytes": size, "sha256": sha(path)})
        if len(samples) == MAX_SAMPLES:
            break
    return {"accounted_bytes": sum(size for _, size in rows), "entries": len(rows),
            "roots_present": [path.exists() for path in paths],
            "dependency_rlib_count": len(libraries), "dependency_samples": samples}


def observe(directory, phase, api):
    root, temp, paths = cache.roots()
    if root != ROOT or os.environ.get("CI_RUST_CACHE_PROFILE") != "debug":
        raise ValueError("Unexpected cache root or profile")
    key = os.environ["CI_RUST_CACHE_KEY"]
    if not key.startswith("debug-"):
        raise ValueError("Production setup did not prepare a debug cache key")
    seed = json.loads((directory / "preflight.json").read_text(encoding="utf-8"))
    measurement = json.loads((temp / "ci-metrics/rust-cache-restore.json").read_text(encoding="utf-8"))
    prefix = f"{seed['cache_prefix_key']}-{key}"
    value = {"measurement": measurement, "source_identity": cache.source_identity(root),
             "key_prefix": key, "service_key_prefix": prefix,
             "service_entries": service_entries(api, prefix), "inventory": inventory(paths),
             "action_exact_hit": os.environ.get("DIAGNOSTIC_CACHE_HIT") == "true",
             "service_outcome": SERVICE_OUTCOME}
    # Save observations before evaluating expectations, including unmet seed prerequisites.
    save(directory, phase, value)
    if measurement["details"]["key_prefix"] != key:
        raise ValueError("Restore measurement does not match the prepared key")
    hit = measurement["cache"]["hit"]
    if value["action_exact_hit"] != hit:
        raise ValueError("Action output and production restore observation disagree")
    entries = value["service_entries"]
    if phase == "warm":
        if value["source_identity"] != seed["source_identity"]:
            raise ValueError("Warm restore changed source inputs")
        if not entries and not hit:
            raise ValueError("Seed prerequisite unmet: the cache service lists no main entry for the exact debug key")
        if entries and not hit:
            raise ValueError("Seed entry exists but the restore was not an exact hit: "
                             "inspect the hosted restore log for a service or extraction error")
        if not entries:
            raise ValueError("Exact hit without a main-scoped service entry; inspect the hosted restore log")
        if not value["inventory"]["dependency_samples"]:
            raise ValueError("Seed prerequisite unmet: the exact hit restored no compiled dependency "
                             "archive, so a downloads-only entry is not compiled reuse")
    else:
        warm = json.loads((directory / "warm.json").read_text(encoding="utf-8"))
        if value["source_identity"] == seed["source_identity"] or key == warm["key_prefix"]:
            raise ValueError("Vendor fixture failed to change the cache identity")
        if entries:
            raise ValueError("Changed key unexpectedly matches a main cache entry")
        if hit:
            raise ValueError("Changed identity reported an exact hit")
        if any(value["inventory"]["roots_present"]):
            raise ValueError("Production recovery left a stale target, registry or Git payload after the miss")
    return value


def mutate(directory):
    seed = json.loads((directory / "preflight.json").read_text(encoding="utf-8"))
    path = ROOT / FIXTURE
    if git("status", "--porcelain", "-z"):
        raise ValueError("Checkout changed before the controlled mutation")
    if sha(path) != seed["fixture_sha256"] or cache.source_identity(ROOT) != seed["source_identity"]:
        raise ValueError("Fixture or cache inputs changed before the controlled mutation")
    original = path.read_bytes()
    path.write_bytes(original + COMMENT)
    changed = cache.source_identity(ROOT)
    if changed == seed["source_identity"]:
        raise ValueError("Controlled vendor mutation did not invalidate the source identity")
    modified = set(filter(None, git("diff", "--name-only", "-z", "--no-renames", "HEAD").decode().split("\0")))
    if modified != {FIXTURE}:
        raise ValueError("Controlled mutation touched more than the fixed vendor fixture")
    return {"fixture": FIXTURE, "before_sha256": seed["fixture_sha256"], "after_sha256": sha(path),
            "source_identity_before": seed["source_identity"], "source_identity": changed,
            "appended_comment": COMMENT.decode(), "disposable_checkout_only": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=("preflight", "warm", "mutate", "invalidated"))
    args = parser.parse_args()
    directory = Path(os.environ["RUNNER_TEMP"]) / "cache-diagnostic"
    directory.mkdir(exist_ok=True)
    try:
        if (os.environ.get("GITHUB_EVENT_NAME") not in EVENTS
                or os.environ.get("GITHUB_REPOSITORY") != github.REPOSITORY
                or os.environ.get("RUNNER_OS") not in {"Linux", "macOS"}):
            raise ValueError("Only a source-repository pull request or manual diagnostic on the native Quality platforms is supported")
        if args.phase == "mutate":
            value = mutate(directory)
        else:
            api = github.GitHub(os.environ.get("GH_TOKEN", ""))
            value = preflight(api) if args.phase == "preflight" else observe(directory, args.phase, api)
        value["result"] = "passed"
        save(directory, args.phase, value)
        print(json.dumps({"phase": args.phase, "result": "passed"}))
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        save(directory, args.phase + "-failure", {"phase": args.phase, "result": "failed", "reason": str(error)})
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
