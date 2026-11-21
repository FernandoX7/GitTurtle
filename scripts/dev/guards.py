#!/usr/bin/env python3
"""Shared repository safety guards for Git hooks and agent tool hooks.

Standard library only; works on macOS and Linux across every CLI agent and human
developer workflow.
"""

from __future__ import annotations

import os
from pathlib import Path
import re
import shlex
import subprocess
import sys

PROTECTED_PREFIXES = (
    ".claude/",
    ".codex/",
    ".agents/",
    ".githooks/",
    ".config/nextest.toml",
    ".github/copilot-instructions.md",
    "scripts/dev/",
    "scripts/check-agent-guidance.py",
    "scripts/check-docs-sync.py",
    "scripts/gate.py",
    "docs/development/tasks.json",
    "docs/development/task.schema.json",
    "docs/development/security-review.md",
    "deny.toml",
    "clippy.toml",
    "rustfmt.toml",
)
PROTECTED_NAMES = ("AGENTS.md", "CLAUDE.md", "GEMINI.md")

SEPARATORS = {";", "&", "&&", "|", "||", "(", ")", "\n", "{", "}"}
PREFIXES = {"do", "then", "else", "elif", "if", "while", "until", "time", "!", "sudo", "command", "exec"}
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
GLOB = re.compile(r"[*?\[]")
SUBSTITUTION = re.compile(r"\$\([^()]*\)|`[^`]*`")
# `$NAME` or `${NAME...}` (a substitution is renamed to one first), then at most one path component.
EXPANSION_ROOT = re.compile(r"^(?:\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*|\$[0-9@*])(?:/[^/]*)?/*$")

# Match real Git merge/diff3 conflict markers (with trailing space/label) while avoiding
# valid 7-character Markdown Setext H1 underlines (`=======`).
CONFLICT_MARKER = re.compile(r"^(?:<{7} |\|{7} |>{7} )")
PROBE_MARKER = re.compile(r"(?://\s*MUTATION\b|\bQ9_PROBE\b)")
AI_TRAILER = re.compile(
    r"(?im)^(?:"
    r"\s*(?:co-authored-by|signed-off-by|generated-by|assisted-by)\s*:.*"
    r"(?:noreply@anthropic\.com|anthropic|claude(?:[-_ ]?(?:code|opus|sonnet|haiku|bot|ai))?\b"
    r"|gemini|openai|chatgpt|copilot|codex|cursor[-_ ]?agent|grok)"
    r"|\s*generated\s+with\s+\[?.*(?:claude|gemini|openai|chatgpt|copilot|codex|cursor|grok)"
    r")"
)
SCISSORS_LINE = "# ------------------------ >8 ------------------------"
HUNK_HEADER = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
# Only this file contains the literal PROBE_MARKER regex itself.
SELF_TEST_PATHS = ("scripts/dev/guards.py",)


