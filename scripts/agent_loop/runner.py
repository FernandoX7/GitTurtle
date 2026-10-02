"""Durable, bounded feature acceptance over isolated local repositories."""

from __future__ import annotations

import argparse
from contextlib import contextmanager, nullcontext
from dataclasses import asdict
from datetime import datetime, timezone
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sys
import tempfile
import time
import uuid

from .claude import CLAUDE_EFFORTS, Claude, UsageLimited, resolve_selection, snapshot_files as claude_snapshot_files
from .codex import Codex, validate_review
from . import inbox, rebase
from .evidence import SHA, checkout_evidence, evidence_paths, fetch_evidence
from .git import changed_paths, clean, clone, commit, committed_paths, git, head, identity, source_root, untracked_paths, within, write_limits
from .process import EnvironmentBlocked, LoopError, MalformedResponse, atomic_json, digest, read_json, run_process, reconcile_processes
from .rust_surface import renders
from .task_spec import Task, evidence_criteria, load_spec, path_allowed, required_evidence, select_ready
from .security_review import candidate_requires_security, validate_security_review


CONTROLS = (".codex", ".agents", ".claude", "scripts/agent_loop", "scripts/operator", "scripts/agent-loop.py", "scripts/check-agent-guidance.py", "docs/development/tasks.json", "docs/development/task.schema.json", "docs/development/security-review.md")
# An IDE sweep writes byte copies of `.claude/` under these protected roots in
# every checkout a session ran in. Nothing a candidate may add lives there, so
# an untracked file under them is left out of the candidate and recorded with
# the attempt instead of failing it; tracked files there stay protected.
MIRRORS = (".codex", ".agents/skills")
# A squash merge's subject is the pull request title plus ` (#N)`; the ASCII
# digit class keeps other numerals from standing in for a PR number.
SQUASH_SUFFIX = re.compile(r"(.+) \(#([0-9]+)\)")
# What `git revert` writes, and what GitHub's Revert button leaves once its pull
# request is squash-merged with its title and description: the subject
# `Revert "<title>" (#M)` and the body line `Reverts <owner>/<repo>#N`.
REVERT_SUBJECT = re.compile(r'Revert "(.+)"')
REVERTS_COMMIT = re.compile(r"^This reverts commit ([0-9a-f]{40}|[0-9a-f]{64})\b", re.MULTILINE)
REVERTS_PULL = re.compile(r"^Reverts (?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#([0-9]+)\b", re.MULTILINE)
COORDINATOR_NOTES = "docs/development/HANDOFF.md"
# The operator's display and session bus; the native-QA gate also gets an empty
# private XDG_RUNTIME_DIR, so neither Wayland nor the default bus path resolves.
HEADLESS_UNSET = ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS")
EVIDENCE_KINDS = {"native", "performance", "package", "vendor"}
# The phases in which a reviewer session holds a gated candidate; an interruption
# there keeps the candidate and its gates, and resume reruns only that review.
REVIEW_PHASES = ("pre_verifying", "verifying", "security_reviewing")
# Everything bound to one candidate; a new attempt starts without any of it.
CANDIDATE_KEYS = (
    "candidate", "pre_review", "pre_review_sha256", "pre_review_inputs", "review", "review_sha256", "review_inputs",
    "security_review", "security_review_sha256", "security_review_inputs", "security_required", "security_paths",
    "attestations", "gate_sha256", "mirror_untracked", "evidence_commit", "evidence_directory", "evidence_gate_sha256",
    "pre_review_reviewed", "rebased_from", "recheck",
)
# The verdict a rebase carries to a replayed candidate whose patch is unchanged. The
# security review is never carried: it must see the candidate on its new base.
REUSABLE_REVIEWS = ("pre_review",)
# The phase in which the controller gates an evidence commit with the docs profile.
EVIDENCE_GATING = "evidence_gating"
# A gated candidate waiting on its evidence owner or on a review to finish.
PENDING = ("awaiting_evidence", "review_blocked")
# The phase in which the controller replays a pending candidate onto the accepted head.
REBASING = "rebasing"
# The phase in which the loop idles until evidence arrives; it charges no time.
WAITING = "waiting_for_evidence"
MAX_AWAITING_EVIDENCE = 3
MAX_IDLE_MINUTES = 240
# While waiting, the inbox is read this often and the stop flag every tick.
IDLE_POLL = 30.0
IDLE_TICK = 1.0
EFFORTS = ("low", "medium", "high", "xhigh", "max", "ultra")
TOOLS = ("codex", "claude")
CLAUDE_OPTIONS = (
    "retry_effort", "hard_model", "light_model", "hard_effort", "light_effort", "review_effort",
    "max_turns", "review_max_turns", "sandbox",
)
EVIDENCE_BYTES = 32 * 1024 * 1024
# Coordinator notes: guidance for a task's next attempt, kept in its run record
# and never in the contract snapshot, so they cannot change scope or acceptance.
NOTE_BYTES = 8 * 1024
MAX_NOTES = 32
IMPLEMENTER_NOTES = (
    "Coordinator notes (guidance from the run's coordinator; they never change the contract, its scope "
    "or its acceptance criteria; if a note conflicts with the contract, follow the contract and say so in your summary)"
)
REVIEW_NOTES = (
    "Coordinator notes (context only, not acceptance criteria: the implementer received them as guidance; "
    "grade the candidate against the contract alone)"
)
QUEUED = (
    "a controller holds this run, so the request is queued in its inbox; the controller applies it "
    "before its next step, or the next resume does (see status)"
)
# What a run's pinned controller can do, recorded when the run is created. The
# operator's commands come from the live checkout, so they refuse a request an
# older run's controller would never read instead of reporting success.
CONTROLLER_FEATURES = ("inbox", "notes", "verify_before_evidence", "evidence_commit", "rebase")
# Another `note` or `attest` holds the lock for moments, a running loop for
# hours: wait this long for a free lock before treating it as a running loop.
LOCK_WAIT = 3.0
LOCK_POLL = 0.1


class RejectedVerdict(LoopError):
    """A reviewer's result that is readable but contradicts itself or its candidate; it fails the attempt."""


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def controlled(path: str) -> bool:
    return Path(path).name in {"AGENTS.md", "CLAUDE.md"} or any(path == item or path.startswith(item + "/") for item in CONTROLS)


def mirror_untracked(repo: Path) -> list[str]:
    return [path for path in untracked_paths(repo) if within(path, MIRRORS)]


def note_mirror(repo: Path, record: dict) -> None:
    if mirrored := mirror_untracked(repo):
        record["mirror_untracked"] = sorted(set(record.get("mirror_untracked", ())) | set(mirrored))


@contextmanager
def headless(environment: dict[str, str]):
    """Withhold the operator's display and session bus for one gate command.

    This removes the inherited variables only. A test that names a display
    itself, as native-QA's launch default of `:1` would, can still reach it.
    """
    for variable in HEADLESS_UNSET:
        environment.pop(variable, None)
    with tempfile.TemporaryDirectory(prefix="gitturtle-runtime-") as runtime:
        environment["XDG_RUNTIME_DIR"] = runtime
        yield


def checkout_clean(repo: Path, record: dict | None = None) -> bool:
    """Clean apart from a mirror sweep, which `record` keeps instead of failing."""
    if record is not None:
        note_mirror(repo, record)
    return clean(repo, ignore_untracked=MIRRORS)


def validate_patch(repo: Path, task: Task, base: str) -> list[str]:
    if head(repo) != base:
        raise LoopError("worker changed HEAD; the controller alone owns commits")
    if git(repo, "diff", "--cached", "--name-only", "-z"):
        raise LoopError("worker changed the index; the controller alone owns staging")
    paths = changed_paths(repo, ignore_untracked=MIRRORS)
    if not paths:
        raise LoopError("worker produced no patch; split verification-only work from implementation tasks")
    original_entries = git(repo, "ls-tree", "-z", base, "--", *paths).split("\0")
    if any(entry.startswith(("120000 ", "160000 ")) for entry in original_entries):
        raise LoopError("patch changes a stored symlink or submodule; use an interactive review")
    for path in paths:
        if not path_allowed(path, task.scope) or controlled(path):
            raise LoopError(f"patch changed a protected or out-of-scope path: {path}")
        if any(part.is_symlink() for part in [repo / path, *(repo / path).parents] if part != repo.parent):
            raise LoopError(f"patch contains a symlink: {path}; handle it through an interactive review")
    return paths


