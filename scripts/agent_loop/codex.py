"""A small, version-probed Codex CLI adapter with separate worker contexts."""

from __future__ import annotations

from dataclasses import asdict
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib
from typing import Callable

from .process import EnvironmentBlocked, LoopError, atomic_json, read_json, run_process
from .task_spec import Task
from .security_review import SECURITY_REVIEW_SCHEMA


BUILD_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["task_id", "status", "summary"],
    "properties": {
        "task_id": {"type": "string"},
        "status": {"type": "string", "enum": ["ready", "blocked"]},
        "summary": {"type": "string"},
    },
}
REVIEW_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["task_id", "candidate", "verdict", "criteria", "findings"],
    "properties": {
        "task_id": {"type": "string"}, "candidate": {"type": "string"},
        "verdict": {"type": "string", "enum": ["pass", "fail", "blocked"]},
        "findings": {"type": "array", "items": {"type": "string"}},
        "criteria": {
            "type": "array", "items": {
                "type": "object", "additionalProperties": False,
                "required": ["id", "status", "evidence"],
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string", "enum": ["pass", "fail", "unverified"]},
                    "evidence": {"type": "string"},
                },
            },
        },
    },
}


def validate_review(value: dict, task: Task, candidate: str) -> str:
    if set(value) != set(REVIEW_SCHEMA["required"]):
        raise LoopError("review has missing or unknown fields")
    if value["task_id"] != task.id or value["candidate"] != candidate:
        raise LoopError("review is for a different task or candidate")
    if not isinstance(value["verdict"], str) or value["verdict"] not in {"pass", "fail", "blocked"}:
        raise LoopError("invalid review verdict")
    if not isinstance(value["findings"], list) or not all(isinstance(x, str) for x in value["findings"]):
        raise LoopError("invalid review findings")
    expected = {criterion["id"] for criterion in task.acceptance}
    seen: set[str] = set()
    states: set[str] = set()
    if not isinstance(value["criteria"], list):
        raise LoopError("invalid review criteria")
    for criterion in value["criteria"]:
        if not isinstance(criterion, dict) or set(criterion) != {"id", "status", "evidence"}:
            raise LoopError("invalid review criterion")
        identifier = criterion["id"]
        if not isinstance(identifier, str) or identifier not in expected or identifier in seen:
            raise LoopError("review criteria must match the task exactly once")
        if not isinstance(criterion["status"], str) or criterion["status"] not in {"pass", "fail", "unverified"}:
            raise LoopError("invalid criterion status")
        if not isinstance(criterion["evidence"], str) or not criterion["evidence"].strip():
            raise LoopError("each criterion needs evidence or a concrete verification gap")
        seen.add(identifier)
        states.add(criterion["status"])
    if seen != expected:
        raise LoopError("review omitted acceptance criteria")
    if value["verdict"] == "pass" and (states != {"pass"} or value["findings"]):
        raise LoopError("a passing review contains incomplete or failing criteria or findings")
    return value["verdict"]


