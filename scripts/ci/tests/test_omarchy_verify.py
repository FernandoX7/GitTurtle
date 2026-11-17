"""The Omarchy check's prompts, held without a desktop.

A static test over every tracked shell script under scripts/: no apostrophe in
the word of a "${name:-word}" expansion that sits in double quotes. Bash reads
such an apostrophe as the start of a quoted stretch, and with one in each of
two prompts of scripts/omarchy-verify.sh the first prompt ran on into the
second (found by the first run with a person at the prompts, 2026-11-17).

A replay of the four prompts of `--hands`. It sources the script, which runs
`main` only when it is executed, and calls `check_hands` itself; nothing of the
prompts is repeated here. What touches a desktop is replaced: the app, the
pictures, Hyprland and Omarchy's shell by functions, and the clipboard by
stand-ins for wl-copy and wl-paste that keep it in a file, first on a PATH of
the replay's own, in an environment that names no desktop session. The answers
come through a pseudo-terminal on descriptor 3, as a person's do, and are given
only when the prompt they answer has been printed.
"""
from __future__ import annotations

import dataclasses
import os
from pathlib import Path
import platform
import re
import selectors
import shlex
import shutil
import signal
import subprocess
import tempfile
import termios
import time
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


# ---------------------------------------------------------------- the replay of --hands

BASH = shutil.which("bash")
REPLAY_LIMIT = 20  # seconds one replay may take; it takes well under one
PROMPT = re.compile(r"^\[(\d)/4\] (.*)\n", re.MULTILINE)  # a prompt's number and what it asks the person to do
PICTURE = re.compile(r"picture \S+: (?:not )?taken by the replay")
HASH = re.compile(r"[0-9a-f]{40}")
KEYS = ("SUPER + C", "SUPER + V")  # what the bindings file gives for copy and paste, where it has them
FALLBACKS = ("copy binding", "paste binding")  # what a prompt names where it has none
# What each prompt is about: words its own text holds, and words that belong to
# another prompt. A prompt that ran on into the next one, as the first did on
# 2026-11-17, holds both.
ABOUT = {1: (("commit", "copy"), ("paste",)),
         2: (("search field", "paste"), ("copy",)),
         3: (("Ctrl+O",), ("copy", "paste", "Cancel")),
         4: (("Cancel",), ("copy", "paste", "Ctrl+O"))}
QUESTION_ABOUT = {2: "search field", 3: "folder picker", 4: "as it was"}
# Commands that reach a desktop. A stand-in for each is on the replay's PATH, and
# the replay fails if one of them was called.
DESKTOP_COMMANDS = ("hyprctl", "grim", "omarchy-shell", "omarchy-toggle-idle")

# Every stand-in first writes down that it was called, by what, and what the call
# left it holding of the caller's: wl-copy stays behind as a process, so it must
# hold neither the script's output nor the terminal of the answers.
STAND_IN = r'''#!@BASH@
# The replay's stand-in for @TOOL@: the clipboard is a file, and no desktop is reached.
held=""
[[ /dev/fd/0 -ef /dev/null ]] || held+=" stdin"
[[ /dev/fd/1 -ef /dev/null ]] || held+=" stdout"
[[ /dev/fd/2 -ef /dev/null ]] || held+=" stderr"
{ : <&3; } 2>/dev/null && held+=" descriptor-3"
parent=$(cat "/proc/$PPID/comm" 2>/dev/null || ps -o comm= -p "$PPID" 2>/dev/null)
printf '%s\t%s\t%s\t%s\n' @TOOL@ "${parent##*/}" "${held# }" "$*" >>@LOG@
'''
FAILS = "exit 1\n"
# wl-copy takes --sensitive since wl-clipboard 2.3; one from before that refuses
# the call and leaves the clipboard as it was.
REFUSES_SENSITIVE = r'''for argument in "$@"; do
  [[ $argument != -- ]] || break
  if [[ $argument == --sensitive ]]; then
    echo "wl-copy: unrecognized option '--sensitive'" >&2
    exit 1
  fi
done
'''
COPIES = r'''[[ ${1:-} != --sensitive ]] || shift
if [[ $* == --clear ]]; then
  : >@CLIPBOARD@
elif [[ ${1:-} == -- && $# -ge 2 ]]; then
  shift
  printf '%s' "$*" >@CLIPBOARD@
else
  echo "the stand-in for wl-copy does not know this call: $*" >&2
  exit 2
fi
'''
FAILS_ONCE = r'''if [[ ! -e @CLIPBOARD@.read ]]; then
  : >@CLIPBOARD@.read
  exit 1
fi
'''
PASTES = r'''if [[ $* != --no-newline ]]; then
  echo "the stand-in for wl-paste does not know this call: $*" >&2
  exit 2
fi
if [[ ! -s @CLIPBOARD@ ]]; then
  echo "Nothing is copied" >&2
  exit 1
fi
cat @CLIPBOARD@
'''

