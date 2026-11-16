#!/usr/bin/env python3
"""PreToolUse hook: refuse `rm` shapes that Claude Code always stops to ask about.

Claude Code holds a removal for explicit approval when it cannot resolve the
target statically, and neither a permission rule nor the bypass mode skips that
prompt. In an unattended or hours-long session the prompt parks the agent until
somebody answers. This hook refuses the two shapes that keep occurring before
the prompt is reached and says how to write the removal instead, so the agent
rewrites the command and carries on:

- a relative glob target after the command changed directory
  (`cd run && rm -rf cfg/*`);
- a target that is a shell expansion on its own or followed by one path
  component, which is a top-level path when the expansion is empty
  (`rm -f $E/*.png`, `rm -rf "$DIR"`).

It reads the command as text and never expands it. Anything it cannot read
(a heredoc, unbalanced quoting) passes through to Claude Code's own check.
"""
import json
import re
import shlex
import sys

SEPARATORS = {";", "&", "&&", "|", "||", "(", ")", "\n", "{", "}"}
PREFIXES = {"do", "then", "else", "elif", "if", "while", "until", "time", "!", "sudo", "command", "exec"}
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
GLOB = re.compile(r"[*?\[]")
SUBSTITUTION = re.compile(r"\$\([^()]*\)|`[^`]*`")
# `$NAME` or `${NAME...}` (a substitution is renamed to one first), then at most one path component.
EXPANSION_ROOT = re.compile(r"^(?:\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*|\$[0-9@*])(?:/[^/]*)?/*$")


def segments(command: str) -> list[list[str]]:
    """The simple commands of `command`, each as its words with quotes removed."""
    # A substitution is one word to the shell; name it so its parentheses do not split it.
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


def problem(command: str) -> str | None:
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


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        return 0
    if data.get("tool_name") != "Bash":
        return 0
    command = (data.get("tool_input") or {}).get("command")
    if not isinstance(command, str) or "rm" not in command:
        return 0
    found = problem(command)
    if found is None:
        return 0
    sys.stderr.write(
        f"{found}. Claude Code cannot resolve that target statically, so it would stop and wait for "
        "a person to approve the removal, whatever the permission mode. Run the removal again with "
        "the literal absolute path written out (for example `rm -rf /abs/run/cfg/*`), in a command "
        "that does not `cd` first and names no variable at the start of the target. Nothing was "
        "removed.\n"
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
