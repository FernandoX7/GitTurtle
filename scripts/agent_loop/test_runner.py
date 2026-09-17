"""End-to-end controller tests using real disposable Git repositories."""

from __future__ import annotations

from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from agent_loop.git import clean, git, head
from agent_loop.process import LoopError, atomic_json, read_json
from agent_loop.runner import Runner, attest, create_run, locked, main, profiles_for, validate_patch
from agent_loop.task_spec import parse_spec
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


CONTROLLER = Path(__file__).resolve().parents[2]


def task(identifier="one", dependencies=(), profiles=("docs",)):
    return {
        "id": identifier, "title": f"Document {identifier}", "description": f"Explain the {identifier} workflow.",
        "depends_on": list(dependencies), "scope": [f"docs/{identifier}.md"],
        "acceptance": [{"id": "content", "description": "The guide explains the actual workflow."}],
        "profiles": list(profiles), "commit": f"docs: explain {identifier} workflow",
    }


class FakeCodex:
    output_tokens = 0

    def __init__(self, failures=0, mutate=None, blocked=False):
        self.failures = failures
        self.mutate = mutate
        self.blocked = blocked
        self.calls = []
        self.sessions = []
        self.output_tokens = 0

    def preflight(self):
        return "fixture Codex"

    def run(self, role, feature, repo, directory, timeout, stop, *, candidate=None, context="", **options):
        self.calls.append((role, feature.id))
        self.sessions.append((role, options))
        self.output_tokens += 10
        if role == "implementer":
            (repo / "docs").mkdir(exist_ok=True)
            (repo / "docs" / f"{feature.id}.md").write_text(f"# {feature.title}\n\n{feature.description}\n")
            return {"task_id": feature.id, "status": "ready", "summary": "Implemented guide."}
        if self.mutate:
            self.mutate(repo)
        verdict = "fail" if self.failures else "blocked" if self.blocked else "pass"
        self.failures = max(0, self.failures - 1)
        return {
            "task_id": feature.id, "candidate": candidate, "verdict": verdict,
            "criteria": [{"id": "content", "status": "fail" if verdict == "fail" else "unverified" if verdict == "blocked" else "pass", "evidence": "Inspected the actual guide."}],
            "findings": [] if verdict == "pass" else ["The required explanation is incomplete."],
        }


