from __future__ import annotations

import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "changes.py"
QUALITY = Path(__file__).resolve().parents[3] / ".github/workflows/quality.yml"
SPEC = importlib.util.spec_from_file_location("ci_changes", SCRIPT)
changes = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(changes)


def privacy(status="scanned", result="success"):
    return {"image-privacy": {"result": result, "outputs": {"status": status}}}


def workflow_jobs():
    """Quality's top-level job blocks by id, read as text (no YAML dependency)."""
    body = QUALITY.read_text(encoding="utf-8").split("\njobs:\n", 1)[1]
    parts = re.split(r"^  ([a-z][a-z0-9-]*):\n", body, flags=re.M)
    return dict(zip(parts[1::2], parts[2::2]))


def needs_for(plan):
    return {
        "changes": {"result": "success", "outputs": {
            "plan": json.dumps(plan), **{lane: str(plan[lane]).lower() for lane in changes.LANES}}},
        **{job: {"result": "success" if plan[lane] else "skipped"}
           for job, lane in changes.JOBS.items()},
        **privacy(),
    }


class ClassificationTests(unittest.TestCase):
    def test_representative_input_fixtures(self):
        cases = json.loads((Path(__file__).parent / "fixtures/change-routing.json").read_text())
        for case in cases:
            with self.subTest(case=case["name"]):
                plan = changes.classify_paths([path.encode() for path in case["paths"]])
                self.assertEqual({lane for lane in changes.LANES if plan[lane]}, set(case["required"]))
                changes.validate_plan(plan)

    def test_path_boundaries_do_not_match_similar_prefixes(self):
        for path in [b"website-old/public.js", b"docs-old/README.md", b"scripts/ci-other/tool.py",
                     b"scripts/native_qa-old/qa.py", b".claude-old/settings.json", b".claudeignore",
                     b"crates/app/CLAUDE.md.orig", b"NOTCLAUDE.md"]:
            with self.subTest(path=path):
                self.assertTrue(changes.classify_paths([path])["product"])

    def test_untrusted_or_ambiguous_names_select_full_coverage(self):
        for path in [b"docs/a\nproduct=false.md", b"docs/\xff.md", b"/docs/a.md", b"docs/../a.md",
                     b"docs//a.md", b"docs/./a.md", b"docs\\a.md", b"docs/a\x1b.md"]:
            with self.subTest(path=path):
                self.assertTrue(all(changes.classify_paths([path])[lane] for lane in changes.LANES))

    def test_nul_parser_retains_literal_shell_syntax_and_spaces(self):
        raw = b"docs/$(touch nope) `command` notes.md\0website/public/a b.js\0"
        paths = changes.parse_paths(raw)
        self.assertEqual(len(paths), 2)
        self.assertEqual(changes.classify_paths(paths)["product"], False)
        self.assertEqual(changes.classify_paths(paths)["website"], True)

    def test_parser_refuses_incomplete_or_oversized_comparison(self):
        for raw in [b"docs/a.md", b"docs/a.md\0\0", b"a" * (changes.MAX_INPUT_BYTES + 1),
                    b"docs/a.md\0" * (changes.MAX_PATHS + 1)]:
            with self.subTest(size=len(raw)):
                with self.assertRaises(changes.PolicyError):
                    changes.parse_paths(raw)

    def test_comparison_failure_requires_full_validation(self):
        with patch.object(changes, "git_read", side_effect=changes.PolicyError("unavailable")):
            plan = changes.classify_checkout(Path("."), "pull_request", {}, "a" * 40)
        self.assertTrue(all(plan[lane] for lane in changes.LANES))

    def test_manual_dispatch_always_requests_full_validation(self):
        plan = changes.classify_checkout(Path("."), "workflow_dispatch", {}, "")
        self.assertTrue(all(plan[lane] for lane in changes.LANES))