def profiles_for(task: Task, paths: list[str], sources=None) -> set[str]:
    """Infer the profiles a candidate touching `paths` must satisfy.

    `sources` maps a path to its (before, after) text and lets app Rust that adds
    no view, layout or rendering code skip the native profile. Without it every
    app Rust change keeps that profile, so a caller that cannot read the two
    revisions errs toward demanding the evidence.
    """
    profiles = set(task.profiles) | {"docs"}
    if any(path.endswith(".rs") or Path(path).name in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"} for path in paths):
        profiles.add("rust")
    if any(path.startswith(("scripts/", ".github/")) for path in paths):
        profiles.add("tooling")
    if any(path.startswith("vendor/") for path in paths):
        profiles.update({"vendor", "rust"})
    if any(path.startswith("vendor/gpui") for path in paths):
        profiles.add("native")
    app_rust = [path for path in paths if path.startswith("crates/app/") and path.endswith(".rs")]
    if app_rust and (sources is None or any(renders(*sources(path)) for path in app_rust)):
        profiles.add("native")
    if any(
        path.startswith("assets/") or path in {
            "scripts/package-macos.sh", "scripts/package-macos.py", "scripts/package-identity.py",
            "scripts/package-linux.sh", "scripts/install-linux.py", "scripts/render-app-icon.sh",
            "scripts/collect-third-party-licenses.py",
        } or (
            path.startswith("scripts/release/") and Path(path).suffix in {".py", ".sh"}
            and not Path(path).name.startswith(("test_", "test-")) and "tests" not in Path(path).parts
        ) for path in paths
    ):
        profiles.add("package")
    return profiles


def gate_context(path: Path, label: str = "Gate") -> str:
    """The gate result itself, not only where it lives.

    A review session runs with prompts disabled and a command allowlist, so a
    path outside its checkout is unreadable to it: pointing at the evidence left
    every gate-backed criterion unverified. The outcome is small, so it travels
    in the prompt; the path stays for identification.
    """
    summary = f"{label} evidence: {path}"
    try:
        report = read_json(path)
    except LoopError:
        return summary + " (unreadable)"
    checks = [
        {key: check.get(key) for key in ("name", "returncode", "elapsed", "stopped")}
        for check in report.get("checks", []) if isinstance(check, dict)
    ]
    return summary + f"\n{label} result: " + json.dumps({"passed": report.get("passed"), "checks": checks})


def revision_sources(repo: Path, base: str, candidate: str):
    """Read a path at both revisions; an absent path reads as empty text."""
    def read(revision: str, path: str) -> str:
        if not git(repo, "ls-tree", "-z", revision, "--", path):
            return ""
        return git(repo, "show", f"{revision}:{path}")

    return lambda path: (read(base, path), read(candidate, path))


def evidence_gated(task: Task, record: dict) -> dict[str, list[str]]:
    """Each criterion that waits for evidence this candidate requires, with those kinds.

    A criterion naming a kind the candidate does not require has nothing to wait
    for, so it is graded with the others before the evidence round.
    """
    required = set(record.get("required_evidence", ()))
    return {
        identifier: sorted(kinds & required)
        for identifier, kinds in evidence_criteria(task).items() if kinds & required
    }


def review_inputs(record: dict, key: str) -> dict:
    """What a stored verdict is bound to; it is reused only while these are unchanged.

    The pre-evidence and security reviews need no external evidence, so neither
    binds the attestations or the final review: both stay valid across `attest`
    and the final verification of the same candidate.
    """
    inputs = {"base": record["base"], "candidate": record["candidate"], "gate_sha256": record["gate_sha256"]}
    if key == "review":
        inputs["attestations"] = record.get("attestations", {})
        # Only the final review sees an evidence commit, so only it is bound to one.
        if "evidence_commit" in record:
            inputs.update(evidence_commit=record["evidence_commit"], evidence_gate_sha256=record.get("evidence_gate_sha256"))
    elif key == "security_review":
        inputs["paths"] = record["security_paths"]
    return inputs


def checked_gate(path: Path, expected: str) -> dict:
    """A stored gate report whose bytes and command logs are unchanged; it may record a failure."""
    if path.is_symlink() or not path.is_file() or digest(path) != expected:
        raise LoopError("gate evidence changed")
    report = read_json(path)
    for check in report.get("checks", []):
        log = Path(check["log"])
        if log.is_symlink() or not log.is_file() or digest(log) != check["sha256"]:
            raise LoopError("gate command log changed")
    return report


def accepted_target(record: dict) -> str:
    """The commit acceptance fast-forwards to: the evidence commit on top of the candidate, if any."""
    return record.get("evidence_commit") or record["candidate"]


def evidence_context(record: dict, paths: list[str], gates: Path) -> str:
    """What the final verifier needs to grade the candidate and its evidence commit together."""
    return (
        f"Evidence commit: {record['evidence_commit']}. This checkout is at it, and its only parent is the candidate: "
        "base..candidate is the code under review, and candidate..evidence adds only these evidence files (data): "
        + json.dumps(paths) + ". Grade the evidence-gated criteria from the committed frames and records, the dated "
        "validation entry and the attestations: open each frame the entry names with the Read tool, which shows "
        "images, and confirm it shows what the entry says. A frame you cannot open, or one that contradicts the "
        "entry, leaves its criterion unverified or failing.\n" + gate_context(gates, "Evidence commit docs gate")
    )


def reviewed_identity(record: dict, key: str) -> tuple[str, str]:
    """The base and candidate the stored verdict under `key` names.

    A verdict a rebase carried over names the candidate its reviewer saw, whose
    patch the record's candidate replays unchanged; any other names the record's own.
    """
    seen = record.get(key + "_reviewed") or {}
    return seen.get("base", record["base"]), seen.get("candidate", record["candidate"])


def rebased_reason(record: dict, onto: str, dropped: str) -> str:
    """What a rebased candidate waits for: a re-check of its replayed evidence commit, or the evidence itself."""
    if recheck := record.get("recheck"):
        return (f"rebased onto {onto}; re-check the committed frames with `qa.py recheck` against "
                f"{recheck['evidence_commit']} and attest with --evidence-commit {recheck['evidence_commit']} "
                f"--evidence-repo {recheck['evidence_repo']}")
    return f"rebased onto {onto}; " + (f"{dropped}; " if dropped else "") + record["reason"]


def review_validator(task: Task, record: dict, key: str, identity: tuple[str, str] | None = None):
    """Validate a verdict on the record's candidate, or on `identity` for a stored one."""
    base, candidate = identity or (record["base"], record["candidate"])
    if key == "security_review":
        return lambda value: validate_security_review(value, task, base, candidate, record["security_paths"])
    deferred = frozenset(evidence_gated(task, record)) if key == "pre_review" else frozenset()
    return lambda value: validate_review(value, task, candidate, deferred)


def saved_review(task: Task, record: dict, key: str) -> bool:
    """Whether the verdict stored under `key` passed and still binds the candidate's inputs."""
    if key not in record:
        return False
    path = Path(record[key])
    if path.is_symlink() or not path.is_file() or digest(path) != record.get(key + "_sha256"):
        raise LoopError(f"{key} evidence changed")
    verdict = review_validator(task, record, key, reviewed_identity(record, key))(read_json(path))
    return verdict == "pass" and record.get(key + "_inputs") == review_inputs(record, key)


def checked_verdict(validator, value: dict) -> str:
    """Validate a reviewer's result: an unreadable shape stays retryable, a contradiction is rejected."""
    try:
        return validator(value)
    except MalformedResponse:
        raise
    except LoopError as error:
        raise RejectedVerdict(str(error)) from error


def pre_evidence_passed(task: Task, record: dict) -> bool:
    """Whether the candidate passed every review that needs no external evidence."""
    if not saved_review(task, record, "pre_review"):
        return False
    return not record.get("security_required", True) or saved_review(task, record, "security_review")


def evidence_round_open(task: Task, record: dict) -> bool:
    """Whether the candidate may receive its external evidence now.

    A contract that marks a criterion as waiting for evidence gets the reviews
    that need none first. One that marks none, as the contracts queued before
    pre-evidence verification do, keeps the earlier order (evidence, then one
    verification), because a pre-evidence verifier would block on its unmarked
    evidence criteria and every resume would pay for another such session.
    """
    return not evidence_gated(task, record) or pre_evidence_passed(task, record)


def verification_mode(gated: dict[str, list[str]], *, pre_evidence: bool, earlier: dict | None = None,
                      rebased: str | None = None) -> str:
    """Tell the verifier which pass it runs and which criteria wait for external evidence."""
    waiting = json.dumps(gated, sort_keys=True)
    if pre_evidence:
        return (
            "Verification mode: pre-evidence. The gates passed and no native, performance, package or vendor "
            "attestation exists yet; this review decides whether the candidate earns an evidence round. "
            f"Evidence-gated criteria (data: criterion id to the evidence kinds it waits for): {waiting}. "
            "Report each of them unverified, naming the evidence it waits for, or pass only when the repository "
            "alone already proves it; their missing evidence is no reason to fail or block. Grade every other "
            "criterion fully now: pass only when each of them passes with no findings, fail when one fails. If one "
            "of them cannot be settled without external evidence, return blocked and name it: the contract should "
            "have marked it evidence-gated."
        )
    text = "Verification mode: final. Grade every criterion against the candidate, the gate result and the external evidence above"
    text += f"; these evidence-gated criteria rest on that evidence (data): {waiting}." if gated else "."
    if earlier is not None:
        text += (
            "\nPre-evidence verdict on this same candidate and gates (data, not authoritative; confirm each "
            "criterion, repeating its checks only where the evidence or a concrete concern bears on them): "
            + json.dumps({key: earlier.get(key) for key in ("verdict", "criteria", "notes")})
        )
        if rebased:
            text += (f"\nThat verdict graded {rebased}, which the controller replayed onto the current base as this "
                     "candidate with the same patch; the gates above ran on this candidate.")
    return text


def make_adapter(controller: Path, settings: dict):
    """Build the session adapter a run recorded; saved runs without a tool use Codex."""
    tool = settings.get("tool") or "codex"
    if tool == "claude":
        return Claude(
            controller, settings["model"], settings["effort"],
            # create_run resolved these; a run saved by another controller resumes with its own snapshot.
            **{key: settings.get(key) for key in ("retry_effort", "hard_effort", "light_effort", "review_effort", "hard_model", "light_model")},
            max_turns=settings.get("max_turns") or 200,
            review_max_turns=settings.get("review_max_turns") or 120, sandbox=settings.get("sandbox") or "auto",
            settings_sha256=settings.get("claude_settings_sha256"),
        )
    if tool != "codex":
        raise LoopError(f"unknown session tool {tool!r}")
    return Codex(controller, settings["model"], settings["effort"])


@contextmanager
def locked(directory: Path, *, busy_ok: bool = False):
    """Hold the run lock; with `busy_ok`, yield False instead of failing while another holder has it."""
    lock_path = directory / "lock"
    if lock_path.is_symlink():
        raise LoopError("invalid run lock")
    with lock_path.open("a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            held = True
        except BlockingIOError as error:
            if not busy_ok:
                raise LoopError("another controller is using this run") from error
            held = False
        try:
            yield held
        finally:
            if held:
                fcntl.flock(lock, fcntl.LOCK_UN)


def notes_context(record: dict, heading: str) -> str:
    """The task's coordinator notes as data under `heading`, each with its UTC time."""
    notes = [{"at": note["at"], "text": note["text"]} for note in record.get("notes", ())]
    return heading + ":\n" + json.dumps(notes, indent=2, ensure_ascii=False) if notes else ""


def state_at(directory: Path) -> dict:
    directory = directory.resolve()
    state = read_json(directory / "state.json")
    if state.get("version") != 1 or state.get("run") != str(directory):
        raise LoopError("run state identity does not match this directory")
    if digest(directory / "tasks.json") != state.get("spec_sha256"):
        raise LoopError("task snapshot changed; create a new run for a revised contract")
    for relative, expected in state.get("controller_files", {}).items():
        path = directory / "controller" / relative
        if path.is_symlink() or not path.is_file() or digest(path) != expected:
            raise LoopError(f"controller snapshot changed: {relative}")
    return state


class Runner:
    def __init__(self, directory: Path, *, adapter=None, gate_runner=None):
        self.directory = directory.resolve()
        self.state = state_at(self.directory)
        self.tasks = load_spec(self.directory / "tasks.json")
        self.controller = self.directory / "controller"
        live_controller = Path(__file__).resolve().parents[2]
        for relative, expected in self.state["controller_files"].items():
            path = live_controller / relative
            if not path.is_file() or digest(path) != expected:
                raise LoopError(f"controller version changed; resume using {self.controller / 'scripts/agent-loop.py'}")
        self.repo = self.directory / "accepted"
        self.adapter = adapter or make_adapter(self.controller, self.state)
        if isinstance(self.adapter, Claude):
            # Its result retry is a second session inside one adapter call, so
            # it asks the budget this runner checks between sessions first.
            self.adapter.budget_stop = self.budget_stop
        self.usage_limited: str | None = None
        self.gate_runner = gate_runner
        self.started = time.monotonic()
        # Waiting for evidence spends no model or gate time, so it is not charged.
        self.idle_seconds = 0.0
        self.idle_since: float | None = None
        # The idle wait's clock and sleep; tests replace them to wait without waiting.
        self.idle_clock = time.monotonic
        self.idle_sleep = time.sleep
        self.initial_seconds = self.state["remaining_seconds"]
        self.initial_tokens = self.state.get("output_tokens", 0)
        if self.state.get("budget_running"):
            reconcile_processes(self.directory)
            # Charge downtime after an abrupt controller exit conservatively; an
            # explicit renewed time allowance is required if that exhausts it.
            elapsed = (datetime.now(timezone.utc) - datetime.fromisoformat(self.state["updated_at"])).total_seconds()
            self.initial_seconds = max(0, self.initial_seconds - max(0, elapsed))
            self.recover_usage()
        self.interrupted = False
        self.reported_phase: tuple[str, str | None] | None = None
        self.reported_status: dict[str, tuple[str, int, str]] = {}

    def recover_usage(self) -> None:
        reported = 0
        incomplete = False
        recover = getattr(self.adapter, "recover_usage", None)
        if callable(recover):
            reported, incomplete = recover(self.directory / "attempts")
            self.initial_tokens = max(self.initial_tokens, reported)
            self.state["output_usage_incomplete"] = self.state.get("output_usage_incomplete", False) or incomplete
            return
        for log in (self.directory / "attempts").glob("**/*.jsonl"):
            if log.name not in {"implementer.jsonl", "verifier.jsonl", "security-reviewer.jsonl"}:
                continue
            completed = False
            if log.is_symlink() or log.stat().st_size > 32 * 1024 * 1024:
                raise LoopError("invalid session usage log")
            for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                if isinstance(event, dict) and event.get("type") == "turn.completed":
                    completed = True
                    usage = event.get("usage", {})
                    count = usage.get("output_tokens") if isinstance(usage, dict) else None
                    if type(count) is int and count >= 0:
                        reported += count
                    else:
                        incomplete = True
            incomplete = incomplete or not completed
        self.initial_tokens = max(self.initial_tokens, reported)
        self.state["output_usage_incomplete"] = self.state.get("output_usage_incomplete", False) or incomplete

    def elapsed(self) -> float:
        """Seconds this controller charged to the run's time budget: all but its idle waits."""
        current = time.monotonic()
        idle = self.idle_seconds + (current - self.idle_since if self.idle_since is not None else 0.0)
        return current - self.started - idle

    def save(self) -> None:
        self.state["updated_at"] = now()
        self.state["remaining_seconds"] = max(0, self.initial_seconds - self.elapsed())
        self.state["output_tokens"] = self.initial_tokens + self.adapter.output_tokens
        self.state["output_usage_incomplete"] = self.state.get("output_usage_incomplete", False) or getattr(self.adapter, "output_usage_incomplete", False)
        atomic_json(self.directory / "state.json", self.state)
        self.report()

    def report(self) -> None:
        """Print each phase and task-status change once, so a watched run shows where it is."""
        stamp = self.state["updated_at"][11:19] + "Z"
        active = (self.state.get("active") or {}).get("task")
        if (self.state["phase"], active) != self.reported_phase:
            self.reported_phase = (self.state["phase"], active)
            if active:
                print(f"{stamp} {self.state['phase']} {active} attempt {self.state['tasks'][active]['attempts']}", flush=True)
            elif self.state["phase"] not in {"idle", WAITING}:  # a wait prints its own start and end
                print(f"{stamp} {self.state['phase']}", flush=True)
        if active:
            return  # a step's intermediate statuses are reported by its outcome
        for task_id, record in self.state["tasks"].items():
            reason = " ".join(str(record.get("reason", "")).split())[:240]
            # The reason counts: a refused evidence commit keeps the task awaiting evidence.
            outcome = (record["status"], record["attempts"], reason)
            if self.reported_status.get(task_id) == outcome:
                continue
            if record["status"] == "awaiting_evidence" and not self.missing_evidence(record) and not self.evidence_refused(record):
                continue  # reviewed and about to be accepted; only waiting on an owner is news
            self.reported_status[task_id] = outcome
            where = f" at {accepted_target(record)[:12]}" if record["status"] == "accepted" and record.get("candidate") else ""
            print(f"{stamp} {task_id}: {record['status']}{where}" + (f" ({reason})" if reason else ""), flush=True)

    def stop_requested(self) -> bool:
        return self.interrupted or (self.directory / "STOP").exists()

    def timeout(self) -> float:
        return min(self.state["session_minutes"] * 60, max(0, self.initial_seconds - self.elapsed()))

    def budget_stop(self) -> str | None:
        if self.stop_requested():
            return "stop requested"
        if self.usage_limited:
            return self.usage_limited
        if self.timeout() <= 0:
            return "time budget exhausted"
        cap = self.state.get("max_output_tokens")
        if cap is not None and (self.state.get("output_usage_incomplete") or getattr(self.adapter, "output_usage_incomplete", False)):
            return "output usage is incomplete; explicitly renew --max-output-tokens on resume"
        if cap is not None and self.initial_tokens + self.adapter.output_tokens >= cap:
            return "output-token budget exhausted"
        return None

    def gates(self, repo: Path, profiles: set[str], directory: Path) -> dict:
        directory.mkdir(parents=True, exist_ok=True)
        if self.gate_runner is not None:
            report = self.gate_runner(repo, profiles, directory)
            atomic_json(directory / "gates.json", report)
            return report
        commands = [("guidance", [sys.executable, "-B", str(self.controller / "scripts/check-agent-guidance.py"), "--root", str(repo)])]
        if "tooling" in profiles:
            commands.append(("tooling", [sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/agent_loop", "-t", "scripts", "-p", "test_*.py"]))
            commands.append(("native-qa-tooling", [sys.executable, "-B", "-m", "unittest", "discover", "-s", "scripts/native_qa", "-t", "scripts", "-p", "test_*.py"]))
        if profiles & {"rust", "native", "performance", "package", "vendor"}:
            commands.extend([
                ("format", ["cargo", "fmt", "--all", "--", "--check"]),
                ("check", ["cargo", "check", "--locked", "-p", "gitturtle"]),
                ("workspace-tests", ["cargo", "test", "--locked", "--workspace"]),
                ("clippy", ["cargo", "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"]),
            ])
        if profiles & {"performance", "package"}:
            commands.append(("release", ["cargo", "build", "--release", "--locked", "-p", "gitturtle"]))
        results = []
        for name, argv in commands:
            if self.budget_stop():
                raise EnvironmentBlocked(self.budget_stop())
            environment = os.environ.copy()
            environment["PYTHONDONTWRITEBYTECODE"] = "1"
            # Candidate code runs here: mark it as the controller's own, so the
            # operator's commands refuse it as they refuse a session.
            environment["GITTURTLE_LOOP"] = "1"
            environment["CARGO_TARGET_DIR"] = str(self.directory / "build")
            environment["CARGO_TERM_COLOR"] = "never"
            with headless(environment) if name == "native-qa-tooling" else nullcontext():
                result = run_process(argv, repo, directory / f"{name}.log", self.timeout(), env=environment, stop=self.stop_requested)
            entry = asdict(result) | {"name": name, "log": str(directory / f"{name}.log"), "sha256": digest(directory / f"{name}.log")}
            results.append(entry)
            report = {"passed": not result.stopped and result.returncode == 0, "checks": results}
            atomic_json(directory / "gates.json", report)
            print(f"{now()[11:19]}Z   {name} gate {'passed' if report['passed'] else 'failed'} in {result.elapsed:.0f}s", flush=True)
            if result.stopped:
                raise EnvironmentBlocked(f"{name} gate interrupted: {result.stopped}")
            if not report["passed"]:
                return report
        return {"passed": True, "checks": results}

    def reconcile(self) -> None:
        reconcile_processes(self.directory)
        actual = head(self.repo)
        if not checkout_clean(self.repo):
            raise LoopError("accepted checkout has unexpected changes; preserve and inspect it")
        for record in self.state["tasks"].values():
            # A blocked review holds the candidate it waits on. No path here
            # leaves one without (a run resumes only under the controller that
            # created it), but such a record, from a defect or a hand edit, has
            # nothing to review and would crash the selection loop: count it as
            # a failed attempt instead.
            if record["status"] == "review_blocked" and not record.get("candidate"):
                record["status"] = "failed"
        phase = self.state["phase"]
        active = self.state.get("active")
        if phase == "accepting" and active:
            task_id = active["task"]
            record = self.state["tasks"][task_id]
            if actual == accepted_target(record):
                self.finish_acceptance(task_id, record)
                self.ingest()
                return
            if actual != self.state["accepted_head"]:
                raise LoopError("accepted checkout moved during interrupted acceptance")
            # Evidence was persisted before the Git transition. Revalidate before retry.
            record["status"] = "awaiting_evidence"
            self.state["active"] = None
        elif actual != self.state["accepted_head"]:
            raise LoopError("accepted checkout HEAD differs from the journal")
        if active and phase != "accepting":
            record = self.state["tasks"][active["task"]]
            if phase in REVIEW_PHASES and record.get("candidate") and record.get("gate_sha256"):
                self.validate_candidate(record)
                record.update(status="review_blocked", reason="independent review interrupted; candidate and gates retained")
            elif phase == EVIDENCE_GATING and record.get("candidate") and record.get("gate_sha256"):
                # Only the evidence commit's own gate was running; it reruns in a fresh checkout.
                self.validate_candidate(record)
                record.update(status="awaiting_evidence", reason="evidence commit gate interrupted; resume reruns it")
            elif phase == REBASING and record.get("candidate") and record.get("gate_sha256"):
                # The replay works in a clone of its own and the record adopts it
                # in one save, so the record holds either the old candidate, which
                # the next step replays again, or the adopted one with its reviews
                # pending; either keeps its status.
                self.validate_candidate(record)
                if record["base"] != self.state["accepted_head"]:
                    record["reason"] = "rebase interrupted; the next step replays the candidate again"
            else:
                record["status"] = "interrupted"
                record["reason"] = f"interrupted during {phase}; preserved attempt, retry from accepted source"
            self.state["active"] = None
        self.state["phase"] = "idle"
        self.save()
        self.ingest()

    def ingest(self) -> set[str]:
        """Apply or refuse the requests queued while this controller held the run lock.

        Runs under the lock at safe points: after reconciliation and before the
        loop selects each next task, never during a step. Each request passes
        the same rules as when it applies directly, in name order, and moves to
        `inbox/done/` with its outcome; a refusal never stops the loop. Returns
        the tasks that received evidence, so the loop reconsiders them.
        """
        attested = set()
        for name in inbox.pending(self.directory):
            request = None
            changed = False
            try:
                request = inbox.read(self.directory, name)
                evidence = sha256 = None
                if request.get("kind") == "attest":
                    sha256 = request.get("sha256")
                    if "artifact" not in request or not isinstance(sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", sha256):
                        raise LoopError("queued attestation carries no recorded evidence copy")
                    evidence = inbox.artifact(self.directory, request)
                changed = apply_request(self.directory, self.state, request, evidence, sha256)
                if request.get("kind") == "attest":
                    attested.add(request["task"])
                outcome = "applied"
            except LoopError as error:
                outcome = f"refused: {error}"
            if changed:
                # Outside the refusal path: a failed write propagates and leaves
                # the request pending, and the next ingestion applies it once.
                self.save()
            inbox.finish(self.directory, name, request, outcome)
            subject = f"{request.get('kind')} for {request.get('task')}" if request else name
            print(f"{now()[11:19]}Z inbox {subject}: {outcome}", flush=True)
        return attested

    def execute(self) -> dict:
        with write_limits(self.timeout, self.stop_requested):
            return self._execute()

    def _execute(self) -> dict:
        self.reconcile()
        self.state["budget_running"] = True
        self.save()
        if reason := self.budget_stop():
            self.state.update(phase="paused", reason=reason, active=None, budget_running=False)
            self.save()
            return self.state
        self.state["tool_version"] = self.adapter.preflight()
        if (self.state.get("tool") or "codex") == "codex":
            self.state["codex_version"] = self.state["tool_version"]
        if not self.state.get("baseline_passed"):
            self.state["phase"] = "preflight"
            self.save()
            profiles = {profile for task in self.tasks for profile in task.profiles}
            baseline = self.directory / "baseline" / uuid.uuid4().hex
            report = self.gates(self.repo, profiles, baseline)
            self.state["baseline_evidence"] = str(baseline / "gates.json")
            if not report["passed"] or not checkout_clean(self.repo):
                failed = next((check for check in report["checks"] if check.get("returncode") != 0), None)
                if report["passed"]:
                    reason = "baseline gates left the accepted checkout with changes"
                elif failed:
                    reason = f"baseline {failed['name']} gate failed; inspect {failed['log']}"
                else:
                    reason = f"baseline gates failed; inspect {baseline / 'gates.json'}"
                self.state.update(phase="baseline_failed", reason=reason, budget_running=False)
                self.save()
                return self.state
            self.state["baseline_passed"] = True
            self.save()
        visited: set[str] = set()
        # Tasks whose rebase a stop or the time budget interrupted; a later run replays them.
        deferred: set[str] = set()
        while True:
            # Evidence that reached the inbox reopens its parked task in this run.
            visited -= self.ingest()
            reason = self.budget_stop()
            if reason:
                self.state.update(phase="paused", reason=reason)
                break
            accepted = {key for key, value in self.state["tasks"].items() if value["status"] == "accepted"}
            # --max-tasks bounds work accepted by this run; tasks that landed before it only satisfy dependencies.
            accepted_here = len([key for key in accepted if "landed" not in self.state["tasks"][key]])
            if accepted_here >= self.state["max_tasks"]:
                self.state.update(phase="complete" if len(accepted) == len(self.tasks) else "paused", reason="accepted-task limit reached")
                break
            # An acceptance moved the head: replay each pending candidate onto it, one per step, before other work.
            if stale := self.stale_candidate(deferred):
                task, record = stale
                if not self.rebase_candidate(task, record):
                    deferred.add(task.id)
                elif record["status"] == "review_blocked":
                    visited.add(task.id)  # its review just ran and was blocked; resume reruns it
                else:
                    # A rebased candidate's next step follows from its status: an owner's
                    # evidence, its final review, or a normal attempt while attempts remain.
                    visited.discard(task.id)
                continue
            blocked = visited | deferred
            for task_id, record in self.state["tasks"].items():
                if record["attempts"] >= self.state["max_attempts"] and record["status"] not in PENDING:
                    blocked.add(task_id)
            ready = select_ready(self.tasks, accepted, blocked)
            # Evidence that arrived and reviews left to finish come before new work.
            task = next((item for item in ready if self.state["tasks"][item.id]["status"] in PENDING), None)
            if task is None:
                waiting = self.awaiting_owner()
                full = self.no_room(accepted_here, waiting)
                if ready and full is None:
                    task = ready[0]
                elif waiting and self.idle_limit() > 0:
                    reopened, ended = self.wait_for_evidence(waiting, full or "no other eligible work")
                    visited -= reopened
                    if reopened or ended is None:  # a stop is reported by the budget check
                        continue
                    self.state.update(phase="paused", reason=ended)
                    break
                elif ready:
                    # Room is gone and --max-idle-minutes 0 never waits, or no candidate awaits an owner.
                    self.state.update(phase="paused", reason=f"{full}; attest or finish the pending candidates, then resume")
                    break
                else:
                    self.state.update(phase="complete" if len(accepted) == len(self.tasks) else "blocked", reason="no eligible tasks remain")
                    break
            record = self.state["tasks"][task.id]
            if record["status"] in PENDING:
                missing = self.missing_evidence(record)
                # Waiting on its owner: evidence not yet attested, or an evidence
                # commit whose docs gate failed and needs replacing.
                if (missing and evidence_round_open(task, record)) or (not missing and self.evidence_refused(record)):
                    visited.add(task.id)
                    continue
                try:
                    # Until its evidence arrives, a candidate only finishes the reviews that need none.
                    (self.pre_evidence if missing else self.review_and_accept)(task, record)
                except (EnvironmentBlocked, MalformedResponse) as error:
                    if self.state["phase"] == "accepting":
                        raise
                    self.note_limit(error)
                    # A stopped evidence gate reruns as it was; a stopped review is retried.
                    status = "awaiting_evidence" if self.state["phase"] == EVIDENCE_GATING else "review_blocked"
                    record.update(status=status, reason=str(error))
                except RejectedVerdict as error:
                    # As on an attempt's first pass: a verdict that contradicts itself
                    # fails the attempt instead of stopping every resume at it.
                    record.update(status="failed", reason=str(error))
                self.state.update(phase="idle", active=None)
                self.save()
                if record["status"] in PENDING:
                    visited.add(task.id)
                continue
            self.attempt(task, record)
            if record["status"] in {"blocked", "awaiting_evidence", "review_blocked"}:
                visited.add(task.id)
        self.state["active"] = None
        self.state["budget_running"] = False
        self.save()
        return self.state

    def attempt(self, task: Task, record: dict) -> None:
        previous = {key: record[key] for key in ("reason", "directory") if key in record}
        record["attempts"] += 1
        record.update(status="building", base=self.state["accepted_head"])
        for key in CANDIDATE_KEYS:
            record.pop(key, None)
        directory = self.directory / "attempts" / task.id / str(record["attempts"])
        record["directory"] = str(directory)
        self.state.update(phase="building", active={"task": task.id})
        self.save()
        repo = directory / "repo"
        configure = getattr(self.adapter, "configure_attempt", None)
        if callable(configure):
            record["session"] = configure(task, record["attempts"])
            self.save()
        try:
            clone(self.repo, repo, record["base"], tuple(self.state["author"]), owner=self.directory)
            context = "Previous attempt evidence (read-only): " + json.dumps(previous)
            if notes := notes_context(record, IMPLEMENTER_NOTES):
                context += "\n\n" + notes
            result = self.adapter.run("implementer", task, repo, directory, self.timeout(), self.stop_requested,
                                      context=context, spec_path=self.state["spec_path"])
            if self.budget_stop():
                raise LoopError(self.budget_stop())
            if result["status"] == "blocked":
                record.update(status="blocked", reason=result["summary"])
                return
            paths = validate_patch(repo, task, record["base"])
            note_mirror(repo, record)
            if self.state["spec_path"] in paths:
                raise LoopError("worker changed the task contract")
            record["candidate"] = commit(repo, paths, task.commit, f"{task.description}\n\nTask: {task.id}")
            committed = committed_paths(repo, record["base"], record["candidate"])
            if set(committed) != set(paths) or any(controlled(path) or path == self.state["spec_path"] or not path_allowed(path, task.scope) for path in committed):
                raise LoopError("commit hooks or filters changed the reviewed patch scope")
            # Keep the patch and Git object even when gates/review fail.
            patch = git(repo, "diff", "--binary", "--no-ext-diff", "--no-textconv", record["base"], record["candidate"])
            (directory / "candidate.patch").write_bytes(patch.encode("utf-8", "surrogateescape"))
            profiles = profiles_for(task, paths, revision_sources(repo, record["base"], record["candidate"]))
            record["profiles"] = sorted(profiles)
            record["required_evidence"] = sorted(required_evidence(task) | (profiles & EVIDENCE_KINDS))
            self.state["phase"] = "gating"
            self.save()
            gates = self.gates(repo, profiles, directory / "checks")
            record["gate_sha256"] = digest(directory / "checks/gates.json")
            if not gates["passed"]:
                record.update(status="failed", reason="required gate failed; inspect checks/gates.json")
                return
            if head(repo) != record["candidate"] or not checkout_clean(repo, record):
                raise LoopError("gate changed candidate source or HEAD")
            if missing := self.missing_evidence(record):
                if evidence_gated(task, record):
                    self.pre_evidence(task, record)
                else:
                    # No criterion is marked as waiting for evidence: keep the earlier order.
                    record.update(status="awaiting_evidence", reason="required external evidence: " + ", ".join(missing))
                return
            record["status"] = "awaiting_evidence"
            self.review_and_accept(task, record)
        except EnvironmentBlocked as error:
            if self.state["phase"] == "accepting":
                raise
            self.note_limit(error)
            if self.state["phase"] in REVIEW_PHASES:
                status = "review_blocked"
            else:
                status = "interrupted" if isinstance(error, UsageLimited) else "blocked"
            record.update(status=status, reason=str(error))
        except MalformedResponse as error:
            if self.state["phase"] == "accepting":
                raise
            # The candidate passed its gates; only the verdict was unreadable.
            # Retry the review on it instead of spending an attempt rebuilding it.
            # Before a candidate exists the implementer's own result was
            # unreadable, which spends the attempt like any unusable one.
            record.update(status="review_blocked" if record.get("candidate") else "failed", reason=str(error))
        except LoopError as error:
            if self.state["phase"] == "accepting":
                raise  # Git may have moved; preserve intent for reconciliation.
            record.update(status="interrupted" if self.budget_stop() else "failed", reason=str(error))
        finally:
            if self.state["phase"] != "accepting":
                self.state.update(phase="idle", active=None)
            self.save()

    def note_limit(self, error: Exception) -> None:
        # A usage-limit refusal affects every later session; pause the run instead
        # of consuming an attempt per task.
        if isinstance(error, UsageLimited) and not self.usage_limited:
            self.usage_limited = str(error)

    def missing_evidence(self, record: dict) -> list[str]:
        supplied = record.get("attestations", {})
        missing = []
        for kind in record["required_evidence"]:
            proof = supplied.get(kind)
            if not proof or proof["candidate"] != record["candidate"]:
                missing.append(kind)
                continue
            artifact = self.directory / proof["artifact"]
            if artifact.is_symlink() or not artifact.is_file() or digest(artifact) != proof["sha256"]:
                raise LoopError(f"attested {kind} evidence changed")
        return missing

    def route_security(self, record: dict) -> None:
        # Git's no-renames diff includes both endpoints and deleted paths. Never
        # trust an implementer's declared profiles to waive security review.
        repo = Path(record["directory"]) / "repo"
        paths = committed_paths(repo, record["base"], record["candidate"])
        record.update(security_required=candidate_requires_security(repo, record["base"], record["candidate"], paths), security_paths=paths)

    def pre_evidence(self, task: Task, record: dict) -> None:
        """Run the reviews that need no external evidence, then park the candidate for it.

        The verifier grades every criterion that waits for no evidence and the
        security review follows, so a failure spends the attempt before anyone
        gathers native, performance, package or vendor evidence. Only a candidate
        that passes both is parked `awaiting_evidence`; a review that could not
        finish leaves it `review_blocked` for resume to rerun just that review.
        """
        if record["base"] != self.state["accepted_head"]:
            raise LoopError("candidate base is stale")
        self.validate_candidate(record)
        missing = self.missing_evidence(record)
        self.route_security(record)
        if not saved_review(task, record, "pre_review"):
            if reason := self.budget_stop():
                record.update(status="review_blocked", reason=f"pre-evidence verification not run: {reason}")
                return
            context = (gate_context(Path(record["directory"]) / "checks/gates.json") + "\n"
                       + verification_mode(evidence_gated(task, record), pre_evidence=True))
            if notes := notes_context(record, REVIEW_NOTES):
                context += "\n" + notes
            if self.verify(task, record, "pre_review", context)["verdict"] != "pass":
                return
        if record["security_required"] and not saved_review(task, record, "security_review"):
            if reason := self.budget_stop():
                record.update(status="review_blocked", reason=f"pre-evidence security review not run: {reason}")
                return
            if not self.review_security(task, record, "pre_review"):
                return
        record.update(status="awaiting_evidence", reason="required external evidence: " + ", ".join(missing))

    def review_and_accept(self, task: Task, record: dict) -> None:
        directory = Path(record["directory"])
        repo = directory / "repo"
        candidate = record["candidate"]
        if record["base"] != self.state["accepted_head"]:
            raise LoopError("candidate base is stale")
        self.validate_candidate(record)
        if self.missing_evidence(record):
            record["status"] = "awaiting_evidence"
            return
        self.route_security(record)
        if self.budget_stop():
            record["status"] = "awaiting_evidence"
            return
        # An evidence commit is gated before the final verification grades it with the candidate.
        evidence_files = None
        if "evidence_commit" in record and (evidence_files := self.evidence_stage(task, record)) is None:
            return
        if not saved_review(task, record, "review"):
            # The final verification; with evidence required, the pre-evidence verdict is its context.
            earlier = read_json(Path(record["pre_review"])) if saved_review(task, record, "pre_review") else None
            graded = reviewed_identity(record, "pre_review")[1] if earlier is not None else candidate
            context = (gate_context(directory / "checks/gates.json")
                       + "\nExternal evidence: " + json.dumps(record.get("attestations", {})) + "\n"
                       + verification_mode(evidence_gated(task, record), pre_evidence=False, earlier=earlier,
                                           rebased=graded if graded != candidate else None))
            if evidence_files is not None:
                context += "\n" + evidence_context(record, evidence_files, Path(record["evidence_directory"]) / "checks/gates.json")
            if notes := notes_context(record, REVIEW_NOTES):
                context += "\n" + notes
            if self.verify(task, record, "review", context)["verdict"] != "pass":
                return
        if self.budget_stop():
            record["status"] = "awaiting_evidence"
            self.save()
            return
        # A security verdict from before the evidence round is reused while its inputs hold.
        if record["security_required"] and not saved_review(task, record, "security_review"):
            if not self.review_security(task, record, "review"):
                return
        if self.budget_stop():
            record["status"] = "awaiting_evidence"
            self.save()
            return
        # Accept the evidence commit, which carries the candidate, when there is one.
        target, source = candidate, repo
        if "evidence_commit" in record:
            evidence_paths(repo, task, candidate, record["evidence_commit"], protected=controlled)
            target, source = record["evidence_commit"], self.evidence_checkout(record)
        # Persist the acceptance intent before touching the private accepted ref.
        self.state.update(phase="accepting", active={"task": task.id})
        record["status"] = "accepting"
        self.save()
        git(self.repo, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head", str(source), target)
        git(self.repo, "merge", "--quiet", "--ff-only", target)
        self.finish_acceptance(task.id, record)

    def verify(self, task: Task, record: dict, key: str, context: str) -> dict:
        """Run one verifier session and store its validated verdict under `key`.

        The final verification of a candidate with an evidence commit runs in the
        gated evidence checkout, so it reads the committed frames beside the code.
        """
        directory = Path(record["directory"])
        repo, at = directory / "repo", record["candidate"]
        if key == "review" and "evidence_commit" in record:
            repo, at = self.evidence_checkout(record), record["evidence_commit"]
        pre = key == "pre_review"
        review_dir = directory / (("pre-review-" if pre else "review-") + uuid.uuid4().hex[:10])
        self.state.update(phase="pre_verifying" if pre else "verifying", active={"task": task.id})
        self.save()
        value = self.adapter.run(
            "verifier", task, repo, review_dir, self.timeout(), self.stop_requested,
            candidate=record["candidate"], context=context,
        )
        verdict = checked_verdict(review_validator(task, record, key), value)
        if head(repo) != at or not checkout_clean(repo, record):
            raise LoopError("verifier changed candidate source or HEAD")
        self.validate_candidate(record)
        if at != record["candidate"]:
            self.evidence_checkout(record)
        self.store_review(record, key, review_dir, value, None if verdict == "pass" else {
            "status": "review_blocked" if verdict == "blocked" else "failed", "reason": json.dumps(value["findings"]),
        })
        return value

    def review_security(self, task: Task, record: dict, general: str) -> bool:
        """Run the security review the changed paths route to; False when it did not pass.

        `general` names the passing verifier verdict it follows: the pre-evidence
        one for a candidate that still needs evidence, else the final one.
        """
        directory = Path(record["directory"])
        repo = directory / "repo"
        review_dir = directory / ("security-review-" + uuid.uuid4().hex[:10])
        self.state.update(phase="security_reviewing", active={"task": task.id})
        self.save()
        # It needs no external evidence, so none is offered: its verdict binds
        # only the base, candidate, gates and paths, and outlives an attestation.
        context = (gate_context(directory / "checks/gates.json")
                   + "\nChanged paths (data): " + json.dumps(record["security_paths"])
                   + "\nGeneral review evidence (not authoritative): " + record[general])
        if notes := notes_context(record, REVIEW_NOTES):
            context += "\n" + notes
        value = self.adapter.run(
            "security-reviewer", task, repo, review_dir, self.timeout(), self.stop_requested,
            candidate=record["candidate"], base=record["base"], context=context,
        )
        verdict = checked_verdict(review_validator(task, record, "security_review"), value)
        if head(repo) != record["candidate"] or not checkout_clean(repo, record):
            raise LoopError("security reviewer changed candidate source or HEAD")
        self.validate_candidate(record)
        # Security review cannot invalidate and silently replace the already
        # passing general review, its gates, or attestations.
        if not saved_review(task, record, general) or (general == "review" and self.missing_evidence(record)):
            raise LoopError("acceptance evidence changed during security review")
        self.store_review(record, "security_review", review_dir, value, None if verdict == "pass" else {
            "status": "review_blocked" if verdict == "blocked" else "failed",
            "reason": json.dumps({"findings": value["findings"], "gaps": value["gaps"]}),
        })
        return verdict == "pass"

    def store_review(self, record: dict, key: str, directory: Path, value: dict, outcome: dict | None) -> None:
        """Persist a validated verdict; `outcome` is the task's status when it did not pass.

        A passing verdict keeps the step active until its caller decides what
        follows, so no progress line shows the candidate between two reviews. A
        verdict that ends the step is saved with its status and the idle phase
        in one write, so a crash cannot reconcile a failure back into a retry.
        """
        atomic_json(directory / "verdict.json", value)
        record[key] = str(directory / "verdict.json")
        record[key + "_sha256"] = digest(Path(record[key]))
        record[key + "_inputs"] = review_inputs(record, key)
        record.pop(key + "_reviewed", None)  # a fresh verdict names this candidate itself
        if outcome is not None:
            record.update(outcome)
            self.state.update(phase="idle", active=None)
        self.save()

    def finish_acceptance(self, task_id: str, record: dict) -> None:
        target = accepted_target(record)
        if head(self.repo) != target or not checkout_clean(self.repo):
            raise LoopError("accepted checkout changed during acceptance")
        self.validate_candidate(record)
        task = next(task for task in self.tasks if task.id == task_id)
        if target != record["candidate"]:
            # The accepted checkout now holds both; recheck what the evidence commit may change.
            evidence_paths(self.repo, task, record["candidate"], target, protected=controlled)
            self.evidence_checkout(record)
            if self.evidence_refused(record):
                raise LoopError("acceptance requires an evidence commit whose docs gate passed")
        if not saved_review(task, record, "review"):
            raise LoopError("acceptance requires a passing independent review")
        repo = Path(record["directory"]) / "repo"
        paths = committed_paths(repo, record["base"], record["candidate"])
        needs_security = candidate_requires_security(repo, record["base"], record["candidate"], paths)
        if record.get("security_required") != needs_security or record.get("security_paths") != paths:
            raise LoopError("security review routing changed during acceptance")
        if needs_security and not saved_review(task, record, "security_review"):
            raise LoopError("acceptance requires a passing independent security review")
        if self.missing_evidence(record):
            raise LoopError("acceptance requires all external evidence")
        record.update(status="accepted", accepted_at=now(), reason="all required evidence accepted")
        self.state.update(accepted_head=target, phase="idle", active=None)
        self.save()

    def validate_candidate(self, record: dict) -> None:
        directory = Path(record["directory"])
        repo = directory / "repo"
        if head(repo) != record["candidate"] or not checkout_clean(repo, record):
            raise LoopError("candidate source or HEAD changed")
        if checked_gate(directory / "checks/gates.json", record["gate_sha256"]).get("passed") is not True:
            raise LoopError("required gates did not pass")

    def evidence_stage(self, task: Task, record: dict) -> list[str] | None:
        """Check and gate the evidence commit; return its paths, or None when its gate refused it.

        The docs gate runs here, in the loop under its lock, in a fresh private
        clone of the attempt at the evidence commit; an interrupted gate leaves
        no half-made checkout behind as evidence. A red gate keeps the task
        awaiting a corrected evidence commit and spends no attempt.
        """
        evidence = record["evidence_commit"]
        attempt = Path(record["directory"]) / "repo"
        paths = evidence_paths(attempt, task, record["candidate"], evidence, protected=controlled)
        if "evidence_gate_sha256" not in record:
            directory = Path(record["directory"]) / ("evidence-" + uuid.uuid4().hex[:10])
            self.state.update(phase=EVIDENCE_GATING, active={"task": task.id})
            self.save()
            checkout_evidence(attempt, directory / "repo", task.id, record["candidate"], evidence,
                              tuple(self.state["author"]), owner=self.directory)
            self.gates(directory / "repo", {"docs"}, directory / "checks")
            record.update(evidence_directory=str(directory), evidence_gate_sha256=digest(directory / "checks/gates.json"))
            self.save()
        self.evidence_checkout(record)
        if self.evidence_refused(record):
            gates = Path(record["evidence_directory"]) / "checks/gates.json"
            record.update(status="awaiting_evidence", reason=f"evidence commit {evidence[:12]} failed its docs gate; "
                          f"attest a corrected one with --replace-evidence-commit (inspect {gates})")
            return None
        return paths

    def evidence_checkout(self, record: dict) -> Path:
        """The gated checkout of the evidence commit, unchanged since its gate ran."""
        repo = Path(record["evidence_directory"]) / "repo"
        if head(repo) != record["evidence_commit"] or not checkout_clean(repo):
            raise LoopError("evidence checkout source or HEAD changed")
        checked_gate(Path(record["evidence_directory"]) / "checks/gates.json", record["evidence_gate_sha256"])
        return repo

    def evidence_refused(self, record: dict) -> bool:
        """Whether the evidence commit's docs gate ran and failed, so only a replacement can proceed."""
        if "evidence_commit" not in record or "evidence_gate_sha256" not in record:
            return False
        gates = Path(record["evidence_directory"]) / "checks/gates.json"
        return checked_gate(gates, record["evidence_gate_sha256"]).get("passed") is not True

    def stale_candidate(self, deferred: set[str]) -> tuple[Task, dict] | None:
        """The first pending candidate, in contract order, whose base is no longer the accepted head."""
        for task in self.tasks:
            record = self.state["tasks"][task.id]
            if (task.id not in deferred and record["status"] in PENDING and record.get("candidate")
                    and record["base"] != self.state["accepted_head"]):
                return task, record
        return None

    def awaiting_owner(self) -> list[str]:
        """Parked tasks waiting for their evidence owner: unattested evidence or a refused evidence commit.

        A candidate on an older base waits for its rebase, not for its owner.
        """
        return [
            task.id for task in self.tasks
            if (record := self.state["tasks"][task.id])["status"] == "awaiting_evidence"
            and record.get("base") == self.state["accepted_head"]
            and (self.missing_evidence(record) or self.evidence_refused(record))
        ]

    def no_room(self, accepted_here: int, waiting: list[str]) -> str | None:
        """Why no new attempt may start while candidates are pending, or None when one may.

        Up to --max-awaiting-evidence candidates wait for evidence while the loop
        implements others. A new attempt also waits while the tasks this run
        accepted and the pending candidates already reach --max-tasks, since its
        candidate could not be accepted in this run.
        """
        cap = self.state.get("max_awaiting_evidence", MAX_AWAITING_EVIDENCE)
        if len(waiting) >= cap:
            return f"--max-awaiting-evidence {cap} reached"
        pending = sum(1 for record in self.state["tasks"].values() if record["status"] in PENDING and record.get("candidate"))
        if accepted_here + pending >= self.state["max_tasks"]:
            return f"accepted tasks and pending candidates reach --max-tasks {self.state['max_tasks']}"
        return None

    def idle_limit(self) -> float:
        """Seconds one wait for evidence may last; 0 never waits."""
        return float(self.state.get("max_idle_minutes", MAX_IDLE_MINUTES)) * 60

    def wait_for_evidence(self, waiting: list[str], why: str) -> tuple[set[str], str | None]:
        """Idle until an attestation reopens a parked task, a stop, or the idle limit.

        Returns the reopened tasks, and with none the reason to pause, or None
        after a stop, which the loop's budget check reports. Nothing runs while
        waiting, so the wait is not charged to --max-minutes, and the budget is
        recorded as idle, so a controller that dies here is charged no downtime.
        --max-idle-minutes bounds one continuous wait.
        """
        limit = self.idle_limit()
        minutes = f"{limit / 60:g}"
        print(f"{now()[11:19]}Z waiting for evidence on {', '.join(waiting)} ({why}); idling at most {minutes} minutes", flush=True)
        self.state.update(phase=WAITING, budget_running=False)
        self.save()
        self.idle_since = time.monotonic()
        start = polled = self.idle_clock()
        reopened: set[str] = set()
        ended = "stop requested"
        try:
            while not reopened:
                if self.stop_requested():
                    break
                current = self.idle_clock()
                if current - start >= limit:
                    ended = f"waited {minutes} minutes for evidence on {', '.join(waiting)} and none arrived; attest, then resume"
                    break
                if current - polled >= IDLE_POLL:
                    polled = current
                    reopened = self.ingest()
                    continue
                self.idle_sleep(max(0.0, min(IDLE_TICK, polled + IDLE_POLL - current, start + limit - current)))
        finally:
            self.idle_seconds += time.monotonic() - self.idle_since
            self.idle_since = None
            self.state.update(phase="idle", budget_running=True)
        outcome = f"evidence arrived for {', '.join(sorted(reopened))}" if reopened else ended.split(";")[0]
        print(f"{now()[11:19]}Z wait ended after {(self.idle_clock() - start) / 60:.0f} minutes: {outcome}", flush=True)
        self.save()
        return reopened, None if reopened or ended == "stop requested" else ended

    def rebase_candidate(self, task: Task, record: dict) -> bool:
        """Replay a pending candidate onto the accepted head without an implementer session.

        A clean replay with green gates keeps the attempt count, and its status
        then follows from what it still needs (`settle_rebase`). Anything else
        that stops the replay, a conflict, a refused patch, a red gate, a gate
        or Git limit, an old candidate that fails its own checks, or a failure
        after adoption, leaves it `stale` with the reason, for a normal attempt.
        Returns False only when a stop or the run's budget interrupted the
        replay, which leaves the record as it was for a later run.
        """
        onto = self.state["accepted_head"]
        self.state.update(phase=REBASING, active={"task": task.id})
        self.save()
        try:
            self.validate_candidate(record)
            replayed = self.replay_candidate(task, record, onto)
        except EnvironmentBlocked as error:
            self.note_limit(error)
            if self.budget_stop():
                record["reason"] = f"rebase onto {onto[:12]} interrupted: {error}; a later run replays the candidate again"
                self.state.update(phase="idle", active=None)
                self.save()
                return False
            replayed = str(error)  # a gate deadline or Git limit, not the run's budget
        except LoopError as error:
            replayed = str(error)
        failure = replayed if isinstance(replayed, str) else None
        if failure is None:
            try:
                self.adopt_rebase(task, record, onto, replayed)
                self.settle_rebase(task, record, onto, replayed["dropped"])
            except (EnvironmentBlocked, MalformedResponse) as error:
                self.note_limit(error)
                record.update(status="review_blocked", reason=str(error))
            except RejectedVerdict as error:
                record.update(status="failed", reason=str(error))
            except LoopError as error:
                failure = str(error)
        if failure is not None:
            record.update(status="stale", reason=f"rebase onto {onto[:12]} failed: {failure}; a new attempt rebuilds the task")
        self.state.update(phase="idle", active=None)
        self.save()
        return True

    def settle_rebase(self, task: Task, record: dict, onto: str, dropped: str) -> None:
        """Give an adopted candidate the status of what it still needs, as a new candidate's gates do.

        Evidence-gated criteria get the reviews that need no evidence first:
        the verifier's when it did not carry over, and always the security
        review. A candidate that needs only evidence is parked with the
        re-check or evidence reason; one that needs none goes to its review.
        """
        missing = self.missing_evidence(record)
        if not missing:
            record.update(status="awaiting_evidence", reason=f"rebased onto {onto}; its final review follows")
            return
        if evidence_gated(task, record):
            self.pre_evidence(task, record)
        else:
            # No criterion is marked as waiting for evidence: keep the earlier order, as an attempt does.
            record.update(status="awaiting_evidence", reason="required external evidence: " + ", ".join(missing))
        if record["status"] == "awaiting_evidence":
            record["reason"] = rebased_reason(record, onto, dropped)

    def replay_candidate(self, task: Task, record: dict, onto: str) -> dict | str:
        """Replay the candidate, and its evidence commit, onto `onto` in a fresh clone and gate it there.

        Returns what the record adopts, or why it cannot. The checks applied to
        a new candidate apply to the replayed one, and those applied to an
        attested evidence commit to the replayed one. An evidence commit that
        conflicts only in the validation log is replayed by union
        (`rebase.replay_evidence`); one that still conflicts, or fails those
        checks, is dropped and its evidence asked for again. The old record and
        its clone never change here.
        """
        evidence = record.get("evidence_commit")
        directory = self.directory / "attempts" / task.id / str(record["attempts"]) / ("rebase-" + uuid.uuid4().hex[:10])
        repo = directory / "repo"
        rebase.prepare(Path(record["directory"]) / "repo", repo, self.repo, onto, record["candidate"], evidence,
                       task.id, tuple(self.state["author"]), owner=self.directory)
        candidate, why = rebase.replay(repo, record["candidate"], owner=self.directory)
        if candidate is None:
            return why
        paths = committed_paths(repo, onto, candidate)
        for path in paths:
            if controlled(path) or path == self.state["spec_path"] or not path_allowed(path, task.scope):
                return f"the replayed patch changes a protected or out-of-scope path: {path}"
        if links := rebase.link_paths(repo, onto, candidate):
            return f"the replayed patch changes a stored symlink or submodule: {links[0]}"
        replayed, dropped = None, ""
        if evidence:
            replayed, why = rebase.replay_evidence(repo, record["candidate"], evidence, owner=self.directory)
            if replayed is not None:
                # One ref per evidence commit, as for an attested one, before HEAD leaves it.
                fetch_evidence(repo, repo, task.id, replayed)
                git(repo, "switch", "--quiet", "--detach", candidate, owner=self.directory)
                try:
                    evidence_paths(repo, task, candidate, replayed, protected=controlled)
                except LoopError as error:
                    replayed, why = None, str(error)
            if replayed is None:
                dropped = f"its evidence commit {evidence[:12]} was dropped: {why}"
        if head(repo) != candidate or not checkout_clean(repo):
            raise LoopError("rebased candidate checkout changed")
        patch = git(repo, "diff", "--binary", "--no-ext-diff", "--no-textconv", onto, candidate)
        (directory / "candidate.patch").write_bytes(patch.encode("utf-8", "surrogateescape"))
        # The recorded profiles stay; a changed patch may add some, never drop one.
        profiles = set(record.get("profiles", ())) | profiles_for(task, paths, revision_sources(repo, onto, candidate))
        report = self.gates(repo, profiles, directory / "checks")
        if not report["passed"]:
            return f"its gates failed on the new base; inspect {directory / 'checks/gates.json'}"
        if head(repo) != candidate or not checkout_clean(repo):
            raise LoopError("gate changed the rebased candidate source or HEAD")
        return {
            "directory": str(directory), "candidate": candidate, "evidence_commit": replayed, "dropped": dropped,
            "gate_sha256": digest(directory / "checks/gates.json"), "profiles": sorted(profiles),
            "same_patch": rebase.fingerprint(repo, record["base"], record["candidate"]) == rebase.fingerprint(repo, onto, candidate),
        }

    def adopt_rebase(self, task: Task, record: dict, onto: str, replayed: dict) -> None:
        """Bind the record to the replayed candidate in one save.

        Every attestation and the final review named the old candidate, so they
        go, and so does the security review, which must see the candidate on
        its new base. A passing pre-evidence verdict carries over only when the
        patch is unchanged: rebound to the new inputs, it still names the
        candidate its reviewer saw. Until its reviews finish the candidate is
        `review_blocked`, so an interruption never leaves it `awaiting_evidence`
        before they passed.
        """
        entry = {"at": now(), "onto": onto, "base": record["base"], "candidate": record["candidate"],
                 "directory": record["directory"], "patch": "unchanged" if replayed["same_patch"] else "changed"}
        if "evidence_commit" in record:
            entry["evidence_commit"] = record["evidence_commit"]
        carried = {}
        if replayed["same_patch"]:
            for key in REUSABLE_REVIEWS:
                if saved_review(task, record, key):
                    base, candidate = reviewed_identity(record, key)
                    carried[key] = {key: record[key], key + "_sha256": record[key + "_sha256"],
                                    key + "_reviewed": {"base": base, "candidate": candidate}}
        entry["reused"] = sorted(carried)
        history = [*record.get("rebased_from", ()), entry]
        for key in CANDIDATE_KEYS:
            record.pop(key, None)
        record.update(
            base=onto, candidate=replayed["candidate"], directory=replayed["directory"], gate_sha256=replayed["gate_sha256"],
            profiles=replayed["profiles"], rebased_from=history, status="review_blocked",
            required_evidence=sorted(set(record["required_evidence"]) | required_evidence(task) | (set(replayed["profiles"]) & EVIDENCE_KINDS)),
            reason=f"rebased onto {onto}; its reviews rerun on the rebased candidate",
        )
        if replayed["evidence_commit"]:
            record["evidence_commit"] = replayed["evidence_commit"]
            # The frames are committed; the owner re-checks them on the rebased build instead of retaking them.
            record["recheck"] = {"evidence_commit": replayed["evidence_commit"], "frames_from": entry["evidence_commit"],
                                 "evidence_repo": str(Path(replayed["directory"]) / "repo")}
        self.route_security(record)
        for key, values in carried.items():
            record.update(values)
            record[key + "_inputs"] = review_inputs(record, key)
        self.save()


def revert_target(subject: str) -> str | None:
    """The subject a ``Revert "…"`` commit names, with or without a squash suffix."""
    squashed = SQUASH_SUFFIX.fullmatch(subject)
    for text in (subject, squashed[1] if squashed else ""):
        named = REVERT_SUBJECT.fullmatch(text)
        if named:
            return named[1]
    return None


def landed_tasks(root: Path, base: str, tasks: list[Task], queue: str = "") -> dict[str, str]:
    """Map each task already committed on the source branch to its newest commit.

    The queue is status-free, so a queue continued in a fresh run would otherwise
    redo accepted work. A task counts as landed only when a single-parent commit
    reachable from ``base`` carries its exact subject, changes at least one path
    inside its scope and no other path, is still in effect at ``base``, and every
    dependency landed too.

    Pull requests land by squash merge, which appends `` (#N)`` to the subject
    and folds in the coordinator's own bookkeeping, so the subject may carry that
    one suffix and the commit may also touch the handoff notes and ``queue``, the
    run's queue file relative to ``root``. Those two paths are set aside before
    the scope check, so they can neither land a task alone nor refuse one.

    A later commit reverts an earlier one when its body says ``This reverts
    commit <sha>``, or ``Reverts <owner>/<repo>#N`` or ``Reverts #N`` for the
    commit whose subject ends in `` (#N)``, or when its subject is ``Revert
    "<subject>"``; either subject may carry a squash suffix, since GitHub titles
    the revert of pull request #N after that request's title, which lacks it. A
    reverted commit is out of effect unless its revert was reverted in turn.
    """
    commits = [record.split("\x1f", 2) for record in git(
        root, "log", "-z", "--no-merges", "--topo-order", "--format=%H%x1f%s%x1f%b", base,
    ).split("\0") if record]
    newest: dict[str, str] = {}
    for sha, subject, _ in commits:
        newest.setdefault(subject, sha)
        squashed = SQUASH_SUFFIX.fullmatch(subject)
        if squashed:
            newest.setdefault(squashed[1], sha)
    # Oldest first, so a revert only ever names a commit that precedes it; a
    # body naming a later or unknown commit is ignored, which rules out cycles.
    reverters: dict[str, list[str]] = {}
    latest: dict[str, str] = {}
    pulls: dict[str, str] = {}
    seen: set[str] = set()
    for sha, subject, body in reversed(commits):
        targets = set(REVERTS_COMMIT.findall(body)) & seen
        targets.update(pulls[number] for number in REVERTS_PULL.findall(body) if number in pulls)
        named = revert_target(subject)
        if named in latest:
            targets.add(latest[named])
        for target in targets:
            reverters.setdefault(target, []).append(sha)
        latest[subject] = sha
        squashed = SQUASH_SUFFIX.fullmatch(subject)
        if squashed:
            latest[squashed[1]] = sha
            pulls[squashed[2]] = sha
        seen.add(sha)
    in_effect: dict[str, bool] = {}
    for sha, _, _ in commits:  # newest first: every reverter is already decided
        in_effect[sha] = not any(in_effect[reverter] for reverter in reverters.get(sha, ()))
    bookkeeping = {COORDINATOR_NOTES, queue} - {""}
    candidates = {}
    for task in tasks:
        sha = newest.get(task.commit)
        if not sha or not in_effect[sha]:
            continue
        paths = list(filter(None, git(root, "diff-tree", "--no-commit-id", "--root", "-r", "--name-only", "-z", "--no-renames", sha).split("\0")))
        work = [path for path in paths if path not in bookkeeping]
        if work and all(path_allowed(path, task.scope) for path in work):
            candidates[task.id] = sha
    landed: dict[str, str] = {}
    changed = True
    while changed:
        changed = False
        for task in tasks:
            if task.id in candidates and task.id not in landed and set(task.depends_on) <= landed.keys():
                landed[task.id] = candidates[task.id]
                changed = True
    return landed


def create_run(repo: Path, spec_path: Path, controller: Path, options: dict) -> Path:
    root = source_root(repo)
    spec_path = spec_path.resolve()
    if not spec_path.is_relative_to(root) or spec_path.is_symlink():
        raise LoopError("task specification must be inside the source checkout")
    relative_spec = spec_path.relative_to(root).as_posix()
    git(root, "ls-files", "--error-unmatch", "--", relative_spec)
    tasks = load_spec(spec_path)
    if not tasks:
        raise LoopError("task queue is empty; define and commit an authorized milestone first")
    author = identity(root)
    tool = options.get("tool") or "codex"
    if tool not in TOOLS:
        raise LoopError(f"unknown session tool {tool!r}")
    if tool == "claude":
        # Resolve every step once, so the saved run names each session's model and effort.
        options = options | resolve_selection(options["model"], options["effort"], options)
    adapter = make_adapter(controller, options | {"tool": tool})
    adapter.preflight()
    state_parent = root / ".local" / "agent-loop"
    if (root / ".local").is_symlink() or state_parent.is_symlink():
        raise LoopError("run storage must not be symlinked")
    # Require the local state exclusion before creating anything in the source tree.
    git(root, "check-ignore", "--quiet", ".local/agent-loop/probe")
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    directory = state_parent / run_id
    directory.mkdir(parents=True, mode=0o700)
    os.chmod(directory, 0o700)
    shutil.copyfile(spec_path, directory / "tasks.json")
    controller_files = {}
    sources: list[tuple[str, Path]] = []
    roles = (".codex/agents",) if tool == "codex" else ()
    for relative in ("scripts/agent_loop", "scripts/agent-loop.py", "scripts/check-agent-guidance.py", *roles):
        source = controller / relative
        files = sorted(source.rglob("*.py")) if source.is_dir() and relative.startswith("scripts") else sorted(source.glob("*.toml")) if source.is_dir() else [source]
        for file in files:
            if file.is_symlink() or not file.is_file():
                raise LoopError(f"invalid controller source: {file}")
            sources.append((file.relative_to(controller).as_posix(), file))
    if tool == "claude":
        sources.extend(claude_snapshot_files(controller))
    for relative, file in sources:
        target = directory / "controller" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(file, target)  # follows a symlinked skill to its content
        controller_files[relative] = digest(target)
    prepared = adapter.prepare_run(directory) if hasattr(adapter, "prepare_run") else {}
    base = head(root)
    landed = landed_tasks(root, base, tasks, relative_spec)
    clone(root, directory / "accepted", base, author, owner=directory)
    git(directory / "accepted", "switch", "--quiet", "-c", getattr(adapter, "branch_prefix", "codex/agent-") + run_id, owner=directory)
    records = {task.id: {"status": "pending", "attempts": 0} for task in tasks}
    for task_id, sha in landed.items():
        records[task_id] = {
            "status": "accepted", "attempts": 0, "accepted_at": now(), "landed": sha,
            "reason": f"landed on the source branch before this run as {sha[:12]}",
        }
    state = {
        "version": 1, "run": str(directory), "source": str(root), "source_head": base,
        "accepted_head": base, "spec_path": relative_spec, "spec_sha256": digest(directory / "tasks.json"),
        "controller_files": controller_files, "author": author, "phase": "idle", "active": None,
        "created_at": now(), "updated_at": now(), "baseline_passed": False,
        "remaining_seconds": options["max_minutes"] * 60, "output_tokens": 0,
        "tasks": records, "controller_features": list(CONTROLLER_FEATURES),
        "tool": tool, **prepared,
        **{key: options[key] for key in ("model", "effort", "max_tasks", "max_attempts", "session_minutes", "max_output_tokens")},
        "max_awaiting_evidence": options.get("max_awaiting_evidence") or MAX_AWAITING_EVIDENCE,
        "max_idle_minutes": MAX_IDLE_MINUTES if options.get("max_idle_minutes") is None else options["max_idle_minutes"],
        **{key: options[key] for key in CLAUDE_OPTIONS if tool == "claude" and key in options},
    }
    atomic_json(directory / "state.json", state)
    return directory


def task_record(state: dict, task_id) -> dict | None:
    return state["tasks"].get(task_id) if isinstance(task_id, str) else None


def checked_note(state: dict, request: dict) -> dict | None:
    """The task record a note request applies to, or None when it already did.

    The one rule set for a note applied directly, a note queued while a
    controller holds the run, and that controller's ingestion of it.
    """
    record = task_record(state, request.get("task"))
    if record is None:
        raise LoopError(f"task {request.get('task')!r} is not part of this run")
    if any(note.get("id") == request.get("id") for note in record.get("notes", ())):
        return None
    try:
        if not isinstance(request.get("id"), str) or not inbox.REQUEST_ID.fullmatch(request["id"]):
            raise ValueError
        datetime.fromisoformat(request["at"])
    except (KeyError, TypeError, ValueError) as error:
        raise LoopError("note request is malformed") from error
    text = request.get("text")
    if not isinstance(text, str) or not text.strip():
        raise LoopError("a note needs nonempty text")
    try:
        size = len(text.encode("utf-8"))
    except UnicodeEncodeError as error:
        raise LoopError("a note must be UTF-8 text") from error
    if size > NOTE_BYTES:
        raise LoopError(f"a note holds at most {NOTE_BYTES} bytes of UTF-8 text; this one has {size}")
    if len(record.get("notes", ())) >= MAX_NOTES:
        raise LoopError(f"task {request['task']} already holds {MAX_NOTES} notes")
    return record


def apply_note(state: dict, request: dict) -> bool:
    record = checked_note(state, request)
    if record is None:
        return False
    record.setdefault("notes", []).append({key: request[key] for key in ("id", "at", "text")})
    return True


def checked_attestation(directory: Path, state: dict, request: dict, *, commit_check: str = "fetch") -> dict | None:
    """The task record an attestation request applies to, or None when it already did.

    The one rule set for an attestation applied directly, one queued while a
    controller holds the run, and that controller's ingestion of it. The
    evidence file itself is checked as it is copied: a regular file, not a
    symlink, of at most 32 MiB, and for a queued request the digest recorded
    when it was queued. `commit_check` sets how far a named evidence commit is
    checked (see `checked_evidence_commit`).
    """
    task_id, candidate, kind, summary = (request.get(key) for key in ("task", "candidate", "evidence_kind", "summary"))
    record = task_record(state, task_id)
    if (not record or not isinstance(candidate, str) or record.get("candidate") != candidate
            or not re.fullmatch(r"[0-9a-f]{40,64}", candidate)):
        raise LoopError("attestation does not identify the pending candidate")
    if not isinstance(kind, str) or not isinstance(request.get("id"), str):
        raise LoopError("attestation request is malformed")
    if (record.get("attestations") or {}).get(kind, {}).get("request") == request["id"]:
        return None
    if record.get("base") != state["accepted_head"]:
        if "rebase" not in state.get("controller_features", ()):
            raise LoopError("candidate base is stale; resume to rebuild before gathering evidence")
        if record["status"] in PENDING:
            raise LoopError("candidate base is stale; the controller replays it onto the accepted head at its next step "
                            "(or on resume), so attest the rebased candidate that status then shows")
        raise LoopError(f"candidate base is stale and it is {record['status']}: {record.get('reason', '')}")
    if record["status"] not in {"awaiting_evidence", "review_blocked"} or kind not in record["required_evidence"]:
        raise LoopError("this candidate is not waiting for that evidence kind")
    if not isinstance(summary, str) or not summary.strip():
        raise LoopError("provide a nonempty summary and a regular evidence file no larger than 32 MiB")
    task = next((item for item in load_spec(directory / "tasks.json") if item.id == task_id), None)
    if task is None:
        raise LoopError(f"task {task_id!r} is not part of this run's contract")
    # Evidence rounds are the expensive step; spend one only on a candidate
    # that already passed every review that needs no evidence. A run whose
    # controller predates that order, or a contract that marks no criterion as
    # waiting for evidence, never runs those reviews.
    if "verify_before_evidence" in state.get("controller_features", ()) and not evidence_round_open(task, record):
        raise LoopError("candidate has not passed its pre-evidence verification and security review; resume to finish them first")
    checked_evidence_commit(state, task, record, request, commit_check)
    return record


def checked_evidence_commit(state: dict, task: Task, record: dict, request: dict, check: str) -> None:
    """The evidence-commit rules of an attestation request; an attestation may name none.

    `check` sets how far they go. "fetch", under the run lock, copies the
    commit from the owner's repository into the attempt clone under its own ref
    and checks it there. "inspect" checks it read-only in the owner's
    repository, so a request refused anyway is never queued, and writes no run
    state while a controller holds the lock. "later" checks the request alone,
    for the polls that wait for a busy lock.
    """
    evidence_commit, source, replace = (request.get(key) for key in ("evidence_commit", "evidence_repo", "replace_evidence_commit"))
    if (evidence_commit is None) != (source is None) or replace not in {None, False, True}:
        raise LoopError("name an evidence commit and the repository holding it together")
    if evidence_commit is None:
        if replace:
            raise LoopError("replacing the evidence commit needs the new one")
        if (recheck := record.get("recheck")) and record.get("evidence_commit") == recheck["evidence_commit"]:
            # Its frames came from the old build: an attestation must vouch for them on this one.
            raise LoopError(f"this rebased candidate carries the replayed evidence commit {recheck['evidence_commit']}; "
                            "re-check its frames on the rebased build and attest with --evidence-commit naming it, "
                            "or commit new evidence and attest with --replace-evidence-commit")
        return
    if "evidence_commit" not in state.get("controller_features", ()):
        raise LoopError("this run's saved controller predates evidence commits and would never accept one; "
                        "attest without --evidence-commit, or use a run created by this controller")
    if not isinstance(evidence_commit, str) or not SHA.fullmatch(evidence_commit) or not isinstance(source, str) or not Path(source).is_absolute():
        raise LoopError("an evidence commit is named by its full hexadecimal sha and the absolute path of the repository holding it")
    current = record.get("evidence_commit")
    if current and current != evidence_commit and not replace:
        raise LoopError(f"candidate already carries evidence commit {current[:12]}; replacing it needs "
                        "--replace-evidence-commit, which drops the attestations bound to it")
    if check == "later":
        return
    attempt = Path(record["directory"]) / "repo"
    if head(attempt) != record["candidate"] or not checkout_clean(attempt):
        raise LoopError("candidate source or HEAD changed")
    if check == "fetch":
        fetch_evidence(attempt, Path(source), task.id, evidence_commit)
        evidence_paths(attempt, task, record["candidate"], evidence_commit, protected=controlled)
    else:
        evidence_paths(Path(source), task, record["candidate"], evidence_commit, protected=controlled, checkout=attempt)


def apply_attestation(directory: Path, state: dict, request: dict, evidence: Path, sha256: str | None = None) -> bool:
    record = checked_attestation(directory, state, request)
    if record is None:
        return False
    candidate, kind = request["candidate"], request["evidence_kind"]
    artifact = Path("attestations") / request["task"] / f"{candidate}-{kind}-{uuid.uuid4().hex}.evidence"
    copied = inbox.copy_supplied(evidence, directory / artifact, EVIDENCE_BYTES, "evidence file")
    if sha256 is not None and copied != sha256:
        (directory / artifact).unlink()
        raise LoopError("queued evidence changed after it was submitted")
    evidence_commit = request.get("evidence_commit")
    if evidence_commit and (record.get("evidence_commit") != evidence_commit or request.get("replace_evidence_commit")):
        # One evidence commit per candidate: a replacement unbinds every
        # attestation made for the old one, and any replacement, even by the
        # same commit, clears its docs-gate result so the gate runs again.
        old = record.get("evidence_commit")
        if old != evidence_commit:
            record["attestations"] = {
                key: value for key, value in record.get("attestations", {}).items()
                if not old or value.get("evidence_commit") != old
            }
            # New evidence replaces the replayed commit, so no re-check of it remains.
            record.pop("recheck", None)
        for key in ("evidence_directory", "evidence_gate_sha256"):
            record.pop(key, None)
        record["evidence_commit"] = evidence_commit
    proof = {
        "candidate": candidate, "kind": kind, "summary": request["summary"], "artifact": artifact.as_posix(),
        "sha256": copied, "recorded_at": now(),
    }
    if evidence_commit:
        proof["evidence_commit"] = evidence_commit
    if sha256 is not None:
        proof["request"] = request["id"]  # makes a re-run ingestion of this queued request a no-op
    record.setdefault("attestations", {})[kind] = proof
    return True


def check_request(directory: Path, state: dict, request: dict, *, commit_check: str = "later") -> None:
    """Validate a request against `state` without changing anything."""
    kind = request.get("kind")
    if kind == "note":
        checked_note(state, request)
    elif kind == "attest":
        checked_attestation(directory, state, request, commit_check=commit_check)
    else:
        raise LoopError(f"unknown request kind {kind!r}")


def apply_request(directory: Path, state: dict, request: dict, evidence: Path | None = None, sha256: str | None = None) -> bool:
    """Validate and apply one request to `state`; False when it was already applied."""
    kind = request.get("kind")
    if kind == "note":
        return apply_note(state, request)
    if kind == "attest":
        if evidence is None:
            raise LoopError("attestation request carries no evidence file")
        return apply_attestation(directory, state, request, evidence, sha256)
    raise LoopError(f"unknown request kind {kind!r}")


def submit(directory: Path, request: dict, evidence: Path | None = None) -> str:
    """Apply an operator request under the run lock, or queue it while a controller holds the lock.

    Returns "applied" or "queued". A queued request has passed every check that
    needs no lock, and the controller checks it again before applying it. The
    controller's own sessions and gate commands carry GITTURTLE_LOOP=1 and are
    refused either way, as they were while the lock alone kept them out. That
    restores a guard against workers invoking this CLI; it is not a boundary
    against hostile code running as the same user.
    """
    if os.environ.get("GITTURTLE_LOOP") == "1":
        raise LoopError("another controller is using this run: its own sessions and gate commands cannot note or attest")
    deadline = time.monotonic() + LOCK_WAIT
    while True:
        with locked(directory, busy_ok=True) as held:
            state = state_at(directory)
            features = state.get("controller_features", ())
            if request.get("kind") == "note" and "notes" not in features:
                raise LoopError("this run's saved controller predates coordinator notes, so none of its sessions would see one; "
                                "notes need a run created by this controller")
            if held:
                apply_request(directory, state, request, evidence)
                state["updated_at"] = now()
                atomic_json(directory / "state.json", state)
                return "applied"
            check_request(directory, state, request)
            if time.monotonic() >= deadline:
                if "inbox" not in features:
                    raise LoopError(
                        "another controller is using this run, and its saved controller predates the run inbox, so it would "
                        f"never apply a queued request; stop it (agent-loop.py stop --run {directory}), then attest with the "
                        f"run's saved controller: python3 {directory / 'controller/scripts/agent-loop.py'} attest ..."
                    )
                # Once, before queuing: an evidence commit is checked where it lives, read-only.
                check_request(directory, state, request, commit_check="inspect")
                inbox.submit(directory, request, evidence, EVIDENCE_BYTES, "evidence file")
                return "queued"
        time.sleep(LOCK_POLL)


def attest(directory: Path, task_id: str, candidate: str, kind: str, evidence: Path, summary: str, *,
           evidence_commit: str | None = None, evidence_repo: Path | None = None, replace: bool = False) -> str:
    request = {
        "version": 1, "kind": "attest", "id": uuid.uuid4().hex, "at": now(), "task": task_id,
        "candidate": candidate, "evidence_kind": kind, "summary": summary,
    }
    if evidence_commit is not None or evidence_repo is not None or replace:
        # A queued request carries these to the controller, which fetches the commit at ingestion.
        request.update(evidence_commit=evidence_commit, replace_evidence_commit=replace,
                       evidence_repo=None if evidence_repo is None else str(evidence_repo.resolve()))
    return submit(directory, request, evidence)


def note(directory: Path, task_id: str, text: str) -> str:
    return submit(directory, {"version": 1, "kind": "note", "id": uuid.uuid4().hex, "at": now(), "task": task_id, "text": text})


def note_text(path: Path) -> str:
    data = inbox.read_supplied(path, NOTE_BYTES, "note file")
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise LoopError("a note file must hold UTF-8 text") from error


def positive(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return parsed


def non_negative(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be zero or positive")
    return parsed


def concurrency_options(command: argparse.ArgumentParser, *, defaults: bool) -> None:
    """The evidence-concurrency bounds; `run` sets them, `resume` may renew them."""
    keep = "" if defaults else "; default: the run's saved value"
    command.add_argument("--max-awaiting-evidence", type=positive, default=MAX_AWAITING_EVIDENCE if defaults else None,
                         help=f"implement other tasks while fewer candidates than this await evidence (default {MAX_AWAITING_EVIDENCE}{keep})")
    command.add_argument("--max-idle-minutes", type=non_negative, default=MAX_IDLE_MINUTES if defaults else None,
                         help=f"longest wait for evidence, uncharged to --max-minutes, before the run pauses; 0 never waits "
                              f"(default {MAX_IDLE_MINUTES}{keep})")


def recheck_wanted(record: dict) -> bool:
    """Whether a required kind still lacks an attestation that vouches for the replayed evidence commit."""
    recheck = record.get("recheck")
    if not recheck or record.get("status") == "accepted":
        return False
    proofs = record.get("attestations", {})
    return any(
        (proofs.get(kind) or {}).get("candidate") != record.get("candidate")
        or proofs[kind].get("evidence_commit") != recheck["evidence_commit"]
        for kind in record.get("required_evidence", ())
    )


def rebase_summary(state: dict) -> dict:
    """Each rebased task's lineage and whether its owner should re-check committed frames."""
    return {
        task_id: {"candidate": record.get("candidate"), "rebased_from": record["rebased_from"],
                  "recheck_wanted": recheck_wanted(record), "recheck": record.get("recheck")}
        for task_id, record in state["tasks"].items() if record.get("rebased_from")
    }


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("validate", "run"):
        command = sub.add_parser(name)
        command.add_argument("--repo", type=Path, default=Path.cwd())
        command.add_argument("--tasks", type=Path, default=Path("docs/development/tasks.json"))
        if name == "run":
            command.add_argument("--model", required=True)
            command.add_argument("--effort", choices=EFFORTS, required=True)
            command.add_argument("--max-tasks", type=positive, required=True)
            command.add_argument("--max-attempts", type=positive, required=True)
            command.add_argument("--max-minutes", type=positive, required=True)
            command.add_argument("--session-minutes", type=positive, default=45)
            command.add_argument("--max-output-tokens", type=positive)
            command.add_argument("--tool", choices=TOOLS, default="codex", help="session CLI: codex (default) or claude")
            concurrency_options(command, defaults=True)
            claude = command.add_argument_group("claude", "options used only with --tool claude")
            claude.add_argument("--retry-effort", choices=CLAUDE_EFFORTS, help="effort for attempt 2 (default: one step above --effort)")
            claude.add_argument("--hard-model", help="model for attempt 3 onward via the implementer-hard agent (default: --model); none disables")
            claude.add_argument("--hard-effort", choices=CLAUDE_EFFORTS, help="effort for attempt 3 onward (default: the retry effort)")
            claude.add_argument("--light-model", help="model for docs/tooling-only tasks on attempt 1 (default: --model); none disables")
            claude.add_argument("--light-effort", choices=CLAUDE_EFFORTS, help="effort for docs/tooling-only tasks on attempt 1 (default: medium)")
            claude.add_argument("--review-effort", choices=CLAUDE_EFFORTS, help="effort for verifier and security-review sessions (default: --effort)")
            claude.add_argument("--max-turns", type=positive, default=200, help="turn cap per implementer session")
            claude.add_argument("--review-max-turns", type=positive, default=120, help="turn cap per review session")
            claude.add_argument("--sandbox", choices=("auto", "on", "off"), default="auto", help="Bash sandbox for implementer sessions")
    for name in ("status", "resume", "stop", "attest", "note"):
        command = sub.add_parser(name)
        command.add_argument("--run", type=Path, required=True)
        if name == "note":
            command.add_argument("--task", required=True)
            source = command.add_mutually_exclusive_group(required=True)
            source.add_argument("--text", help=f"the note itself: nonempty UTF-8, at most {NOTE_BYTES} bytes")
            source.add_argument("--file", type=Path, help="a regular file (not a symlink) holding the note")
        if name == "resume":
            for option in ("max-minutes", "max-tasks", "max-attempts", "max-output-tokens"):
                command.add_argument("--" + option, type=positive)
            concurrency_options(command, defaults=False)
        if name == "attest":
            command.add_argument("--task", required=True)
            command.add_argument("--candidate", required=True)
            command.add_argument("--kind", choices=sorted(EVIDENCE_KINDS), required=True)
            command.add_argument("--evidence", type=Path, required=True)
            command.add_argument("--summary", required=True)
            command.add_argument("--evidence-commit", help="full sha of the evidence commit on top of the candidate")
            command.add_argument("--evidence-repo", type=Path, help="repository holding the evidence commit")
            command.add_argument("--replace-evidence-commit", action="store_true",
                                 help="replace the candidate's evidence commit, dropping the attestations bound to it")
    args = parser.parse_args(argv)
    # Run directories and records are private to this user: records.py refuses a
    # group- or other-writable record directory or file, so every path the
    # controller creates must stay private regardless of the shell's umask.
    os.umask(0o077)
    try:
        if args.command == "validate":
            path = args.tasks if args.tasks.is_absolute() else args.repo / args.tasks
            tasks = load_spec(path)
            print(f"valid task graph: {len(tasks)} tasks")
            return 0
        if args.command == "run":
            path = args.tasks if args.tasks.is_absolute() else args.repo / args.tasks
            directory = create_run(args.repo, path, Path(__file__).resolve().parents[2], vars(args))
            print(f"run: {directory}", flush=True)
        else:
            directory = args.run.resolve()
        if args.command == "status":
            state = state_at(directory)
            print(json.dumps(state | {"inbox": inbox.summary(directory), "rebases": rebase_summary(state)}, indent=2))
            return 0
        if args.command == "stop":
            state_at(directory)
            (directory / "STOP").touch()
            print("stop requested; the controller will terminate owned work and preserve the attempt")
            return 0
        if args.command == "attest":
            outcome = attest(directory, args.task, args.candidate, args.kind, args.evidence, args.summary,
                             evidence_commit=args.evidence_commit, evidence_repo=args.evidence_repo,
                             replace=args.replace_evidence_commit)
            print("evidence recorded; resume to independently verify the candidate" if outcome == "applied" else "evidence " + QUEUED)
            return 0
        if args.command == "note":
            text = args.text if args.file is None else note_text(args.file)
            outcome = note(directory, args.task, text)
            print(f"note recorded for {args.task}; its next implementer session receives it" if outcome == "applied" else "note " + QUEUED)
            return 0
        with locked(directory):
            runner = Runner(directory)
            if args.command == "resume":
                for name in ("max_tasks", "max_attempts", "max_output_tokens", "max_awaiting_evidence", "max_idle_minutes"):
                    if getattr(args, name) is not None:
                        runner.state[name] = getattr(args, name)
                if args.max_minutes is not None:
                    runner.initial_seconds = args.max_minutes * 60
                if args.max_output_tokens is not None:
                    runner.state["output_usage_incomplete"] = False
                (directory / "STOP").unlink(missing_ok=True)
                runner.save()
            old_handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
            for sig in old_handlers:
                signal.signal(sig, lambda *_: setattr(runner, "interrupted", True))
            try:
                state = runner.execute()
            except (LoopError, OSError, ValueError):
                # Keep the active phase for reconciliation; never pretend rollback occurred.
                runner.save()
                raise
            finally:
                for sig, handler in old_handlers.items():
                    signal.signal(sig, handler)
            print(f"{state['phase']}: {state.get('reason', '')}")
            print(f"reviewable checkout: {directory / 'accepted'}")
            return 0 if state["phase"] == "complete" else 2
    except (LoopError, OSError, ValueError) as error:
        print(f"agent-loop: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
