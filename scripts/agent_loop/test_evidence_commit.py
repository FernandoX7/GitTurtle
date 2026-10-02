"""Evidence committed on top of a candidate: its checks, its docs gate and acceptance at it."""

from __future__ import annotations

import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from agent_loop import inbox, test_runner as fixtures
from agent_loop.evidence import build_references, evidence_paths
from agent_loop.git import git, head
from agent_loop.process import LoopError, atomic_json, read_json
from agent_loop.runner import attest, controlled, create_run, locked
from agent_loop.task_spec import parse_spec
# Records and run state stay private even when the host umask is permissive.
from agent_loop.test_support import setUpModule, tearDownModule


EVIDENCE_SCOPE = ["docs/evidence/one/**", "docs/validation.md"]
FRAME = b"\x89PNG\r\n\x1a\nfixture frame"
ENTRY = b"## October 2, 2026: one\n\n- Frames: `docs/evidence/one/focus.png` shows the focused row.\n"


def red_on_evidence(repo, profiles, directory):
    """Every gate passes except the docs gate of an evidence checkout."""
    return {"passed": not Path(directory).parent.name.startswith("evidence-"), "checks": []}


class EvidenceCommitTests(unittest.TestCase):
    setUp = fixtures.RunnerTests.setUp
    prepare = fixtures.RunnerTests.prepare
    land = fixtures.RunnerTests.land
    execute = fixtures.RunnerTests.execute

    def create(self, scope=EVIDENCE_SCOPE, profiles=("native",), seed=()):
        contract = fixtures.evidence_task(profiles=profiles)
        contract["scope"] += scope
        specification = self.prepare([contract])
        if seed:
            self.land(seed, "docs: seed earlier evidence")
        with patch("agent_loop.runner.Codex.preflight", return_value="fixture Codex"):
            return create_run(self.root, specification, fixtures.CONTROLLER, self.options)

    def park(self, **options):
        """A gated, pre-verified and security-reviewed candidate waiting for its evidence."""
        directory = self.create(**options)
        record = self.execute(directory, fixtures.FakeCodex(script=True))["tasks"]["one"]
        self.assertEqual(record["status"], "awaiting_evidence")
        return directory, record

    def commit_evidence(self, record, files=None, *, deletions=(), executable=(), parent=None):
        """The evidence owner's commit on top of the candidate, in a clone of the attempt."""
        owner = self.root.parent / "owner"
        if not owner.exists():
            git(self.root.parent, "clone", "--quiet", "--", str(Path(record["directory"]) / "repo"), str(owner))
        git(owner, "switch", "--quiet", "--detach", parent or record["candidate"])
        for path, content in ({"docs/evidence/one/focus.png": FRAME, "docs/validation.md": ENTRY} if files is None else files).items():
            target = owner / path
            target.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(content, str):
                target.symlink_to(content)  # a str names a symlink's target
            else:
                target.write_bytes(content)
        for path in deletions:
            (owner / path).unlink()
        for path in executable:
            (owner / path).chmod(0o755)
        git(owner, "add", "-A")
        git(owner, "commit", "--quiet", "-m", "docs(evidence): record one")
        return owner, head(owner)

    def native(self, directory, record, kind="native", **evidence):
        report = self.root.parent / f"{kind}.md"
        report.write_text(f"Fixture {kind} report naming the committed frames.\n")
        return attest(directory, "one", record["candidate"], kind, report, f"Checked {kind}.", **evidence)

    def test_acceptance_fast_forwards_to_the_evidence_commit(self):
        directory, record = self.park()
        candidate, security = record["candidate"], record["security_review"]
        owner, evidence = self.commit_evidence(record)
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        # Only the final verifier ran; the security verdict from before the round was reused.
        self.assertEqual(adapter.calls, [("verifier", "one")])
        self.assertEqual((record["status"], record["attempts"], record["security_review"]), ("accepted", 1, security))
        self.assertEqual((state["accepted_head"], head(directory / "accepted")), (evidence, evidence))
        self.assertEqual(git(directory / "accepted", "rev-list", "--parents", "-n", "1", evidence).split(), [evidence, candidate])
        self.assertIn(f"one: accepted at {evidence[:12]}", self.output)
        # It graded the candidate and its evidence together, at the evidence commit.
        self.assertEqual(head(adapter.repos[0][1]), evidence)
        context = adapter.context_log[0][1]
        self.assertIn(f"Evidence commit: {evidence}", context)
        self.assertIn('["docs/evidence/one/focus.png", "docs/validation.md"]', context)
        self.assertIn("open each frame the entry names", context)
        self.assertEqual((record["review_inputs"]["evidence_commit"], record["review_inputs"]["evidence_gate_sha256"]),
                         (evidence, record["evidence_gate_sha256"]))
        self.assertEqual(record["attestations"]["native"]["evidence_commit"], evidence)
        self.assertNotIn("evidence_commit", record["pre_review_inputs"])
        self.assertNotIn("evidence_commit", record["security_review_inputs"])

    def test_a_task_without_an_evidence_commit_is_accepted_at_its_candidate(self):
        directory, record = self.park()
        self.native(directory, record)
        state = self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual(state["accepted_head"], record["candidate"])
        self.assertNotIn("evidence_commit", state["tasks"]["one"]["review_inputs"])

    def test_the_evidence_commit_must_be_the_candidates_only_child(self):
        directory, record = self.park()
        owner, child = self.commit_evidence(record)
        for parent in (record["base"], child):
            with self.subTest(parent=parent[:12]):
                _, evidence = self.commit_evidence(record, {"docs/evidence/one/other.png": FRAME}, parent=parent)
                with self.assertRaisesRegex(LoopError, "only parent"):
                    self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertNotIn("evidence_commit", read_json(directory / "state.json")["tasks"]["one"])

    def test_the_evidence_commit_adds_only_scoped_regular_evidence_files(self):
        directory, record = self.park(seed=["docs/validation.md", "docs/evidence/one/old.png"])
        cases = [
            ({"crates/app/src/lib.rs": b"pub fn built() {}\n"}, (), "outside the evidence paths"),
            ({"docs/evidence/two/focus.png": FRAME}, (), "outside the evidence paths"),
            ({"docs/benchmarks/2026-10-02-one.json": b"{}\n"}, (), "outside the evidence paths"),
            ({"docs/evidence/one/link.png": "focus.png", "docs/evidence/one/focus.png": FRAME}, (), "regular non-executable"),
            ({"docs/evidence/one/probe.py": b"print('ran')\n"}, (), r"a \.py file there"),
            ({}, ("docs/validation.md",), "may delete evidence files only"),
        ]
        for files, deletions, message in cases:
            with self.subTest(files=sorted(files), deletions=deletions):
                owner, evidence = self.commit_evidence(record, files, deletions=deletions)
                with self.assertRaisesRegex(LoopError, message):
                    self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        owner, evidence = self.commit_evidence(record, {}, executable=("docs/evidence/one/old.png",))
        with self.assertRaisesRegex(LoopError, "regular non-executable"):
            self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertNotIn("evidence_commit", read_json(directory / "state.json")["tasks"]["one"])
        # Replacing and deleting the task's own frames, and adding the dated entry, is evidence.
        _, evidence = self.commit_evidence(record, deletions=("docs/evidence/one/old.png",))
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertEqual(read_json(directory / "state.json")["tasks"]["one"]["evidence_commit"], evidence)

    def test_the_evidence_commit_holds_records_and_no_instructions(self):
        directory, record = self.park(scope=["docs/evidence/**", "docs/validation.md", "docs/benchmarks/**"])
        for path, message in (
            ("docs/evidence/one/CLAUDE.md", "instruction or control file"),
            ("docs/evidence/one/AGENTS.md", "instruction or control file"),
            ("docs/benchmarks/CLAUDE.local.md", "instruction or control file"),
            ("docs/benchmarks/2026-10-02-one/SKILL.md", "instruction or control file"),
            ("docs/evidence/one/.claude/settings.json", "hidden path component"),
            ("docs/benchmarks/.hidden/samples.csv", "hidden path component"),
            ("docs/evidence/one/notes.md", r"a \.md file there"),
            ("docs/evidence/one/samples.csv", r"a \.csv file there"),
            ("docs/evidence/one/frame.svg", r"a \.svg file there"),
            ("docs/evidence/one/README", "a suffix-less file there"),
        ):
            with self.subTest(path=path):
                owner, evidence = self.commit_evidence(record, {path: b"{}\n"})
                with self.assertRaisesRegex(LoopError, message):
                    self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        # Frames, plain records, and the benchmarks' prose, tables and logs are evidence.
        owner, evidence = self.commit_evidence(record, {
            "docs/evidence/one/focus.png": FRAME, "docs/evidence/one/tree.txt": b"AT-SPI tree\n",
            "docs/benchmarks/2026-10-02-one.md": b"# One\n", "docs/benchmarks/2026-10-02-one/samples.csv": b"ms\n4\n",
            "docs/benchmarks/2026-10-02-one/samples.tsv": b"ms\n4\n", "docs/benchmarks/2026-10-02-one/run.log": b"ok\n",
            "docs/benchmarks/2026-10-02-one/raw.jsonl": b"{}\n", "docs/validation.md": ENTRY,
        })
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertEqual(read_json(directory / "state.json")["tasks"]["one"]["evidence_commit"], evidence)

    def test_a_scope_that_grants_every_evidence_directory_may_retake_earlier_frames(self):
        directory, record = self.park(scope=["docs/evidence/**", "docs/validation.md", "docs/benchmarks/**"],
                                      seed=["docs/evidence/two/focus.png"])
        owner, evidence = self.commit_evidence(record, {
            "docs/evidence/two/focus.png": FRAME, "docs/benchmarks/2026-10-02-one.json": b"{}\n",
        }, deletions=())
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        state = self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual(state["accepted_head"], evidence)

    def test_a_red_docs_gate_keeps_the_task_waiting_without_spending_an_attempt(self):
        directory, record = self.park()
        owner, evidence = self.commit_evidence(record)
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter, red_on_evidence)
        waiting = state["tasks"]["one"]
        self.assertEqual(adapter.calls, [])
        self.assertEqual((waiting["status"], waiting["attempts"]), ("awaiting_evidence", 1))
        self.assertIn(f"evidence commit {evidence[:12]} failed its docs gate", waiting["reason"])
        self.assertIn("failed its docs gate", self.output)
        # Resuming changes nothing until a corrected commit replaces it.
        again = fixtures.FakeCodex(script=True)
        self.assertEqual(self.execute(directory, again, red_on_evidence)["tasks"]["one"]["attempts"], 1)
        self.assertEqual(again.calls, [])
        _, corrected = self.commit_evidence(record, {"docs/evidence/one/focus.png": FRAME + b" retaken", "docs/validation.md": ENTRY})
        self.native(directory, record, evidence_commit=corrected, evidence_repo=owner, replace=True)
        final = self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual((final["accepted_head"], final["tasks"]["one"]["attempts"]), (corrected, 1))

    def test_replacing_an_evidence_commit_with_itself_reruns_its_docs_gate(self):
        directory, record = self.park()
        owner, evidence = self.commit_evidence(record)
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertEqual(self.execute(directory, fixtures.FakeCodex(script=True), red_on_evidence)["tasks"]["one"]["status"],
                         "awaiting_evidence")
        # Same commit, no flag: nothing changes, so the red result stands.
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertIn("evidence_gate_sha256", read_json(directory / "state.json")["tasks"]["one"])
        # A flaky gate is retried by replacing the commit with itself; its attestations stay bound.
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner, replace=True)
        replaced = read_json(directory / "state.json")["tasks"]["one"]
        self.assertNotIn("evidence_gate_sha256", replaced)
        self.assertEqual(replaced["attestations"]["native"]["evidence_commit"], evidence)
        state = self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual((state["accepted_head"], state["tasks"]["one"]["attempts"]), (evidence, 1))

    def test_one_evidence_commit_per_candidate_and_a_replacement_unbinds_its_attestations(self):
        directory, record = self.park(profiles=("native", "performance", "package"))
        owner, first = self.commit_evidence(record)
        self.native(directory, record, evidence_commit=first, evidence_repo=owner)
        # Another kind may name the same commit or none.
        self.native(directory, record, "package")
        self.native(directory, record, "performance", evidence_commit=first, evidence_repo=owner)
        _, second = self.commit_evidence(record, {"docs/evidence/one/focus.png": FRAME + b" retaken", "docs/validation.md": ENTRY})
        with self.assertRaisesRegex(LoopError, "replace-evidence-commit"):
            self.native(directory, record, "performance", evidence_commit=second, evidence_repo=owner)
        self.native(directory, record, "performance", evidence_commit=second, evidence_repo=owner, replace=True)
        replaced = read_json(directory / "state.json")["tasks"]["one"]
        self.assertEqual(replaced["evidence_commit"], second)
        # The native attestation was bound to the old commit and must be registered again; package named none.
        self.assertEqual(set(replaced["attestations"]), {"package", "performance"})
        self.assertEqual(replaced["attestations"]["performance"]["evidence_commit"], second)
        with self.assertRaisesRegex(LoopError, "replace-evidence-commit"):
            self.native(directory, record, evidence_commit=first, evidence_repo=owner)
        self.native(directory, record, evidence_commit=second, evidence_repo=owner)
        state = self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual(state["accepted_head"], second)

    def test_an_interrupted_docs_gate_reruns_in_a_fresh_checkout_without_an_attempt(self):
        directory, record = self.park()
        owner, evidence = self.commit_evidence(record)
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)

        def dying_gate(repo, profiles, gate_directory):
            if Path(gate_directory).parent.name.startswith("evidence-"):
                raise LoopError("fixture controller died during the evidence gate")
            return {"passed": True, "checks": []}

        with self.assertRaisesRegex(LoopError, "died"):
            self.execute(directory, fixtures.FakeCodex(script=True), dying_gate)
        self.assertEqual(read_json(directory / "state.json")["phase"], "evidence_gating")
        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter)
        record = state["tasks"]["one"]
        self.assertEqual(adapter.calls, [("verifier", "one")])
        self.assertEqual((record["status"], record["attempts"], state["accepted_head"]), ("accepted", 1, evidence))
        self.assertEqual(len(list(Path(record["directory"]).glob("evidence-*"))), 2)

    def test_a_queued_attestation_carries_its_evidence_commit_to_ingestion(self):
        directory, record = self.park()
        owner, child = self.commit_evidence(record)
        _, stray = self.commit_evidence(record, {"docs/evidence/one/other.png": FRAME}, parent=record["base"])
        attempt = Path(record["directory"]) / "repo"
        with locked(directory):
            # Checked read-only where it lives before queuing, so a wrong commit is never queued.
            with self.assertRaisesRegex(LoopError, "only parent"):
                self.native(directory, record, evidence_commit=stray, evidence_repo=owner)
            self.assertFalse((directory / "inbox").exists())
            self.assertEqual(self.native(directory, record, evidence_commit=child, evidence_repo=owner), "queued")
        # Nothing reached the attempt clone while the controller held the run.
        self.assertEqual(git(attempt, "for-each-ref", "refs/gitturtle"), "")
        [name] = inbox.pending(directory)
        queued = inbox.read(directory, name)
        self.assertEqual((queued["evidence_commit"], queued["evidence_repo"], queued["replace_evidence_commit"]),
                         (child, str(owner), False))
        state = self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual((state["tasks"]["one"]["status"], state["accepted_head"]), ("accepted", child))
        self.assertIn(child, git(attempt, "for-each-ref", "--format=%(refname)", "refs/gitturtle"))

    def test_a_run_whose_controller_predates_evidence_commits_refuses_one(self):
        directory, record = self.park()
        state = read_json(directory / "state.json")
        state["controller_features"].remove("evidence_commit")
        atomic_json(directory / "state.json", state)
        owner, evidence = self.commit_evidence(record)
        with self.assertRaisesRegex(LoopError, "predates evidence commits"):
            self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        self.assertEqual(self.native(directory, record), "applied")

    def test_interrupted_acceptance_reconciles_to_the_evidence_commit(self):
        directory, record = self.park()
        owner, evidence = self.commit_evidence(record)
        self.native(directory, record, evidence_commit=evidence, evidence_repo=owner)
        real_git = git

        def uncertain_git(repo, *args, **options):
            result = real_git(repo, *args, **options)
            if args[0] == "merge":
                raise LoopError("simulated uncertain merge result")
            return result

        with patch("agent_loop.runner.git", side_effect=uncertain_git):
            with self.assertRaisesRegex(LoopError, "uncertain"):
                self.execute(directory, fixtures.FakeCodex(script=True))
        self.assertEqual((read_json(directory / "state.json")["phase"], head(directory / "accepted")), ("accepting", evidence))
        adapter = fixtures.FakeCodex(script=True)
        state = self.execute(directory, adapter)
        self.assertEqual(adapter.calls, [])
        self.assertEqual((state["tasks"]["one"]["status"], state["accepted_head"]), ("accepted", evidence))

    def test_evidence_cannot_ride_on_a_candidate_whose_build_reads_documentation(self):
        self.prepare()
        crate = self.root / "crates/probe"
        (crate / "src").mkdir(parents=True)
        (crate / "Cargo.toml").write_text('[package]\nname = "probe"\n')
        (crate / "src/lib.rs").write_text('pub const LOG: &str = include_str!("../../../docs/validation.md");\n')
        git(self.root, "add", "--", "crates")
        git(self.root, "commit", "--quiet", "-m", "test: build reads the validation log")
        candidate = head(self.root)
        (self.root / "docs/validation.md").parent.mkdir(exist_ok=True)
        (self.root / "docs/validation.md").write_bytes(ENTRY)
        git(self.root, "add", "--", "docs/validation.md")
        git(self.root, "commit", "--quiet", "-m", "docs(evidence): record probe")
        evidence = head(self.root)
        git(self.root, "switch", "--quiet", "--detach", candidate)
        contract = fixtures.evidence_task()
        contract["scope"] += EVIDENCE_SCOPE
        task = parse_spec({"version": 1, "tasks": [contract]})[0]
        with self.assertRaisesRegex(LoopError, "build reads evidence paths.*: crates/probe/src/lib.rs:1"):
            evidence_paths(self.root, task, candidate, evidence, protected=controlled)


