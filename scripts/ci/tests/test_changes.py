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


LINUX_LABELS = ("linux-x86_64", "linux-aarch64")
UPLOADED = "uploaded by its release leg; downloaded and verified by the later job"
HELD = "held by its release leg; nothing uploaded and nothing verified"


def packaged(eligible="true", transfer="verified", labels=LINUX_LABELS):
    """The release job's and the later job's entries as a run leaves them, literally: for an upload each
    Linux leg hands on its answer, its artifact's ID and its provenance digest, and the later job its word."""
    release = {f"{label}-eligible": eligible for label in labels}
    if eligible == "true":
        release.update({f"{label}-artifact-id": "14598682882" for label in labels})
        release.update({f"{label}-provenance-sha256": "5e" * 32 for label in labels})
    return {"rust-release": {"result": "success", "outputs": release},
            "package-transfer": {"result": "success", "outputs": {f"{label}-transfer": transfer for label in labels}}}


def needs_for(plan):
    needs = {
        "changes": {"result": "success", "outputs": {
            "plan": json.dumps(plan), **{lane: str(plan[lane]).lower() for lane in changes.LANES}}},
        **{job: {"result": "success" if plan[lane] else "skipped", "outputs": {}}
           for job, lane in changes.JOBS.items()},
        **privacy(),
    }
    return {**needs, **packaged()} if plan["product"] else needs


def expected_lines(results, privacy_line="image-privacy: added or changed images template-scanned; no match"):
    """What the gate prints for literal job results: a line a job in the given order, then a line for each
    Linux package when the release job ran, then the image privacy job's."""
    lines = [f"{job}: {'success' if result == 'success' else 'not needed by classification'}"
             for job, result in results.items()]
    if results["rust-release"] == "success":
        lines += [f"package-transfer: {label} {UPLOADED}" for label in LINUX_LABELS]
    return [*lines, privacy_line]


