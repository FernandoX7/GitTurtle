"""The Omarchy check's prompts, held without a desktop.

A static test over every tracked shell script under scripts/: no apostrophe in
the word of a "${name:-word}" expansion that sits in double quotes. Bash reads
such an apostrophe as the start of a quoted stretch, and with one in each of
two prompts of scripts/omarchy-verify.sh the first prompt ran on into the
second (found by the first run with a person at the prompts, 2026-11-17).
"""
from __future__ import annotations

from pathlib import Path
import re
import subprocess
import unittest

SCRIPTS = Path(__file__).resolve().parents[2]
ROOT = SCRIPTS.parent
VERIFY = SCRIPTS / "omarchy-verify.sh"

# The two prompts as they stood before the repair (f72de4e7), kept to show that
# the static test catches them: each names a fallback with an apostrophe inside
# the word of a "${name:-word}".
PROMPTS_BEFORE_THE_REPAIR = r'''
  ask 1 hands-copy "Click a commit in History, then press ${copy:-Omarchy's copy binding} (Omarchy's Universal copy)." \
    "Did GitTurtle say that it copied?" && report_clipboard
  ask 2 hands-paste "Click History's search field, then press ${paste:-Omarchy's paste binding} (Omarchy's Universal paste)." \
    "Is the commit's hash in the search field?"
'''


class Unreadable(ValueError):
    """The scan lost its place in a script: it says so, and never guesses."""


