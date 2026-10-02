"""A version-probed Claude Code CLI adapter running one fresh headless session per phase.

Each implementer, verifier and security-review session is a separate `claude -p`
process with no conversation history, an explicit model/effort selection, a
schema-validated JSON result and a settings snapshot pinned by the run. A
result the controller cannot read is asked for once more by resuming the same
session before it counts as unreadable. Codex
remains the default adapter; this module changes nothing about that path.
"""

from __future__ import annotations

from dataclasses import asdict
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
from typing import Callable

from .codex import BUILD_SCHEMA, REVIEW_SCHEMA
from .process import EnvironmentBlocked, LoopError, MalformedResponse, atomic_json, digest, read_json, run_process
from .result_schema import check_schema, schema_error, text_result
from .security_review import SECURITY_REVIEW_SCHEMA
from .task_spec import Task


CLAUDE_EFFORTS = ("low", "medium", "high", "xhigh", "max")
MINIMUM_VERSION = (2, 1, 257)
REQUIRED_FLAGS = (
    "--agent", "--json-schema", "--permission-mode", "--permission-prompts", "--settings",
    "--strict-mcp-config", "--output-format", "--effort", "--model", "--tools",
    "--disallowedTools", "--allowedTools", "--resume",
)
ROLES = ("implementer", "verifier", "security-reviewer")
# The second try of a session whose result could not be read keeps its records
# beside the first under this suffix (`verifier.retry.stdout.log`, ...).
RETRY = ".retry"
# Claude Code session IDs are UUIDs; anything else is not passed to --resume.
SESSION_ID = re.compile(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
# The resumed turn already holds the contract, the schema and its own work.
RESULT_RETRY_PROMPT = (
    "Your final message did not carry a result object the controller could read. Do not redo, "
    "extend or re-check the work, and run no tools. Return only the result object, through the "
    "structured output schema you were given, using exactly its field names and nothing outside it."
)
REVIEW_TOOLS = "Read,Grep,Glob,Bash"
REVIEW_DISALLOWED = "Edit,Write,NotebookEdit,Agent"
# Runs inside bwrap: enter a nested user namespace and map it, as Claude Code's
# sandbox does before applying its seccomp filter. Exits nonzero on refusal.
NESTED_NAMESPACE_PROBE = """
import ctypes, os
uid, gid = os.getuid(), os.getgid()
if ctypes.CDLL(None, use_errno=True).unshare(0x10000000) != 0:
    raise SystemExit(1)
for name, text in (("setgroups", "deny"), ("uid_map", f"0 {uid} 1"), ("gid_map", f"0 {gid} 1")):
    with open(f"/proc/self/{name}", "w") as handle:
        handle.write(text)
"""
# The verifier is asked to run the repository's own checks rather than trust a
# recorded excerpt, so it reaches every script in scripts/ except the controller
# itself, which claude-settings.json denies. The Python suites the tooling
# profile documents run through `python3 -m unittest discover`, which that
# pattern cannot reach, so each gets a prefix naming its start directory and no
# other `python3 -m` form is allowed. A prefix cannot stop a later argument, such
# as a second `-s`, from pointing discovery elsewhere; that reaches no further
# than `cargo *` already does. The runner still requires the checkout to be
# clean and at the candidate sha when the review returns.
REVIEW_ALLOWED = (
    "Bash(cargo *)", "Bash(git diff *)", "Bash(git log *)", "Bash(git show *)",
    "Bash(git status *)", "Bash(python3 scripts/*)", "Bash(rustc -vV)",
    "Bash(python3 -m unittest discover -s scripts/agent_loop *)",
    "Bash(python3 -m unittest discover -s scripts/native_qa *)",
    "Bash(python3 -m unittest discover -s scripts/ci/tests *)",
)
LIGHT_PROFILES = frozenset({"docs", "tooling"})
# Paths pinned into the run snapshot and protected during unattended attempts.
SNAPSHOT_PATHS = (".claude/agents", ".claude/settings.json", ".claude/hooks", ".claude/rules", ".claude/skills")
SETTINGS_TEMPLATE = "scripts/agent_loop/claude-settings.json"
CHILD_ENVIRONMENT = {
    "GITTURTLE_LOOP": "1",
    "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS": "1",
    "CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH": "1",
    "CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS": "4",
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "0",
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW": "300000",
    "INSTA_UPDATE": "no",
    "CARGO_TERM_COLOR": "never",
}
# Claude Code variables that can replace the explicit --model/--effort a session is
# launched and recorded with: CLAUDE_CODE_EFFORT_LEVEL even outranks --effort,
# MAX_THINKING_TOKENS=0 lowers the effort sent to some models, and the rest move
# alias, default, background or subagent models.
SELECTION_OVERRIDES = (
    "CLAUDE_CODE_EFFORT_LEVEL", "MAX_THINKING_TOKENS", "ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL", "ANTHROPIC_SMALL_FAST_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL_FORCE",
)
LIMIT_PATTERN = re.compile(
    r"usage limit|rate limit|session limit|weekly limit|fable limit|hit your (?:\w+ )?limit"
    r"|reached your (?:\w+ )?limit|limit reached|try again (?:at|in|after)|out of (?:extra )?usage"
    r"|usage credits|too many requests|\b429\b",
    re.IGNORECASE,
)


class UsageLimited(EnvironmentBlocked):
    """The subscription window is exhausted; pause the run instead of failing the task."""


def effort_above(effort: str) -> str:
    index = CLAUDE_EFFORTS.index(effort)
    return CLAUDE_EFFORTS[min(index + 1, len(CLAUDE_EFFORTS) - 1)]


def resolve_selection(model: str, effort: str, chosen: dict) -> dict:
    """Every step's model and effort, from the operator's choices and the base selection.

    A run resolves this once so its state records the concrete values each
    session uses. By default one model serves every step: the retry is one
    effort step up, the hard step matches the retry, the light step runs at
    medium and reviews at the base effort. `none` keeps a step disabled.
    """
    def pick(key, default):
        return default if chosen.get(key) is None else chosen[key]

    retry = pick("retry_effort", effort_above(effort) if effort in CLAUDE_EFFORTS else None)
    return {
        "retry_effort": retry, "hard_effort": pick("hard_effort", retry), "light_effort": pick("light_effort", "medium"),
        "review_effort": pick("review_effort", effort), "hard_model": pick("hard_model", model),
        "light_model": pick("light_model", model),
    }


def parse_version(text: str) -> tuple[int, int, int]:
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", text)
    if not match:
        raise EnvironmentBlocked(f"cannot parse Claude Code version from {text!r}")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def mentions_limit(*texts: str) -> bool:
    return any(LIMIT_PATTERN.search(text or "") for text in texts)


def snapshot_files(controller: Path) -> list[tuple[str, Path]]:
    """Files pinned for a Claude run: role, settings, hook, rule and skill sources.

    Skill entries may be symlinks into another discovery directory; their targets
    are hashed and copied as long as they stay inside the controller root.
    """
    root = controller.resolve()
    selected: list[tuple[str, Path]] = []
    for relative in SNAPSHOT_PATHS:
        source = controller / relative
        if source.is_symlink():
            raise LoopError(f"invalid controller source: {source}")
        if source.is_file():
            selected.append((relative, source))
            continue
        if not source.is_dir():
            continue
        suffixes = {".claude/hooks": {".py"}}.get(relative, {".md"})
        # Skill entries may be symlinks into another discovery directory; walk
        # through them but keep every hashed target inside the controller root.
        for parent, children, files in os.walk(source, followlinks=True):
            children[:] = sorted(
                name for name in children
                if name != "__pycache__" and (Path(parent) / name).resolve().is_relative_to(root)
            )
            for name in sorted(files):
                path = Path(parent) / name
                if path.suffix not in suffixes or not path.is_file() or not path.resolve().is_relative_to(root):
                    continue
                selected.append((path.relative_to(controller).as_posix(), path))
    template = controller / SETTINGS_TEMPLATE
    if template.is_file() and not template.is_symlink():
        selected.append((SETTINGS_TEMPLATE, template))
    return selected


class Claude:
    branch_prefix = "claude/agent-"

    def __init__(
        self, controller: Path, model: str, effort: str, *, retry_effort: str, hard_effort: str,
        light_effort: str, review_effort: str, hard_model: str, light_model: str, max_turns: int = 200,
        review_max_turns: int = 120, sandbox: str = "auto", settings_sha256: str | None = None,
    ):
        # Every step arrives resolved (see resolve_selection); a missing value is
        # refused rather than launched as `--effort None`.
        if effort not in CLAUDE_EFFORTS:
            raise LoopError(f"Claude Code effort must be one of {', '.join(CLAUDE_EFFORTS)}")
        for name, value in (("retry", retry_effort), ("hard", hard_effort), ("light", light_effort), ("review", review_effort)):
            if value not in CLAUDE_EFFORTS:
                raise LoopError(f"{name} effort must be one of {', '.join(CLAUDE_EFFORTS)}")
        if sandbox not in {"auto", "on", "off"}:
            raise LoopError("sandbox must be auto, on or off")
        for value in (model, hard_model, light_model):
            if not value or not isinstance(value, str):
                raise LoopError("Claude Code sessions require an explicit model for every step")
        self.controller = controller
        self.model = model
        self.effort = effort
        self.retry_effort = retry_effort
        self.hard_model = None if hard_model == "none" else hard_model
        self.light_model = None if light_model == "none" else light_model
        self.hard_effort = hard_effort
        self.light_effort = light_effort
        self.review_effort = review_effort
        self.max_turns = max_turns
        self.review_max_turns = review_max_turns
        self.sandbox = sandbox
        self.settings_sha256 = settings_sha256
        self.executable = shutil.which("claude")
        self.output_tokens = 0
        self.output_usage_incomplete = False
        self.version = ""
        self.selection: dict | None = None
        # The runner's between-session budget (stop, time, output cap); a
        # result retry starts a session without returning to the runner.
        self.budget_stop: Callable[[], str | None] = lambda: None

    # -- capability checks ---------------------------------------------------

    def _command(self, *args: str, timeout: float = 20) -> subprocess.CompletedProcess:
        try:
            return subprocess.run([self.executable or "claude", *args], capture_output=True, text=True, timeout=timeout, check=True)
        except (OSError, subprocess.SubprocessError) as error:
            raise EnvironmentBlocked(f"cannot inspect Claude Code ({' '.join(args)}): {error}") from error

    def sandbox_available(self) -> bool:
        if platform.system() == "Darwin":
            return True
        if not shutil.which("bwrap") or not shutil.which("socat"):
            return False
        # Claude Code's Linux sandbox applies its seccomp filter from a user
        # namespace nested inside bwrap. A host can allow bwrap's own namespace
        # and still refuse the nested one: Ubuntu's AppArmor userns restriction
        # confines bwrap's child, so the sandboxed Bash fails on every command
        # while a bare bwrap probe passes. Probe that nested step too.
        try:
            probe = subprocess.run(
                ["bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--unshare-all", "--",
                 sys.executable, "-I", "-c", NESTED_NAMESPACE_PROBE],
                capture_output=True, timeout=10, check=False,
            )
        except (OSError, subprocess.SubprocessError):
            return False
        return probe.returncode == 0

    def preflight(self) -> str:
        if not self.executable:
            raise EnvironmentBlocked("Claude Code CLI is not installed")
        version = parse_version(self._command("--version").stdout)
        if version < MINIMUM_VERSION:
            raise EnvironmentBlocked("Claude Code %s is older than the required %s" % (
                ".".join(map(str, version)), ".".join(map(str, MINIMUM_VERSION))))
        help_text = self._command("--help").stdout
        for flag in REQUIRED_FLAGS:
            if flag not in help_text:
                raise EnvironmentBlocked(f"installed Claude Code does not advertise required flag {flag}")
        try:
            status = json.loads(self._command("auth", "status").stdout)
        except ValueError as error:
            raise EnvironmentBlocked("cannot read Claude Code sign-in status") from error
        if not isinstance(status, dict) or status.get("loggedIn") is not True:
            raise EnvironmentBlocked("Claude Code is not signed in; complete an interactive sign-in first")
        for role in self.required_roles():
            path = self.controller / ".claude" / "agents" / f"{role}.md"
            if path.is_symlink() or not path.is_file():
                raise EnvironmentBlocked(f"missing Claude agent definition: {path}")
        if self.sandbox == "on" and not self.sandbox_available():
            raise EnvironmentBlocked("sandbox requested but bubblewrap/socat are unavailable or refused a namespace probe")
        self.version = self._command("--version").stdout.strip()
        return self.version

    def required_roles(self) -> tuple[str, ...]:
        roles = list(ROLES)
        if self.hard_model:
            roles.append("implementer-hard")
        return tuple(roles)

    # -- run preparation -----------------------------------------------------

    def prepare_run(self, directory: Path) -> dict:
        """Write the effective settings snapshot for this run from the pinned template."""
        template = directory / "controller" / SETTINGS_TEMPLATE
        settings = read_json(template)
        environment = dict(settings.get("env", {}))
        environment.update(CHILD_ENVIRONMENT)
        settings["env"] = environment
        enabled = self.sandbox == "on" or (self.sandbox == "auto" and self.sandbox_available())
        if enabled:
            settings["sandbox"] = {
                "enabled": True, "autoAllowBashIfSandboxed": True,
                "filesystem": {"allowWrite": ["~/.cargo", "~/.rustup", str(directory / "build")]},
            }
        else:
            settings.pop("sandbox", None)
        path = directory / "controller" / "claude-settings.json"
        atomic_json(path, settings)
        self.settings_sha256 = digest(path)
        return {"claude_settings_sha256": self.settings_sha256, "sandbox_enabled": enabled}

    def configure_attempt(self, task: Task, attempt: int) -> dict:
        """Choose agent, model and effort for one implementation attempt.

        Attempt 1 uses the operator's selection (or the light step for tasks that
        declare only docs/tooling profiles), attempt 2 moves to the retry effort
        on the base model, and attempt 3 onward hands the task to the hard step's agent.
        The task schema has no hardness field, so routing is by attempt only.
        The returned record also names the selection the attempt's reviews use.
        """
        light = self.light_model is not None and set(task.profiles) <= LIGHT_PROFILES
        if attempt >= 3 and self.hard_model:
            selection = {"agent": "implementer-hard", "model": self.hard_model, "effort": self.hard_effort}
        elif attempt == 1 and light:
            selection = {"agent": "implementer", "model": self.light_model, "effort": self.light_effort}
        elif attempt == 1:
            selection = {"agent": "implementer", "model": self.model, "effort": self.effort}
        else:
            # A docs/tooling retry never runs below the effort its first attempt used.
            effort = max(self.retry_effort, self.light_effort, key=CLAUDE_EFFORTS.index) if light else self.retry_effort
            selection = {"agent": "implementer", "model": self.model, "effort": effort}
        selection["max_turns"] = self.max_turns
        self.selection = selection
        return dict(selection, review=self.review_selection())

    def review_selection(self) -> dict:
        return {"model": self.model, "effort": self.review_effort, "max_turns": self.review_max_turns}

    # -- sessions ------------------------------------------------------------

    def run(
        self, role: str, task: Task, repo: Path, directory: Path, timeout: float,
        stop: Callable[[], bool], *, candidate: str | None = None,
        context: str = "", base: str | None = None, spec_path: str | None = None,
    ) -> dict:
        if role not in ROLES:
            raise LoopError("unknown controller role")
        if role != "implementer" and not candidate:
            raise LoopError("review sessions require an explicit candidate revision")
        if role == "security-reviewer" and not base:
            raise LoopError("security review sessions require an explicit base revision")
        schema = SECURITY_REVIEW_SCHEMA if role == "security-reviewer" else REVIEW_SCHEMA if role == "verifier" else BUILD_SCHEMA
        # Refused before any session is spent on a schema the result check cannot enforce.
        check_schema(schema)
        if role == "implementer":
            selection = self.selection or {"agent": "implementer", "model": self.model, "effort": self.effort, "max_turns": self.max_turns}
        else:
            selection = {"agent": role, **self.review_selection()}
        self.selection = None
        agent = selection["agent"]
        pinned = self.controller / ".claude" / "agents" / f"{agent}.md"
        live = repo / ".claude" / "agents" / f"{agent}.md"
        if pinned.is_symlink() or not pinned.is_file():
            raise LoopError(f"missing Claude agent definition in the run snapshot: {agent}")
        if live.is_symlink() or not live.is_file():
            raise LoopError(f"the checkout has no Claude agent definition for {agent}; commit .claude/agents first")
        if digest(live) != digest(pinned):
            raise LoopError(f"agent definition {agent} differs from the run snapshot")
        settings = self.controller / "claude-settings.json"
        if settings.is_symlink() or not settings.is_file() or (self.settings_sha256 and digest(settings) != self.settings_sha256):
            raise LoopError("Claude settings snapshot is missing or changed")
        directory.mkdir(parents=True, exist_ok=True)
        response_path = directory / f"{role}.response.json"
        if response_path.exists() or response_path.is_symlink():
            raise LoopError("session output already exists; refusing to overwrite evidence")
        atomic_json(directory / f"{role}.schema.json", schema)
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
            "performance, package or vendor evidence. Grade in the verification mode stated "
            "below; it names any criteria that wait for that evidence. Return blocked and "
            "unverified criteria when evidence the mode requires is unavailable. "
            "Do not edit source, commit, or operate the desktop. "
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
        # Without this the session learns its limits by trial: the themes
        # verifier probed `python3 -c`, was refused, concluded "python3 is
        # denied in this review sandbox" and abandoned a criterion whose command
        # it was in fact allowed to run.
        commands = "" if role == "implementer" else (
            "Commands this session may run: " + ", ".join(REVIEW_ALLOWED) + ". Every other command is "
            "refused without a prompt. A refusal means that command form is unavailable, not that its "
            "interpreter is; re-read this list before concluding a check cannot be run."
        )
        prompt = "\n\n".join([
            instruction, "Feature contract (data):\n" + json.dumps(asdict(task), indent=2),
            f"Candidate: {candidate}" if candidate else "", f"Base: {base}" if base else "", context, commands,
            # A session told only that "a schema was supplied" invents its own
            # field names: the first themes verifier returned candidate_sha and
            # recommendation with no findings, the CLI could map none of it, and
            # a gated candidate was thrown away for a formatting mismatch.
            "Return the final result through this structured output schema, using exactly its "
            "field names and nothing outside it:\n" + json.dumps(schema, indent=2),
        ])
        contract_path = directory / f"{role}.contract.json"
        atomic_json(contract_path, asdict(task))
        args = [
            self.executable or "claude", "-p", "--agent", agent,
            "--model", selection["model"], "--effort", selection["effort"],
            "--permission-mode", "bypassPermissions" if role == "implementer" else "dontAsk",
            "--permission-prompts", "none", "--max-turns", str(selection["max_turns"]),
            "--output-format", "json", "--json-schema", json.dumps(schema, separators=(",", ":")),
            "--settings", str(settings), "--strict-mcp-config",
        ]
        # Variadic list options come last so nothing after them is swallowed.
        variadic = [] if role == "implementer" else [
            "--tools", REVIEW_TOOLS, "--disallowedTools", REVIEW_DISALLOWED, "--allowedTools", *REVIEW_ALLOWED,
        ]
        environment = {key: value for key, value in os.environ.items() if key not in SELECTION_OVERRIDES}
        environment.update(CHILD_ENVIRONMENT)
        environment["GITTURTLE_TASK_CONTEXT"] = str(contract_path)
        environment["CARGO_TARGET_DIR"] = str(self.controller.parent / "build")
        if spec_path:
            # The hook protects the run's own queue, which may live outside docs/development/tasks.json.
            environment["GITTURTLE_TASKS_PATH"] = spec_path
        payload, elapsed = self._session(role, role, [*args, *variadic], prompt, repo, directory, timeout, stop, environment)
        value, problem = self._result_object(payload, schema)
        if value is None:
            # One more try before the result counts as unreadable: a review that
            # passed but wrote its verdict as prose after a sentence otherwise
            # costs a whole re-review on resume. Resuming the same session keeps
            # its selection, permissions, settings and schema and asks only for
            # the object. A review without a usable session ID is run fresh
            # instead; an implementer is not, since its first try already
            # changed the checkout, and its attempt fails.
            log = directory / f"{role}.stdout.log"
            session_id = payload.get("session_id")
            resume = session_id if isinstance(session_id, str) and SESSION_ID.fullmatch(session_id) else None
            if role == "implementer" and not resume:
                raise MalformedResponse(f"{role} session returned no usable result object ({problem}) and cannot be resumed; inspect {log}")
            # Both tries share the one session time the runner granted, and
            # the retry starts only within the budget it checks between sessions.
            remaining = timeout - elapsed
            reason = self.budget_stop() or ("stop requested" if stop() else None) or ("time budget exhausted" if remaining <= 0 else None)
            if reason:
                # As between sessions: an implementer attempt ends, a review keeps its candidate.
                raise (LoopError if role == "implementer" else EnvironmentBlocked)(
                    f"{role} result retry not started ({problem}): {reason}; inspect {log}")
            name = role + RETRY
            atomic_json(directory / f"{name}.json", {"reason": problem, "resumed_session": resume})
            retry_args = [*args, "--resume", resume, *variadic] if resume else [*args, *variadic]
            payload, _ = self._session(role, name, retry_args, RESULT_RETRY_PROMPT if resume else prompt, repo,
                                       directory, remaining, stop, environment)
            value, retry_problem = self._result_object(payload, schema)
            if value is None:
                raise MalformedResponse(
                    f"{role} session returned no usable result object, also after one retry ({problem}; then "
                    f"{retry_problem}); inspect {log} and {directory / (name + '.stdout.log')}"
                )
        atomic_json(response_path, value)
        if role == "implementer":
            if (set(value) != set(BUILD_SCHEMA["required"]) or value["task_id"] != task.id
                or not isinstance(value["status"], str) or value["status"] not in {"ready", "blocked"}
                or not isinstance(value["summary"], str) or not value["summary"].strip()):
                raise LoopError("invalid implementation response")
        return value

    def _session(
        self, role: str, name: str, args: list[str], prompt: str, repo: Path, directory: Path,
        timeout: float, stop: Callable[[], bool], environment: dict[str, str],
    ) -> tuple[dict, float]:
        """Run one `claude -p` process and return its successful result record and elapsed time.

        `name` is the role for a first try and carries the retry suffix for the
        second, so each try keeps its own prompt, transcript, process and usage
        records. A stop, a usage limit or a failed session raises as it would
        for any try.
        """
        if timeout <= 0:
            # Refused before the prompt record exists: nothing ran, so no usage is unknown.
            raise EnvironmentBlocked(f"{role} session not started: time budget exhausted")
        (directory / f"{name}.prompt.txt").write_text(prompt, encoding="utf-8")
        log = directory / f"{name}.stdout.log"
        stderr_log = directory / f"{name}.stderr.log"
        result = run_process(args, repo, log, timeout, stdin=prompt, stop=stop, env=environment, stderr_path=stderr_log)
        atomic_json(directory / f"{name}.process.json", asdict(result))
        if result.stopped:
            self.output_usage_incomplete = True
            raise EnvironmentBlocked(f"{role} session incomplete: {result.stopped}; inspect {log}")
        stdout = log.read_text(encoding="utf-8", errors="replace")
        stderr = stderr_log.read_text(encoding="utf-8", errors="replace") if stderr_log.is_file() else ""
        payload = self._result_payload(stdout)
        if payload is None:
            self.output_usage_incomplete = True
            if mentions_limit(stdout, stderr):
                raise UsageLimited(f"{role} session hit a usage limit before producing a result; resume after the window resets (inspect {stderr_log})")
            raise EnvironmentBlocked(f"{role} session produced no result record (exit {result.returncode}); inspect {log}")
        self._record_usage(directory, name, payload)
        text = payload.get("result") if isinstance(payload.get("result"), str) else ""
        failed = payload.get("is_error") is True or result.returncode != 0 or payload.get("subtype") != "success"
        if failed and (mentions_limit(text, stderr) or payload.get("api_error_status") == 429):
            raise UsageLimited(f"{role} session was refused by a usage or rate limit; resume after the window resets (inspect {directory / (name + '.session.json')})")
        if failed:
            subtype = payload.get("subtype")
            if subtype == "error_max_turns":
                raise LoopError(f"{role} session exhausted its turn limit without a result; inspect {log}")
            raise EnvironmentBlocked(f"{role} session failed ({subtype or result.returncode}); inspect {log}")
        return payload, result.elapsed

    @staticmethod
    def _result_object(payload: dict, schema: dict) -> tuple[dict | None, str]:
        """The session's result object, held strictly to its schema, or why there is none.

        Structured output is the result whenever the CLI returns one. Otherwise
        the CLI did not surface it, as happens when a long final message ends in
        the verdict, and the object is read from that message's text. Either
        way the whole schema applies; the task id, candidate sha and verdict
        rules are still the caller's to check.
        """
        value = payload.get("structured_output")
        if isinstance(value, dict):
            problem = schema_error(value, schema)
            return (None, "structured output: " + problem) if problem else (value, "")
        text = payload.get("result") if isinstance(payload.get("result"), str) else ""
        return text_result(text, schema)

    @staticmethod
    def _result_payload(stdout: str) -> dict | None:
        try:
            payload = json.loads(stdout)
            if isinstance(payload, dict) and payload.get("type") == "result":
                return payload
        except ValueError:
            pass
        found = None
        for line in stdout.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if isinstance(event, dict) and event.get("type") == "result":
                found = event
        return found

    def _record_usage(self, directory: Path, name: str, payload: dict) -> None:
        usage = payload.get("usage") if isinstance(payload.get("usage"), dict) else {}
        count = usage.get("output_tokens")
        if type(count) is int and count >= 0:
            self.output_tokens += count
        else:
            self.output_usage_incomplete = True
        session = {
            "session_id": payload.get("session_id"), "subtype": payload.get("subtype"),
            "is_error": payload.get("is_error"), "num_turns": payload.get("num_turns"),
            "stop_reason": payload.get("stop_reason"), "output_tokens": count if type(count) is int else None,
            "usage": usage, "total_cost_usd": payload.get("total_cost_usd"),
            "model_usage": payload.get("modelUsage"), "permission_denials": payload.get("permission_denials"),
            "duration_ms": payload.get("duration_ms"),
        }
        atomic_json(directory / f"{name}.session.json", session)

    def recover_usage(self, attempts: Path) -> tuple[int, bool]:
        """Replay reported output usage from durable session records after a crash.

        A result retry is a session of its own, with its own prompt and record.
        """
        reported = 0
        incomplete = False
        for prompt in attempts.glob("**/*.prompt.txt"):
            name = prompt.name[: -len(".prompt.txt")]
            if name.removesuffix(RETRY) not in ROLES:
                continue
            session = prompt.with_name(f"{name}.session.json")
            if session.is_symlink() or not session.is_file():
                incomplete = True
                continue
            count = read_json(session).get("output_tokens")
            if type(count) is int and count >= 0:
                reported += count
            else:
                incomplete = True
        return reported, incomplete