DRIVER = r'''# The replay's driver: check_hands itself, as scripts/omarchy-verify.sh defines it.
for tool in wl-copy wl-paste; do
  if [[ $(command -v "$tool") != "$REPLAY_SHIMS/$tool" ]]; then
    echo "replay refused: $tool is not the replay's own stand-in" >&2
    exit 97
  fi
done
if [[ -n ${WAYLAND_DISPLAY:-}${HYPRLAND_INSTANCE_SIGNATURE:-} || $HOME != "$REPLAY_HOME" ]]; then
  echo "replay refused: the environment names a desktop session, or a home that is not the replay's" >&2
  exit 97
fi
source "$REPLAY_SCRIPT" || exit 98

# What touches a desktop, replaced.
skip_gate() { :; }
desktop_gate() { :; }
launch_app() { :; }
wait_for_app_window() { :; }
silence_notifications() { :; }
hypr_json() { echo '[]'; }
binding_keys() {
  case $1 in
    *copy) printf '%s' "$REPLAY_COPY_KEYS" ;;
    *paste) printf '%s' "$REPLAY_PASTE_KEYS" ;;
  esac
}
PICTURES_ASKED=0
picture() {
  PICTURES_ASKED=$((PICTURES_ASKED + 1))
  if [[ " $REPLAY_NO_PICTURE " == *" $PICTURES_ASKED "* ]]; then
    report "picture $1: not taken by the replay"
    return 1
  fi
  report "picture $1: taken by the replay"
}
# The names of this host and its user are no part of the replay.
redact() { cat; }
# A tool the replay calls absent: `have` says so, and a stand-in that fails stays
# on PATH to write down a call that should never come.
if [[ -n $REPLAY_ABSENT ]]; then
  eval "$(declare -f have | sed '1s/^have/installed/')"
  have() { [[ " $REPLAY_ABSENT " != *" $1 "* ]] && installed "$1"; }
fi

# The state the checks before this one leave, and where the report goes.
HANDS=$REPLAY_HANDS INSTALLED_BINARY=$REPLAY_RUN/no-such-executable DEMO=$REPLAY_DEMO
OUT=$REPLAY_RUN/results WORK=$OUT/work RECORDS=$OUT/records PICTURES=$OUT/pictures
REPORT=$OUT/report.txt RESTORE=$WORK/restore LAUNCH=$WORK/launch
mkdir -p "$RECORDS" "$PICTURES" "$LAUNCH" || exit 98

# The end of a run, as the script's own exit has it: what is still registered is
# restored. A stop arrives as it does in a real run, through on_signal.
ending() {
  local line
  {
    printf 'done\t%s\n' "$HANDS_DONE"
    printf 'stopped by\t%s\n' "$STOPPED_BY"
    for line in "${PROBLEMS[@]}"; do printf 'problem\t%s\n' "$line"; done
    for line in "${PARTIALS[@]}"; do printf 'not covered\t%s\n' "$line"; done
    for line in "${RESTORES[@]}"; do printf 'left to the exit\t%s\n' "$line"; done
  } >"$REPLAY_RUN/state.txt"
  restore_all
  for line in "${RESTORE_FAILURES[@]}"; do printf 'restore failed\t%s\n' "$line" >>"$REPLAY_RUN/state.txt"; done
}
trap ending EXIT
trap 'on_signal TERM 143' TERM
# The answers on descriptor 3 and nowhere else, as main leaves them.
exec 3<&"$REPLAY_ANSWERS"
[[ $REPLAY_ANSWERS == 3 ]] || eval "exec $REPLAY_ANSWERS<&-"
check_hands
'''


def replay_refusal() -> str | None:
    """Why the replay cannot run on this machine, or None where it can."""
    if not BASH:
        return "the replay of the hands check needs bash, and none is installed"
    version = subprocess.run([BASH, "-c", 'echo "${BASH_VERSINFO[0]} ${BASH_VERSINFO[1]}"'],
                             capture_output=True, text=True, timeout=10).stdout.split()
    if len(version) != 2 or (int(version[0]), int(version[1])) < (4, 4):
        return ("the replay of the hands check needs bash 4.4 or newer, as scripts/omarchy-verify.sh does, and this is "
                f"bash {'.'.join(version) or 'of an unknown version'} (macOS ships 3.2)")
    missing = [tool for tool in ("git", "timeout") if not shutil.which(tool)]
    if missing:
        return f"the replay of the hands check needs {' and '.join(missing)}, as scripts/omarchy-verify.sh does"
    return None