class ShellScan:
    """Reads shell text far enough to know where double quotes hold, and notes
    each apostrophe in the word of a default-value expansion written there: in
    a double-quoted string or in a here-document whose delimiter is not quoted,
    which bash expands in the same way. Outside both, an apostrophe quotes, as
    it also does in the pattern of "${name#pattern}" and its relatives."""

    PARAMETER = re.compile(r"[!#]?(?:[A-Za-z_][A-Za-z0-9_]*|[0-9]+|[@*#?$!-])?(?:\[[^\]]*\])?")
    DEFAULT = re.compile(r":?[-=+?]")  # ${name:-word}, ${name-word}, and the same with =, + and ?
    HEREDOC = re.compile(r"(-?)[ \t]*(?:'([^'\n]*)'|\"([^\"\n]*)\"|\\?([A-Za-z0-9_.]+))")
    KEYWORD = re.compile(r"(?<![\w$./-])(case|esac)(?=[\s;)]|$)")

    def __init__(self, text: str) -> None:
        self.text = text
        self.found: list[int] = []  # the line of each such apostrophe
        self.heredocs: list[tuple[str, bool, bool]] = []  # opened on this line: delimiter, tabs stripped, literal

    def line(self, index: int) -> int:
        return self.text.count("\n", 0, index) + 1

    def lost(self, what: str, index: int) -> Unreadable:
        return Unreadable(f"{what} that opens on line {self.line(index)} never closes")

    def code(self, index: int, closer: str | None = None, arithmetic: bool = False) -> int:
        """Shell code up to its closer: ")" for "$(", a backtick, or the end of the text."""
        text, start, parentheses, cases = self.text, index, 0, 0
        while index < len(text):
            character = text[index]
            if character == "\\":
                index += 2
            elif character == "'":
                index = self.single_quoted(index + 1)
            elif character == '"':
                index = self.quoted(index + 1)
            elif character == "`":
                if closer == "`":
                    return index + 1
                index = self.code(index + 1, "`")
            elif character == "$":
                index = self.dollar(index, quoted=False)
            elif character == "#" and not arithmetic and (index == 0 or text[index - 1] in " \t\n;&|()"):
                index = self.end_of_line(index)  # a comment; its newline still ends the line
            elif character == "<" and not arithmetic and text.startswith("<<", index):
                index = index + 3 if text.startswith("<<<", index) else self.heredoc_opener(index + 2)
            elif character == "\n":
                index = self.heredoc_bodies(index + 1)
            elif character == "(":
                parentheses += 1
                index += 1
            elif character == ")":
                if parentheses:
                    parentheses -= 1
                elif not cases and closer == ")":  # inside "case", this one ends a pattern
                    return index + 1
                index += 1
            elif keyword := self.KEYWORD.match(text, index):
                cases = max(0, cases + (1 if keyword[1] == "case" else -1))
                index = keyword.end()
            else:
                index += 1
        if closer is not None:
            raise self.lost("a command substitution", start)
        return index

    def end_of_line(self, index: int) -> int:
        end = self.text.find("\n", index)
        return len(self.text) if end < 0 else end

    def single_quoted(self, index: int) -> int:
        end = self.text.find("'", index)
        if end < 0:
            raise self.lost("a single-quoted string", index)
        return end + 1

    def quoted(self, index: int, stop: int | None = None) -> int:
        """A double-quoted string up to its closing quote or, with `stop`, the
        body of a here-document, where a double quote is a character like any."""
        text, start = self.text, index
        end = len(text) if stop is None else stop
        while index < end:
            character = text[index]
            if character == "\\":
                index += 2
            elif character == '"' and stop is None:
                return index + 1
            elif character == "`":
                index = self.code(index + 1, "`")
            elif character == "$":
                index = self.dollar(index, quoted=True)
            else:
                index += 1
        if stop is None:
            raise self.lost("a double-quoted string", start)
        return end

    def dollar(self, index: int, quoted: bool) -> int:
        text = self.text
        if text.startswith("$((", index):
            index = self.code(index + 3, ")", arithmetic=True)
            return index + 1 if text.startswith(")", index) else index
        if text.startswith("$(", index):
            return self.code(index + 2, ")")
        if text.startswith("${", index):
            return self.expansion(index + 2, quoted)
        if not quoted and text.startswith("$'", index):
            return self.ansi_quoted(index + 2)
        return index + 1

    def ansi_quoted(self, index: int) -> int:
        text, start = self.text, index
        while index < len(text):
            if text[index] == "\\":
                index += 2
            elif text[index] == "'":
                return index + 1
            else:
                index += 1
        raise self.lost("a $'...' string", start)

    def expansion(self, index: int, quoted: bool) -> int:
        """What follows "${": the parameter, then its word up to the closing
        brace. Where double quotes hold and the word is a default value, each
        apostrophe in it is noted; anywhere else an apostrophe quotes."""
        text, start = self.text, index
        index = self.PARAMETER.match(text, index).end()
        default = self.DEFAULT.match(text, index) if quoted else None
        if default:
            index = default.end()
        while index < len(text):
            character = text[index]
            if character == "\\":
                index += 2
            elif character == "}":
                return index + 1
            elif character == "'" and default:
                self.found.append(self.line(index))
                index += 1
            elif character == "'":
                index = self.single_quoted(index + 1)
            elif character == '"':
                index = self.quoted(index + 1)
            elif character == "`":
                index = self.code(index + 1, "`")
            elif character == "$":
                index = self.dollar(index, quoted)
            else:
                index += 1
        raise self.lost("a ${...} expansion", start)

    def heredoc_opener(self, index: int) -> int:
        opener = self.HEREDOC.match(self.text, index)
        if not opener:
            raise Unreadable(f"line {self.line(index)}: a here-document without a delimiter the scan can read")
        literal = opener[2] is not None or opener[3] is not None or self.text[opener.start(4) - 1] == "\\"
        self.heredocs.append((opener[2] or opener[3] or opener[4], bool(opener[1]), literal))
        return opener.end()

    def heredoc_bodies(self, index: int) -> int:
        """The bodies of the here-documents opened on the line that just ended."""
        text = self.text
        while self.heredocs:
            delimiter, strip_tabs, literal = self.heredocs.pop(0)
            start = index
            while True:
                if index >= len(text):
                    raise self.lost(f"the here-document ending in {delimiter}", start)
                end = self.end_of_line(index)
                last = text[index:end].lstrip("\t") if strip_tabs else text[index:end]
                if last == delimiter:
                    break
                index = end + 1
            if not literal:
                self.quoted(start, stop=index)
            index = min(end + 1, len(text))
        return index


