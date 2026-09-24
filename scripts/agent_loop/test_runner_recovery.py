"""Regression coverage for recovery, capability pauses and evidence boundaries."""

from contextlib import redirect_stdout
from datetime import datetime, timedelta, timezone
import io
import json
from pathlib import Path
import time
import unittest
from unittest.mock import patch

from agent_loop import test_runner as fixtures
from agent_loop.git import head, write_limits
from agent_loop.process import EnvironmentBlocked, LoopError, atomic_json, read_json
from agent_loop.runner import Runner, attest, main, profiles_for, validate_patch
from agent_loop.task_spec import parse_spec
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


class UnavailableCodex(fixtures.FakeCodex):
    def __init__(self, role):
        super().__init__()
        self.unavailable_role = role

    def run(self, role, feature, *args, **kwargs):
        if role == self.unavailable_role and feature.id == "one":
            self.calls.append((role, feature.id))
            raise EnvironmentBlocked("fixture provider unavailable; restore the session capability")
        return super().run(role, feature, *args, **kwargs)


class RecoveryTests(unittest.TestCase):
    # Reuse real-Git fixtures without inheriting and rerunning the original suite.
    setUp = fixtures.RunnerTests.setUp
    prepare = fixtures.RunnerTests.prepare
    create = fixtures.RunnerTests.create
    execute = fixtures.RunnerTests.execute

    def crash_record(self, directory, *, age=30, remaining=300, tokens=0):
        state = read_json(directory / "state.json")
        state.update(
            phase="building", active={"task": "one"}, budget_running=True,
            baseline_passed=True, remaining_seconds=remaining, output_tokens=tokens,
            updated_at=(datetime.now(timezone.utc) - timedelta(seconds=age)).isoformat(),
        )
        state["tasks"]["one"].update(status="building", attempts=1)
        atomic_json(directory / "state.json", state)
        return state

    def session_log(self, directory, relative, events):
        path = directory / "attempts" / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("stderr diagnostic\n" + "\n".join(json.dumps(event) for event in events) + "\n")
        return path

    def resume_cli(self, directory, *options):
        adapter = fixtures.FakeCodex()
        original_runner = Runner
        with patch("agent_loop.runner.Runner", side_effect=lambda path: original_runner(
            path, adapter=adapter, gate_runner=fixtures.green_gates,
        )), redirect_stdout(io.StringIO()):
            code = main(["resume", "--run", str(directory), *options])
        return code, adapter

    def test_environment_block_does_not_retry_build_or_start_dependents(self):
        directory = self.create([
            fixtures.task(), fixtures.task("dependent", ["one"]), fixtures.task("independent"),
        ])
        adapter = UnavailableCodex("implementer")
        state = self.execute(directory, adapter)
        self.assertEqual(adapter.calls.count(("implementer", "one")), 1)
        self.assertEqual(state["tasks"]["one"]["status"], "blocked")
        self.assertEqual(state["tasks"]["one"]["attempts"], 1)
        self.assertNotIn("candidate", state["tasks"]["one"])
        self.assertEqual(state["tasks"]["dependent"]["attempts"], 0)
        self.assertEqual(state["tasks"]["independent"]["status"], "accepted")

    def test_gate_capability_failure_preserves_one_candidate_without_build_retry(self):
        directory = self.create()
        calls = []

        def unavailable_gate(repo, profiles, evidence_directory):
            calls.append(repo)
            if repo != directory / "accepted":
                raise EnvironmentBlocked("fixture gate deadline exceeded")
            return fixtures.green_gates(repo, profiles, evidence_directory)

        adapter = fixtures.FakeCodex()
        state = self.execute(directory, adapter, unavailable_gate)
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "blocked")
        self.assertEqual(record["attempts"], 1)
        self.assertEqual(adapter.calls, [("implementer", "one")])
        self.assertEqual(len(calls), 2)
        self.assertEqual(head(Path(record["directory"]) / "repo"), record["candidate"])
        self.assertEqual(state["accepted_head"], self.original_head)

    def test_review_capability_failure_resumes_same_candidate_without_rebuild(self):
        directory = self.create()
        unavailable = UnavailableCodex("verifier")
        state = self.execute(directory, unavailable)
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "review_blocked")
        self.assertEqual(record["attempts"], 1)
        candidate = record["candidate"]
        recovered = fixtures.FakeCodex()
        final = self.execute(directory, recovered)
        self.assertEqual(recovered.calls, [("verifier", "one")])
        self.assertEqual(final["tasks"]["one"]["candidate"], candidate)
        self.assertEqual(final["tasks"]["one"]["status"], "accepted")
        self.assertEqual(final["tasks"]["one"]["attempts"], 1)

    def test_crash_during_review_reuses_candidate_and_successful_gates(self):
        directory = self.create()
        state = self.execute(directory, fixtures.FakeCodex(blocked=True))
        candidate = state["tasks"]["one"]["candidate"]
        gate_hash = state["tasks"]["one"]["gate_sha256"]
        state.update(phase="verifying", active={"task": "one"}, budget_running=True)
        atomic_json(directory / "state.json", state)

        def unexpected_gate(*_):
            self.fail("unchanged candidate's successful gates were rerun")

        recovered = fixtures.FakeCodex()
        final = self.execute(directory, recovered, unexpected_gate)
        self.assertEqual(recovered.calls, [("verifier", "one")])
        self.assertEqual(final["accepted_head"], candidate)
        self.assertEqual(final["tasks"]["one"]["gate_sha256"], gate_hash)
        self.assertEqual(final["tasks"]["one"]["attempts"], 1)

    def test_unavailable_review_after_attestation_keeps_evidence_and_candidate(self):
        directory = self.create([fixtures.task(profiles=["native"])])
        state = self.execute(directory)
        candidate = state["tasks"]["one"]["candidate"]
        evidence = self.root.parent / "native-report.md"
        evidence.write_text("Observed candidate-bound native fixture results.\n")
        attest(directory, "one", candidate, "native", evidence, "Observed the identified build.")
        unavailable = UnavailableCodex("verifier")
        paused = self.execute(directory, unavailable)
        record = paused["tasks"]["one"]
        self.assertEqual(record["status"], "review_blocked")
        self.assertEqual(record["attempts"], 1)
        self.assertEqual(unavailable.calls, [("verifier", "one")])
        self.assertIn("native", record["attestations"])
        recovered = fixtures.FakeCodex()
        accepted = self.execute(directory, recovered)
        self.assertEqual(recovered.calls, [("verifier", "one")])
        self.assertEqual(accepted["tasks"]["one"]["candidate"], candidate)
        self.assertEqual(accepted["tasks"]["one"]["attempts"], 1)

    def test_changed_paths_enforce_native_and_package_evidence_profiles(self):
        feature = parse_spec({"version": 1, "tasks": [fixtures.task()]})[0]
        for path in ("crates/app/src/workspace_view.rs", "vendor/gpui/src/window.rs"):
            with self.subTest(path=path):
                self.assertIn("native", profiles_for(feature, [path]))
        for path in (
            "assets/AppIcon.icon/icon.json", "scripts/package-macos.sh", "scripts/render-app-icon.sh",
            "scripts/package-linux.sh", "scripts/install-linux.py", "scripts/collect-third-party-licenses.py",
            "scripts/package-macos.py", "scripts/package-identity.py",
            "scripts/release/identity.py", "scripts/release/sign-macos.py", "scripts/release/publish.sh",
        ):
            with self.subTest(path=path):
                self.assertIn("package", profiles_for(feature, [path]))
        self.assertNotIn("native", profiles_for(feature, ["crates/git-core/src/status.rs"]))
        for path in ("docs/one.md", "scripts/release/test_release_identity.py", "scripts/release/tests/fixture.py", "scripts/release/schema.json"):
            with self.subTest(path=path):
                self.assertNotIn("package", profiles_for(feature, [path]))

    def test_all_agents_policies_remain_protected_under_broad_scope(self):
        self.prepare()
        contract = fixtures.task()
        contract["scope"] = ["**"]
        feature = parse_spec({"version": 1, "tasks": [contract]})[0]
        for relative in ("AGENTS.md", "crates/app/AGENTS.md", "scripts/helpers/AGENTS.md", "docs/reference/AGENTS.md"):
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Changed policy.\n")
            try:
                with self.subTest(path=relative), self.assertRaisesRegex(LoopError, "protected"):
                    validate_patch(self.root, feature, self.original_head)
            finally:
                path.unlink()

    def test_removing_or_replacing_a_stored_symlink_requires_interactive_review(self):
        self.prepare()
        link = self.root / "docs/link.md"
        link.symlink_to("../protected.txt")
        fixtures.git(self.root, "add", "--", "docs/link.md")
        fixtures.git(self.root, "commit", "--quiet", "-m", "test: add stored symlink fixture")
        base = head(self.root)
        contract = fixtures.task()
        contract["scope"] = ["docs/link.md"]
        feature = parse_spec({"version": 1, "tasks": [contract]})[0]
        link.unlink()
        with self.assertRaisesRegex(LoopError, "stored symlink"):
            validate_patch(self.root, feature, base)
        link.write_text("replacement regular file\n")
        with self.assertRaisesRegex(LoopError, "stored symlink"):
            validate_patch(self.root, feature, base)
        self.assertEqual((self.root / "protected.txt").read_text(), "original\n")

    def test_custom_task_contract_cannot_be_changed_by_its_worker(self):
        contract = fixtures.task()
        contract["scope"] = ["**"]
        directory = self.create([contract])  # Fixture intake lives at tasks.json, outside the default path.

        class ContractEditor(fixtures.FakeCodex):
            def run(self, role, feature, repo, *args, **kwargs):
                result = super().run(role, feature, repo, *args, **kwargs)
                if role == "implementer":
                    (repo / "tasks.json").write_text('{"version":1,"tasks":[]}\n')
                return result

        adapter = ContractEditor()
        state = self.execute(directory, adapter)
        self.assertEqual(state["accepted_head"], self.original_head)
        self.assertNotIn(("verifier", "one"), adapter.calls)
        self.assertIn("task contract", state["tasks"]["one"]["reason"])
        self.assertNotIn("candidate", state["tasks"]["one"])

    def test_attestation_refuses_candidate_after_an_independent_acceptance_advances_base(self):
        directory = self.create([fixtures.task(profiles=["native"]), fixtures.task("independent")])
        state = self.execute(directory)
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "awaiting_evidence")
        self.assertNotEqual(record["base"], state["accepted_head"])
        evidence = self.root.parent / "stale-native.md"
        evidence.write_text("Native report for an older candidate.\n")
        before = (directory / "state.json").read_bytes()
        with self.assertRaisesRegex(LoopError, "stale"):
            attest(directory, "one", record["candidate"], "native", evidence, "Older native build.")
        self.assertEqual((directory / "state.json").read_bytes(), before)
        self.assertFalse((directory / "attestations").exists())

    def test_crash_downtime_exhausts_time_until_explicit_renewal(self):
        directory = self.create()
        self.crash_record(directory, age=120, remaining=30)
        runner = Runner(directory, adapter=fixtures.FakeCodex(), gate_runner=fixtures.green_gates)
        self.assertEqual(runner.initial_seconds, 0)
        code, paused_adapter = self.resume_cli(directory)
        self.assertEqual(code, 2)
        self.assertEqual(paused_adapter.calls, [])
        self.assertEqual(read_json(directory / "state.json")["phase"], "paused")
        code, renewed_adapter = self.resume_cli(directory, "--max-minutes", "5")
        self.assertEqual(code, 0)
        self.assertEqual(renewed_adapter.calls, [("implementer", "one"), ("verifier", "one")])

    def test_orderly_pause_does_not_consume_budget_while_stopped(self):
        directory = self.create()
        state = self.crash_record(directory, age=86400, remaining=200)
        state.update(budget_running=False, phase="paused", active=None)
        atomic_json(directory / "state.json", state)
        runner = Runner(directory, adapter=fixtures.FakeCodex())
        self.assertEqual(runner.initial_seconds, 200)

    def test_replays_reported_output_usage_without_double_counting_saved_usage(self):
        directory = self.create()
        self.crash_record(directory, tokens=20)
        self.session_log(directory, "one/1/implementer.jsonl", [
            {"type": "turn.completed", "usage": {"output_tokens": 37}},
        ])
        self.session_log(directory, "one/1/review-fixture/verifier.jsonl", [
            {"type": "turn.completed", "usage": {"output_tokens": 13}},
        ])
        runner = Runner(directory, adapter=fixtures.FakeCodex())
        self.assertEqual(runner.initial_tokens, 50)
        self.assertFalse(runner.state["output_usage_incomplete"])
        runner.save()
        recovered_again = Runner(directory, adapter=fixtures.FakeCodex())
        self.assertEqual(recovered_again.initial_tokens, 50)

    def test_unknown_output_usage_requires_explicit_cap_renewal_before_another_session(self):
        self.options["max_output_tokens"] = 100
        directory = self.create()
        self.crash_record(directory)
        self.session_log(directory, "one/1/implementer.jsonl", [
            {"type": "turn.started"}, {"type": "error", "message": "fixture interrupted reply"},
        ])
        code, adapter = self.resume_cli(directory)
        self.assertEqual(code, 2)
        self.assertEqual(adapter.calls, [])
        state = read_json(directory / "state.json")
        self.assertTrue(state["output_usage_incomplete"])
        self.assertIn("renew", state["reason"])
        code, renewed = self.resume_cli(directory, "--max-output-tokens", "150")
        self.assertEqual(code, 0)
        self.assertEqual(renewed.calls, [("implementer", "one"), ("verifier", "one")])
        self.assertFalse(read_json(directory / "state.json")["output_usage_incomplete"])

    def test_completed_event_without_usable_usage_remains_unknown_under_a_cap(self):
        self.options["max_output_tokens"] = 100
        directory = self.create()
        for usage in ({}, {"output_tokens": "37"}, {"output_tokens": -1}, {"output_tokens": True}):
            with self.subTest(usage=usage):
                self.crash_record(directory)
                self.session_log(directory, "one/1/implementer.jsonl", [
                    {"type": "turn.completed", "usage": usage},
                ])
                runner = Runner(directory, adapter=fixtures.FakeCodex())
                self.assertTrue(runner.state["output_usage_incomplete"])
                self.assertIn("renew", runner.budget_stop())

    def assert_slow_hook_interrupted(self, mode):
        self.prepare()
        (self.root / "protected.txt").write_text("candidate content\n")
        fixtures.git(self.root, "add", "--", "protected.txt")
        hook = self.root / ".git/hooks/pre-commit"
        hook.write_text(
            "#!/bin/sh\n"
            "printf 'started\\n' > .git/hook-started\n"
            "sleep 30\n"
            "printf 'finished\\n' > .git/hook-finished\n"
        )
        hook.chmod(0o700)
        marker = self.root / ".git/hook-started"
        started = time.monotonic()
        with write_limits(
            lambda: 1.0 if mode == "deadline" else 10.0,
            lambda: mode == "stop" and marker.exists(),
        ), self.assertRaisesRegex(EnvironmentBlocked, "interrupted"):
            fixtures.git(self.root, "commit", "--quiet", "-m", "docs: candidate with slow hook")
        self.assertLess(time.monotonic() - started, 5)
        self.assertTrue(marker.is_file(), "the hook must actually have started")
        self.assertFalse((self.root / ".git/hook-finished").exists())
        self.assertEqual(head(self.root), self.original_head)
        self.assertEqual(fixtures.git(self.root, "diff", "--cached", "--name-only"), "protected.txt\n")

    def test_slow_commit_hook_obeys_remaining_deadline(self):
        self.assert_slow_hook_interrupted("deadline")

    def test_slow_commit_hook_obeys_stop_request(self):
        self.assert_slow_hook_interrupted("stop")


if __name__ == "__main__":
    unittest.main()