def write_stand_in(path: Path, body: str, clipboard: Path, log: Path) -> None:
    text = (STAND_IN + body).replace("@BASH@", BASH).replace("@TOOL@", path.name)
    path.write_text(text.replace("@CLIPBOARD@", shlex.quote(str(clipboard))).replace("@LOG@", shlex.quote(str(log))))
    path.chmod(0o755)


@dataclasses.dataclass
class Prompt:
    number: int
    do: str  # what it asks the person to do
    said: str  # what the terminal showed from there to the next prompt
    records: list[str]  # its "hands N: <do> | ..." lines in the report


@dataclasses.dataclass
class Replay:
    directory: Path
    status: int
    out: str
    err: str
    report: list[str]
    state: dict[str, list[str]]
    calls: list[list[str]]  # of the stand-ins: tool, parent, what it was left holding, arguments
    clipboard_at: dict[int, str]  # what the clipboard held when each prompt was printed
    clipboard_after: str
    prompts: list[Prompt]

    @property
    def introduction(self) -> str:
        """What the terminal told the person before the first prompt, beyond the
        lines that also go to the report."""
        first = PROMPT.search(self.out)
        shown = self.out[:first.start()] if first else self.out
        return "\n".join(line for line in shown.splitlines() if line not in self.report)

    @property
    def done(self) -> int:
        return int(self.state["done"][0])

    @property
    def problems(self) -> list[str]:
        return self.state.get("problem", [])

    @property
    def not_covered(self) -> list[str]:
        return self.state.get("not covered", [])

    @property
    def result(self) -> str:
        return next(line for line in reversed(self.report) if line.startswith("RESULT hands"))

    def cleared(self) -> int:
        """How often the script cleared the clipboard."""
        return sum(call[0] == "wl-copy" and call[3] == "--clear" for call in self.calls)

    def offered(self) -> list[str]:
        """The arguments of each call that offered the clipboard a text, whether wl-copy took it or not."""
        return [call[3] for call in self.calls if call[0] == "wl-copy" and call[3] != "--clear"]


