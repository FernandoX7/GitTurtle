"""Durable, bounded feature acceptance over isolated local repositories."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
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
import time
import uuid

from .codex import Codex, validate_review
from .git import changed_paths, clean, clone, commit, committed_paths, git, head, identity, source_root, write_limits
from .process import EnvironmentBlocked, LoopError, atomic_json, digest, read_json, run_process, reconcile_processes
from .task_spec import Task, load_spec, path_allowed, required_evidence, select_ready
from .security_review import candidate_requires_security, validate_security_review


CONTROLS = (".codex", ".agents", "scripts/agent_loop", "scripts/agent-loop.py", "scripts/check-agent-guidance.py", "docs/development/tasks.json", "docs/development/task.schema.json", "docs/development/security-review.md")
EVIDENCE_KINDS = {"native", "performance", "package", "vendor"}
EFFORTS = ("low", "medium", "high", "xhigh", "max", "ultra")


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def controlled(path: str) -> bool:
    return Path(path).name == "AGENTS.md" or any(path == item or path.startswith(item + "/") for item in CONTROLS)


def validate_patch(repo: Path, task: Task, base: str) -> list[str]:
    if head(repo) != base:
        raise LoopError("worker changed HEAD; the controller alone owns commits")
    if git(repo, "diff", "--cached", "--name-only", "-z"):
        raise LoopError("worker changed the index; the controller alone owns staging")
    paths = changed_paths(repo)
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


def profiles_for(task: Task, paths: list[str]) -> set[str]:
    profiles = set(task.profiles) | {"docs"}
    if any(path.endswith(".rs") or Path(path).name in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"} for path in paths):
        profiles.add("rust")
    if any(path.startswith(("scripts/", ".github/")) for path in paths):
        profiles.add("tooling")
    if any(path.startswith("vendor/") for path in paths):
        profiles.update({"vendor", "rust"})
    if any((path.startswith("crates/app/") and path.endswith(".rs")) or path.startswith("vendor/gpui") for path in paths):
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


@contextmanager
def locked(directory: Path):
    lock_path = directory / "lock"
    if lock_path.is_symlink():
        raise LoopError("invalid run lock")
    with lock_path.open("a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise LoopError("another controller is using this run") from error
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


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
        self.adapter = adapter or Codex(self.controller, self.state["model"], self.state["effort"])
        self.gate_runner = gate_runner
        self.started = time.monotonic()
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

    def recover_usage(self) -> None:
        reported = 0
        incomplete = False
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

    def save(self) -> None:
        self.state["updated_at"] = now()
        self.state["remaining_seconds"] = max(0, self.initial_seconds - (time.monotonic() - self.started))
        self.state["output_tokens"] = self.initial_tokens + self.adapter.output_tokens
        self.state["output_usage_incomplete"] = self.state.get("output_usage_incomplete", False) or getattr(self.adapter, "output_usage_incomplete", False)
        atomic_json(self.directory / "state.json", self.state)

    def stop_requested(self) -> bool:
        return self.interrupted or (self.directory / "STOP").exists()

    def timeout(self) -> float:
        return min(self.state["session_minutes"] * 60, max(0, self.initial_seconds - (time.monotonic() - self.started)))

    def budget_stop(self) -> str | None:
        if self.stop_requested():
            return "stop requested"
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
            environment["CARGO_TARGET_DIR"] = str(self.directory / "build")
            environment["CARGO_TERM_COLOR"] = "never"
            result = run_process(argv, repo, directory / f"{name}.log", self.timeout(), env=environment, stop=self.stop_requested)
            entry = asdict(result) | {"name": name, "log": str(directory / f"{name}.log"), "sha256": digest(directory / f"{name}.log")}
            results.append(entry)
            report = {"passed": not result.stopped and result.returncode == 0, "checks": results}
            atomic_json(directory / "gates.json", report)
            if result.stopped:
                raise EnvironmentBlocked(f"{name} gate interrupted: {result.stopped}")
            if not report["passed"]:
                return report
        return {"passed": True, "checks": results}

    def reconcile(self) -> None:
        reconcile_processes(self.directory)
        actual = head(self.repo)
        if not clean(self.repo):
            raise LoopError("accepted checkout has unexpected changes; preserve and inspect it")
        phase = self.state["phase"]
        active = self.state.get("active")
        if phase == "accepting" and active:
            task_id = active["task"]
            record = self.state["tasks"][task_id]
            candidate = record["candidate"]
            if actual == candidate:
                self.finish_acceptance(task_id, record)
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
            if phase in {"verifying", "security_reviewing"} and record.get("candidate") and record.get("gate_sha256"):
                self.validate_candidate(record)
                record.update(status="review_blocked", reason="independent review interrupted; candidate and gates retained")
            else:
                record["status"] = "interrupted"
                record["reason"] = f"interrupted during {phase}; preserved attempt, retry from accepted source"
            self.state["active"] = None
        self.state["phase"] = "idle"
        self.save()

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
        self.state["codex_version"] = self.adapter.preflight()
        if not self.state.get("baseline_passed"):
            self.state["phase"] = "preflight"
            self.save()
            profiles = {profile for task in self.tasks for profile in task.profiles}
            baseline = self.directory / "baseline" / uuid.uuid4().hex
            report = self.gates(self.repo, profiles, baseline)
            self.state["baseline_evidence"] = str(baseline / "gates.json")
            if not report["passed"] or not clean(self.repo):
                self.state["phase"] = "baseline_failed"
                self.state["budget_running"] = False
                self.save()
                return self.state
            self.state["baseline_passed"] = True
            self.save()
        visited: set[str] = set()
        while True:
            reason = self.budget_stop()
            if reason:
                self.state.update(phase="paused", reason=reason)
                break
            accepted = {key for key, value in self.state["tasks"].items() if value["status"] == "accepted"}
            if len(accepted) >= self.state["max_tasks"]:
                self.state.update(phase="complete" if len(accepted) == len(self.tasks) else "paused", reason="accepted-task limit reached")
                break
            blocked = set(visited)
            for task_id, record in self.state["tasks"].items():
                if record["attempts"] >= self.state["max_attempts"] and record["status"] not in {"awaiting_evidence", "review_blocked"}:
                    blocked.add(task_id)
            ready = select_ready(self.tasks, accepted, blocked)
            if not ready:
                self.state.update(phase="complete" if len(accepted) == len(self.tasks) else "blocked", reason="no eligible tasks remain")
                break
            task = ready[0]
            record = self.state["tasks"][task.id]
            if record["status"] in {"awaiting_evidence", "review_blocked"}:
                if record["base"] != self.state["accepted_head"]:
                    record.update(status="stale", reason="accepted base advanced; candidate evidence cannot be reused")
                    self.save()
                    if record["attempts"] >= self.state["max_attempts"]:
                        visited.add(task.id)
                        continue
                elif self.missing_evidence(record):
                    visited.add(task.id)
                    continue
                else:
                    try:
                        self.review_and_accept(task, record)
                    except EnvironmentBlocked as error:
                        if self.state["phase"] == "accepting":
                            raise
                        record.update(status="review_blocked", reason=str(error))
                        self.state.update(phase="idle", active=None)
                        self.save()
                    if record["status"] in {"review_blocked", "awaiting_evidence"}:
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
        for key in ("candidate", "review", "review_sha256", "review_inputs", "security_review", "security_review_sha256", "security_review_inputs", "security_required", "security_paths", "attestations", "gate_sha256"):
            record.pop(key, None)
        directory = self.directory / "attempts" / task.id / str(record["attempts"])
        record["directory"] = str(directory)
        self.state.update(phase="building", active={"task": task.id})
        self.save()
        repo = directory / "repo"
        try:
            clone(self.repo, repo, record["base"], tuple(self.state["author"]), owner=self.directory)
            result = self.adapter.run("implementer", task, repo, directory, self.timeout(), self.stop_requested,
                                      context="Previous attempt evidence (read-only): " + json.dumps(previous))
            if self.budget_stop():
                raise LoopError(self.budget_stop())
            if result["status"] == "blocked":
                record.update(status="blocked", reason=result["summary"])
                return
            paths = validate_patch(repo, task, record["base"])
            if self.state["spec_path"] in paths:
                raise LoopError("worker changed the task contract")
            record["candidate"] = commit(repo, paths, task.commit, f"{task.description}\n\nTask: {task.id}")
            committed = committed_paths(repo, record["base"], record["candidate"])
            if set(committed) != set(paths) or any(controlled(path) or path == self.state["spec_path"] or not path_allowed(path, task.scope) for path in committed):
                raise LoopError("commit hooks or filters changed the reviewed patch scope")
            # Keep the patch and Git object even when gates/review fail.
            patch = git(repo, "diff", "--binary", "--no-ext-diff", "--no-textconv", record["base"], record["candidate"])
            (directory / "candidate.patch").write_bytes(patch.encode("utf-8", "surrogateescape"))
            profiles = profiles_for(task, paths)
            record["profiles"] = sorted(profiles)
            record["required_evidence"] = sorted(required_evidence(task) | (profiles & EVIDENCE_KINDS))
            self.state["phase"] = "gating"
            self.save()
            gates = self.gates(repo, profiles, directory / "checks")
            record["gate_sha256"] = digest(directory / "checks/gates.json")
            if not gates["passed"]:
                record.update(status="failed", reason="required gate failed; inspect checks/gates.json")
                return
            if head(repo) != record["candidate"] or not clean(repo):
                raise LoopError("gate changed candidate source or HEAD")
            record["status"] = "awaiting_evidence"
            if self.missing_evidence(record):
                record["reason"] = "required external evidence: " + ", ".join(self.missing_evidence(record))
                return
            self.review_and_accept(task, record)
        except EnvironmentBlocked as error:
            if self.state["phase"] == "accepting":
                raise
            record.update(status="review_blocked" if self.state["phase"] in {"verifying", "security_reviewing"} else "blocked", reason=str(error))
        except LoopError as error:
            if self.state["phase"] == "accepting":
                raise  # Git may have moved; preserve intent for reconciliation.
            record.update(status="interrupted" if self.budget_stop() else "failed", reason=str(error))
        finally:
            if self.state["phase"] != "accepting":
                self.state.update(phase="idle", active=None)
            self.save()

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

    def review_inputs(self, record: dict, *, security: bool = False) -> dict:
        inputs = {
            "base": record["base"], "candidate": record["candidate"],
            "gate_sha256": record["gate_sha256"], "attestations": record.get("attestations", {}),
        }
        if security:
            inputs.update(paths=record["security_paths"], review_sha256=record["review_sha256"])
        return inputs

    def saved_review(self, record: dict, key: str, validator) -> bool:
        if key not in record:
            return False
        path = Path(record[key])
        if path.is_symlink() or not path.is_file() or digest(path) != record.get(key + "_sha256"):
            raise LoopError(f"{key} evidence changed")
        verdict = validator(read_json(path))
        return verdict == "pass" and record.get(key + "_inputs") == self.review_inputs(record, security=key == "security_review")

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
        # Git's no-renames diff includes both endpoints and deleted paths. Never
        # trust an implementer's declared profiles to waive security review.
        paths = committed_paths(repo, record["base"], candidate)
        record.update(security_required=candidate_requires_security(repo, record["base"], candidate, paths), security_paths=paths)
        if self.budget_stop():
            record["status"] = "awaiting_evidence"
            return
        context = "Gate evidence: " + str(directory / "checks/gates.json") + "\nExternal evidence: " + json.dumps(record.get("attestations", {}))
        general_validator = lambda value: validate_review(value, task, candidate)
        if not self.saved_review(record, "review", general_validator):
            review_dir = directory / ("review-" + uuid.uuid4().hex[:10])
            self.state.update(phase="verifying", active={"task": task.id})
            self.save()
            value = self.adapter.run(
                "verifier", task, repo, review_dir, self.timeout(), self.stop_requested,
                candidate=candidate, context=context,
            )
            verdict = general_validator(value)
            if head(repo) != candidate or not clean(repo):
                raise LoopError("verifier changed candidate source or HEAD")
            self.validate_candidate(record)
            self.store_review(record, "review", review_dir, value)
            if verdict != "pass":
                record.update(status="review_blocked" if verdict == "blocked" else "failed", reason=json.dumps(value["findings"]))
                self.save()
                return
        if self.budget_stop():
            record["status"] = "awaiting_evidence"
            self.save()
            return
        if record["security_required"]:
            security_validator = lambda value: validate_security_review(value, task, record["base"], candidate, paths)
            if not self.saved_review(record, "security_review", security_validator):
                review_dir = directory / ("security-review-" + uuid.uuid4().hex[:10])
                self.state.update(phase="security_reviewing", active={"task": task.id})
                self.save()
                value = self.adapter.run(
                    "security-reviewer", task, repo, review_dir, self.timeout(), self.stop_requested,
                    candidate=candidate, base=record["base"],
                    context=context + "\nChanged paths (data): " + json.dumps(paths)
                    + "\nGeneral review evidence (not authoritative): " + record["review"],
                )
                verdict = security_validator(value)
                if head(repo) != candidate or not clean(repo):
                    raise LoopError("security reviewer changed candidate source or HEAD")
                self.validate_candidate(record)
                # Security review cannot invalidate and silently replace the
                # already passing general review, its gates, or attestations.
                if not self.saved_review(record, "review", general_validator) or self.missing_evidence(record):
                    raise LoopError("acceptance evidence changed during security review")
                self.store_review(record, "security_review", review_dir, value)
                if verdict != "pass":
                    record.update(status="review_blocked" if verdict == "blocked" else "failed", reason=json.dumps({"findings": value["findings"], "gaps": value["gaps"]}))
                    self.save()
                    return
        if self.budget_stop():
            record["status"] = "awaiting_evidence"
            self.save()
            return
        # Persist the acceptance intent before touching the private accepted ref.
        self.state.update(phase="accepting", active={"task": task.id})
        record["status"] = "accepting"
        self.save()
        git(self.repo, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head", str(repo), candidate)
        git(self.repo, "merge", "--quiet", "--ff-only", candidate)
        self.finish_acceptance(task.id, record)

    def store_review(self, record: dict, key: str, directory: Path, value: dict) -> None:
        atomic_json(directory / "verdict.json", value)
        record[key] = str(directory / "verdict.json")
        record[key + "_sha256"] = digest(Path(record[key]))
        record[key + "_inputs"] = self.review_inputs(record, security=key == "security_review")
        self.state.update(phase="idle", active=None)
        self.save()

    def finish_acceptance(self, task_id: str, record: dict) -> None:
        if head(self.repo) != record["candidate"] or not clean(self.repo):
            raise LoopError("accepted checkout changed during acceptance")
        self.validate_candidate(record)
        task = next(task for task in self.tasks if task.id == task_id)
        if not self.saved_review(record, "review", lambda value: validate_review(value, task, record["candidate"])):
            raise LoopError("acceptance requires a passing independent review")
        repo = Path(record["directory"]) / "repo"
        paths = committed_paths(repo, record["base"], record["candidate"])
        needs_security = candidate_requires_security(repo, record["base"], record["candidate"], paths)
        if record.get("security_required") != needs_security or record.get("security_paths") != paths:
            raise LoopError("security review routing changed during acceptance")
        if needs_security and not self.saved_review(record, "security_review", lambda value: validate_security_review(
            value, task, record["base"], record["candidate"], paths,
        )):
            raise LoopError("acceptance requires a passing independent security review")
        if self.missing_evidence(record):
            raise LoopError("acceptance requires all external evidence")
        record.update(status="accepted", accepted_at=now(), reason="all required evidence accepted")
        self.state.update(accepted_head=record["candidate"], phase="idle", active=None)
        self.save()
        print(f"accepted {task_id} at {record['candidate'][:12]}", flush=True)

    def validate_candidate(self, record: dict) -> None:
        directory = Path(record["directory"])
        repo = directory / "repo"
        if head(repo) != record["candidate"] or not clean(repo):
            raise LoopError("candidate source or HEAD changed")
        report_path = directory / "checks/gates.json"
        if digest(report_path) != record["gate_sha256"]:
            raise LoopError("gate evidence changed")
        report = read_json(report_path)
        if report.get("passed") is not True:
            raise LoopError("required gates did not pass")
        for check in report["checks"]:
            log = Path(check["log"])
            if log.is_symlink() or not log.is_file() or digest(log) != check["sha256"]:
                raise LoopError("gate command log changed")


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
    adapter = Codex(controller, options["model"], options["effort"])
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
    for relative in ("scripts/agent_loop", "scripts/agent-loop.py", "scripts/check-agent-guidance.py", ".codex/agents"):
        source = controller / relative
        files = sorted(source.rglob("*.py")) if source.is_dir() and relative.startswith("scripts") else sorted(source.glob("*.toml")) if source.is_dir() else [source]
        for file in files:
            if file.is_symlink() or not file.is_file():
                raise LoopError(f"invalid controller source: {file}")
            target = directory / "controller" / file.relative_to(controller)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(file, target)
            controller_files[file.relative_to(controller).as_posix()] = digest(target)
    base = head(root)
    clone(root, directory / "accepted", base, author, owner=directory)
    git(directory / "accepted", "switch", "--quiet", "-c", "codex/agent-" + run_id, owner=directory)
    state = {
        "version": 1, "run": str(directory), "source": str(root), "source_head": base,
        "accepted_head": base, "spec_path": relative_spec, "spec_sha256": digest(directory / "tasks.json"),
        "controller_files": controller_files, "author": author, "phase": "idle", "active": None,
        "created_at": now(), "updated_at": now(), "baseline_passed": False,
        "remaining_seconds": options["max_minutes"] * 60, "output_tokens": 0,
        "tasks": {task.id: {"status": "pending", "attempts": 0} for task in tasks},
        **{key: options[key] for key in ("model", "effort", "max_tasks", "max_attempts", "session_minutes", "max_output_tokens")},
    }
    atomic_json(directory / "state.json", state)
    return directory


def attest(directory: Path, task_id: str, candidate: str, kind: str, evidence: Path, summary: str) -> None:
    with locked(directory):
        state = state_at(directory)
        record = state["tasks"].get(task_id)
        if not record or record.get("candidate") != candidate or not re.fullmatch(r"[0-9a-f]{40,64}", candidate):
            raise LoopError("attestation does not identify the pending candidate")
        if record.get("base") != state["accepted_head"]:
            raise LoopError("candidate base is stale; resume to rebuild before gathering evidence")
        if record["status"] not in {"awaiting_evidence", "review_blocked"} or kind not in record["required_evidence"]:
            raise LoopError("this candidate is not waiting for that evidence kind")
        if not summary.strip() or evidence.is_symlink() or not evidence.is_file() or evidence.stat().st_size > 32 * 1024 * 1024:
            raise LoopError("provide a nonempty summary and a regular evidence file no larger than 32 MiB")
        artifact = Path("attestations") / task_id / f"{candidate}-{kind}-{uuid.uuid4().hex}.evidence"
        destination = directory / artifact
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(evidence, destination)
        record.setdefault("attestations", {})[kind] = {
            "candidate": candidate, "kind": kind, "summary": summary,
            "artifact": artifact.as_posix(), "sha256": digest(destination), "recorded_at": now(),
        }
        state["updated_at"] = now()
        atomic_json(directory / "state.json", state)


def positive(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return parsed


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
    for name in ("status", "resume", "stop", "attest"):
        command = sub.add_parser(name)
        command.add_argument("--run", type=Path, required=True)
        if name == "resume":
            for option in ("max-minutes", "max-tasks", "max-attempts", "max-output-tokens"):
                command.add_argument("--" + option, type=positive)
        if name == "attest":
            command.add_argument("--task", required=True)
            command.add_argument("--candidate", required=True)
            command.add_argument("--kind", choices=sorted(EVIDENCE_KINDS), required=True)
            command.add_argument("--evidence", type=Path, required=True)
            command.add_argument("--summary", required=True)
    args = parser.parse_args(argv)
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
            print(json.dumps(state_at(directory), indent=2))
            return 0
        if args.command == "stop":
            state_at(directory)
            (directory / "STOP").touch()
            print("stop requested; the controller will terminate owned work and preserve the attempt")
            return 0
        if args.command == "attest":
            attest(directory, args.task, args.candidate, args.kind, args.evidence, args.summary)
            print("evidence recorded; resume to independently verify the candidate")
            return 0
        with locked(directory):
            runner = Runner(directory)
            if args.command == "resume":
                for name in ("max_tasks", "max_attempts", "max_output_tokens"):
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
