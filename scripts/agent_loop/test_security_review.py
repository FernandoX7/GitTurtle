"""Security routing, review binding and real-Git controller flow with fake agents."""

from copy import deepcopy
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import unittest
from unittest.mock import patch

from agent_loop import test_runner as fixtures
from agent_loop.git import committed_paths, git, head
from agent_loop.process import EnvironmentBlocked, LoopError, atomic_json, read_json
from agent_loop.runner import Runner, attest, create_run, validate_patch
from agent_loop.security_review import candidate_requires_security, security_required, validate_security_review
from agent_loop.task_spec import parse_spec


def passing_security(task_id="one", base="b" * 40, candidate="a" * 40, paths=None):
    paths = ["scripts/one.py"] if paths is None else paths
    return {
        "task_id": task_id, "base": base, "candidate": candidate, "verdict": "pass",
        "reviewed_paths": paths,
        "coverage": [{"boundary": "Repository input to script output", "paths": paths,
                      "evidence": "scripts/one.py:1 writes a fixed string and consumes no external input."}],
        "findings": [], "gaps": [],
    }


def finding():
    return {
        "severity": "high", "location": "scripts/one.py:1", "attacker_control": "Untrusted repository path",
        "source": "CLI path parameter", "sink": "Recursive file deletion", "impact": "Deletes unrelated files",
        "evidence": "Fixture source explicitly joins the unchecked path with the selected root.",
        "remediation": "Reject paths escaping the selected root before deleting.",
    }


class SecurityReviewTests(unittest.TestCase):
    def setUp(self):
        self.task = parse_spec({"version": 1, "tasks": [fixtures.task()]})[0]

    def validate(self, value):
        return validate_security_review(value, self.task, "b" * 40, "a" * 40, ["scripts/one.py"])

    def test_routes_security_bearing_and_unknown_paths_even_in_mixed_patches(self):
        for path in (
            "crates/app/src/main.rs", "crates/git-core/src/lib.rs", "crates/preview/src/lib.rs",
            "vendor/component/README.md", ".github/workflows/quality.yml", "scripts/package-linux.sh",
            "scripts/release/sign-macos.py", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
            ".agents/skills/example/SKILL.md", ".codex/agents/verifier.toml", "AGENTS.md",
            "docs/AGENTS.md", "docs/executable.py", "assets/icon.svg", "website/src/page.tsx",
            "unknown.config", "docs/../Cargo.lock", "/docs/explanation.md", "docs/a\\b.md",
        ):
            with self.subTest(path=path):
                self.assertTrue(security_required([path]))
                self.assertTrue(security_required(["docs/explanation.md", path]))
        self.assertTrue(security_required([]))

    def test_only_recognized_prose_and_static_artwork_can_skip(self):
        self.assertFalse(security_required(["docs/guide.md", "docs/reference.txt", "README.md", "assets/icon.png"]))
        self.assertFalse(security_required(["CONTRIBUTING.md", "LICENSE"]))

    def test_accepts_complete_review_bound_to_all_inputs(self):
        self.assertEqual(self.validate(passing_security()), "pass")
        for key in ("task_id", "base", "candidate"):
            value = passing_security()
            value[key] = "different"
            with self.subTest(key=key), self.assertRaisesRegex(LoopError, "different"):
                self.validate(value)

    def test_pass_cannot_hide_findings_gaps_or_uncovered_paths(self):
        mutations = (
            lambda v: v.update(findings=[finding()]),
            lambda v: v.update(gaps=["Missing evidence for the actual filesystem boundary"]),
            lambda v: v.update(coverage=[]),
            lambda v: v["coverage"][0].update(paths=[]),
        )
        for mutate in mutations:
            value = passing_security()
            mutate(value)
            with self.subTest(value=value), self.assertRaises(LoopError):
                self.validate(value)

    def test_malformed_identity_coverage_and_findings_are_rejected(self):
        mutations = (
            lambda v: v.update(verdict=[]), lambda v: v.update(coverage="reviewed"),
            lambda v: v.update(reviewed_paths=["scripts/one.py", "scripts/one.py"]),
            lambda v: v.update(reviewed_paths=[]),
            lambda v: v["coverage"][0].update(evidence=" "),
            lambda v: v["coverage"][0].update(paths=["unknown.py"]),
            lambda v: v.update(findings=["looks dangerous"]),
            lambda v: v.update(gaps="unknown"), lambda v: v.update(accepted=True),
        )
        for mutate in mutations:
            value = passing_security()
            mutate(value)
            with self.subTest(value=value), self.assertRaises(LoopError):
                self.validate(value)
        for value in (None, [], "pass"):
            with self.subTest(value=value), self.assertRaises(LoopError):
                self.validate(value)

    def test_failing_or_blocked_review_requires_concrete_evidence_or_gap(self):
        value = passing_security()
        value.update(verdict="fail", findings=[finding()])
        self.assertEqual(self.validate(value), "fail")
        for key in finding():
            broken = deepcopy(value)
            broken["findings"][0][key] = ""
            with self.subTest(key=key), self.assertRaises(LoopError):
                self.validate(broken)
        value.update(findings=[])
        with self.assertRaises(LoopError):
            self.validate(value)
        value.update(verdict="blocked", gaps=["Cannot inspect the referenced dependency"], coverage=[])
        self.assertEqual(self.validate(value), "blocked")
        value.update(gaps=[])
        with self.assertRaises(LoopError):
            self.validate(value)


