"""NEVER MERGE: behavior of the disposable cache invalidation diagnostic on real Git fixtures."""
from contextlib import redirect_stderr, redirect_stdout
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.parse

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("cache_diagnostic", ROOT / "scripts/ci/cache_diagnostic.py")
diagnostic = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(diagnostic)
cache = diagnostic.cache
github = diagnostic.github

GIT_ENV = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
           "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
           "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
           "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"}
ACTION = b"runs:\n  steps:\n    - with:\n        prefix-key: gitturtle-rust-v1\n    - with:\n        prefix-key: gitturtle-rust-v1\n"
REQUIRED = ["Quality gate", "Rust formatting", "Rust tests and Clippy · macos-15",
            "Rust tests and Clippy · ubuntu-24.04", "Rust release · macos-26", "Rust release · ubuntu-24.04"]
WARM_KEY = "debug-" + "a" * 64
CHANGED_KEY = "debug-" + "b" * 64


def entry(key, identifier=1):
    return {"id": identifier, "key": key, "ref": "refs/heads/main", "version": "v", "size_in_bytes": 1024,
            "created_at": "2026-09-16T12:45:10.112140Z", "last_accessed_at": "2026-09-16T13:18:16.187488Z"}


class Service:
    """Simulated read-only GitHub API: seed run, its jobs, the main ref and cache listings."""

    def __init__(self, seed, main):
        self.run = {"event": "push", "head_sha": seed, "head_branch": "main",
                    "path": ".github/workflows/quality.yml", "repository": {"full_name": github.REPOSITORY},
                    "head_repository": {"full_name": github.REPOSITORY}, "status": "completed",
                    "conclusion": "success", "run_attempt": 1}
        self.jobs = [dict(name=name, head_sha=seed, status="completed", conclusion="success") for name in REQUIRED]
        self.main = {"object": {"sha": main}}
        self.caches = []
        self.routes = []

    def request(self, method, route):
        assert method == "GET", method
        self.routes.append(route)
        if route == f"/repos/{github.REPOSITORY}/actions/runs/123":
            return copy.deepcopy(self.run)
        if route == f"/repos/{github.REPOSITORY}/git/ref/heads/main":
            return copy.deepcopy(self.main)
        raise AssertionError(route)

    def pages(self, route, key):
        self.routes.append(route)
        if route == f"/repos/{github.REPOSITORY}/actions/runs/123/attempts/1/jobs" and key == "jobs":
            return copy.deepcopy(self.jobs)
        if route.startswith(f"/repos/{github.REPOSITORY}/actions/caches?") and key == "actions_caches":
            query = urllib.parse.parse_qs(route.split("?", 1)[1], strict_parsing=True)
            assert query["ref"] == ["refs/heads/main"], query
            return [copy.deepcopy(row) for row in self.caches if row["key"].startswith(query["key"][0])]
        raise AssertionError(route)


class Response:
    def __init__(self, value):
        self.raw = json.dumps(value).encode()

    def read(self, limit):
        return self.raw[:limit]

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


class Opener:
    """Serve the simulated service through the real production transport's URL opener."""

    def __init__(self, service):
        self.service = service
        self.urls = []

    def open(self, request, timeout=None):
        self.urls.append(request.full_url)
        assert request.get_method() == "GET" and timeout == 30, (request.get_method(), timeout)
        assert request.get_header("Authorization") == "Bearer fixture-not-a-token"
        route, _, query = request.full_url.removeprefix("https://api.github.com").partition("?")
        if route == f"/repos/{github.REPOSITORY}/actions/runs/123/attempts/1/jobs":
            return Response({"jobs": self.service.pages(route, "jobs")})
        if route == f"/repos/{github.REPOSITORY}/actions/caches":
            listing = self.service.pages(f"{route}?{query.replace('&per_page=100&page=1', '')}", "actions_caches")
            return Response({"actions_caches": listing})
        return Response(self.service.request("GET", route))