class ClassificationTests(unittest.TestCase):
    def test_representative_input_fixtures(self):
        cases = json.loads((Path(__file__).parent / "fixtures/change-routing.json").read_text())
        for case in cases:
            with self.subTest(case=case["name"]):
                plan = changes.classify_paths([path.encode() for path in case["paths"]])
                self.assertEqual({lane for lane in changes.LANES if plan[lane]}, set(case["required"]))
                changes.validate_plan(plan)

    def test_the_package_installation_helper_selects_the_full_plan_for_its_own_reason(self):
        # Only the Rust jobs' setup and the Image privacy job run it, so the
        # tooling lane alone would let a change of it reach main unexercised.
        plan = changes.classify_paths([b"scripts/ci/apt_install.py"])
        self.assertEqual(plan, changes.full_plan("package-installation-helper"))
        changes.validate_plan(plan)
        self.assertNotEqual(plan["reason"], changes.classify_paths([b"scripts/ci/tools.py"])["reason"])
        for paths in ([b"docs/ci.md", b"scripts/ci/apt_install.py"], [b"scripts/ci/executed_tests.py", b"scripts/ci/apt_install.py"]):
            with self.subTest(paths=paths):
                self.assertEqual(changes.classify_paths(paths), plan)
        # Its tests and a similar name are ordinary CI helpers.
        for path in (b"scripts/ci/tests/test_apt_install.py", b"scripts/ci/apt_install_old.py", b"scripts/ci/apt_install.py.orig"):
            with self.subTest(path=path):
                self.assertEqual({lane for lane in changes.LANES if changes.classify_paths([path])[lane]}, {"tooling"})

    def test_the_collector_selects_the_full_plan_for_its_own_reason(self):
        # Every Rust job runs its commands under it, so the tooling lane alone
        # would let a change of it reach main without one of them having run.
        plan = changes.classify_paths([b"scripts/ci/metrics.py"])
        self.assertEqual(plan, changes.full_plan("ci-collector"))
        changes.validate_plan(plan)
        for other in (b"scripts/ci/packages.py", b"scripts/ci/tools.py", b"scripts/ci/apt_install.py"):
            self.assertNotEqual(plan["reason"], changes.classify_paths([other])["reason"])
        for paths in ([b"docs/ci.md", b"scripts/ci/metrics.py"], [b"scripts/ci/metrics.py", b"scripts/ci/tests/test_metrics.py"],
                      [b"scripts/ci/executed_tests.py", b"scripts/ci/metrics.py", b"website/check.py"]):
            with self.subTest(paths=paths):
                self.assertEqual(changes.classify_paths(paths), plan)
        # Its tests and a similar name are ordinary CI helpers.
        for path in (b"scripts/ci/tests/test_metrics.py", b"scripts/ci/metrics_old.py", b"scripts/ci/metrics.py.orig",
                     b"scripts/ci/tests/metrics.py"):
            with self.subTest(path=path):
                self.assertEqual({lane for lane in changes.LANES if changes.classify_paths([path])[lane]}, {"tooling"})

    def test_path_boundaries_do_not_match_similar_prefixes(self):
        for path in [b"website-old/public.js", b"docs-old/README.md", b"scripts/ci-other/tool.py",
                     b"scripts/native_qa-old/qa.py", b"scripts/operator-old/land.py", b".claude-old/settings.json", b".claudeignore",
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
                # A job that ran carries what a real run's does, so a refusal here is its result's alone.
                ran = packaged()
                needs = {"changes": needs_for(plan)["changes"], **privacy(),
                         **{job: ran[job] if result == "success" and job in ran else {"result": result, "outputs": {}}
                            for job, result in case["results"].items()}}
                if case["accept"]:
                    self.assertEqual(changes.gate(needs), expected_lines(case["results"]))
                else:
                    with self.assertRaises(changes.PolicyError):
                        changes.gate(needs)
        # The fixture names the later job in every case that names the release job, and fails it each way.
        self.assertEqual([case["name"] for case in cases if "rust-release" in case["results"]
                          and "package-transfer" not in case["results"]], ["missing package transfer result cannot pass"])
        self.assertEqual(sorted(case["results"]["package-transfer"] for case in cases
                                if case["name"].startswith("package-transfer ")), ["cancelled", "failure", "skipped"])

    def test_required_lanes_pass_and_justified_skips_pass(self):
        jobs = ["rust-format", "rust-debug", "rust-clippy", "rust-release", "package-transfer", "rust-arch",
                "agent-tooling", "website"]
        lanes = {"product": jobs[:6], "tooling": jobs[6:7], "website": jobs[7:]}
        for paths, required in [([b"docs/user-guide.md"], set()), ([b"website/check.py"], {"website"}),
                                ([b"scripts/ci/executed_tests.py"], {"tooling"}),
                                ([b"scripts/ci/metrics.py"], {"product", "tooling", "website"}),
                                ([b"crates/app/src/main.rs"], {"product", "tooling", "website"})]:
            with self.subTest(paths=paths):
                results = {job: "success" if lane in required else "skipped" for job in jobs
                           for lane, members in lanes.items() if job in members}
                # Eleven lines when the product lane ran, two of them the Linux packages'; nine when it did not.
                lines = changes.gate(needs_for(changes.classify_paths(paths)))
                self.assertEqual(lines, expected_lines(results))
                self.assertEqual(len(lines), 11 if "product" in required else 9)

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
        for job in ["rust-clippy", "rust-release", "package-transfer", "rust-arch"]:
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
        # A release job that ran although no lane asked for it is judged as any other: with its packages.
        with self.assertRaisesRegex(changes.PolicyError, "package-transfer: linux-x86_64 is neither"):
            changes.gate(needs)
        needs.update(packaged())
        lines = changes.gate(needs)
        self.assertEqual(lines, expected_lines({job: "success" for job in changes.JOBS}))
        self.assertEqual(len(lines), 11)

    def test_each_linux_package_is_judged_as_the_pair_of_its_release_leg_and_the_later_job(self):
        baseline = needs_for(changes.full_plan("full"))
        jobs = [f"{job}: success" for job in changes.JOBS]
        privacy_line = changes.PRIVACY_RESULTS["scanned"]
        # Uploaded and verified, held on both sides, and one of each: a line a package either way.
        self.assertEqual(changes.gate(baseline), [*jobs, f"package-transfer: linux-x86_64 {UPLOADED}",
                                                  f"package-transfer: linux-aarch64 {UPLOADED}", privacy_line])
        self.assertEqual(changes.gate({**baseline, **packaged("false", "held")}),
                         [*jobs, f"package-transfer: linux-x86_64 {HELD}", f"package-transfer: linux-aarch64 {HELD}",
                          privacy_line])
        mixed = copy.deepcopy(baseline)
        mixed["rust-release"]["outputs"] = {"linux-x86_64-eligible": "true", "linux-aarch64-eligible": "false"}
        mixed["package-transfer"]["outputs"] = {"linux-x86_64-transfer": "verified", "linux-aarch64-transfer": "held"}
        self.assertEqual(changes.gate(mixed)[len(jobs):-1], [f"package-transfer: linux-x86_64 {UPLOADED}",
                                                             f"package-transfer: linux-aarch64 {HELD}"])
        # Every other pair is refused for either label alone, the other label's pair being whole.
        absent = object()
        wrong = [("true", "held"), ("false", "verified"), ("true", ""), ("true", absent), ("true", None), ("", "verified"),
                 (absent, "verified"), (None, "verified"), (absent, absent), ("", ""), ("false", ""), ("false", absent),
                 (absent, "held"), ("TRUE", "verified"), ("true", "Verified"), ("true", "verified\n"), (" true", "verified"),
                 ("yes", "verified"), ("true", "success"), ("true", "passed"), (True, "verified"), ("true", True),
                 ("true", ["verified"]), (["true"], "verified"), ("true", {"verified": True}), ("held", "false"),
                 ("verified", "true")]
        for label in LINUX_LABELS:
            for eligible, transfer in wrong:
                with self.subTest(label=label, eligible=eligible, transfer=transfer):
                    needs = copy.deepcopy(baseline)
                    for job, name, value in (("rust-release", f"{label}-eligible", eligible),
                                             ("package-transfer", f"{label}-transfer", transfer)):
                        if value is absent:
                            del needs[job]["outputs"][name]
                        else:
                            needs[job]["outputs"][name] = value
                    with self.assertRaisesRegex(changes.PolicyError, f"package-transfer: {label} is neither an uploaded and "
                                                                     "verified package nor a held one"):
                        changes.gate(needs)

    def test_a_package_pair_needs_both_jobs_outputs_and_both_jobs_to_have_run(self):
        baseline = needs_for(changes.full_plan("full"))
        absent = object()
        for job in ("rust-release", "package-transfer"):
            for outputs in (absent, None, {}, [], "verified", {"linux-x86_64-transfer": "verified"}):
                with self.subTest(job=job, outputs=outputs):
                    needs = copy.deepcopy(baseline)
                    if outputs is absent:
                        del needs[job]["outputs"]
                    else:
                        needs[job]["outputs"] = outputs
                    with self.assertRaisesRegex(changes.PolicyError, "is neither an uploaded and verified package"):
                        changes.gate(needs)
        # A skipped later job while the product lane ran, whatever its outputs still say.
        skipped = copy.deepcopy(baseline)
        skipped["package-transfer"]["result"] = "skipped"
        with self.assertRaisesRegex(changes.PolicyError, r"package-transfer: required result was not successful \('skipped'\)"):
            changes.gate(skipped)
        # Where no lane asked for the product jobs, the two still stand or fall together.
        unasked = needs_for(changes.classify_paths([b"README.md"]))
        self.assertEqual(changes.gate(unasked)[3:5], ["rust-release: not needed by classification",
                                                     "package-transfer: not needed by classification"])
        for ran in ("rust-release", "package-transfer"):
            with self.subTest(ran=ran):
                needs = {**unasked, ran: packaged()[ran]}
                with self.assertRaisesRegex(changes.PolicyError,
                                            "package-transfer: the release job and the later job did not both run"):
                    changes.gate(needs)

    def test_a_refused_pair_is_named_without_printing_what_was_handed_on(self):
        baseline = needs_for(changes.full_plan("full"))
        marker = "VALUE-7f3a"
        for job, name, shown in (("rust-release", "linux-aarch64-eligible", "eligible: unexpected; transfer: verified"),
                                 ("package-transfer", "linux-aarch64-transfer", "eligible: true; transfer: unexpected")):
            with self.subTest(job=job):
                needs = copy.deepcopy(baseline)
                needs[job]["outputs"][name] = marker
                with self.assertRaises(changes.PolicyError) as refused:
                    changes.gate(needs)
                self.assertEqual(str(refused.exception), "package-transfer: linux-aarch64 is neither an uploaded and "
                                                         f"verified package nor a held one ({shown})")
                del needs[job]["outputs"][name]
                with self.assertRaises(changes.PolicyError) as refused:
                    changes.gate(needs)
                self.assertIn(shown.replace("unexpected", "missing"), str(refused.exception))

    def test_the_gate_reads_the_labels_and_words_that_the_package_helper_writes(self):
        spec = importlib.util.spec_from_file_location("ci_packages_for_gate", SCRIPT.with_name("packages.py"))
        packages = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(packages)
        # A label for each target of a platform whose package is uploaded: today the two Linux targets.
        uploaded = [label for target, label in packages.TARGETS.items()
                    if packages.platform_of(target) in packages.HOSTED_UPLOAD_PLATFORMS]
        self.assertEqual(list(changes.TRANSFER_LABELS), uploaded)
        self.assertEqual(uploaded, list(LINUX_LABELS))
        self.assertEqual(set(changes.TRANSFER_PAIRS), {("true", packages.VERIFIED), ("false", packages.HELD)})
        self.assertEqual((packages.VERIFIED, packages.HELD), ("verified", "held"))
        self.assertEqual(changes.TRANSFER_PAIRS, {("true", "verified"): UPLOADED, ("false", "held"): HELD})
        self.assertEqual((changes.RELEASE_JOB, changes.TRANSFER_JOB), ("rust-release", "package-transfer"))
        self.assertEqual(changes.JOBS[changes.RELEASE_JOB], changes.JOBS[changes.TRANSFER_JOB])

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

    def classify_pull_request(self, base, branch="pull-request-base"):
        head = self.git("rev-parse", "HEAD")
        self.git("checkout", "-b", branch, base)
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

    def test_a_pull_request_that_changes_the_collector_runs_every_lane(self):
        self.write("scripts/ci/metrics.py", "collector\n")
        self.write("scripts/ci/executed_tests.py", "an ordinary helper\n")
        base = self.commit("helpers")
        plans = []
        # An ordinary helper, then the collector alone, with a documentation file and with a website file.
        for number, paths in enumerate((("scripts/ci/executed_tests.py",), ("scripts/ci/metrics.py",),
                                        ("docs/guide.md", "scripts/ci/metrics.py"),
                                        ("scripts/ci/metrics.py", "website/public/index.html"))):
            self.git("checkout", "-b", f"contribution-{number}", base)
            for path in paths:
                self.write(path, "changed by the pull request\n")
            self.commit("change " + ", ".join(paths))
            plans.append(self.classify_pull_request(base, f"pull-request-base-{number}"))
        self.assertEqual({lane for lane in changes.LANES if plans[0][lane]}, {"tooling"})
        self.assertEqual(plans[1:], [changes.full_plan("ci-collector")] * 3)

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