class BuildInputTests(unittest.TestCase):
    def test_nothing_the_repository_builds_reads_what_an_evidence_commit_changes(self):
        # Why an evidence commit can be accepted without rebuilding: nothing it may change is a build input.
        self.assertEqual(build_references(fixtures.CONTROLLER), [])

    def found(self, sources):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for path, text in {"crates/app/Cargo.toml": "[package]\n", "vendor/kit/Cargo.toml": "[package]\n", **sources}.items():
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text(text)
            return build_references(root)

    def test_every_include_form_that_reads_the_evidence_paths_is_found(self):
        forms = {
            "plain": 'include_str!("../../../docs/validation.md")',
            "raw": 'include_str!(r"../../../docs/validation.md")',
            "raw with hashes": 'include_bytes!(r#"../../../docs/evidence/one/focus.png"#)',
            "brackets": 'include_str!["../../../docs/validation.md"]',
            "split over lines": 'include_bytes!(\n    "../../../docs/benchmarks/a.json",\n)',
            "manifest dir": 'include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/benchmarks/a.json"))',
            "concat alone": 'include_str!(concat!("../../../docs/", "evidence/one/a.txt"))',
            "the whole docs directory": 'include!(concat!("../../../", "docs"))',
            "unresolved but mentions docs": 'include_str!(concat!(env!("OUT_DIR"), "/docs.txt"))',
            "a constant that mentions docs": 'include_str!(PATH, "docs")',
        }
        for name, form in forms.items():
            with self.subTest(form=name):
                self.assertEqual(len(self.found({"crates/app/src/lib.rs": f"const X: &str = {form};\n"})), 1, form)

    def test_includes_that_read_no_evidence_path_are_not_reported(self):
        sources = {
            "crates/app/src/lib.rs": (
                # The form crates/app/src/appearance/sources.rs uses: a note no evidence commit may change.
                'const NOTE: &str = include_str!("../../../docs/development/note.md");\n'
                'const ICON: &[u8] = include_bytes!(r"../../../assets/icon.png");\n'
                'const GENERATED: &str = include!(concat!(env!("OUT_DIR"), "/generated.rs"));\n'
                # A quote in a character literal or a comment pairs with nothing.
                "const QUOTE: char = '\"'; // include_str!(\"../../../docs/validation.md\") was here\n"
                "fn lifetime<'a>(text: &'a str) -> &'a str { text }\n"
            ),
            "vendor/kit/src/lib.rs": 'const OWN: &str = include_str!("../docs/readme.md");\n',
        }
        self.assertEqual(self.found(sources), [])

    def test_embedded_folders_and_build_scripts_are_resolved_too(self):
        found = self.found({
            "crates/app/src/main.rs": '#[derive(RustEmbed)]\n#[folder = "../../docs/evidence/"]\nstruct Frames;\n',
            "crates/app/build.rs": 'fn main() {\n    println!("cargo:rerun-if-changed=../../docs");\n    let _ = r"docs/validation.md";\n}\n',
        })
        self.assertEqual([entry.split(" reads ")[0] for entry in found],
                         ["crates/app/build.rs:2", "crates/app/build.rs:3", "crates/app/src/main.rs:2"])


if __name__ == "__main__":
    unittest.main()