class DiagnosticFixture(unittest.TestCase):
    def setUp(self):
        (ROOT / ".local").mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="cache-diagnostic-test-", dir=ROOT / ".local")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.runner = self.base / "runner"
        self.report = self.runner / "cache-diagnostic"
        self.report.mkdir(parents=True)
        self.cargo = self.runner / "gitturtle-cargo"
        self.cargo.mkdir()
        self.root = self.base / "repo"
        self.paths = (self.root / "target", self.cargo / "registry", self.cargo / "git")
        env = {**GIT_ENV, "GITHUB_WORKSPACE": str(self.root), "RUNNER_TEMP": str(self.runner),
               "CARGO_HOME": str(self.cargo), "CARGO_TARGET_DIR": str(self.paths[0]),
               "GITHUB_EVENT_NAME": "pull_request", "GITHUB_BASE_REF": "main",
               "GITHUB_REPOSITORY": github.REPOSITORY, "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64",
               "GH_TOKEN": "fixture-not-a-token", "DIAGNOSTIC_SEED_RUN": "123", "CI_RUST_CACHE_PROFILE": "debug"}
        patcher = patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        root_patch = patch.object(diagnostic, "ROOT", self.root)
        root_patch.start()
        self.addCleanup(root_patch.stop)
        self.build()

    def build(self, main_files=None):
        """Seed commit, then current main, then the diagnostic head with only its owned files."""
        if self.root.exists():
            shutil.rmtree(self.root)
        self.root.mkdir()
        self.git("init", "-q")
        self.seed = self.commit({
            "Cargo.toml": b"[workspace]\n", "Cargo.lock": b"locked\n", "rust-toolchain.toml": b"pinned\n",
            diagnostic.FIXTURE: b"fixture vendor source\n", ".gitignore": b"/target/\n__pycache__/\n*.py[cod]\n",
            ".github/actions/setup-rust/action.yml": ACTION,
            ".github/actions/setup-rust/cache.py": b"# fixture helper\n",
            ".github/workflows/quality.yml": b"name: Quality\n", "crates/app/src/main.rs": b"fn main() {}\n",
            "scripts/ci/metrics.py": (ROOT / "scripts/ci/metrics.py").read_bytes()})
        self.main = self.commit(main_files or {".gitignore": b"/target/\n__pycache__/\n*.py[cod]\n/.hypervisor/\n"})
        self.head = self.commit({".github/workflows/quality.yml": b"name: Quality\n# NEVER MERGE diagnostic\n",
                                 "scripts/ci/cache_diagnostic.py": b"# fixture\n",
                                 "scripts/ci/tests/test_cache_diagnostic.py": b"# fixture\n"})
        os.environ["DIAGNOSTIC_SEED_COMMIT"] = self.seed
        os.environ["DIAGNOSTIC_MAIN_COMMIT"] = self.main
        self.service = Service(self.seed, self.main)

    def git(self, *arguments):
        return subprocess.run(["git", "-C", str(self.root), *arguments], check=True, capture_output=True).stdout

    def commit(self, files):
        """Write the given bytes, or delete a path given as None, then commit everything."""
        for name, content in files.items():
            path = self.root / name
            if content is None:
                path.unlink()
                continue
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        self.git("add", "--all")
        self.git("commit", "-q", "-m", "fixture")
        return self.git("rev-parse", "HEAD").decode().strip()

    def commit_on(self, base, files):
        """Commit on top of another commit and return to the previous checkout."""
        previous = self.git("rev-parse", "--abbrev-ref", "HEAD").decode().strip()
        previous = self.git("rev-parse", "HEAD").decode().strip() if previous == "HEAD" else previous
        self.git("checkout", "-q", "--detach", base)
        created = self.commit(files)
        self.git("checkout", "-q", previous)
        return created

    def seed_payload(self):
        library = self.paths[0] / "debug/deps/libfixture-abc.rlib"
        library.parent.mkdir(parents=True, exist_ok=True)
        library.write_bytes(diagnostic.ARCHIVE_MAGIC + b"fixture-only")
        for path in self.paths[1:]:
            path.mkdir(parents=True, exist_ok=True)
            (path / "fixture").write_bytes(b"download fixture")
        return library

    def restore(self, key, hit):
        """Run the production restore consumer exactly as the setup action does after rust-cache."""
        (self.runner / "gitturtle-cache-start").write_text("1")
        os.environ.update({"CI_RUST_CACHE_KEY": key, "CACHE_HIT": str(hit).lower(), "CACHE_OUTCOME": "success",
                           "CI_RUST_CACHE_LOCK": cache.digest((self.root / "Cargo.lock").read_bytes()),
                           "DIAGNOSTIC_CACHE_HIT": str(hit).lower()})
        cache.restored()

    def measurement(self, **changes):
        path = self.runner / "ci-metrics/rust-cache-restore.json"
        value = json.loads(path.read_text())
        for name, replacement in changes.items():
            value["details" if name == "key_prefix" else "cache"][name] = replacement
        path.write_text(json.dumps(value))

    def refuses(self, expected, call, *arguments):
        with self.assertRaises(ValueError) as caught:
            call(*arguments)
        self.assertIn(expected, str(caught.exception))

    def warm(self):
        seed = diagnostic.preflight(self.service)
        diagnostic.save(self.report, "preflight", seed)
        self.seed_payload()
        self.service.caches = [entry(f"gitturtle-rust-v1-{WARM_KEY}-Linux-x64-93796ee2-1f85ec39")]
        self.restore(WARM_KEY, True)
        return seed, diagnostic.observe(self.report, "warm", self.service)

    def test_preflight_binds_trusted_seed_to_current_main_with_equal_cache_inputs(self):
        value = diagnostic.preflight(self.service)
        self.assertEqual((value["seed_commit"], value["main_commit"], value["checkout"]["head_commit"]),
                         (self.seed, self.main, self.head))
        self.assertEqual(value["source_identity"], cache.source_identity(self.root))
        self.assertEqual(value["seed_run"]["required_jobs"], sorted(REQUIRED))
        self.assertEqual(value["cache_prefix_key"], "gitturtle-rust-v1")
        self.assertEqual(value["api_main_commit"], self.main)
        self.assertEqual(value["main_compare"], {"status": "identical", "ahead_by": 0, "behind_by": 0,
                                                 "total_commits": 0, "files": []})
        self.assertEqual(value["checkout"], {"head_commit": self.head, "base_commit": self.main,
                                             "candidate_commit": self.head, "parents": 1})
        self.assertEqual(value["candidate_changed_paths"], sorted(diagnostic.OWNED))
        self.assertEqual(value["head_changed_paths"], sorted(diagnostic.OWNED))
        self.assertEqual(value["fixture_sha256"], diagnostic.sha(self.root / diagnostic.FIXTURE))
        self.assertFalse(value["cache_write"] or value["rust_build"])
        self.assertFalse(any(path.name.startswith("cache-diagnostic-") for path in self.runner.iterdir()))
        self.assertEqual(self.git("worktree", "list", "--porcelain").count(b"worktree "), 1)
        self.assertEqual(self.git("status", "--porcelain"), b"")

    def test_preflight_refuses_unsaveable_or_stale_seed_bindings(self):
        self.service.run["event"] = "workflow_dispatch"
        self.refuses("cannot save caches", diagnostic.preflight, self.service)
        self.service.run["event"] = "push"
        release = next(job for job in self.service.jobs if job["name"] == "Rust release · macos-26")
        release["conclusion"] = "skipped"
        self.refuses("Rust release · macos-26", diagnostic.preflight, self.service)
        release["conclusion"] = "success"
        self.service.main["object"]["sha"] = self.seed
        self.refuses("neither current main nor one of its ancestors", diagnostic.preflight, self.service)
        self.service.main["object"]["sha"] = self.commit_on(self.seed, {"docs/diverged.md": b"diverged\n"})
        self.refuses("neither current main nor one of its ancestors", diagnostic.preflight, self.service)
        self.service.main["object"]["sha"] = "c" * 40
        self.refuses("not present in the checkout", diagnostic.preflight, self.service)
        self.service.main["object"]["sha"] = self.main
        with patch.dict(os.environ, {"GITHUB_BASE_REF": "feature"}):
            self.refuses("must target main", diagnostic.preflight, self.service)
        diagnostic.preflight(self.service)

    def test_preflight_refuses_source_drift_from_the_seed(self):
        (self.root / "Cargo.lock").write_bytes(b"dirty\n")
        self.refuses("start clean", diagnostic.preflight, self.service)
        self.git("checkout", "--", "Cargo.lock")
        self.git("reset", "-q", "--soft", self.main)
        extra = self.commit({"docs/note.md": b"extra\n"})
        self.assertNotEqual(extra, self.head)
        self.refuses("Only the diagnostic workflow, helper and test", diagnostic.preflight, self.service)
        self.git("reset", "-q", "--hard", self.head)
        self.commit({".github/workflows/quality.yml": b"name: Quality\n# second candidate commit\n"})
        self.refuses("directly on the bound main commit", diagnostic.preflight, self.service)
        self.git("reset", "-q", "--hard", self.head)
        unrelated = self.git("commit-tree", "-m", "unrelated", self.git("rev-parse", "HEAD^{tree}").decode().strip()).decode().strip()
        self.service.run["head_sha"] = unrelated
        for job in self.service.jobs:
            job["head_sha"] = unrelated
        with patch.dict(os.environ, {"DIAGNOSTIC_SEED_COMMIT": unrelated}):
            self.refuses("one of its ancestors", diagnostic.preflight, self.service)
        self.build(main_files={"Cargo.lock": b"relocked\n"})
        self.refuses("Cache build inputs changed after the seed", diagnostic.preflight, self.service)
        self.assertFalse(any(path.name.startswith("cache-diagnostic-") for path in self.runner.iterdir()))

    def test_preflight_tolerates_main_moving_past_the_bound_commit_and_merge_checkouts(self):
        moved = self.commit_on(self.main, {"docs/moved.md": b"moved\n"})
        self.service.main["object"]["sha"] = moved
        value = diagnostic.preflight(self.service)
        self.assertEqual((value["main_commit"], value["api_main_commit"]), (self.main, moved))
        self.assertEqual(value["main_compare"], {"status": "ahead", "ahead_by": 1, "behind_by": 0, "total_commits": 1,
                                                 "files": [{"filename": "docs/moved.md", "status": "added"}]})
        self.assertEqual(value["checkout"]["parents"], 1)
        # A pull request run checks out the candidate merged into the newer main.
        self.git("checkout", "-q", "--detach", moved)
        self.git("merge", "-q", "--no-ff", "--no-edit", self.head)
        merge = self.git("rev-parse", "HEAD").decode().strip()
        self.service.main["object"]["sha"] = self.commit_on(moved, {"docs/later.md": b"later\n"})
        value = diagnostic.preflight(self.service)
        self.assertEqual(value["checkout"], {"head_commit": merge, "base_commit": moved,
                                             "candidate_commit": self.head, "parents": 2})
        self.assertEqual(value["main_compare"]["ahead_by"], 2)
        self.assertEqual(value["head_changed_paths"], sorted(diagnostic.OWNED))
        self.assertEqual(value["source_identity"], cache.source_identity(self.root))
        with patch.dict(os.environ, {"DIAGNOSTIC_MAIN_COMMIT": self.seed}):
            self.refuses("directly on the bound main commit", diagnostic.preflight, self.service)
        # A newer main that changed a cache input cannot hide behind the bound identity.
        relocked = self.commit_on(moved, {"Cargo.lock": b"relocked\n"})
        self.git("checkout", "-q", "--detach", relocked)
        self.git("merge", "-q", "--no-ff", "--no-edit", self.head)
        self.service.main["object"]["sha"] = relocked
        self.refuses("differ between the checkout and the bound main commit", diagnostic.preflight, self.service)
        self.assertEqual(self.git("worktree", "list", "--porcelain").count(b"worktree "), 1)

    def test_main_comparison_is_computed_locally_from_the_checkout(self):
        self.assertEqual(diagnostic.main_compare(self.main, self.main),
                         {"status": "identical", "ahead_by": 0, "behind_by": 0, "total_commits": 0, "files": []})
        moved = self.commit_on(self.main, {"docs/moved.md": b"moved\n", ".gitignore": b"/target/\n"})
        later = self.commit_on(moved, {"docs/moved.md": b"", "crates/app/src/main.rs": None})
        self.assertEqual(diagnostic.main_compare(self.main, later),
                         {"status": "ahead", "ahead_by": 2, "behind_by": 0, "total_commits": 2,
                          "files": [{"filename": ".gitignore", "status": "modified"},
                                    {"filename": "crates/app/src/main.rs", "status": "removed"},
                                    {"filename": "docs/moved.md", "status": "added"}]})
        self.refuses("neither current main nor one of its ancestors", diagnostic.main_compare, later, self.main)
        diverged = self.commit_on(self.seed, {"docs/diverged.md": b"diverged\n"})
        self.refuses("neither current main nor one of its ancestors", diagnostic.main_compare, self.main, diverged)
        self.refuses("not present in the checkout", diagnostic.main_compare, self.main, "c" * 40)
        self.assertEqual(self.git("status", "--porcelain"), b"")

    def test_github_reads_pass_the_production_transport_route_guard(self):
        api = github.GitHub("fixture-not-a-token")
        with self.assertRaises(github.Error) as caught:
            api.request("GET", f"/repos/{github.REPOSITORY}/compare/{self.main}...{self.head}")
        self.assertIn("Invalid GitHub API route", str(caught.exception))
        opener = Opener(self.service)
        api.opener = opener
        self.service.main["object"]["sha"] = self.commit_on(self.main, {"docs/moved.md": b"moved\n"})
        seed = diagnostic.preflight(api)
        self.assertEqual(seed["main_compare"]["files"], [{"filename": "docs/moved.md", "status": "added"}])
        diagnostic.save(self.report, "preflight", seed)
        self.seed_payload()
        self.service.caches = [entry(f"gitturtle-rust-v1-{WARM_KEY}-Linux-x64-93796ee2-1f85ec39")]
        self.restore(WARM_KEY, True)
        warm = diagnostic.observe(self.report, "warm", api)
        self.assertEqual(len(warm["service_entries"]), 1)
        diagnostic.mutate(self.report)
        self.restore(CHANGED_KEY, False)
        value = diagnostic.observe(self.report, "invalidated", api)
        self.assertEqual(value["service_entries"], [])
        self.assertTrue(opener.urls and all(url.startswith("https://api.github.com/repos/") for url in opener.urls))
        self.assertFalse(any(".." in url for url in opener.urls))
        self.assertEqual(sorted({url.split("?")[0].rsplit("/", 1)[-1] for url in opener.urls}),
                         ["123", "caches", "jobs", "main"])

    def test_warm_observation_requires_exact_listed_hit_with_compiled_archives(self):
        seed, warm = self.warm()
        self.assertTrue(warm["action_exact_hit"] and warm["measurement"]["cache"]["hit"])
        self.assertEqual(warm["measurement"]["details"]["key_prefix"], WARM_KEY)
        self.assertEqual(warm["source_identity"], seed["source_identity"])
        self.assertEqual(warm["service_key_prefix"], f"gitturtle-rust-v1-{WARM_KEY}")
        self.assertEqual([row["key"] for row in warm["service_entries"]], [self.service.caches[0]["key"]])
        self.assertEqual(warm["inventory"]["roots_present"], [True, True, True])
        library = self.paths[0] / "debug/deps/libfixture-abc.rlib"
        self.assertEqual(warm["inventory"]["dependency_samples"],
                         [{"path": "debug/deps/libfixture-abc.rlib", "bytes": library.stat().st_size,
                           "sha256": diagnostic.sha(library)}])
        self.assertEqual(json.loads((self.report / "warm.json").read_text())["key_prefix"], WARM_KEY)

    def test_warm_observation_records_then_refuses_unmet_seed_prerequisites(self):
        self.warm()
        library = self.paths[0] / "debug/deps/libfixture-abc.rlib"
        library.unlink()
        self.refuses("downloads-only", diagnostic.observe, self.report, "warm", self.service)
        library.write_bytes(diagnostic.ARCHIVE_MAGIC + b"fixture-only")
        listed = self.service.caches
        self.service.caches = []
        self.refuses("Exact hit without a main-scoped service entry", diagnostic.observe, self.report, "warm", self.service)
        self.measurement(hit=False)
        self.refuses("disagree", diagnostic.observe, self.report, "warm", self.service)
        os.environ["DIAGNOSTIC_CACHE_HIT"] = "false"
        self.refuses("lists no main entry", diagnostic.observe, self.report, "warm", self.service)
        self.service.caches = listed
        self.refuses("service or extraction error", diagnostic.observe, self.report, "warm", self.service)
        self.assertFalse(json.loads((self.report / "warm.json").read_text())["measurement"]["cache"]["hit"])
        self.measurement(hit=True, key_prefix="debug-other")
        os.environ["DIAGNOSTIC_CACHE_HIT"] = "true"
        self.refuses("does not match the prepared key", diagnostic.observe, self.report, "warm", self.service)

    def test_mutation_changes_real_identity_and_invalidated_restore_leaves_no_payload(self):
        seed, warm = self.warm()
        mutation = diagnostic.mutate(self.report)
        fixture = self.root / diagnostic.FIXTURE
        self.assertTrue(fixture.read_bytes().endswith(diagnostic.COMMENT))
        self.assertEqual(mutation["after_sha256"], diagnostic.sha(fixture))
        self.assertNotEqual(mutation["source_identity"], seed["source_identity"])
        self.assertEqual(mutation["source_identity"], cache.source_identity(self.root))
        self.assertEqual(self.git("diff", "--name-only").decode().split(), [diagnostic.FIXTURE])
        self.refuses("before the controlled mutation", diagnostic.mutate, self.report)
        # A bypassed consumer would leave the warm payload behind a reported miss.
        self.measurement(hit=False, key_prefix=CHANGED_KEY)
        os.environ.update({"CI_RUST_CACHE_KEY": CHANGED_KEY, "DIAGNOSTIC_CACHE_HIT": "false"})
        self.refuses("stale target, registry or Git payload", diagnostic.observe, self.report, "invalidated", self.service)
        self.assertTrue(all(path.exists() for path in self.paths))
        self.restore(CHANGED_KEY, False)
        self.assertFalse(any(path.exists() for path in self.paths))
        value = diagnostic.observe(self.report, "invalidated", self.service)
        self.assertEqual(value["inventory"], {"accounted_bytes": 0, "entries": 0, "roots_present": [False] * 3,
                                              "dependency_rlib_count": 0, "dependency_samples": []})
        self.assertEqual((value["service_entries"], value["action_exact_hit"]), ([], False))
        self.assertNotEqual(value["key_prefix"], warm["key_prefix"])
        self.service.caches.append(entry(f"gitturtle-rust-v1-{CHANGED_KEY}-Linux-x64-93796ee2-1f85ec39", 2))
        self.refuses("unexpectedly matches", diagnostic.observe, self.report, "invalidated", self.service)
        self.service.caches.pop()
        self.measurement(hit=True)
        os.environ["DIAGNOSTIC_CACHE_HIT"] = "true"
        self.refuses("reported an exact hit", diagnostic.observe, self.report, "invalidated", self.service)
        self.measurement(hit=False, key_prefix=WARM_KEY)
        os.environ.update({"CI_RUST_CACHE_KEY": WARM_KEY, "DIAGNOSTIC_CACHE_HIT": "false"})
        self.refuses("failed to change the cache identity", diagnostic.observe, self.report, "invalidated", self.service)

    def test_samples_are_bounded_dependency_archives_only(self):
        deps = self.paths[0] / "debug/deps"
        deps.mkdir(parents=True)
        for name in ("libd", "libc", "libb", "liba"):
            (deps / f"{name}-1.rlib").write_bytes(diagnostic.ARCHIVE_MAGIC + name.encode())
        (deps / "libtext-1.rlib").write_bytes(b"not an archive")
        (deps / "product-1.rlib").write_bytes(diagnostic.ARCHIVE_MAGIC + b"no lib prefix")
        (self.paths[0] / "debug/libz-1.rlib").write_bytes(diagnostic.ARCHIVE_MAGIC + b"outside deps")
        with (deps / "liba-0.rlib").open("wb") as stream:
            stream.truncate(diagnostic.MAX_SAMPLE_BYTES + 1)
        with patch.object(diagnostic, "sha", side_effect=lambda path: path.name) as hashed:
            result = diagnostic.inventory(self.paths)
        self.assertEqual([row["path"] for row in result["dependency_samples"]],
                         ["debug/deps/liba-1.rlib", "debug/deps/libb-1.rlib", "debug/deps/libc-1.rlib"])
        self.assertEqual(hashed.call_count, 3)
        self.assertEqual(result["dependency_rlib_count"], 6)
        self.assertEqual(result["roots_present"], [True, False, False])

    def test_prefix_key_is_read_from_the_production_action(self):
        self.assertEqual(diagnostic.prefix_key(ROOT / ".github/actions/setup-rust/action.yml"), "gitturtle-rust-v1")
        action = self.root / ".github/actions/setup-rust/action.yml"
        action.write_bytes(ACTION.replace(b"gitturtle-rust-v1\n", b"other-prefix\n", 1))
        self.refuses("exactly one cache prefix key", diagnostic.prefix_key, action)
        action.write_bytes(b"runs:\n  steps: []\n")
        self.refuses("exactly one cache prefix key", diagnostic.prefix_key, action)

    def test_entry_point_refuses_untrusted_contexts_and_records_every_failure(self):
        for variant in ({"GITHUB_EVENT_NAME": "push"}, {"GITHUB_REPOSITORY": "someone/GitTurtle"}, {"RUNNER_OS": "Windows"}):
            with self.subTest(variant=variant), patch.dict(os.environ, variant), \
                    patch.object(sys, "argv", ["cache_diagnostic.py", "preflight"]), redirect_stderr(io.StringIO()), \
                    redirect_stdout(io.StringIO()):
                self.assertEqual(diagnostic.main(), 2)
            failure = json.loads((self.report / "preflight-failure.json").read_text())
            self.assertEqual((failure["phase"], failure["result"]), ("preflight", "failed"))
            (self.report / "preflight-failure.json").unlink()
        with patch.object(diagnostic.github, "GitHub", return_value=self.service), \
                patch.object(sys, "argv", ["cache_diagnostic.py", "preflight"]), redirect_stderr(io.StringIO()), \
                redirect_stdout(io.StringIO()) as printed:
            self.assertEqual(diagnostic.main(), 0)
        self.assertEqual(json.loads(printed.getvalue()), {"phase": "preflight", "result": "passed"})
        self.assertEqual(json.loads((self.report / "preflight.json").read_text())["result"], "passed")
        self.assertFalse((self.report / "preflight-failure.json").exists())
        with patch.object(diagnostic.github, "GitHub", return_value=self.service), \
                patch.object(sys, "argv", ["cache_diagnostic.py", "warm"]), redirect_stderr(io.StringIO()), \
                redirect_stdout(io.StringIO()):
            self.assertEqual(diagnostic.main(), 2)
        self.assertIn("reason", json.loads((self.report / "warm-failure.json").read_text()))


if __name__ == "__main__":
    unittest.main()