def repo_root(cwd: Path) -> Path:
    """Resolve the Git repository top-level directory for `cwd`, or `cwd` itself."""
    resolved = cwd.resolve()
    try:
        completed = subprocess.run(
            ["git", "-C", str(resolved), "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, errors="replace", check=False,
            env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"},
        )
    except OSError:
        return resolved
    if completed.returncode == 0 and completed.stdout.strip():
        return Path(completed.stdout.strip()).resolve()
    return resolved


def candidate_relative_paths(raw: str, cwd: Path) -> tuple[list[str], bool]:
    """Return candidate repo-relative paths (lexical + resolved) and whether the target escapes the repo."""
    root = repo_root(cwd)
    raw_path = Path(raw)
    joined = raw_path if raw_path.is_absolute() else (cwd / raw_path)
    lexical = Path(os.path.normpath(str(joined)))
    resolved = joined.resolve()

    candidates: list[str] = []
    escaped = False
    for path in (lexical, resolved):
        try:
            rel = path.relative_to(root).as_posix()
            if rel not in candidates:
                candidates.append(rel)
        except ValueError:
            escaped = True
    return candidates, escaped


def relative_path(raw: str, cwd: Path) -> str:
    candidates, _ = candidate_relative_paths(raw, cwd)
    if candidates:
        return candidates[0]
    path = Path(raw)
    if not path.is_absolute():
        path = cwd / path
    return Path(os.path.normpath(str(path))).as_posix()


def active_queue() -> str:
    """The repository-relative task queue this run was started from, if any."""
    return (os.environ.get("GITTURTLE_TASKS_PATH") or "").strip()


def is_protected(relative: str) -> bool:
    if Path(relative).name in PROTECTED_NAMES:
        return True
    if any(relative == p.rstrip("/") or relative.startswith(p) for p in PROTECTED_PREFIXES):
        return True
    parts = Path(relative).parts
    return "snapshots" in parts and relative.endswith((".snap", ".snap.new"))


def path_protection_reason(raw: str, cwd: Path, queue: str = "") -> str | None:
    """Return a refusal reason if `raw` targets a protected path, active queue, or escapes repo root."""
    candidates, escaped = candidate_relative_paths(raw, cwd)
    for rel in candidates:
        if queue and rel == queue:
            return f"{rel} is this run's active task queue"
        if is_protected(rel):
            return f"{rel} is a protected policy path"
    if escaped:
        return f"{raw} resolves outside the repository root"
    return None


def segments(command: str) -> list[list[str]]:
    """The simple commands of `command`, each as its words with quotes removed."""
    command = SUBSTITUTION.sub("$SUBSTITUTION", command)
    lexer = shlex.shlex(command, posix=True, punctuation_chars="();<>|&\n")
    lexer.whitespace = " \t\r"
    lexer.whitespace_split = True
    result: list[list[str]] = [[]]
    for token in lexer:
        if token in SEPARATORS or (token and set(token) <= set(";&|()\n")):
            result.append([])
        else:
            result[-1].append(token)
    return [words for words in result if words]


def command_words(words: list[str]) -> list[str]:
    """`words` without leading keywords, wrappers and variable assignments."""
    index = 0
    while index < len(words) and (words[index] in PREFIXES or ASSIGNMENT.match(words[index])):
        index += 1
    return words[index:]


def targets(arguments: list[str]) -> list[str]:
    found, options = [], True
    for argument in arguments:
        if options and argument == "--":
            options = False
        elif options and argument.startswith("-") and len(argument) > 1:
            continue
        else:
            found.append(argument)
    return found


def rm_problem(command: str) -> str | None:
    if "<<" in command:
        return None
    try:
        parsed = segments(command)
    except ValueError:
        return None
    changed_directory = False
    for words in parsed:
        words = command_words(words)
        if not words:
            continue
        if words[0] in ("cd", "pushd", "popd"):
            changed_directory = True
            continue
        if words[0] != "rm":
            continue
        for target in targets(words[1:]):
            if EXPANSION_ROOT.match(target):
                return (
                    f"`rm` target `{target}` is a shell expansion that is a top-level path when the "
                    "expansion is empty"
                )
            relative = not target.startswith(("/", "~", "$", "`"))
            if changed_directory and relative and GLOB.search(target):
                return f"`rm` target `{target}` is a relative glob after the command changed directory"
    return None


def check_staged_paths(
    added_or_modified: list[str],
    *,
    all_touched: list[str] | None = None,
    loop_mode: bool = False,
    queue: str = "",
) -> list[str]:
    issues: list[str] = []
    for path in added_or_modified:
        if path.endswith(".snap.new"):
            issues.append(f"staged insta preview snapshot must not be committed: {path}")
    if loop_mode:
        checked = added_or_modified if all_touched is None else all_touched
        for path in checked:
            if queue and path == queue:
                issues.append(f"protected active task queue in loop mode: {path}")
            elif is_protected(path):
                issues.append(f"protected policy path in loop mode: {path}")
    return issues


def check_staged_diff(diff_text: str) -> list[str]:
    """Scan added lines in a unified=0 diff for conflict markers and temporary probe markers."""
    issues: list[str] = []
    current_path = ""
    line_no = 0
    remaining_new_lines = 0
    in_hunk = False

    for raw in diff_text.splitlines():
        if not in_hunk:
            if raw.startswith("diff --git "):
                current_path = ""
                continue
            if raw.startswith("+++ b/"):
                current_path = raw[6:]
                continue
            if raw.startswith("+++ /dev/null"):
                current_path = ""
                continue
            match = HUNK_HEADER.match(raw)
            if match:
                line_no = int(match.group(1)) - 1
                remaining_new_lines = int(match.group(2)) if match.group(2) is not None else 1
                in_hunk = remaining_new_lines > 0
                continue
            continue

        # Inside a unified=0 hunk body: never treat payload lines as file headers.
        if raw.startswith("+"):
            line_no += 1
            remaining_new_lines -= 1
            body = raw[1:]
            if current_path and current_path not in SELF_TEST_PATHS:
                if current_path.endswith((".md", ".yml", ".yaml", ".toml", ".json", ".py", ".sh", ".html", ".css", ".js")):
                    if CONFLICT_MARKER.match(body):
                        issues.append(f"{current_path}:{line_no}: merge conflict marker")
                if current_path.startswith(("crates/", "vendor/", "scripts/")) and PROBE_MARKER.search(body):
                    issues.append(f"{current_path}:{line_no}: temporary mutation or probe marker")
            if remaining_new_lines <= 0:
                in_hunk = False
        elif raw.startswith(" "):
            line_no += 1
            remaining_new_lines -= 1
            if remaining_new_lines <= 0:
                in_hunk = False
        elif raw.startswith(("-", "\\")):
            continue
        else:
            # Unexpected non-hunk line (e.g., next `diff --git` after pure deletion hunk).
            in_hunk = False
            if raw.startswith("diff --git "):
                current_path = ""
            elif raw.startswith("+++ b/"):
                current_path = raw[6:]
            elif raw.startswith("+++ /dev/null"):
                current_path = ""
            else:
                match = HUNK_HEADER.match(raw)
                if match:
                    line_no = int(match.group(1)) - 1
                    remaining_new_lines = int(match.group(2)) if match.group(2) is not None else 1
                    in_hunk = remaining_new_lines > 0
    return issues


def check_commit_message(message: str) -> list[str]:
    # Strip verbose `git commit -v` diff below scissors and Git comment lines.
    if SCISSORS_LINE in message:
        message = message.split(SCISSORS_LINE, 1)[0]
    cleaned = "\n".join(line for line in message.splitlines() if not line.startswith("#"))
    issues: list[str] = []
    if AI_TRAILER.search(cleaned):
        issues.append("commit message must not contain AI attribution or co-author trailers")
    return issues


def run_pre_commit(repo: Path) -> int:
    env = {**os.environ, "GIT_OPTIONAL_LOCKS": "0"}
    staged_acmr = subprocess.run(
        ["git", "-C", str(repo), "diff", "--cached", "--name-only", "--diff-filter=ACMRT", "--no-renames", "-z"],
        capture_output=True, check=False, env=env,
    )
    if staged_acmr.returncode != 0:
        sys.stderr.write(staged_acmr.stderr.decode("utf-8", errors="replace"))
        return staged_acmr.returncode
    added_or_modified = [p.decode("utf-8", errors="surrogateescape") for p in staged_acmr.stdout.split(b"\0") if p]

    staged_all = subprocess.run(
        ["git", "-C", str(repo), "diff", "--cached", "--name-only", "--no-renames", "-z"],
        capture_output=True, check=False, env=env,
    )
    if staged_all.returncode != 0:
        sys.stderr.write(staged_all.stderr.decode("utf-8", errors="replace"))
        return staged_all.returncode
    all_touched = [p.decode("utf-8", errors="surrogateescape") for p in staged_all.stdout.split(b"\0") if p]

    issues = check_staged_paths(
        added_or_modified,
        all_touched=all_touched,
        loop_mode=os.environ.get("GITTURTLE_LOOP") == "1",
        queue=active_queue(),
    )
    diff = subprocess.run(
        [
            "git", "-C", str(repo),
            "-c", "core.quotePath=false",
            "-c", "diff.noprefix=false",
            "-c", "diff.mnemonicPrefix=false",
            "diff", "--cached", "--unified=0", "--no-color", "--no-ext-diff", "--no-textconv",
        ],
        capture_output=True, text=True, errors="replace", check=False, env=env,
    )
    if diff.returncode != 0:
        sys.stderr.write(f"pre-commit guard: git diff --cached failed (exit {diff.returncode})\n")
        if diff.stderr:
            sys.stderr.write(diff.stderr)
        return diff.returncode
    issues.extend(check_staged_diff(diff.stdout))
    if issues:
        for issue in issues:
            sys.stderr.write(f"pre-commit guard: {issue}\n")
        return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if not args:
        sys.stderr.write("usage: guards.py <pre-commit|commit-msg <file>>\n")
        return 2
    if args[0] == "pre-commit":
        return run_pre_commit(Path.cwd())
    if args[0] == "commit-msg" and len(args) == 2:
        text = Path(args[1]).read_text(encoding="utf-8", errors="replace")
        issues = check_commit_message(text)
        if issues:
            for issue in issues:
                sys.stderr.write(f"commit-msg guard: {issue}\n")
            return 1
        return 0
    sys.stderr.write("usage: guards.py <pre-commit|commit-msg <file>>\n")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