class HandsReplayTests(unittest.TestCase):
    """`check_hands` with canned answers. A copy is the person's: the clipboard
    file is written when the prompt that asks for it has been printed, and only
    then is Enter pressed."""

    @classmethod
    def setUpClass(cls):
        refusal = replay_refusal()
        if refusal and platform.system() == "Linux" and "CI" in os.environ:
            raise AssertionError(f"{refusal}; CI is set on Linux, where the replay must run")
        if refusal:
            raise unittest.SkipTest(refusal)
        cls.directory = Path(tempfile.mkdtemp(prefix="gitturtle-hands-replay-"))
        cls.addClassCleanup(shutil.rmtree, cls.directory, ignore_errors=True)
        (cls.directory / "driver.sh").write_text(DRIVER)
        # The demo: a real repository, with a subject that holds an apostrophe.
        cls.demo = cls.directory / "demo"
        cls.demo.mkdir()
        environment = {"PATH": os.environ.get("PATH", os.defpath), "HOME": str(cls.directory),
                       "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
                       "GIT_AUTHOR_NAME": "Maya Chen", "GIT_AUTHOR_EMAIL": "maya@aurora.example.invalid",
                       "GIT_COMMITTER_NAME": "Maya Chen", "GIT_COMMITTER_EMAIL": "maya@aurora.example.invalid",
                       "GIT_AUTHOR_DATE": "2026-01-05T10:00:00Z", "GIT_COMMITTER_DATE": "2026-01-05T10:00:00Z"}

        def git(*arguments: str) -> str:
            return subprocess.run(["git", "-C", str(cls.demo), *arguments], env=environment, check=True,
                                  capture_output=True, text=True, timeout=30).stdout

        git("init", "--quiet", "--initial-branch=main")
        for subject in ("Start the replay's demo", "Say what the demo is", "Describe how to use it"):
            (cls.demo / "README.md").write_text(f"{subject}\n")
            git("add", "--all")
            git("commit", "--quiet", "--message", subject)
        cls.commits = [tuple(line.split(" ", 1)) for line in git("log", "--reverse", "--format=%H %s").splitlines()]

    def replay(self, *, before: str = "", copies: dict[int, str] | None = None, keys: tuple[str, str] = KEYS,
               leaves_at: int | None = None, stops_at: int | None = None, absent: tuple[str, ...] = (),
               broken: tuple[str, ...] = (), no_picture: tuple[int, ...] = (), hands: str = "yes",
               terminal: bool = True, impostor: bool = False, sensitive: bool = True,
               first_read_fails: bool = False) -> Replay:
        """One run of check_hands. `before` is what the clipboard holds when the
        check starts, and `copies` what the person copies when a prompt has been
        printed, by the prompt's number. The person presses Enter at every prompt
        and answers y to every question, except that at `leaves_at` they close
        the terminal instead and at `stops_at` the run is stopped. Without
        `sensitive`, wl-copy is one that does not know --sensitive; with
        `first_read_fails`, wl-paste fails the first time it is called."""
        copies = copies or {}
        run = Path(tempfile.mkdtemp(dir=self.directory))
        shims, home, clipboard, log = run / "shims", run / "home", run / "clipboard", run / "calls.log"
        shims.mkdir()
        home.mkdir()
        clipboard.write_text(before)
        log.touch()
        for tool, body in (("wl-copy", COPIES if sensitive else REFUSES_SENSITIVE + COPIES),
                           ("wl-paste", FAILS_ONCE + PASTES if first_read_fails else PASTES)):
            write_stand_in(shims / tool, FAILS if tool in absent or tool in broken else body, clipboard, log)
        for tool in DESKTOP_COMMANDS:
            write_stand_in(shims / tool, FAILS, clipboard, log)
        path = [str(shims), os.environ.get("PATH", os.defpath)]
        if impostor:  # another wl-copy, found before the replay's own
            (run / "impostor").mkdir()
            write_stand_in(run / "impostor/wl-copy", FAILS, clipboard, log)
            path.insert(0, str(run / "impostor"))
        if terminal:
            person, answers = os.openpty()
            attributes = termios.tcgetattr(answers)
            attributes[3] &= ~termios.ECHO
            termios.tcsetattr(answers, termios.TCSANOW, attributes)
        else:
            person, answers = None, os.open(os.devnull, os.O_RDONLY)
        # Built from nothing: no desktop session, no real home, no Git setting of the caller's.
        environment = {"PATH": os.pathsep.join(path), "HOME": str(home), "TMPDIR": str(run), "LC_ALL": "C",
                       "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "OMARCHY_PATH": str(run / "no-omarchy"),
                       "REPLAY_SCRIPT": str(VERIFY), "REPLAY_SHIMS": str(shims), "REPLAY_HOME": str(home),
                       "REPLAY_RUN": str(run), "REPLAY_DEMO": str(self.demo), "REPLAY_HANDS": hands,
                       "REPLAY_COPY_KEYS": keys[0], "REPLAY_PASTE_KEYS": keys[1], "REPLAY_ABSENT": " ".join(absent),
                       "REPLAY_NO_PICTURE": " ".join(map(str, no_picture)), "REPLAY_ANSWERS": str(answers)}
        out, err, clipboard_at = bytearray(), bytearray(), {}
        deadline = time.monotonic() + REPLAY_LIMIT
        try:
            process = subprocess.Popen([BASH, str(self.directory / "driver.sh")], stdin=subprocess.DEVNULL,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment, cwd=run,
                                       pass_fds=(answers,), start_new_session=True)
        finally:
            os.close(answers)
        try:
            with process, selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ, out)
                selector.register(process.stderr, selectors.EVENT_READ, err)
                while selector.get_map():
                    for key, _ in selector.select(max(0.0, deadline - time.monotonic())):
                        chunk = os.read(key.fd, 65536)
                        if chunk:
                            key.data.extend(chunk)
                        else:
                            selector.unregister(key.fileobj)
                    for header in PROMPT.finditer(out.decode(errors="replace")):
                        number = int(header[1])
                        if number in clipboard_at:
                            continue
                        # The person, once a prompt is printed: a copy if this is where they copy, then the keys.
                        clipboard_at[number] = clipboard.read_text()
                        if number in copies:
                            clipboard.write_text(copies[number])
                        if number == stops_at:
                            process.send_signal(signal.SIGTERM)
                        elif number == leaves_at:
                            os.write(person, b"\x04")  # the end of the terminal's input
                        else:
                            os.write(person, b"\n" if number == 1 else b"\ny\n")
                    if time.monotonic() >= deadline:
                        process.kill()  # the replay's own process, by its pid
                        self.fail(f"the replay did not end within {REPLAY_LIMIT} s; the terminal showed:\n"
                                  f"{out.decode(errors='replace')}\n{err.decode(errors='replace')}")
        finally:
            if person is not None:
                os.close(person)
        report = run / "results/report.txt"
        report_lines = report.read_text().splitlines() if report.exists() else []
        state: dict[str, list[str]] = {}
        if (run / "state.txt").exists():
            for line in (run / "state.txt").read_text().splitlines():
                name, _, value = line.partition("\t")
                state.setdefault(name, []).append(value)
        calls = [line.split("\t") for line in log.read_text().splitlines()]
        text = out.decode()
        headers = list(PROMPT.finditer(text))
        prompts = [Prompt(int(header[1]), header[2],
                          text[header.end():headers[place + 1].start() if place + 1 < len(headers) else len(text)],
                          [line for line in report_lines if line.startswith(f"hands {header[1]}: {header[2]} | ")])
                   for place, header in enumerate(headers)]
        # Whatever the case, the desktop was not reached for, and no call could hold the script.
        self.assertEqual([], [call for call in calls if call[0] in DESKTOP_COMMANDS], "the replay reached for the desktop")
        for tool, parent, held, arguments in calls:
            self.assertEqual("timeout", parent, f"{tool} {arguments} ran without a time limit")
            if tool == "wl-copy":
                self.assertEqual("", held, f"wl-copy {arguments} was left holding what is the script's")
                # Omarchy's clipboard history keeps every copy but one offered as sensitive.
                self.assertTrue(arguments == "--clear" or arguments.startswith("--sensitive -- "),
                                f"wl-copy {arguments} offered a text without --sensitive in front of it")
        return Replay(run, process.returncode, text, err.decode(), report_lines, state, calls, clipboard_at,
                      clipboard.read_text(), prompts)

    def assert_asked(self, run: Replay, numbers: tuple[int, ...] = (1, 2, 3, 4), keys: tuple[str, str] = KEYS,
                     entered: tuple[int, ...] | None = None, withheld: tuple[int, ...] = ()) -> None:
        """The prompts were printed in order and once each, every one with its own
        words; and each that got its Enter took its picture and asked its own
        question once, except the first, which asks none. For a prompt in
        `withheld` the picture was not asked for at all."""
        entered = numbers if entered is None else entered
        self.assertEqual("", run.err)
        self.assertEqual(list(numbers), [prompt.number for prompt in run.prompts], run.out)
        questions = []
        for prompt in run.prompts:
            own, of_others = ABOUT[prompt.number]
            for word in own:
                self.assertIn(word, prompt.do)
            for word in of_others:
                self.assertNotIn(word, prompt.do)
            if prompt.number <= 2:  # the copy keys in the first, the paste keys in the second
                self.assertIn(keys[prompt.number - 1], prompt.do)
                self.assertNotIn(keys[2 - prompt.number], prompt.do)
            if prompt.number not in entered:
                self.assertEqual([], prompt.records)
                continue
            self.assertEqual(1, len(prompt.records), run.report)
            # No picture while the folder picker shows: it lists the owner's folders.
            self.assertEqual(0 if prompt.number == 3 or prompt.number in withheld else 1,
                             len(PICTURE.findall(prompt.said)), prompt.said)
            if prompt.number == 1:
                self.assertNotIn("[y/n]", prompt.said)
                continue
            question, answer = prompt.records[0].removeprefix(f"hands {prompt.number}: {prompt.do} | ").rsplit(" ", 1)
            self.assertEqual("y", answer)
            self.assertIn(QUESTION_ABOUT[prompt.number], question)
            self.assertEqual(1, prompt.said.count(f"      {question} [y/n]: "), prompt.said)
            self.assertEqual(1, prompt.said.count("[y/n]"), prompt.said)
            questions.append(question)
        self.assertEqual(len(run.prompts), len({prompt.do for prompt in run.prompts}))
        self.assertEqual(len(questions), len(set(questions)))

    def assert_marked(self, run: Replay, before: str = "") -> str:
        """Before the first prompt was printed the check had put a line of its own
        on the clipboard and said so. Returns that line."""
        marker = run.clipboard_at[1]
        self.assertNotEqual(before, marker)
        self.assertRegex(marker, r"\A[ -~]+\Z")  # one line of plain text
        self.assertIsNone(HASH.search(marker))
        self.assertIn("clipboard", run.introduction)
        self.assertIn(f'"{marker}"', run.introduction)
        return marker

    def assert_never_written(self, run: Replay, word: str) -> None:
        self.assertNotIn(word, run.out)
        self.assertNotIn(word, run.err)
        for path in run.directory.rglob("*"):
            if path.is_file() and path.name != "clipboard":
                self.assertNotIn(word.encode(), path.read_bytes(), path.name)

    def the_problem(self, run: Replay) -> str:
        """The check's one problem, which fails it and which the report holds."""
        self.assertEqual(1, len(run.problems), "\n".join(run.report))
        self.assertIn(f"PROBLEM {run.problems[0]}", run.report)
        self.assertEqual(f"RESULT hands FAILED: {run.problems[0]}", run.result)
        return run.problems[0]

    def the_part_not_covered(self, run: Replay) -> str:
        """The one part the check did not cover, with nothing failed: its result is partial."""
        self.assertEqual((1, []), (len(run.not_covered), run.problems), "\n".join(run.report))
        self.assertIn(f"NOT COVERED {run.not_covered[0]}", run.report)
        self.assertEqual(f"RESULT hands partial: {run.not_covered[0]}", run.result)
        return run.not_covered[0]

    def test_a_copied_commit_is_reported_with_its_subject_and_all_four_prompts_count(self):
        (_, first), (commit, subject), (_, last) = self.commits
        run = self.replay(before="what the owner had copied", copies={1: commit})
        self.assert_asked(run)
        self.assert_marked(run, "what the owner had copied")
        self.assertIn(f'"{subject}"', run.prompts[0].records[0])
        for other in (first, last):
            self.assertNotIn(other, "\n".join(run.report))
        self.assertEqual((4, [], []), (run.done, run.problems, run.not_covered))
        self.assertEqual("RESULT hands ok", run.result)
        self.assertEqual([], run.state.get("left to the exit", []))

    def test_nothing_copied_is_a_problem_and_the_other_prompts_are_still_asked(self):
        run = self.replay()
        self.assert_asked(run)
        marker = self.assert_marked(run)
        self.assertEqual(marker, run.clipboard_at[2])
        self.assertRegex(self.the_problem(run), r"^hands 1: nothing was copied")
        self.assertEqual((3, []), (run.done, run.not_covered))

    def test_a_hash_from_before_the_check_is_not_taken_for_a_copy(self):
        # The regression of 2026-11-17: the demo's hashes are the same on every
        # run, and the clipboard still held one from the run before.
        earlier, subject = self.commits[0]
        run = self.replay(before=earlier)
        self.assert_asked(run)
        self.assertNotIn(subject, "\n".join(run.report), "a hash from before the check was reported as this run's copy")
        self.assertRegex(self.the_problem(run), r"^hands 1: nothing was copied")
        self.assertEqual((3, []), (run.done, run.not_covered))
        self.assert_marked(run, earlier)

    def test_without_the_bindings_the_first_two_prompts_name_them_in_words_and_stay_apart(self):
        # The apostrophe of 2026-11-17 as behaviour: each fallback in its own prompt.
        commit, subject = self.commits[2]
        run = self.replay(keys=("", ""), copies={1: commit})
        self.assert_asked(run, keys=FALLBACKS)
        self.assertIn(f'"{subject}"', run.prompts[0].records[0])
        self.assertEqual((4, [], []), (run.done, run.problems, run.not_covered))
        self.assertEqual("RESULT hands ok", run.result)

    def test_other_text_on_the_clipboard_is_a_problem_and_is_never_written_down(self):
        secret = "plum-harbour-41"
        with self.subTest("copied at the prompt in place of a commit"):
            run = self.replay(copies={1: secret})
            # The paste of prompt 2 would show the word, and the search field still shows it at
            # prompt 4, so both go without their pictures.
            self.assert_asked(run, withheld=(2, 4))
            self.assertRegex(self.the_problem(run), r"^hands 1: .*\bneither\b")
            self.assertEqual((1, 2), (run.done, len(run.not_covered)))
            for number, not_covered in zip((2, 4), run.not_covered):
                self.assertRegex(not_covered, rf"^hands {number}: .*\bpicture\b")
            self.assert_never_written(run, secret)
            self.assertEqual((secret, 0), (run.clipboard_after, run.cleared()))
        with self.subTest("held before the check"):
            run = self.replay(before=secret)
            self.assert_asked(run)
            self.assertRegex(self.the_problem(run), r"^hands 1: nothing was copied")
            self.assert_never_written(run, secret)

    def test_the_paste_is_pictured_only_while_the_clipboard_holds_a_demo_hash_or_the_checks_own_line(self):
        # The picture of prompt 2 shows what was pasted and is packed into the
        # results: text that is the owner's own must not get into it.
        commit, secret = self.commits[1][0], "plum-harbour-41"
        for label, copies in (("the hash copied at prompt 1", {1: commit}), ("the check's own line", {}),
                              ("other text, copied after the paste", {1: commit, 3: secret})):
            with self.subTest(label):
                run = self.replay(copies=copies)
                self.assert_asked(run)
                self.assertEqual(3, len(PICTURE.findall(run.out)))
                self.assertEqual([], [line for line in run.report if "withheld" in line])
                self.assert_never_written(run, secret)
        # What was pasted stays in the search field, which the picture of prompt 4
        # shows again: that one is withheld too, whatever the clipboard holds by then.
        for label, copies in (("other text", {1: commit, 2: secret}), ("nothing", {1: commit, 2: ""}),
                              ("other text, and a hash again after the paste", {1: commit, 2: secret, 3: commit})):
            with self.subTest(label):
                run = self.replay(copies=copies)
                self.assert_asked(run, withheld=(2, 4))
                self.assertEqual(1, len(PICTURE.findall(run.out)))  # of prompt 1
                self.assertEqual(([], 2, 2), (run.problems, run.done, len(run.not_covered)))
                for number, not_covered in zip((2, 4), run.not_covered):
                    self.assertEqual(1, len([line for line in run.report
                                             if re.match(rf"hands {number}: .*\bwithheld\b", line)]))
                    self.assertRegex(not_covered, rf"^hands {number}: .*\bpicture\b")
                self.assertRegex(run.result, r"^RESULT hands partial: hands 2: ")
                self.assert_never_written(run, secret)

    def test_without_wl_copy_or_wl_paste_the_copy_is_not_covered(self):
        earlier = self.commits[0][0]
        for absent in (("wl-copy",), ("wl-paste",), ("wl-copy", "wl-paste")):
            with self.subTest(absent=absent):
                run = self.replay(before=earlier, absent=absent)
                self.assertRegex(run.not_covered[0], r"^hands 1: .*\bnot installed\b")
                self.assertEqual([], run.problems)
                self.assertRegex(run.result, r"^RESULT hands partial: hands 1: ")
                if "wl-paste" in absent:
                    # Nothing can tell what the paste of prompt 2 shows, so it goes without its picture,
                    # and prompt 4, which shows the search field again, with it; neither stand-in
                    # was called, so the clipboard was neither read nor written.
                    self.assert_asked(run, withheld=(2, 4))
                    self.assertEqual((1, 3), (run.done, len(run.not_covered)))
                    for number, not_covered in zip((2, 4), run.not_covered[1:]):
                        self.assertRegex(not_covered, rf"^hands {number}: .*\bpicture\b")
                    self.assertEqual([], run.calls)
                else:
                    # The clipboard is read once, before the picture of prompt 2, and never written.
                    self.assert_asked(run)
                    self.assertEqual((3, 1), (run.done, len(run.not_covered)))
                    self.assertEqual(["wl-paste"], [call[0] for call in run.calls])
                self.assertEqual(earlier, run.clipboard_after)

    def test_no_enter_at_the_first_prompt_ends_the_asking_and_clears_the_marker(self):
        run = self.replay(leaves_at=1)
        self.assert_asked(run, numbers=(1,), entered=())
        self.assert_marked(run)
        self.assertEqual(4, len(run.not_covered))
        self.assertRegex(run.not_covered[0], r"^hands 1: no Enter\b")
        for number, not_covered in enumerate(run.not_covered[1:], start=2):
            self.assertRegex(not_covered, rf"^hands {number}: not asked\b")
        self.assertEqual((0, []), (run.done, run.problems))
        self.assertRegex(run.result, r"^RESULT hands skipped: ")
        self.assertEqual(("", 1), (run.clipboard_after, run.cleared()))

    def test_only_its_own_line_is_cleared_when_the_check_ends(self):
        commit = self.commits[1][0]
        with self.subTest("its own line is still there"):
            run = self.replay()
            self.assertEqual(("", 1), (run.clipboard_after, run.cleared()))
            self.assertEqual([], run.state.get("left to the exit", []))
        with self.subTest("the copied hash is there"):
            run = self.replay(copies={1: commit})
            self.assertEqual((commit, 0), (run.clipboard_after, run.cleared()))
        with self.subTest("the owner has copied something since"):
            run = self.replay(copies={3: "the owner's own copy"})
            self.assertRegex(self.the_problem(run), r"^hands 1: nothing was copied")
            self.assertEqual(("the owner's own copy", 0), (run.clipboard_after, run.cleared()))
        with self.subTest("a stop while a prompt waits"):
            run = self.replay(stops_at=2)
            self.assertEqual((143, ["TERM"]), (run.status, run.state["stopped by"]))
            self.assertEqual([1, 2], [prompt.number for prompt in run.prompts])
            self.assertEqual(("", 1), (run.clipboard_after, run.cleared()))
            self.assertEqual([], run.state.get("restore failed", []))

    def test_a_hash_proves_no_copy_where_the_marker_could_not_be_set(self):
        earlier, commit = self.commits[0][0], self.commits[1][0]
        for copies in ({}, {1: commit}):  # whether the person copies or not, the two cannot be told apart
            with self.subTest(copies=copies):
                run = self.replay(before=earlier, copies=copies, broken=("wl-copy",))
                self.assert_asked(run)
                self.assertEqual(earlier, run.clipboard_at[1])
                self.assertRegex(self.the_part_not_covered(run), r"^hands 1: .*\bbefore the prompt\b")
                self.assertEqual(3, run.done)
                self.assertEqual((copies.get(1, earlier), 0), (run.clipboard_after, run.cleared()))

    def test_its_own_line_is_offered_as_sensitive_and_once(self):
        # Clearing the clipboard takes nothing out of Omarchy's clipboard history,
        # so the line must never get there: every route offers it as sensitive.
        commit = self.commits[1][0]
        for label, arguments in (("a commit is copied", {"copies": {1: commit}}), ("nothing is copied", {}),
                                 ("the terminal is closed", {"leaves_at": 1}), ("the run is stopped", {"stops_at": 2})):
            with self.subTest(label):
                run = self.replay(**arguments)
                self.assertEqual([f"--sensitive -- {self.assert_marked(run)}"], run.offered())

    def test_a_wl_copy_that_refuses_sensitive_leaves_the_line_unset_and_the_copy_not_covered(self):
        earlier, commit = self.commits[0][0], self.commits[1][0]
        for copies in ({}, {1: commit}):
            with self.subTest(copies=copies):
                run = self.replay(before=earlier, copies=copies, sensitive=False)
                self.assert_asked(run)
                # Refused once and not tried in another way: the clipboard is as it was.
                self.assertEqual((1, earlier), (len(run.offered()), run.clipboard_at[1]))
                self.assertRegex(self.the_part_not_covered(run), r"^hands 1: .*\bbefore the prompt\b")
                self.assertEqual(3, run.done)
                self.assertEqual((copies.get(1, earlier), 0), (run.clipboard_after, run.cleared()))

    def test_other_text_is_no_problem_where_the_marker_could_not_be_set(self):
        # A clipboard that did not take the check's line and give it back is not
        # known to work, and what a tool failed to do is no finding about the app.
        secret = "plum-harbour-41"
        for label, arguments in (("wl-copy fails", {"before": secret, "broken": ("wl-copy",)}),
                                 ("wl-copy refuses --sensitive", {"before": secret, "sensitive": False}),
                                 ("copied at the prompt", {"copies": {1: secret}, "broken": ("wl-copy",)}),
                                 # The line did not read back, and the word copied at the prompt then does.
                                 ("wl-paste fails the first time", {"copies": {1: secret}, "first_read_fails": True}),
                                 ("wl-paste fails", {"copies": {1: secret}, "broken": ("wl-paste",)})):
            with self.subTest(label):
                run = self.replay(**arguments)
                self.assertEqual(secret, run.clipboard_after)  # the word was there for the check to read
                self.assert_asked(run, withheld=(2, 4))
                self.assertEqual(([], 3), (run.problems, len(run.not_covered)))
                self.assertRegex(run.not_covered[0], r"^hands 1: .*\bneither\b.*\bnot known to work\b")
                self.assertRegex(run.result, r"^RESULT hands partial: hands 1: ")
                self.assertEqual(1, run.done)
                self.assert_never_written(run, secret)

    def test_its_own_line_found_after_the_prompt_is_a_problem_even_where_it_did_not_read_back_before(self):
        run = self.replay(first_read_fails=True)
        self.assert_asked(run)
        self.assertNotIn(run.clipboard_at[1], run.introduction)  # the check did not say its line was in place
        self.assertRegex(self.the_problem(run), r"^hands 1: nothing was copied")
        self.assertEqual((3, [], "", 1), (run.done, run.not_covered, run.clipboard_after, run.cleared()))

    def test_a_copy_counts_only_with_its_picture(self):
        commit, subject = self.commits[1]
        run = self.replay(copies={1: commit}, no_picture=(1,))
        self.assert_asked(run)
        self.assertIn(f'"{subject}"', run.prompts[0].records[0])
        self.assertRegex(self.the_part_not_covered(run), r"^hands 1: .*\bpicture\b")
        self.assertEqual(3, run.done)

    def test_a_check_that_does_not_run_leaves_the_clipboard_alone(self):
        for label, arguments in (("--hands was not given", {"hands": "no"}),
                                 ("the answers do not come from a terminal", {"terminal": False})):
            with self.subTest(label):
                run = self.replay(before="what the owner had copied", **arguments)
                self.assertEqual(("", []), (run.err, run.prompts))
                self.assertRegex(run.result, r"^RESULT hands skipped: ")
                self.assertEqual(([], "what the owner had copied"), (run.calls, run.clipboard_after))

    def test_the_help_names_the_line_the_check_puts_on_the_clipboard(self):
        marker = self.assert_marked(self.replay())
        # `usage` alone, from the sourced script: its help is a quoted text that cannot take the line from the code.
        usage = subprocess.run([BASH, "-c", 'source "$1" && usage', "bash", str(VERIFY)], capture_output=True, text=True,
                               timeout=30, env={"PATH": os.environ.get("PATH", os.defpath), "HOME": str(self.directory)})
        self.assertEqual((0, ""), (usage.returncode, usage.stderr))
        self.assertIn(f'"{marker}"', usage.stdout)

    def test_the_replay_refuses_a_clipboard_that_is_not_its_own_stand_in(self):
        run = self.replay(before="what the owner had copied", impostor=True)
        self.assertEqual(97, run.status)
        self.assertIn("replay refused", run.err)
        self.assertEqual(([], [], {}), (run.report, run.calls, run.state))
        self.assertEqual("what the owner had copied", run.clipboard_after)


if __name__ == "__main__":
    unittest.main()