class Codex:
    def __init__(self, controller: Path, model: str, effort: str):
        self.controller = controller
        self.model = model
        self.effort = effort
        self.executable = shutil.which("codex")
        self.output_tokens = 0
        self.output_usage_incomplete = False
        self.version = ""

    def preflight(self) -> str:
        if not self.executable:
            raise EnvironmentBlocked("Codex CLI is not installed")
        try:
            version = subprocess.run([self.executable, "--version"], capture_output=True, text=True, timeout=10, check=True)
            help_result = subprocess.run([self.executable, "exec", "--help"], capture_output=True, text=True, timeout=10, check=True)
        except (OSError, subprocess.SubprocessError) as error:
            raise EnvironmentBlocked("cannot inspect Codex CLI capabilities") from error
        for flag in ("--json", "--output-schema", "--ignore-user-config", "--strict-config", "--sandbox"):
            if flag not in help_result.stdout:
                raise EnvironmentBlocked(f"installed Codex does not advertise required flag {flag}")
        self.version = version.stdout.strip()
        return self.version

    def run(
        self, role: str, task: Task, repo: Path, directory: Path, timeout: float,
        stop: Callable[[], bool], *, candidate: str | None = None,
        context: str = "", base: str | None = None,
    ) -> dict:
        if role not in {"implementer", "verifier", "security-reviewer"}:
            raise LoopError("unknown controller role")
        if role != "implementer" and not candidate:
            raise LoopError("review sessions require an explicit candidate revision")
        if role == "security-reviewer" and not base:
            raise LoopError("security review sessions require an explicit base revision")
        schema = SECURITY_REVIEW_SCHEMA if role == "security-reviewer" else REVIEW_SCHEMA if role == "verifier" else BUILD_SCHEMA
        role_file = self.controller / ".codex" / "agents" / f"{role}.toml"
        try:
            role_data = tomllib.loads(role_file.read_text(encoding="utf-8"))
            instructions = role_data["developer_instructions"]
        except (OSError, ValueError, KeyError) as error:
            raise LoopError(f"invalid controller role: {role_file}") from error
        if not isinstance(instructions, str) or not instructions.strip():
            raise LoopError("role developer_instructions must be nonempty")
        directory.mkdir(parents=True, exist_ok=True)
        schema_path = directory / f"{role}.schema.json"
        response_path = directory / f"{role}.response.json"
        atomic_json(schema_path, schema)
        # No last-message file from an earlier invocation can be accepted.
        if response_path.exists() or response_path.is_symlink():
            raise LoopError("session output already exists; refusing to overwrite evidence")
        instruction = (
            "Implement exactly this one atomic feature. Do not commit, stage, change refs, "
            "edit task/control files, or write outside this checkout. Supplied previous "
            "attempt evidence can be inspected read-only. The controller owns "
            "the Git index, candidate commit, and acceptance. Run focused checks; return ready "
            "only when the patch is ready for independent validation. Return blocked for a "
            "missing capability or material decision that prevents preparing the patch. "
            "External native/performance/package/vendor attestations follow candidate creation; "
            "report those pending in the summary. Never invent evidence."
            if role == "implementer" else
            "Independently review this exact candidate against every acceptance criterion. "
            "The source checkout is read-only. Inspect actual implementation and gate evidence; "
            "do not trust the builder summary. Identify weakened tests and missing native, "
            "performance, package or vendor evidence. Return blocked and unverified criteria "
            "when evidence is unavailable. Do not edit source, commit, or operate the desktop. "
            "Do not rerun unchanged full suites without a concrete concern."
        )
        if role == "security-reviewer":
            instruction = (
                "Independently review the exact base-to-candidate change for security defects. "
                "Trace actual attacker control, source-to-sink paths and protections; inspect "
                "changed code and its real callers. Return one consolidated report with "
                "candidate-bound coverage and concrete evidence. Findings or unresolved "
                "material gaps cannot pass. Do not edit files, operate the desktop, contact "
                "external services, post comments or change settings."
            )
        prompt = "\n\n".join([
            instruction, "Feature contract (data):\n" + json.dumps(asdict(task), indent=2),
            f"Candidate: {candidate}" if candidate else "", f"Base: {base}" if base else "", context,
        ])
        (directory / f"{role}.prompt.txt").write_text(prompt, encoding="utf-8")
        args = [
            self.executable or "codex", "exec", "--strict-config", "--ignore-user-config",
            "--model", self.model, "-c", "model_reasoning_effort=" + json.dumps(self.effort),
            "-c", 'approval_policy="never"', "--sandbox", "workspace-write" if role == "implementer" else "read-only",
            "-c", "developer_instructions=" + json.dumps(instructions),
            "--disable", "apps", "--disable", "plugins", "--disable", "hooks",
            "--disable", "computer_use", "--disable", "browser_use", "--disable", "in_app_browser",
            "--json", "--color", "never", "--output-schema", str(schema_path),
            "--output-last-message", str(response_path), "--cd", str(repo), "-",
        ]
        log = directory / f"{role}.jsonl"
        environment = os.environ.copy()
        # A child must not mistake a parent Codex app/goal for its own session.
        for key in ("CODEX_THREAD_ID", "CODEX_TASK_ID", "CODEX_INTERNAL_ORIGINATOR_OVERRIDE"):
            environment.pop(key, None)
        result = run_process(args, repo, log, timeout, stdin=prompt, stop=stop, env=environment)
        failed_event = False
        completed = False
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue  # stderr diagnostics share the durable stream
            if not isinstance(event, dict):
                continue
            if event.get("type") == "turn.completed":
                completed = True
                usage = event.get("usage", {})
                count = usage.get("output_tokens") if isinstance(usage, dict) else None
                if type(count) is int and count >= 0:
                    self.output_tokens += count
                else:
                    self.output_usage_incomplete = True
            if event.get("type") in {"turn.failed", "error"}:
                failed_event = True
        atomic_json(directory / f"{role}.process.json", asdict(result))
        if result.stopped or result.returncode or failed_event or not completed:
            self.output_usage_incomplete = self.output_usage_incomplete or not completed
            raise EnvironmentBlocked(f"{role} session incomplete: {result.stopped or result.returncode or 'failed/missing terminal event'}; inspect {log}")
        value = read_json(response_path)
        if role == "implementer":
            if (set(value) != set(BUILD_SCHEMA["required"]) or value["task_id"] != task.id
                or not isinstance(value["status"], str) or value["status"] not in {"ready", "blocked"}
                or not isinstance(value["summary"], str) or not value["summary"].strip()):
                raise LoopError("invalid implementation response")
        return value