class GateTests(unittest.TestCase):
    def test_independent_phase_results(self):
        # Literal job names here model the workflow's needs payload independently
        # of JOBS, so dropping an expected phase cannot silently update the test.
        cases = json.loads((Path(__file__).parent / "fixtures/phase-results.json").read_text())
        for case in cases:
            with self.subTest(case=case["name"]):
                plan = changes.classify_paths([case["path"].encode()])
                needs = {"changes": needs_for(plan)["changes"], **privacy(),
                         **{job: {"result": result} for job, result in case["results"].items()}}
                if case["accept"]:
                    self.assertEqual(len(changes.gate(needs)), 7)
                else:
                    with self.assertRaises(changes.PolicyError):
                        changes.gate(needs)

    def test_required_lanes_pass_and_justified_skips_pass(self):
        for paths in [[b"docs/user-guide.md"], [b"website/check.py"], [b"scripts/ci/metrics.py"],
                      [b"crates/app/src/main.rs"]]:
            with self.subTest(paths=paths):
                self.assertEqual(len(changes.gate(needs_for(changes.classify_paths(paths)))), 7)

    def test_required_failure_cancellation_and_skip_each_fail(self):
        baseline = needs_for(changes.full_plan("full"))
        for job in ["changes", "image-privacy", *changes.JOBS]:
            for result in ["failure", "cancelled", "skipped", "timed_out", None]:
                with self.subTest(job=job, result=result):
                    needs = copy.deepcopy(baseline)
                    needs[job]["result"] = result
                    with self.assertRaises(changes.PolicyError):
                        changes.gate(needs)

    def test_unneeded_failure_is_not_silently_ignored(self):
        baseline = needs_for(changes.classify_paths([b"README.md"]))
        for job in ["rust-release", "rust-arch"]:
            for result in ["failure", "cancelled"]:
                with self.subTest(job=job, result=result):
                    needs = copy.deepcopy(baseline)
                    needs[job]["result"] = result
                    with self.assertRaises(changes.PolicyError):
                        changes.gate(needs)

    def test_workflow_gate_waits_for_every_required_job_and_each_job_follows_its_lane(self):
        # The gate refuses a needs payload with a missing or unknown job, so the
        # workflow's list and JOBS must agree or every Quality run fails.
        jobs = workflow_jobs()
        needs = re.search(r"^    needs: \[(.*)\]$", jobs["quality"], re.M)
        self.assertIsNotNone(needs)
        self.assertEqual({name.strip() for name in needs.group(1).split(",")},
                         {"changes", changes.PRIVACY_JOB, *changes.JOBS})
        for job, lane in changes.JOBS.items():
            with self.subTest(job=job):
                self.assertIn(f"\n    if: needs.changes.outputs.{lane} == 'true'\n", "\n" + jobs[job])

    def test_extra_successful_coverage_is_harmless(self):
        needs = needs_for(changes.classify_paths([b"README.md"]))
        for job in changes.JOBS:
            needs[job]["result"] = "success"
        self.assertEqual(len(changes.gate(needs)), 7)

    def test_absent_and_unknown_jobs_fail(self):
        baseline = needs_for(changes.full_plan("full"))
        for job in baseline:
            needs = copy.deepcopy(baseline)
            del needs[job]
            with self.assertRaises(changes.PolicyError):
                changes.gate(needs)
        baseline["new-required-lane"] = {"result": "success"}
        with self.assertRaises(changes.PolicyError):
            changes.gate(baseline)

    def test_malformed_and_inconsistent_classification_fails(self):
        valid = changes.classify_paths([b"README.md"])
        invalid = [None, {}, {**valid, "version": 2}, {**valid, "product": "false"},
                   {**valid, "path_count": 0}, {**valid, "path_count": True},
                   {**valid, "reason": "comparison-unavailable", "path_count": None},
                   {**valid, "product": True}, {**valid, "unexpected": False}]
        for plan in invalid:
            with self.subTest(plan=plan):
                needs = needs_for(valid)
                needs["changes"]["outputs"]["plan"] = json.dumps(plan)
                with self.assertRaises(changes.PolicyError):
                    changes.gate(needs)
        needs = needs_for(valid)
        needs["changes"]["outputs"]["product"] = "true"
        with self.assertRaises(changes.PolicyError):
            changes.gate(needs)

    def test_image_privacy_reports_what_it_did_and_never_claims_an_absent_scan(self):
        baseline = needs_for(changes.classify_paths([b"docs/evidence/frame.png"]))
        for status, line in [("scanned", "image-privacy: added or changed images template-scanned; no match"),
                             ("no-images", "image-privacy: no added or changed images"),
                             ("unavailable", "image-privacy: NOT scanned; the templates secret is unavailable "
                                             "to this run (fork pull request or unconfigured repository)"),
                             ("no-comparison", "image-privacy: NOT scanned; this event has no comparison")]:
            with self.subTest(status=status):
                self.assertEqual(changes.gate({**baseline, **privacy(status)})[-1], line)
        for entry in [privacy("scan")["image-privacy"], privacy(["scanned"])["image-privacy"],
                      privacy(None)["image-privacy"], {"result": "success"}, {"result": "success", "outputs": []},
                      privacy("scanned", "failure")["image-privacy"], "success"]:
            with self.subTest(entry=entry), self.assertRaises(changes.PolicyError):
                changes.gate({**baseline, "image-privacy": entry})

    def test_gate_cli_reports_actual_exit_status(self):
        good = needs_for(changes.full_plan("full"))
        bad = copy.deepcopy(good)
        bad["rust-debug"]["result"] = "skipped"
        for needs, expected in [(good, 0), (bad, 1), ({}, 1)]:
            completed = subprocess.run([sys.executable, str(SCRIPT), "gate"],
                                       env={**os.environ, "NEEDS_JSON": json.dumps(needs)},
                                       capture_output=True, text=True)
            self.assertEqual(completed.returncode, expected, completed.stderr)


class GitComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        self.git("init", "-b", "main")
        self.git("config", "user.name", "CI Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write("docs/guide.md", "before\n")
        self.write("crates/sample.rs", "fn sample() {}\n")
        self.base = self.commit("base")

    def git(self, *args):
        env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        return subprocess.run(["git", *args], cwd=self.repo, env=env, check=True,
                              capture_output=True).stdout.decode().strip()

    def write(self, path, content):
        file = self.repo / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content)

    def commit(self, message):
        self.git("add", "--all")
        self.git("-c", "commit.gpgsign=false", "commit", "-m", message)
        return self.git("rev-parse", "HEAD")

    def classify_push(self, before):
        head = self.git("rev-parse", "HEAD")
        return changes.classify_checkout(self.repo, "push",
                                         {"before": before, "after": head, "deleted": False, "ref": "refs/heads/main"}, head)

    def classify_pull_request(self, base):
        head = self.git("rev-parse", "HEAD")
        self.git("checkout", "-b", "pull-request-base", base)
        self.git("-c", "commit.gpgsign=false", "merge", "--no-ff", "-m", "PR merge fixture", head)
        merge = self.git("rev-parse", "HEAD")
        return changes.classify_checkout(self.repo, "pull_request",
                                         {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}, merge)

    def test_main_push_always_runs_full_coverage_for_every_changed_input(self):
        for path in ("docs/guide.md", "website/public/index.html", "scripts/ci/metrics.py", "crates/sample.rs"):
            with self.subTest(path=path):
                before = self.git("rev-parse", "HEAD")
                self.write(path, "changed fixture content\n")
                self.commit("change " + path)
                plan = self.classify_push(before)
                self.assertTrue(all(plan[lane] for lane in changes.LANES))
                self.assertEqual(plan["reason"], "main-full-validation")
                changes.validate_plan(plan)

    def test_docs_move_preserves_cheap_routing(self):
        self.git("mv", "docs/guide.md", "docs/renamed.md")
        self.commit("rename docs")
        self.assertFalse(self.classify_pull_request(self.base)["product"])

    def test_product_renamed_into_docs_still_requires_product(self):
        self.git("mv", "crates/sample.rs", "docs/sample.md")
        self.commit("move former product")
        self.assertTrue(self.classify_pull_request(self.base)["product"])

    def test_deleted_product_is_not_lost(self):
        self.git("rm", "crates/sample.rs")
        self.commit("delete product")
        self.assertTrue(self.classify_pull_request(self.base)["product"])

    def test_deleted_docs_remain_cheap(self):
        self.git("rm", "docs/guide.md")
        self.commit("delete docs")
        self.assertFalse(self.classify_pull_request(self.base)["product"])

    def test_missing_before_and_checkout_mismatch_run_everything(self):
        self.assertTrue(self.classify_push("0" * 40)["product"])
        self.assertTrue(self.classify_push("a" * 40)["product"])
        plan = changes.classify_checkout(self.repo, "push",
                                         {"before": self.base, "after": "b" * 40, "deleted": False},
                                         "b" * 40)
        self.assertTrue(plan["product"])

    def test_pull_request_compares_complete_head_and_actual_merge(self):
        self.git("checkout", "-b", "contribution")
        self.write("docs/guide.md", "contributor docs\n")
        head = self.commit("docs contribution")
        self.git("checkout", "main")
        self.write("website/public/index.html", "<html></html>\n")
        base = self.commit("new main")
        self.git("-c", "commit.gpgsign=false", "merge", "--no-ff", "-m", "test merge", "contribution")
        merge = self.git("rev-parse", "HEAD")
        event = {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
        plan = changes.classify_checkout(self.repo, "pull_request", event, merge)
        self.assertFalse(any(plan[lane] for lane in changes.LANES))
        # Neither a stale event base nor checking out the branch head silently
        # establishes the merge-commit classification used by the workflow.
        event["pull_request"]["base"]["sha"] = self.base
        self.assertTrue(changes.classify_checkout(self.repo, "pull_request", event, merge)["product"])

    def test_literal_shell_filename_cannot_execute_during_diff(self):
        self.write("docs/$(touch ATTACKED).md", "literal name\n")
        self.commit("literal filename")
        self.assertFalse(self.classify_pull_request(self.base)["product"])
        self.assertFalse((self.repo / "ATTACKED").exists())

    def test_classifier_cli_writes_fixed_outputs_without_paths(self):
        self.write("docs/guide.md", "after\n")
        head = self.commit("docs")
        event_file, output_file = self.repo / "event.json", self.repo / "output.txt"
        event_file.write_text(json.dumps({"before": self.base, "after": head, "deleted": False, "ref": "refs/heads/main"}))
        result = subprocess.run([sys.executable, str(SCRIPT), "classify", "--repository", str(self.repo),
                                 "--output", str(output_file)], capture_output=True, text=True,
                                env={**os.environ, "GITHUB_EVENT_NAME": "push", "GITHUB_SHA": head,
                                     "GITHUB_EVENT_PATH": str(event_file)})
        self.assertEqual(result.returncode, 0, result.stderr)
        output = dict(line.split("=", 1) for line in output_file.read_text().splitlines())
        self.assertEqual(output["product"], "true")
        self.assertEqual(len(output), 4)
        self.assertNotIn("guide.md", result.stdout)
        changes.validate_plan(json.loads(output["plan"]))


if __name__ == "__main__":
    unittest.main()