def green_gates(repo, profiles, directory):
    return {"passed": True, "checks": []}


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve() / "source"
        self.root.mkdir()
        self.gitconfig = self.root.parent / "gitconfig"
        self.gitconfig.write_text("[user]\n name = Loop Test\n email = loop@example.invalid\n[commit]\n gpgsign = false\n")
        self.environment = patch.dict(os.environ, {"GIT_CONFIG_GLOBAL": str(self.gitconfig), "GIT_CONFIG_NOSYSTEM": "1"})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        git(self.root, "init", "--quiet")
        (self.root / ".gitignore").write_text("/.local/\n")
        (self.root / "docs").mkdir()
        (self.root / "protected.txt").write_text("original\n")
        self.options = {"model": "gpt-6-astra", "effort": "high", "max_tasks": 10, "max_attempts": 2, "max_minutes": 5, "session_minutes": 1, "max_output_tokens": None}

    def prepare(self, tasks=None):
        tasks = tasks if tasks is not None else [task()]
        specification = self.root / "tasks.json"
        specification.write_text(json.dumps({"version": 1, "tasks": tasks}))
        git(self.root, "add", "--", ".gitignore", "protected.txt", "tasks.json")
        git(self.root, "commit", "--quiet", "-m", "chore: initialize fixture")
        self.original_head = head(self.root)
        return specification

    def create(self, tasks=None):
        specification = self.prepare(tasks)
        with patch("agent_loop.runner.Codex.preflight", return_value="fixture Codex"):
            return create_run(self.root, specification, CONTROLLER, self.options)

    def execute(self, directory, adapter=None, gate=green_gates):
        runner = Runner(directory, adapter=adapter or FakeCodex(), gate_runner=gate)
        with redirect_stdout(io.StringIO()):
            result = runner.execute()
        self.assertEqual(head(self.root), self.original_head)
        self.assertTrue(clean(self.root))
        return result

    def test_accepts_atomic_commits_in_dependency_order_without_source_mutation(self):
        directory = self.create([task("two", ["one"]), task("one")])
        adapter = FakeCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(state["phase"], "complete")
        self.assertEqual(adapter.calls, [("implementer", "one"), ("verifier", "one"), ("implementer", "two"), ("verifier", "two")])
        # Implementer sessions name the run's own queue so the edit hook can protect it.
        self.assertEqual([options.get("spec_path") for role, options in adapter.sessions if role == "implementer"],
                         ["tasks.json", "tasks.json"])
        self.assertEqual([options.get("spec_path") for role, options in adapter.sessions if role == "verifier"],
                         [None, None])
        self.assertEqual(git(directory / "accepted", "log", "-2", "--format=%s").splitlines(), ["docs: explain two workflow", "docs: explain one workflow"])
        self.assertEqual(git(directory / "accepted", "remote").strip(), "")
        self.assertEqual(state["output_tokens"], 40)

    def test_red_review_retries_with_failed_commit_preserved(self):
        directory = self.create()
        state = self.execute(directory, FakeCodex(failures=1))
        record = state["tasks"]["one"]
        self.assertEqual(record["attempts"], 2)
        self.assertEqual(record["status"], "accepted")
        first = directory / "attempts/one/1"
        self.assertTrue((first / "candidate.patch").is_file())
        self.assertNotEqual(head(first / "repo"), self.original_head)

    def test_dirty_preflight_never_changes_source_index_or_creates_run(self):
        spec = self.prepare()
        (self.root / "protected.txt").write_text("user's staged change\n")
        git(self.root, "add", "--", "protected.txt")
        before = git(self.root, "diff", "--cached", "--binary")
        with self.assertRaisesRegex(LoopError, "dirty"):
            create_run(self.root, spec, CONTROLLER, self.options)
        self.assertEqual(git(self.root, "diff", "--cached", "--binary"), before)
        self.assertEqual(head(self.root), self.original_head)
        self.assertFalse((self.root / ".local").exists())

    def test_cli_run_keeps_run_state_private_under_a_permissive_umask(self):
        specification = self.prepare()
        previous = os.umask(0o002)
        self.addCleanup(os.umask, previous)
        output = io.StringIO()
        with patch("agent_loop.runner.Codex.preflight", return_value="fixture Codex"), \
                patch.object(Runner, "execute", lambda runner: runner.state | {"phase": "complete"}), \
                redirect_stdout(output):
            code = main(["run", "--repo", str(self.root), "--tasks", str(specification), "--model", "gpt-6-astra",
                         "--effort", "high", "--max-tasks", "1", "--max-attempts", "1", "--max-minutes", "1"])
        self.assertEqual(code, 0, output.getvalue())
        directory = Path(output.getvalue().splitlines()[0].removeprefix("run: "))
        for path in (directory.parent, directory, directory / "controller", directory / "accepted",
                     directory / "tasks.json", directory / "state.json"):
            self.assertEqual(path.stat().st_mode & 0o077, 0, path)

    def test_hidden_index_change_is_rejected_before_commit(self):
        self.prepare()
        feature = parse_spec({"version": 1, "tasks": [task()]})[0]
        (self.root / "protected.txt").write_text("hidden staged content\n")
        git(self.root, "add", "--", "protected.txt")
        (self.root / "protected.txt").write_text("original\n")
        (self.root / "docs/one.md").write_text("allowed\n")
        with self.assertRaisesRegex(LoopError, "index"):
            validate_patch(self.root, feature, self.original_head)

    def test_native_evidence_blocks_dependents_but_allows_unrelated_task(self):
        directory = self.create([task("one", profiles=["native"]), task("two", ["one"]), task("three")])
        state = self.execute(directory)
        self.assertEqual(state["tasks"]["one"]["status"], "awaiting_evidence")
        self.assertEqual(state["tasks"]["one"]["attempts"], 1)
        self.assertEqual(state["tasks"]["two"]["attempts"], 0)
        self.assertEqual(state["tasks"]["three"]["status"], "accepted")

    def test_attestation_requires_exact_candidate_and_resume_verifies(self):
        directory = self.create([task(profiles=["native"])])
        state = self.execute(directory)
        evidence = self.root.parent / "native.md"
        evidence.write_text("Fixture native interaction report and build identity.\n")
        with self.assertRaisesRegex(LoopError, "pending candidate"):
            attest(directory, "one", "0" * 40, "native", evidence, "Checked the intended executable.")
        attest(directory, "one", state["tasks"]["one"]["candidate"], "native", evidence, "Checked the intended executable.")
        adapter = FakeCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(state["tasks"]["one"]["status"], "accepted")
        self.assertEqual(adapter.calls, [("verifier", "one")])
        self.assertEqual(state["tasks"]["one"]["attempts"], 1)

    def test_mutated_attested_evidence_cannot_pass(self):
        directory = self.create([task(profiles=["native"])])
        state = self.execute(directory)
        evidence = self.root.parent / "native.md"
        evidence.write_text("actual evidence\n")
        attest(directory, "one", state["tasks"]["one"]["candidate"], "native", evidence, "Checked.")
        updated = read_json(directory / "state.json")
        (directory / updated["tasks"]["one"]["attestations"]["native"]["artifact"]).write_text("replaced\n")
        with self.assertRaisesRegex(LoopError, "evidence changed"):
            self.execute(directory)

    def test_verifier_source_write_is_not_accepted(self):
        directory = self.create()
        adapter = FakeCodex(mutate=lambda repo: (repo / "docs/one.md").write_text("reviewer edit\n"))
        state = self.execute(directory, adapter)
        self.assertEqual(state["tasks"]["one"]["status"], "failed")
        self.assertEqual(state["accepted_head"], self.original_head)

    def test_baseline_failure_does_not_start_worker(self):
        directory = self.create()
        adapter = FakeCodex()
        state = self.execute(directory, adapter, lambda *_: {"passed": False, "checks": []})
        self.assertEqual(state["phase"], "baseline_failed")
        self.assertEqual(adapter.calls, [])

    def test_output_budget_stops_before_another_session(self):
        self.options["max_output_tokens"] = 10
        directory = self.create()
        adapter = FakeCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(state["phase"], "paused")
        self.assertEqual(adapter.calls, [("implementer", "one")])
        self.assertEqual(state["accepted_head"], self.original_head)

    def test_stop_marker_starts_no_worker(self):
        directory = self.create()
        (directory / "STOP").touch()
        adapter = FakeCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(state["phase"], "paused")
        self.assertEqual(adapter.calls, [])

    def test_uncertain_fast_forward_preserves_intent_and_reconciles_once(self):
        directory = self.create()
        real_git = git

        def uncertain_git(repo, *args):
            result = real_git(repo, *args)
            if args[0] == "merge":
                raise LoopError("simulated uncertain merge result")
            return result

        with patch("agent_loop.runner.git", side_effect=uncertain_git):
            with self.assertRaisesRegex(LoopError, "uncertain"):
                self.execute(directory)
        state = read_json(directory / "state.json")
        self.assertEqual(state["phase"], "accepting")
        self.assertNotEqual(head(directory / "accepted"), state["accepted_head"])
        adapter = FakeCodex()
        result = self.execute(directory, adapter)
        self.assertEqual(result["phase"], "complete")
        self.assertEqual(adapter.calls, [])
        self.assertEqual(git(directory / "accepted", "rev-list", "--count", self.original_head + "..HEAD").strip(), "1")

    def test_recovery_refuses_dirty_accepted_tree(self):
        directory = self.create()
        with patch.object(Runner, "finish_acceptance", side_effect=LoopError("interrupted acceptance")):
            with self.assertRaises(LoopError):
                self.execute(directory)
        (directory / "accepted/protected.txt").write_text("unexpected change\n")
        with self.assertRaisesRegex(LoopError, "unexpected changes"):
            self.execute(directory)
        self.assertEqual(read_json(directory / "state.json")["phase"], "accepting")

    def test_recovery_refuses_changed_gate_record(self):
        directory = self.create()
        with patch.object(Runner, "finish_acceptance", side_effect=LoopError("interrupted acceptance")):
            with self.assertRaises(LoopError):
                self.execute(directory)
        record = read_json(directory / "state.json")["tasks"]["one"]
        atomic_json(Path(record["directory"]) / "checks/gates.json", {"passed": False, "checks": []})
        with self.assertRaisesRegex(LoopError, "gate evidence changed"):
            self.execute(directory)

    def test_interrupted_build_is_preserved_and_retried(self):
        directory = self.create()
        runner = Runner(directory, adapter=FakeCodex(), gate_runner=green_gates)
        runner.state.update(phase="building", active={"task": "one"})
        runner.state["tasks"]["one"].update(status="building", attempts=1)
        runner.save()
        result = self.execute(directory)
        self.assertEqual(result["tasks"]["one"]["attempts"], 2)
        self.assertEqual(result["tasks"]["one"]["status"], "accepted")

    def test_changed_spec_or_controller_snapshot_refuses_resume(self):
        directory = self.create()
        (directory / "tasks.json").write_text('{"version":1,"tasks":[]}')
        with self.assertRaisesRegex(LoopError, "task snapshot changed"):
            Runner(directory, adapter=FakeCodex())

    def test_lock_rejects_second_controller(self):
        directory = self.create()
        with locked(directory):
            with self.assertRaisesRegex(LoopError, "another controller"):
                with locked(directory):
                    self.fail("acquired duplicate lock")

    def test_actual_code_paths_promote_validation_profiles(self):
        feature = parse_spec({"version": 1, "tasks": [task()]})[0]
        self.assertIn("rust", profiles_for(feature, ["crates/git-core/src/lib.rs"]))
        self.assertIn("tooling", profiles_for(feature, ["scripts/example.py"]))
        self.assertIn("vendor", profiles_for(feature, ["vendor/example/Cargo.toml"]))
        self.assertIn("package", profiles_for(feature, ["assets/app-icon.png"]))

    def test_app_rust_requires_native_evidence_without_the_two_revisions(self):
        feature = parse_spec({"version": 1, "tasks": [task()]})[0]
        self.assertIn("native", profiles_for(feature, ["crates/app/src/views.rs"]))
        self.assertIn("native", profiles_for(feature, ["vendor/gpui/src/window.rs"]))

    def test_app_rust_that_adds_no_rendering_code_skips_native_evidence(self):
        feature = parse_spec({"version": 1, "tasks": [task()]})[0]
        table = "pub mod solarized {\n    pub const BASE03: u32 = 0x002b36;\n}\n"
        sources = {"crates/app/src/appearance/sources.rs": ("", table)}
        profiles = profiles_for(feature, list(sources), sources.__getitem__)
        self.assertIn("rust", profiles)
        self.assertNotIn("native", profiles)

    def test_a_rendering_change_beside_an_inert_one_still_requires_native_evidence(self):
        feature = parse_spec({"version": 1, "tasks": [task()]})[0]
        sources = {
            "crates/app/src/appearance/sources.rs": ("", "pub const BASE03: u32 = 0x002b36;\n"),
            "crates/app/src/views.rs": ('fn header() { div().child("History"); }\n',
                                        'fn header() { div().child("Commits"); }\n'),
        }
        self.assertIn("native", profiles_for(feature, sorted(sources), sources.__getitem__))


if __name__ == "__main__":
    unittest.main()