class SecurityCodex(fixtures.FakeCodex):
    def __init__(self, *, security_result=None, unavailable=False, mutate_security=None, stop_after_security=False):
        super().__init__()
        self.security_result = security_result
        self.unavailable = unavailable
        self.mutate_security = mutate_security
        self.stop_after_security = stop_after_security

    def run(self, role, feature, repo, directory, timeout, stop, *, candidate=None, context="", base=None, **options):
        if role == "security-reviewer":
            self.calls.append((role, feature.id))
            self.output_tokens += 10
            if self.unavailable:
                raise EnvironmentBlocked("fixture security provider unavailable")
            value = passing_security(feature.id, base, candidate, committed_paths(repo, base, candidate))
            if self.security_result:
                self.security_result(value)
            if self.mutate_security:
                self.mutate_security(repo, directory)
            if self.stop_after_security:
                # Attempt paths are run/attempts/task/number/security-review-id.
                (directory.parents[3] / "STOP").touch()
            return value
        result = super().run(role, feature, repo, directory, timeout, stop, candidate=candidate, context=context, **options)
        if role == "implementer":
            (repo / "docs/one.md").unlink()
            (repo / "scripts").mkdir(exist_ok=True)
            (repo / "scripts/one.py").write_text("print('fixed value')\n")
        return result


