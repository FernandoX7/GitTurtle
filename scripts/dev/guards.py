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
    "scripts/dev/guards.py",
    "scripts/dev/guidance.py",
    "scripts/dev/sync_adapters.py",
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

CONFLICT_MARKER = re.compile(r"^(?:<{7}(?: |$)|\|{7}(?: |$)|={7}$|>{7}(?: |$))")
PROBE_MARKER = re.compile(r"(?://\s*MUTATION\b|\bQ9_PROBE\b)")
AI_TRAILER = re.compile(
    r"(?im)^\s*(?:co-authored-by|signed-off-by)\s*:.*\b(?:claude|anthropic|gemini|openai|chatgpt|copilot|codex|cursor|grok)\b"
)
# Files that legitimately contain conflict-marker or probe-marker test strings.
SELF_TEST_PATHS = (
    "scripts/dev/guards.py",
    "scripts/dev/test_guards.py",
    "scripts/dev/guidance.py",
    "scripts/dev/test_guidance.py",
    ".githooks/pre-commit",
    ".githooks/commit-msg",
)


def relative_path(raw: str, cwd: Path) -> str:
    path = Path(raw)
    if not path.is_absolute():
        path = cwd / path
    try:
        return path.resolve().relative_to(cwd.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


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


def check_staged_paths(paths: list[str], *, loop_mode: bool = False, queue: str = "") -> list[str]:
    issues: list[str] = []
    for path in paths:
        if path.endswith(".snap.new"):
            issues.append(f"staged insta preview snapshot must not be committed: {path}")
        if loop_mode:
            if queue and path == queue:
                issues.append(f"protected active task queue in loop mode: {path}")
            elif is_protected(path):
                issues.append(f"protected policy path in loop mode: {path}")
    return issues


def check_staged_diff(diff_text: str) -> list[str]:
    """Scan added lines in a unified diff for conflict markers and temporary probe markers."""
    issues: list[str] = []
    current_path = ""
    line_no = 0
    for raw in diff_text.splitlines():
        if raw.startswith("+++ b/"):
            current_path = raw[6:]
            continue
        if raw.startswith("@@ "):
            match = re.search(r"\+(\d+)", raw)
            line_no = int(match.group(1)) - 1 if match else 0
            continue
        if not current_path or current_path in SELF_TEST_PATHS:
            continue
        if raw.startswith("+") and not raw.startswith("+++"):
            line_no += 1
            body = raw[1:]
            if current_path.endswith((".md", ".yml", ".yaml", ".toml", ".json", ".py", ".sh", ".html", ".css", ".js")):
                if CONFLICT_MARKER.match(body):
                    issues.append(f"{current_path}:{line_no}: merge conflict marker")
            if current_path.startswith(("crates/", "vendor/", "scripts/")) and PROBE_MARKER.search(body):
                issues.append(f"{current_path}:{line_no}: temporary mutation or probe marker")
        elif not raw.startswith("-"):
            line_no += 1
    return issues


def check_commit_message(message: str) -> list[str]:
    issues: list[str] = []
    if AI_TRAILER.search(message):
        issues.append("commit message must not contain AI co-author or sign-off trailers")
    return issues


def run_pre_commit(repo: Path) -> int:
    staged = subprocess.run(
        ["git", "-C", str(repo), "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"],
        capture_output=True, check=False,
    )
    if staged.returncode != 0:
        sys.stderr.write(staged.stderr.decode("utf-8", errors="replace"))
        return staged.returncode
    paths = [p.decode("utf-8", errors="surrogateescape") for p in staged.stdout.split(b"\0") if p]
    issues = check_staged_paths(
        paths,
        loop_mode=os.environ.get("GITTURTLE_LOOP") == "1",
        queue=active_queue(),
    )
    diff = subprocess.run(
        ["git", "-C", str(repo), "diff", "--cached", "--unified=0", "--no-color"],
        capture_output=True, text=True, errors="replace", check=False,
    )
    if diff.returncode == 0:
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