def apostrophes_in_default_words(text: str) -> list[int]:
    """The line of each apostrophe that stands in the word of a default-value
    expansion where double quotes hold."""
    scan = ShellScan(text)
    scan.code(0)
    return scan.found


def tracked_shell_scripts() -> list[Path]:
    listed = subprocess.run(["git", "-C", str(ROOT), "ls-files", "-z", "--", "scripts/*.sh"],
                            capture_output=True, text=True, timeout=30)
    if listed.returncode == 0 and listed.stdout:
        return [ROOT / name for name in listed.stdout.split("\0") if name]
    return sorted(SCRIPTS.rglob("*.sh"))  # not a checkout: every script that is there


class ApostropheInDefaultWordTests(unittest.TestCase):
    def test_the_prompts_as_they_stood_before_the_repair_are_caught(self):
        # One apostrophe in each prompt's fallback; the ones outside the braces,
        # in the same strings, are ordinary text.
        self.assertEqual([2, 4], apostrophes_in_default_words(PROMPTS_BEFORE_THE_REPAIR))

    def test_an_apostrophe_is_caught_wherever_double_quotes_hold(self):
        for text in ('echo "${x:-it\'s}"',
                     'echo "${x-it\'s}"',
                     'echo "${x:=it\'s}"',
                     'echo "${x:+it\'s}"',
                     'echo "${x:?it\'s}"',
                     'echo "${names[$key]:-it\'s}"',
                     'echo "${x:-${y:-it\'s}}"',
                     'echo "${x:-${y}\'s}"',
                     'echo "a ${x:-\'b\'} c"',
                     'value=$(printf \'%s\' "${x:-it\'s}")',
                     'value="$(case $x in a) echo " ${x:-it\'s} " ;; esac)"',
                     'cat <<EOF\n${x:-it\'s}\nEOF\n',
                     'cat <<-EOF\n\t${x:-it\'s}\n\tEOF\n',
                     'echo "one\ntwo ${x:-three\nit\'s}"'):
            with self.subTest(text=text):
                self.assertTrue(apostrophes_in_default_words(text))

    def test_an_apostrophe_that_quotes_or_is_plain_text_is_left_alone(self):
        for text in ('keys="Omarchy\'s copy binding"\necho "press ${copy:-$keys}"',
                     "echo ${x:-'a b'}",
                     "echo '${x:-it'\"'\"'s}'",
                     '# a comment: "${x:-it\'s}"\n',
                     'echo "it\'s ${x:-fine}" # it\'s "${x:-fine}"',
                     'echo "${x#\'a\'}" "${x%\'b\'}" "${x//\'c\'/d}"',
                     'echo "${x:-$(echo \'a b\')}"',
                     'echo "${x:-"it\'s"}"',
                     "cat <<'EOF'\n${x:-it's}\nEOF\n",
                     'cat <<\\EOF\n${x:-it\'s}\nEOF\n',
                     "echo $'it\\'s' \"${x:-fine}\"",
                     'case $x in a) echo "it\'s" ;; esac',
                     'value=$(case $x in a) echo "it\'s ${x:-fine}" ;; esac)',
                     'echo $((x << 2)) "${#x}" "$#" # it\'s'):
            with self.subTest(text=text):
                self.assertEqual([], apostrophes_in_default_words(text))

    def test_a_script_the_scan_cannot_follow_is_an_error_and_not_a_pass(self):
        for text in ('echo "never closed', "echo 'never closed", 'echo "${x:-never closed"', "cat <<EOF\nnever closed\n"):
            with self.subTest(text=text), self.assertRaises(Unreadable):
                apostrophes_in_default_words(text)

    def test_no_tracked_shell_script_has_one(self):
        scripts = tracked_shell_scripts()
        self.assertIn(VERIFY, scripts)
        for script in scripts:
            with self.subTest(script=str(script.relative_to(ROOT))):
                self.assertEqual([], apostrophes_in_default_words(script.read_text(encoding="utf-8")),
                                 "lines with an apostrophe in the word of a \"${name:-word}\": set the word in a variable first")


if __name__ == "__main__":
    unittest.main()