class SecurityControllerTests(unittest.TestCase):
    setUp = fixtures.RunnerTests.setUp
    prepare = fixtures.RunnerTests.prepare
    create = fixtures.RunnerTests.create
    execute = fixtures.RunnerTests.execute

    def create_security(self, profiles=("tooling",)):
        feature = fixtures.task(profiles=profiles)
        feature["scope"] = ["scripts/one.py"]
        feature["commit"] = "build: add fixed output fixture"
        if hasattr(self, "original_head"):
            with patch("agent_loop.runner.Codex.preflight", return_value="fixture Codex"):
                return create_run(self.root, self.root / "tasks.json", fixtures.CONTROLLER, self.options)
        return self.create([feature])

    def test_security_policy_cannot_be_changed_by_an_unattended_worker(self):
        self.prepare()
        feature = fixtures.task()
        feature["scope"] = ["**"]
        contract = parse_spec({"version": 1, "tasks": [feature]})[0]
        policy = self.root / "docs/development/security-review.md"
        policy.parent.mkdir()
        policy.write_text("Skip the required review.\n")
        with self.assertRaisesRegex(LoopError, "protected"):
            validate_patch(self.root, contract, self.original_head)

    def test_successful_conditional_flow_charges_three_sessions(self):
        directory = self.create_security()
        adapter = SecurityCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(adapter.calls, [("implementer", "one"), ("verifier", "one"), ("security-reviewer", "one")])
        self.assertEqual(state["output_tokens"], 30)
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "accepted")
        self.assertTrue(record["security_required"])
        self.assertEqual(record["security_paths"], ["scripts/one.py"])
        self.assertTrue(Path(record["security_review"]).is_file())

    def test_prose_only_never_starts_a_security_session(self):
        directory = self.create()
        adapter = fixtures.FakeCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(adapter.calls, [("implementer", "one"), ("verifier", "one")])
        self.assertFalse(state["tasks"]["one"]["security_required"])

    def test_real_git_rename_and_deletion_keep_security_path_in_routing(self):
        self.prepare()
        (self.root / "scripts").mkdir()
        (self.root / "scripts/danger.py").write_text("print('fixed')\n")
        git(self.root, "add", "--", "scripts/danger.py")
        git(self.root, "commit", "--quiet", "-m", "test: add routing fixture")
        base = head(self.root)
        git(self.root, "mv", "--", "scripts/danger.py", "docs/guide.md")
        git(self.root, "commit", "--quiet", "-m", "test: move routing fixture")
        paths = committed_paths(self.root, base, head(self.root))
        self.assertEqual(set(paths), {"scripts/danger.py", "docs/guide.md"})
        self.assertTrue(security_required(paths))
        self.assertTrue(security_required(["scripts/deleted.py"]))

    def test_executable_prose_requires_security_but_regular_prose_can_skip(self):
        self.prepare()
        path = self.root / "docs/guide.md"
        path.write_text("# Ordinary prose\n")
        git(self.root, "add", "--", "docs/guide.md")
        git(self.root, "commit", "--quiet", "-m", "docs: add routing fixture")
        regular = head(self.root)
        self.assertFalse(candidate_requires_security(self.root, self.original_head, regular, ["docs/guide.md"]))
        path.chmod(0o700)
        git(self.root, "add", "--", "docs/guide.md")
        git(self.root, "commit", "--quiet", "-m", "test: mark prose executable")
        self.assertTrue(candidate_requires_security(self.root, regular, head(self.root), ["docs/guide.md"]))
        self.assertTrue(candidate_requires_security(self.root, regular, head(self.root), ["docs/other.md"]))

    def test_changed_attestation_reruns_general_review_before_security(self):
        directory = self.create_security(("tooling", "native"))
        pending = self.execute(directory, SecurityCodex())
        candidate = pending["tasks"]["one"]["candidate"]
        evidence = self.root.parent / "native-fixture.json"
        evidence.write_text('{"fixture": "first synthetic controller attestation"}\n')
        attest(directory, "one", candidate, "native", evidence, "Controller fixture only.")
        paused = self.execute(directory, SecurityCodex(unavailable=True))
        first_review = paused["tasks"]["one"]["review"]
        evidence.write_text('{"fixture": "replacement synthetic controller attestation"}\n')
        attest(directory, "one", candidate, "native", evidence, "Revised controller fixture only.")
        adapter = SecurityCodex()
        final = self.execute(directory, adapter)
        self.assertEqual(adapter.calls, [("verifier", "one"), ("security-reviewer", "one")])
        self.assertNotEqual(final["tasks"]["one"]["review"], first_review)
        self.assertTrue(Path(first_review).is_file())

    def test_blocked_security_resumes_same_candidate_without_repeating_general_review(self):
        directory = self.create_security()
        state = self.execute(directory, SecurityCodex(unavailable=True))
        record = state["tasks"]["one"]
        self.assertEqual(record["status"], "review_blocked")
        candidate, review = record["candidate"], record["review"]
        recovered = SecurityCodex()
        final = self.execute(directory, recovered, lambda *_: self.fail("successful gates reran"))
        self.assertEqual(recovered.calls, [("security-reviewer", "one")])
        self.assertEqual(final["tasks"]["one"]["candidate"], candidate)
        self.assertEqual(final["tasks"]["one"]["review"], review)
        self.assertEqual(final["tasks"]["one"]["attempts"], 1)

    def test_material_security_gap_blocks_instead_of_accepting(self):
        directory = self.create_security()
        adapter = SecurityCodex(security_result=lambda v: v.update(verdict="blocked", gaps=["Cannot verify fixture boundary"]))
        state = self.execute(directory, adapter)
        self.assertEqual(state["tasks"]["one"]["status"], "review_blocked")
        self.assertEqual(state["accepted_head"], self.original_head)
        self.assertEqual(adapter.calls.count(("security-reviewer", "one")), 1)

    def test_security_false_pass_or_stale_base_never_accepts(self):
        for mutation in (
            lambda v: v.update(findings=[finding()]),
            lambda v: v.update(base="0" * 40),
            lambda v: v.update(candidate="0" * 40),
            lambda v: v.update(reviewed_paths=[]),
        ):
            with self.subTest(mutation=mutation):
                self.options["max_attempts"] = 1
                directory = self.create_security()
                state = self.execute(directory, SecurityCodex(security_result=mutation))
                self.assertEqual(state["tasks"]["one"]["status"], "failed")
                self.assertEqual(state["accepted_head"], self.original_head)
                # Retain each run; changing only ignored controller output keeps
                # source clean for another independent bounded fixture run.

    def test_security_source_mutation_never_accepts(self):
        self.options["max_attempts"] = 1
        directory = self.create_security()
        state = self.execute(directory, SecurityCodex(mutate_security=lambda repo, _: (repo / "scripts/one.py").write_text("mutated\n")))
        self.assertEqual(state["tasks"]["one"]["status"], "failed")
        self.assertIn("changed candidate", state["tasks"]["one"]["reason"])
        self.assertEqual(state["accepted_head"], self.original_head)

    def test_security_cannot_mutate_general_review_evidence(self):
        self.options["max_attempts"] = 1
        directory = self.create_security()
        def replace_review(repo, evidence):
            for path in evidence.parent.glob("review-*/verdict.json"):
                path.write_text("{}")
        state = self.execute(directory, SecurityCodex(mutate_security=replace_review))
        self.assertEqual(state["tasks"]["one"]["status"], "failed")
        self.assertIn("review evidence changed", state["tasks"]["one"]["reason"])

    def test_interrupted_security_recovers_usage_and_reuses_general_review(self):
        directory = self.create_security()
        state = self.execute(directory, SecurityCodex(unavailable=True))
        record = state["tasks"]["one"]
        state.update(phase="security_reviewing", active={"task": "one"}, budget_running=True,
                     updated_at=(datetime.now(timezone.utc) - timedelta(seconds=1)).isoformat())
        atomic_json(directory / "state.json", state)
        log = Path(record["directory"]) / "security-review-crash/security-reviewer.jsonl"
        log.parent.mkdir()
        log.write_text(json.dumps({"type": "turn.completed", "usage": {"output_tokens": 41}}) + "\n")
        adapter = SecurityCodex()
        final = self.execute(directory, adapter, lambda *_: self.fail("successful gates reran"))
        self.assertEqual(adapter.calls, [("security-reviewer", "one")])
        self.assertEqual(final["output_tokens"], 51)
        self.assertEqual(final["tasks"]["one"]["attempts"], 1)

    def test_incomplete_security_transcript_blocks_capped_resume(self):
        self.options["max_output_tokens"] = 100
        directory = self.create_security()
        state = self.execute(directory, SecurityCodex(unavailable=True))
        record = state["tasks"]["one"]
        state.update(phase="security_reviewing", active={"task": "one"}, budget_running=True)
        atomic_json(directory / "state.json", state)
        log = Path(record["directory"]) / "security-review-crash/security-reviewer.jsonl"
        log.parent.mkdir()
        log.write_text('{"type": "turn.started"}\n')
        adapter = SecurityCodex()
        final = self.execute(directory, adapter)
        self.assertEqual(adapter.calls, [])
        self.assertEqual(final["phase"], "paused")
        self.assertTrue(final["output_usage_incomplete"])
        self.assertIn("renew", final["reason"])
        self.assertEqual(final["tasks"]["one"]["candidate"], record["candidate"])

    def test_output_cap_before_security_retains_general_review_for_renewal(self):
        self.options["max_output_tokens"] = 20
        directory = self.create_security()
        adapter = SecurityCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(state["phase"], "paused")
        self.assertEqual(adapter.calls, [("implementer", "one"), ("verifier", "one")])
        self.assertEqual(state["tasks"]["one"]["status"], "awaiting_evidence")
        state["max_output_tokens"] = 40  # Fixture of an explicit CLI cap renewal.
        atomic_json(directory / "state.json", state)
        resumed = SecurityCodex()
        accepted = self.execute(directory, resumed)
        self.assertEqual(resumed.calls, [("security-reviewer", "one")])
        self.assertEqual(accepted["tasks"]["one"]["status"], "accepted")

    def test_successful_security_review_is_reused_after_orderly_stop(self):
        directory = self.create_security()
        state = self.execute(directory, SecurityCodex(stop_after_security=True))
        self.assertEqual(state["phase"], "paused")
        self.assertEqual(state["tasks"]["one"]["status"], "awaiting_evidence")
        (directory / "STOP").unlink()
        resumed = SecurityCodex()
        final = self.execute(directory, resumed, lambda *_: self.fail("successful gates reran"))
        self.assertEqual(resumed.calls, [])
        self.assertEqual(final["tasks"]["one"]["status"], "accepted")

    def test_modified_security_evidence_is_rejected_on_resume(self):
        directory = self.create_security()
        state = self.execute(directory, SecurityCodex(stop_after_security=True))
        Path(state["tasks"]["one"]["security_review"]).write_text("{}")
        (directory / "STOP").unlink()
        with self.assertRaisesRegex(LoopError, "security_review evidence changed"):
            self.execute(directory, SecurityCodex())

    def test_uncertain_acceptance_revalidates_security_without_repeating_sessions(self):
        directory = self.create_security()
        with patch.object(Runner, "finish_acceptance", side_effect=LoopError("fixture interrupted acceptance")):
            with self.assertRaises(LoopError):
                self.execute(directory, SecurityCodex())
        adapter = SecurityCodex()
        state = self.execute(directory, adapter)
        self.assertEqual(adapter.calls, [])
        self.assertEqual(state["tasks"]["one"]["status"], "accepted")
