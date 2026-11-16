#!/bin/bash
# Check a CI-built GitTurtle Linux archive on an Omarchy machine and pack what was
# found as one file to send back.
#
#   scripts/omarchy-verify.sh ARCHIVE [options]        (--help has the whole contract)
#
# Reading order: settings and state; the terminal, the report and privacy; the
# refusals that come before anything runs; Hyprland reads and requests; pictures;
# the restore registry; every desktop change with its restore directly under it;
# one function per check, in the order they run; the end of a run; main.
#
# The archive's installer and executable run only after its checksums match and its
# source revision is the expected one, and only with a disposable home under the
# results directory. Nothing here runs as root or asks the network for anything, and
# the only processes ended are ones this script started itself (the app, one at a
# time; one terminal; the sleep of a wait that a stop cut short), each of them on
# every exit. Omarchy's own theme switch does more than this script does, so the
# themes check runs only where that switch reaches no network, installs nothing
# and raises no dialog, as far as its steps and hooks are known; one that is not
# known stops the check unless the plan named it and --themes-anyway was given:
# see theme_blocker.

set -u

# ---------------------------------------------------------------- settings

readonly APP_ID=com.gitturtle.GitTurtle
readonly SETTLE=3           # seconds a window gets to draw after a change, before its picture
readonly WINDOW_WAIT=20     # seconds for the app's window to appear
readonly SWITCH_WAIT=180    # seconds for one omarchy-theme-set or omarchy-font-set
readonly REST_SECONDS=30    # the rest check's interval without input
readonly EXIT_WAIT_MS=2000  # the process must be gone this long after its window closes
readonly STOP_WAIT_MS=3000  # a process being ended gets this long after TERM, and again after KILL
readonly TERMINAL_LIFE=90   # seconds after which the inactive check's terminal ends by itself
readonly ANSWER_WAIT=600    # seconds a prompt waits for Enter or an answer
readonly MIN_BATTERY=30     # percent; below it, discharging, the desktop part does not start
readonly SCALES=(1 1.25 1.5 2)
readonly THEME_SECONDS=10    # one theme in the plan's estimate: the switch, its intro and the settle time
readonly REST_OF_DESKTOP=120 # seconds the plan's estimate gives everything else in the desktop part
readonly MAX_UNPACKED=$((1024 * 1024 * 1024)) # bytes an archive may unpack to (a release unpacks to about 0.2 GiB)
readonly MAX_ENTRIES=20000   # files and directories an archive may hold
readonly SIZE_DIGITS=12      # a size in an archive's listing, and their sum, is a plain number of at most this many digits
# The files `omarchy-font-set` (Omarchy 4.5) rewrites under ~/.config; it creates
# kitty/ and fontconfig/ when they are missing.
readonly FONT_FILES=(alacritty/alacritty.toml kitty/kitty.conf ghostty/config foot/foot.ini fontconfig/fonts.conf)
# The per-application steps of `omarchy-theme-set` (Omarchy 4.5.0), each read for
# what it does on a switch. Only two have an off switch: omarchy-theme-set-vscode
# (a skip-<editor>-theme-changes toggle for each of its four editors; without it
# the step runs `<editor> --install-extension`) and omarchy-theme-set-herdr-machines
# (the herdr-theme-sync toggle; with it on, the theme is pushed to other machines
# over SSH). omarchy-theme-set-browser has none: it writes the browsers' colour
# under /etc through Omarchy's passwordless sudo rule, or through a pkexec dialog
# where that rule is missing. The others rewrite and reload local configuration.
# A step that is not in this list has not been read, so the themes check does not
# run where omarchy-theme-set names one, unless --themes-anyway was given and the
# plan has named that step.
readonly THEME_STEPS_READ=(omarchy-restart-btop omarchy-restart-helix omarchy-restart-hyprctl omarchy-restart-opencode
  omarchy-restart-terminal omarchy-theme-set-browser omarchy-theme-set-claude omarchy-theme-set-fcitx5
  omarchy-theme-set-foot omarchy-theme-set-gnome omarchy-theme-set-herdr-machines omarchy-theme-set-hermes
  omarchy-theme-set-hunk omarchy-theme-set-keyboard omarchy-theme-set-obsidian omarchy-theme-set-pi
  omarchy-theme-set-t3code omarchy-theme-set-tmux omarchy-theme-set-vscode)
# The hooks Omarchy itself installs under ~/.config/omarchy/hooks: a hook that
# equals one of these byte for byte is Omarchy's (it tells the wallpaper daemon,
# over a local socket, to refresh). Any other hook is one this script cannot vouch
# for: the owner's own program, or a copy of Omarchy's that differs from the
# packaged file (the installed hook is a copy made when its package was installed,
# so an update of the package leaves the two different on a stock machine).
readonly OMARCHY_HOOKS=(/usr/share/owe/10-owe-sync)
readonly BROWSER_POLICY_COMMAND=/usr/bin/omarchy-theme-set-browser-policy # the path Omarchy's sudo rule names
# The checks --skip can leave out; every later check stands on the others.
readonly SKIPPABLE=(package themes font floating scales inactive rest hands)
# Two reasons for skipping the themes check that are recognised again by their
# words: the one --themes-anyway sets aside, and the one sudo's answer gives.
readonly UNREAD="a switch here would also run what this script was not read against"
readonly NO_SUDO_RULE="Omarchy's passwordless sudo rule for the browser colour is missing, so every switch would raise a password dialog"
# A host or user called one of these is not removed from the records: the report
# needs the word itself, and it identifies no one.
readonly GENERIC_WORDS=" omarchy arch archlinux linux hyprland gitturtle localhost home user "

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly SCRIPT_DIR
readonly REPO_ROOT=${SCRIPT_DIR%/*}
: "${OMARCHY_PATH:=/usr/share/omarchy}"
export OMARCHY_PATH
readonly OMARCHY_CURRENT=$HOME/.local/state/omarchy/current
readonly STAY_AWAKE=$HOME/.local/state/omarchy/indicators/stay-awake
readonly TOGGLES=$HOME/.local/state/omarchy/toggles
readonly HOOKS=$HOME/.config/omarchy/hooks

# Parsing below reads English messages and dot decimals. What this script starts
# for the desktop (the app, the terminal, Omarchy's commands) gets the session's
# own locale back: SESSION_ENV goes in front of NAME=value pairs and the command.
if [[ -n ${LC_ALL+set} ]]; then SESSION_ENV=(env "LC_ALL=$LC_ALL"); else SESSION_ENV=(env -u LC_ALL); fi
export LC_ALL=C

# ---------------------------------------------------------------- state

ARCHIVE_ARG="" OUT_ARG="" EXPECT_REVISION="" FONT_CHOICE=""
DESKTOP=yes ASSUME_YES=no HANDS=no KEEP_WORK=no THEMES_ANYWAY=no SKIPS=()
ARCHIVE="" ARCHIVE_NAME="" ARCHIVE_SHA256="" ARCHIVE_TOP="" ARCHIVE_TRUSTED=no
OUT="" OUT_CREATED="" WORK="" RECORDS="" PICTURES="" REPORT="" RESTORE=""
BUNDLE="" BUNDLE_BINARY_SHA256="" BUNDLE_REVISION="" BUILD_REVISION="" IDENTITY_REFUSAL=""
DISPOSABLE_HOME="" INSTALLED_BINARY=""
DISPOSABLE_ENV=()
HOME_REAL="" USER_NAME="" HOST_NAME="" REDACT=() SCAN_PATTERN=""
CHECK="" PROBLEMS=() PARTIALS=() COUNT_OK=0 COUNT_PARTIAL=0 COUNT_FAILED=0 COUNT_SKIPPED=0
START_EPOCH=0 STOPPED_BY="" OWNER_INSTALL_START=""
DESKTOP_REFUSAL="" DESKTOP_START="" RESTORES=() RESTORE_FAILURES=() KEEP_RESTORE=no
START_FOCUS="" START_DND="" START_AWAKE=""
START_WORKSPACE="" LEAVES_WORKSPACE="" ALONE=no HANDS_DONE=0 HANDS_ABSENT=no
LAUNCH="" DEMO="" APP_PID="" APP_ADDRESS=""
START_THEME="" START_WALLPAPER=""
THEME_BLOCKER="" THEME_UNKNOWNS=() SUDO_ASKED=no
SKIP_TOGGLES=() SKIP_TOGGLES_MADE=() TOGGLES_DIRECTORY_MADE=no
START_FONT="" FONT_PROBLEM="" FONT_SAVED=() FONT_DIRECTORY=() FONT_REAL=() FONT_LINK=()
START_FLOATING=""
MONITOR_NAME="" MONITOR_MODE="" MONITOR_POSITION="" MONITOR_START_SCALE="" MONITOR_START_LINE=""
TERMINAL_PID="" TERMINAL_ADDRESS=""

# ---------------------------------------------------------------- help

usage() {
  cat <<'TEXT'
Usage: scripts/omarchy-verify.sh ARCHIVE [options]

Checks a CI-built GitTurtle Linux archive on this Omarchy machine, takes a picture of
the app's own window at each step, puts back what it changed and packs the results as
one file to send back. ARCHIVE is the downloaded .tar.gz; its .sha256 must lie beside
it, and its .manifest.json and ci-provenance.json are read when they do.

  --out DIR              results directory, which must not exist
                         (default: ./gitturtle-omarchy-check-<UTC date and time>)
  --expect-revision SHA  the commit the archive must be built from; with another
                         one, nothing from the archive runs
  --no-desktop          run only the checks that change nothing (host to package)
  --skip LIST            leave out the checks LIST names, separated by commas, from:
                         package, themes, font, floating, scales, inactive, rest,
                         hands. Each reports "skipped: by --skip" and changes
                         nothing; any other name stops the run before anything
                         runs (exit 2). The plan shows what will run
  --themes-anyway        run the themes check although a theme switch here also
                         runs a hook or a step this script was not read against,
                         each of which the plan names; it sets nothing else aside
                         (see below)
  --yes                  do not wait for Enter before starting
  --hands                end with four prompts for you: Omarchy's copy and paste
                         bindings and the folder picker (about two minutes)
  --font NAME            the font the font check sets (default: the first other
                         family that omarchy-font-list names)
  --keep-work            keep DIR/work (the extraction, the disposable home, the demo
                         repository); it holds full paths and is never packed
  -h, --help             this text

Checks, in this order. report.txt gets one section each, appended as it finishes and
ending in "RESULT <check> ok", "RESULT <check> partial: <what was not covered and
why>" (the check ran, but could not cover a part named below), "RESULT <check>
FAILED: <why>" or "RESULT <check> skipped: <why>"; one failure does not stop the
next check. The exit status is 0 when no check FAILED and the results were packed,
1 when a check FAILED or the results could not be packed, and 2 when the script
refused to start or was given an option or a --skip name it does not know.
  host      architecture, kernel, page size, Omarchy and Hyprland versions, each
            monitor's name, mode and scale, the GPU, the current theme and font
  archive   the .sha256, the extraction, SHA256SUMS inside, the source revision
            (see Identity), bin/gitturtle --build-info, ldd, the manifest's fields
  install   the archive's install.py into a disposable home, what it installed,
            desktop-file-validate, the same executable as the archive's
  package   packaging/aur/test-local.sh builds gitturtle-bin from the archive
  launch    the installed executable on a demo repository: its window, no panic
  themes    every theme omarchy-theme-list names, then the starting theme again
  font      one other font with omarchy-font-set, then the starting font again
  floating  the window as it opened, then floating
  scales    the window's monitor at 1, 1.25, 1.5 and 2, where its mode allows
  inactive  a terminal takes the focus while GitTurtle is pictured inactive
  rest      CPU time over 30 s and threads; closed through Hyprland; gone within
            2 s; no core dump and no crash line in the user journal
  hands     with --hands: the copy and paste bindings, the folder picker, Cancel
            (prompt 3 takes no picture: the folder picker lists your folders)

The desktop part (launch to hands) moves to a workspace that holds no window, opens
GitTurtle and one terminal window there, and changes, with Omarchy's and Hyprland's
own commands:
  - the workspace shown (an empty one of this monitor's; --hands then runs beside
    your terminal, on the workspace you started from);
  - the theme (omarchy-theme-set: the wallpaper, and what Omarchy retints on a
    switch: the terminals and the programs in them, Hyprland's configuration,
    GTK's colour scheme and icons, the themes Omarchy keeps for the coding agents
    it supports, T3 Code, Obsidian and fcitx5, the keyboard backlight where
    Omarchy drives one, and the browsers' colour, a file under /etc that Omarchy
    writes through its own passwordless sudo rule before it tells a running
    browser to read it again);
  - Omarchy's skip-<editor>-theme-changes toggles (on for the run, so that a
    switch leaves VS Code, VSCodium and Cursor alone and installs no extension);
  - the monospace font (omarchy-font-set: the terminal and fontconfig files it
    rewrites, and its restart of the Omarchy shell);
  - the scale of the monitor that holds the window; the window's floating state;
    the focused window;
  - notification silencing (on, so that none lands in a picture) and stay-awake
    (on, so that neither the screensaver nor the lock starts during the run).
Each is recorded before it is changed and put back when its check ends and on every
exit, Ctrl+C included, and output that is no longer read (the run piped into a
program that has ended); a stop takes effect when the step in progress returns. The
script prints its plan and waits for Enter; the plan is decided once, and a part it
says will not run is not started later in the run. It refuses the desktop part
when Hyprland is not the compositor, when the session is locked or its lock state
cannot be told, on battery below 30 %, when the desktop is on a named workspace
or shows a special one (the run starts from a numbered workspace and returns to
it), or when this monitor has no workspace without a window. The desktop part
also ends at its start, before GitTurtle opens, when Omarchy's shell does not
confirm that notifications are silenced once the script has asked for that: no
picture could be taken, so silencing and the workspace are put back as they
were, nothing else on the desktop is changed, and every check of the desktop
part reports skipped with that reason. With the desktop
part asked for, a results directory that lies under your home or names you or
the host stops the run at the start (exit 2): GitTurtle's header shows the demo
repository's path, so run it from /var/tmp.
The desktop part takes about two minutes for everything but the themes, and about
ten seconds more for each theme: about six minutes with the 24 themes of a stock
Omarchy 4.5. Both figures are estimates; the run times every switch and writes
it in the report. The plan gives the sum for this machine; --hands adds your own
time.
A kill the script cannot catch (kill -9) or a power loss leaves the desktop as the
step in progress had it. Before the first change the script prints where the
starting state is written (records/desktop-start.txt, and work/restore under the
results directory, with the saved copies and their manifest.txt) and how to put
back by hand what this run changes: a step only for a change the plan lets the
run make, each to be taken only if the desktop differs from the starting value
the step gives, and in the order the script itself uses. In general: the
workspace (SUPER + its number); the theme (omarchy-theme-set, only when the
current theme is another one, because setting the current theme again moves on
to its next wallpaper); the wallpaper (omarchy-theme-bg-set with the path that
manifest.txt keeps); the skip toggles this run made and the toggles directory
if it made that too (remove them); the font
(omarchy-font-set) and only then the font files, which omarchy-font-set rewrites
(copy each saved font-N over the file manifest.txt names), the font only when
the current font is another one, because omarchy-font-set rewrites those files
and replaces a link at three of them whatever the font was; the scale (hyprctl
reload); and, where the run found them off, notification silencing
(omarchy-shell notifications setDnd off) and stay-awake (omarchy-toggle-idle
allow-idle).
The themes check is skipped, with the reason, where a switch would do more than the
above. Two reasons stand whatever is asked: Omarchy's theme sync being on (it
pushes each theme to your herdr machines over SSH), and Omarchy's sudo rule for
the browser colour missing (each switch would raise a password dialog). Two more
are named in the plan, one by one, and --themes-anyway runs the check despite
them: a theme-set hook that is not byte for byte the one Omarchy packages (the
plan gives its file name and says whether it differs from the packaged
/usr/share/owe/10-owe-sync, as the installed copy does after that package was
updated, or is not Omarchy's at all), and a step of omarchy-theme-set that is
not among the 19 read for Omarchy 4.5.0 (a newer Omarchy may add one). The
script cannot know what such a hook or step does: read it first. --themes-anyway
sets aside those two reasons only, and only for what the plan named: never theme
sync, never the sudo rule, and not a hook or step that appears after the plan.
The font check is skipped where a font-set hook would run.

Identity: the checksum beside an archive shows that the download is whole, not who
built it. Take the archive from the green Quality run on main for the commit being
released, never from a pull request's run, and give that commit to
--expect-revision. The script reads the source revision from build-info.json in
the archive before anything from the archive runs; if --expect-revision, the
manifest or ci-provenance.json names another one, the installer, the executable,
the package build and the desktop part are all skipped: identity not established.
Privacy: report.txt and the records name no host, no user and no home directory
(written "~"), and they are packed only after every record was read as text and
none of them does; the app shows only the demo repository made for the run. A
picture is a grab of GitTurtle's own window rectangle, never of the screen, and
is refused (its check FAILED, no file written) while another window or another
monitor overlaps that rectangle, or while Omarchy's shell does not confirm that
notifications are silenced. Where Omarchy's rules make the window translucent,
the wallpaper shows through it faintly; the report gives the opacity Hyprland
reports. The script makes no network request, and the themes check runs only
where what the script has read of Omarchy's theme switch makes none either; a
hook or step that --themes-anyway sets aside is one it has not read.
Safety: it never runs as root. It asks sudo one question, the one Omarchy's own
command asks ("sudo -n -l": it elevates nothing, runs no command and prompts for
nothing), to learn whether Omarchy's passwordless rule for the browser colour
exists; the plan says when it was asked. Omarchy's theme switch then uses that
rule, and nothing else here uses sudo. Besides the changes above, it writes only
under the results directory; your installed GitTurtle and its settings are never
touched (the installer gets a disposable home, the app a new XDG_CONFIG_HOME);
it ends only processes it started, ends the app it started before it starts
another (--hands opens a second one), and ends all of them on every exit; every
wait has a limit; a symlinked or missing archive, a missing .sha256 or a
checksum that does not match stops it before anything from the archive runs.
What it saved before a change it puts back by a copy and a rename, so no file of
yours is ever missing in between; if anything is not as it was found, the saved
copies stay under the results directory and the last lines say where.

It ends by printing "send this file back:" and the packed results, DIR.tar.gz.
TEXT
}

# ---------------------------------------------------------------- terminal, report, privacy

say() { printf '%s\n' "$*" 2>/dev/null || true; }
die() {
  printf 'omarchy-verify: %s\n' "$*" >&2
  exit 2
}
have() { command -v "$1" >/dev/null 2>&1; }
# An interruptible wait: a stop is noticed at once instead of after the sleep.
nap() {
  sleep "$1" &
  wait $!
}
now_ms() { echo $(($(date +%s%N) / 1000000)); }
sha256_of() {
  local sum
  sum=$(sha256sum <"$1") || return 1
  printf '%s' "${sum%% *}"
}

ere_quote() { printf '%s' "$1" | sed 's/[][\.*^$+?(){}|]/\\&/g'; }
# A name worth removing wherever it stands as a word of its own.
private_word() { [[ ${#1} -ge 3 && $GENERIC_WORDS != *" ${1,,} "* ]]; }

# Builds what `redact` replaces and what the last privacy scan looks for: the
# results directory, the home directory, and the user's and the host's names.
build_redaction() {
  local d=$'\001' name quoted label=a
  local -a words=()
  HOME_REAL=$(realpath -e -- "$HOME" 2>/dev/null || printf '%s' "$HOME")
  USER_NAME=$(id -un)
  HOST_NAME=$(uname -n)
  REDACT=(-E -e "s$d$(ere_quote "$OUT")$d<results>${d}g")
  SCAN_PATTERN=$(ere_quote "$OUT")
  for name in "$HOME" "$HOME_REAL"; do
    [[ $name == /?* ]] || continue
    REDACT+=(-e "s$d$(ere_quote "$name")$d~${d}g")
    SCAN_PATTERN+="|$(ere_quote "$name")"
  done
  private_word "$USER_NAME" && words+=("$USER_NAME=<user>")
  private_word "$HOST_NAME" && words+=("$HOST_NAME=<host>")
  [[ ${HOST_NAME%%.*} != "$HOST_NAME" ]] && private_word "${HOST_NAME%%.*}" && words+=("${HOST_NAME%%.*}=<host>")
  for name in "${words[@]}"; do
    quoted="(^|[^[:alnum:]_])$(ere_quote "${name%%=*}")([^[:alnum:]_]|\$)"
    REDACT+=(-e ":$label" -e "s$d$quoted$d\\1${name#*=}\\2$d" -e "t$label")
    SCAN_PATTERN+="|$quoted"
    label+=a
  done
}
redact() { sed "${REDACT[@]}"; }

# One line into report.txt and onto the terminal, at once, private names removed.
report() {
  local line
  line=$(printf '%s\n' "$*" | redact)
  printf '%s\n' "$line" >>"$REPORT"
  say "$line"
}
# Standard input into records/NAME, private names removed.
record() { redact >"$RECORDS/$1"; }
record_more() { redact >>"$RECORDS/$1"; }

section() { # LETTER NAME TITLE
  CHECK=$2
  PROBLEMS=() PARTIALS=()
  report ""
  report "== $1. $2: $3"
}
problem() {
  PROBLEMS+=("$*")
  report "PROBLEM $*"
}
# A named part of the check that could not be covered, and why: the check ran, but
# its RESULT is "partial" rather than "ok", so it never says more than it did.
uncovered() {
  PARTIALS+=("$*")
  report "NOT COVERED $*"
}
# The check's last line: FAILED with its first problem, partial with the first
# part it could not cover, or ok.
result() {
  local more=""
  if ((${#PROBLEMS[@]} > 0)); then
    ((${#PROBLEMS[@]} > 1)) && more=" (and $((${#PROBLEMS[@]} - 1)) more above)"
    report "RESULT $CHECK FAILED: ${PROBLEMS[0]}$more"
    COUNT_FAILED=$((COUNT_FAILED + 1))
  elif ((${#PARTIALS[@]} > 0)); then
    ((${#PARTIALS[@]} > 1)) && more=" (and $((${#PARTIALS[@]} - 1)) more above)"
    report "RESULT $CHECK partial: ${PARTIALS[0]}$more"
    COUNT_PARTIAL=$((COUNT_PARTIAL + 1))
  else
    report "RESULT $CHECK ok"
    COUNT_OK=$((COUNT_OK + 1))
  fi
}
skipped() {
  report "RESULT $CHECK skipped: $*"
  COUNT_SKIPPED=$((COUNT_SKIPPED + 1))
}

# ---------------------------------------------------------------- before anything runs

parse_arguments() {
  while (($# > 0)); do
    case $1 in
      -h | --help)
        usage
        exit 0
        ;;
      --out)
        [[ -n ${2:-} ]] || die "--out needs a directory."
        OUT_ARG=$2
        shift
        ;;
      --expect-revision)
        [[ ${2:-} =~ ^[0-9a-f]{7,40}$ ]] || die "--expect-revision needs 7 to 40 lowercase hex digits."
        EXPECT_REVISION=$2
        shift
        ;;
      --font)
        [[ ${2:-} =~ ^[A-Za-z0-9][A-Za-z0-9\ ._+-]{0,79}$ ]] ||
          die "--font needs a font family as omarchy-font-list prints it (letters, digits, spaces and . _ + -)."
        FONT_CHOICE=$2
        shift
        ;;
      --skip)
        [[ -n ${2:-} ]] || die "--skip needs a list of checks, comma-separated, from: ${SKIPPABLE[*]}."
        add_skips "$2"
        shift
        ;;
      --themes-anyway) THEMES_ANYWAY=yes ;;
      --no-desktop) DESKTOP=no ;;
      --yes) ASSUME_YES=yes ;;
      --hands) HANDS=yes ;;
      --keep-work) KEEP_WORK=yes ;;
      -*) die "unknown option $1 (see --help)." ;;
      *)
        [[ -z $ARCHIVE_ARG ]] || die "one archive at a time: $ARCHIVE_ARG and $1."
        ARCHIVE_ARG=$1
        ;;
    esac
    shift
  done
  [[ -n $ARCHIVE_ARG ]] || die "name the archive to check (see --help)."
}
# --skip LIST: the names of checks to leave out, separated by commas. A name that
# is not one of SKIPPABLE stops the run here, before anything else happens.
add_skips() {
  local name
  local -a names=()
  IFS=, read -ra names <<<"$1"
  for name in "${names[@]}"; do
    [[ $name =~ ^[a-z]+$ && " ${SKIPPABLE[*]} " == *" $name "* ]] ||
      die "--skip: \"$name\" is not a check that can be skipped. It takes, comma-separated: ${SKIPPABLE[*]}."
    skip_named "$name" || SKIPS+=("$name")
  done
}
skip_named() { [[ " ${SKIPS[*]} " == *" $1 "* ]]; }
# The RESULT of a check that --skip named; false then. Every check that can be
# skipped asks this first, so a skipped check reads nothing and changes nothing.
skip_gate() {
  skip_named "$CHECK" || return 0
  skipped "by --skip"
  return 1
}

refuse_missing_tools() {
  local tool missing=()
  for tool in sha256sum tar sed awk grep find stat date realpath timeout cmp comm diff sort jq id uname; do
    have "$tool" || missing+=("$tool")
  done
  ((${#missing[@]} == 0)) || die "missing ${missing[*]}: a stock Omarchy has them all."
}

# Stops before anything from the archive runs: the archive and its .sha256 are
# plain files, not links, and the archive's SHA-256 is the one its .sha256 gives.
refuse_unverified_archive() {
  local sums expected listed
  [[ ! -L $ARCHIVE_ARG ]] || die "the archive is a symbolic link: $ARCHIVE_ARG. Give the file itself."
  [[ -f $ARCHIVE_ARG ]] || die "no such archive file: $ARCHIVE_ARG"
  ARCHIVE=$(realpath -e -- "$ARCHIVE_ARG") || die "cannot resolve $ARCHIVE_ARG"
  ARCHIVE_NAME=${ARCHIVE##*/}
  [[ $ARCHIVE_NAME =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*\.tar\.gz$ ]] || die "unexpected archive name: $ARCHIVE_NAME"
  sums=$ARCHIVE.sha256
  [[ ! -L $sums ]] || die "$ARCHIVE_NAME.sha256 is a symbolic link. Download the file itself."
  [[ -f $sums ]] || die "missing $ARCHIVE_NAME.sha256 beside the archive: download it with the archive."
  expected="" listed=""
  read -r expected listed <"$sums" || true
  [[ $expected =~ ^[0-9a-f]{64}$ ]] || die "$ARCHIVE_NAME.sha256 does not start with a SHA-256."
  [[ ${listed#\*} == "$ARCHIVE_NAME" ]] || die "$ARCHIVE_NAME.sha256 names ${listed#\*}, not this archive."
  ARCHIVE_SHA256=$(sha256_of "$ARCHIVE") || die "cannot read $ARCHIVE_ARG"
  [[ $ARCHIVE_SHA256 == "$expected" ]] ||
    die "checksum mismatch: the archive's SHA-256 is $ARCHIVE_SHA256, its .sha256 says $expected. Nothing from it was run."
}

choose_results_directory() {
  local parent name
  [[ -n $OUT_ARG ]] || OUT_ARG=./gitturtle-omarchy-check-$(date -u +%Y%m%dT%H%M%SZ)
  [[ ! -e $OUT_ARG && ! -L $OUT_ARG ]] || die "the results directory already exists: $OUT_ARG. Choose a new one."
  name=$(basename -- "$OUT_ARG")
  [[ $name =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*$ ]] || die "choose a plain name for the results directory, not $name."
  parent=$(realpath -e -- "$(dirname -- "$OUT_ARG")" 2>/dev/null) || die "no such directory to put the results in: $(dirname -- "$OUT_ARG")"
  OUT=${parent%/}/$name
  [[ ! -e $OUT.tar.gz && ! -L $OUT.tar.gz ]] || die "$OUT.tar.gz already exists. Choose another --out."
  WORK=$OUT/work RECORDS=$OUT/records PICTURES=$OUT/pictures REPORT=$OUT/report.txt
  DISPOSABLE_HOME=$WORK/home LAUNCH=$WORK/launch RESTORE=$WORK/restore
  # Whatever runs from the archive without a window sees only this home.
  DISPOSABLE_ENV=("HOME=$DISPOSABLE_HOME" "XDG_DATA_HOME=$DISPOSABLE_HOME/.local/share"
    "XDG_CONFIG_HOME=$DISPOSABLE_HOME/.config" "XDG_CACHE_HOME=$DISPOSABLE_HOME/.cache"
    "XDG_STATE_HOME=$DISPOSABLE_HOME/.local/state" "TMPDIR=$WORK/tmp" PYTHONDONTWRITEBYTECODE=1)
}

# GitTurtle's header shows the repository's path, and the demo repository lies
# under the results directory: with pictures, that path must name no one.
path_names_someone() {
  local part
  local -a parts
  [[ $1 == "$HOME"/* || $1 == "$HOME_REAL"/* ]] && return 0
  IFS=/ read -ra parts <<<"$1"
  for part in "${parts[@]}"; do
    [[ -n $part && ($part == "$USER_NAME" || $part == "$HOST_NAME") ]] && return 0
  done
  return 1
}

# Omarchy's own test exits 0 locked, 1 unlocked, 2 undetermined. Only "unlocked"
# counts as unlocked here: a state that cannot be determined, a missing command
# or a test that does not answer in time all count as locked.
session_locked() {
  local status
  have omarchy-hyprland-session-locked || return 0
  timeout 5 omarchy-hyprland-session-locked
  status=$?
  ((status != 1))
}

# Prints why, when a system battery is discharging below MIN_BATTERY percent.
battery_low() {
  local supply status capacity
  for supply in /sys/class/power_supply/*; do
    [[ $(cat "$supply/type" 2>/dev/null) == Battery ]] || continue
    [[ $(cat "$supply/scope" 2>/dev/null) != Device ]] || continue # a mouse's or keyboard's battery
    status=$(cat "$supply/status" 2>/dev/null)
    capacity=$(cat "$supply/capacity" 2>/dev/null)
    if [[ $status == Discharging && $capacity =~ ^[0-9]+$ ]] && ((capacity < MIN_BATTERY)); then
      echo "on battery at $capacity %, below $MIN_BATTERY %"
      return
    fi
  done
}

# Why the desktop part cannot run here and now; nothing when it can.
desktop_refusal() {
  local tool low
  if [[ $DESKTOP == no ]]; then
    echo "--no-desktop"
    return
  fi
  for tool in hyprctl grim; do
    if ! have "$tool"; then
      echo "$tool is not installed"
      return
    fi
  done
  if [[ -z ${HYPRLAND_INSTANCE_SIGNATURE:-} ]] || ! hypr_json monitors | jq -e 'length > 0' >/dev/null 2>&1; then
    echo "Hyprland is not the compositor of this session"
    return
  fi
  if session_locked; then
    echo "the session is locked, or Omarchy cannot tell whether it is"
    return
  fi
  low=$(battery_low)
  if [[ -n $low ]]; then
    echo "$low"
    return
  fi
  case $(starting_workspace) in
    special)
      echo "a special workspace (a scratchpad) is shown on this monitor, and the run starts from a numbered workspace and returns to it: hide the special one first"
      return
      ;;
    named)
      echo "the desktop is on a named workspace, and the run starts from one of this monitor's ten numbered workspaces and returns to it: go to a numbered one first"
      return
      ;;
  esac
  if [[ -z $(empty_workspace) ]]; then
    echo "this monitor has no workspace without a window for the run to use: empty one of its ten"
  fi
}

# --- what Omarchy's theme switch does besides the retints, and whether it may run
# The per-application steps omarchy-theme-set names: its two lists, what it adds
# to them, and the mirror it starts last.
theme_steps() {
  local source
  source=$(command -v omarchy-theme-set) && [[ -f $source && -r $source ]] || return 1
  {
    sed -n '/^[a-z_]*_theme_commands=(/,/^)/p' "$source"
    grep -E '_theme_commands\+=\(' "$source"
    grep -E '^[[:space:]]*setsid omarchy-' "$source"
  } | grep -oE 'omarchy-[a-z0-9-]+' | sort -u
}
# The skip toggles those steps ask Omarchy for, one name a line.
theme_skip_toggles() {
  local step source
  while IFS= read -r step; do
    source=$(command -v "$step") && [[ -f $source && -r $source ]] || continue
    grep -oE 'omarchy-toggle-enabled skip-[a-z0-9-]+-theme-changes' "$source"
  done < <(theme_steps) | sed 's/^omarchy-toggle-enabled //' | sort -u
}
# The hooks Omarchy would run for EVENT (theme-set or font-set) that are not byte
# for byte one it packages, one a line: the hook's file name under
# ~/.config/omarchy/hooks, and whether it is a copy of a packaged hook that
# differs from it or no hook of Omarchy's at all. omarchy-hook runs the file of
# that name and every file in its .d directory that is not a .sample.
own_hooks() {
  local hook known name packaged own
  for hook in "$HOOKS/$1" "$HOOKS/$1.d"/*; do
    [[ -f $hook && $hook != *.sample ]] || continue
    own=yes packaged=""
    for known in "${OMARCHY_HOOKS[@]}"; do
      [[ ! -f $known ]] || ! cmp -s -- "$known" "$hook" || own=no
      [[ ${known##*/} != "${hook##*/}" ]] || packaged=$known
    done
    [[ $own == yes ]] || continue
    # A file name is the owner's to choose: only plain characters are repeated.
    name=$(printf '%s' "${hook#"$HOOKS"/}" | tr -c 'A-Za-z0-9._+/-' '?')
    if [[ -z $packaged ]]; then
      echo "the hook $name, which is not one that Omarchy packages"
    elif [[ -f $packaged ]]; then
      echo "the hook $name, which differs from the packaged $packaged"
    else
      echo "the hook $name, which has the name of Omarchy's $packaged, a file that is not installed to compare it with"
    fi
  done
}
# What a theme switch here would run that this script cannot vouch for, one a
# line, each by its file name: a step omarchy-theme-set names that is not among
# those read, and a theme-set hook that is not Omarchy's packaged one. These, and
# only these, are what --themes-anyway sets aside once the plan has named them.
theme_unknowns() {
  local step
  for step in $(theme_steps); do
    [[ " ${THEME_STEPS_READ[*]} " == *" $step "* ]] ||
      echo "the step $step, which is not among the ${#THEME_STEPS_READ[@]} read for Omarchy 4.5.0"
  done
  own_hooks theme-set
}
# True when sudo would run Omarchy's browser-colour command without a password:
# the question omarchy-theme-set-browser-policy asks before it chooses between
# sudo and a pkexec dialog. Listing runs nothing and, under -n, prompts for nothing.
browser_rule_present() {
  have sudo && timeout 10 sudo -n -l -l "$BROWSER_POLICY_COMMAND" 000000 2>/dev/null | grep -q '!authenticate'
}
# Why an omarchy-theme-set here would reach the network, install something or
# raise a dialog, or could not be judged; nothing when it does none of these.
# The sudo question comes last, after everything that can be read from files.
# --themes-anyway sets aside one reason only: a hook or a step that is not known,
# and only one the plan named (THEME_UNKNOWNS). Never theme sync, never the sudo
# rule, never a step that could not be read or an editor without its toggle.
theme_blocker() {
  local steps editor guarded unknown
  local -a unknowns=() late=()
  steps=$(theme_steps)
  if [[ -z $steps ]]; then
    echo "omarchy-theme-set's per-application steps could not be read from its source"
    return
  fi
  editor=$(command -v omarchy-theme-set-vscode) || editor=""
  if [[ -n $editor ]]; then
    guarded=$(grep -cE '^if ! omarchy-toggle-enabled skip-[a-z0-9-]+-theme-changes; then$' "$editor")
    if ((guarded == 0)) || [[ $(grep -cE '^[[:space:]]*set_theme "' "$editor") != "$guarded" ]]; then
      echo "omarchy-theme-set-vscode has an editor step without a skip toggle, and it installs the theme's extension"
      return
    fi
  fi
  if [[ -f $TOGGLES/herdr-theme-sync ]]; then
    echo "Omarchy's theme sync is on, and every switch would push the theme to your herdr machines over SSH (omarchy-toggle-theme-sync turns it off)"
    return
  fi
  mapfile -t unknowns < <(theme_unknowns)
  if ((${#unknowns[@]} > 0)); then
    if [[ $THEMES_ANYWAY == no ]]; then
      echo "$UNREAD: $(printf '%s; ' "${unknowns[@]}" | sed 's/; $//'). Read each one: --themes-anyway then runs the themes check despite these, and despite nothing else"
      return
    fi
    for unknown in "${unknowns[@]}"; do
      [[ $'\n'"$(printf '%s\n' "${THEME_UNKNOWNS[@]}")"$'\n' == *$'\n'"$unknown"$'\n'* ]] || late+=("$unknown")
    done
    if ((${#late[@]} > 0)); then
      echo "$UNREAD, and --themes-anyway covers only what the plan named, not what appeared after it: $(printf '%s; ' "${late[@]}" | sed 's/; $//')"
      return
    fi
  fi
  if ! browser_rule_present; then
    echo "$NO_SUDO_RULE"
  fi
}
# theme_blocker asks sudo its one question only when nothing it read from files
# stands in the way, so the question was asked exactly when the answer REASON is
# none or the missing rule.
sudo_question_asked() { have sudo && [[ -z $1 || $1 == "$NO_SUDO_RULE" ]]; }

# Which checks the plan lets run and which it does not, each with its reason. A
# check that runs can still end "skipped" for a reason it finds itself.
plan_checks() {
  local name reason
  local -a run=(host archive install) reasons=() not=()
  local -A names=()
  for name in package launch themes font floating scales inactive rest hands; do
    reason=""
    if skip_named "$name"; then
      reason="by --skip"
    elif [[ $name != package && -n $DESKTOP_REFUSAL ]]; then
      reason="no desktop part"
    elif [[ $name == themes && -n $THEME_BLOCKER ]]; then
      reason="see below"
    elif [[ $name == hands && $HANDS == no ]]; then
      reason="--hands was not given"
    fi
    if [[ -z $reason ]]; then
      run+=("$name")
    else
      [[ -n ${names[$reason]:-} ]] || reasons+=("$reason")
      names[$reason]+=${names[$reason]:+, }$name
    fi
  done
  say "Checks that run: ${run[*]}."
  for reason in "${reasons[@]}"; do not+=("${names[$reason]} ($reason)"); done
  ((${#not[@]} == 0)) || say "Not run: $(printf '%s; ' "${not[@]}" | sed 's/; $//')."
}
# The plan's word on the one question sudo was asked to decide it, when it was.
say_sudo_asked() {
  [[ $SUDO_ASKED == yes ]] || return 0
  say "    To learn whether that sudo rule exists, sudo was asked one question before this"
  say "    plan was printed (sudo -n -l, the question Omarchy's own command asks): it"
  say "    elevates nothing and runs no command."
}
# The plan's naming of each hook or step that is not known: HEADING, then one a line.
name_unknowns() {
  local unknown
  ((${#THEME_UNKNOWNS[@]} > 0)) || return 0
  say "    $1"
  for unknown in "${THEME_UNKNOWNS[@]}"; do say "      $unknown"; done
}

show_plan_and_wait() {
  local themes="?" answer="" seconds=$REST_OF_DESKTOP
  say "GitTurtle on Omarchy: check of $ARCHIVE_NAME (its .sha256 matches)."
  say "Results: $OUT, packed at the end as ${OUT##*/}.tar.gz beside it."
  plan_checks
  say ""
  say "First, nothing on the desktop changes: this host, the archive, its installer into a"
  if skip_named package; then
    say "disposable home under the results directory (the package build is skipped)."
  else
    say "disposable home under the results directory, and the gitturtle-bin package build."
  fi
  if [[ -n $DESKTOP_REFUSAL ]]; then
    say "The desktop part will NOT run: $DESKTOP_REFUSAL."
  else
    have omarchy-theme-list && themes=$(timeout 10 omarchy-theme-list 2>/dev/null | grep -c .)
    if skip_named themes || [[ -n $THEME_BLOCKER || ! $themes =~ ^[0-9]+$ ]]; then
      say "Then the desktop part, about $((REST_OF_DESKTOP / 60)) minutes (an estimate); leave the keyboard and"
      say "pointer alone:"
    else
      seconds=$((seconds + themes * THEME_SECONDS))
      say "Then the desktop part, about $(((seconds + 59) / 60)) minutes: about $((REST_OF_DESKTOP / 60)) for everything but the themes,"
      say "and about $THEME_SECONDS s for each of the $themes themes. Both are estimates: the run times"
      say "every switch and writes it in the report. Leave the keyboard and pointer alone:"
    fi
    say "  - the desktop moves to an empty workspace of this monitor, where GitTurtle"
    say "    opens on a demo repository; notifications are silenced and the session is"
    say "    kept awake;"
    if skip_named themes; then
      say "  - the themes are NOT switched: by --skip;"
    elif [[ -n $THEME_BLOCKER ]]; then
      say "  - the themes are NOT switched: $THEME_BLOCKER;"
      say_sudo_asked
      [[ $THEME_BLOCKER == "$UNREAD"* ]] ||
        name_unknowns "Also in the way, and all that --themes-anyway would set aside (never the reason above):"
    else
      say "  - each of $themes Omarchy themes is set with omarchy-theme-set. A switch also"
      say "    retints what Omarchy retints: the terminals and the programs in them,"
      say "    Hyprland's configuration, GTK's colour scheme and icons, Omarchy's themes for"
      say "    the coding agents it supports, T3 Code, Obsidian and fcitx5. It still touches"
      say "    two things that are not files of yours: the keyboard backlight, where Omarchy"
      say "    drives one, and the browsers' colour, a file under /etc written through"
      say "    Omarchy's own passwordless sudo rule, which a running browser is told to read again."
      say_sudo_asked
      say "    For the run these Omarchy toggles are on, so that no editor is touched and no"
      say "    extension installed: ${SKIP_TOGGLES[*]:-none exist here}."
      if ((${#THEME_UNKNOWNS[@]} == 0)); then
        say "    Nothing is sent to herdr machines (theme sync is off), and no hook of yours runs;"
      else
        say "    Nothing is sent to herdr machines (theme sync is off)."
        name_unknowns "--themes-anyway was given, so every switch also runs, unread by this script:"
      fi
    fi
    skip_named font || say "  - one other monospace font is set with omarchy-font-set;"
    skip_named floating || say "  - the window floats;"
    skip_named scales || say "  - the window's monitor goes through the scales ${SCALES[*]};"
    skip_named inactive || say "  - one terminal opens and closes;"
    if skip_named rest; then
      say "  - GitTurtle is not rested and closed (the rest check is skipped): the script"
      say "    ends it by its pid, before --hands opens its own or when the run ends."
    else
      say "  - GitTurtle rests for $REST_SECONDS s and is closed."
    fi
    say "Everything is put back as it was found: workspace, theme and wallpaper, the"
    say "toggles, font and the files omarchy-font-set rewrites, scale, tiling, focus,"
    say "notifications and idle. A picture of GitTurtle's own window is taken at each"
    say "step, never of the screen, and only while nothing else overlaps the window and"
    say "notifications are known to be silenced."
    [[ $HANDS == no ]] || skip_named hands || say "At the end it asks four things of you (--hands), beside this terminal."
    say "Before the first change it prints where the desktop's starting state and the saved"
    say "copies are kept, and how to put the desktop back by hand after a kill or a power loss."
  fi
  say "This plan is final: a part it says will not run is not started later in the run."
  [[ $ASSUME_YES == no ]] || return 0
  [[ -t 3 ]] || die "standard input is not a terminal: pass --yes to start without the Enter."
  printf 'Press Enter to start, Ctrl+C to stop: '
  read -r -u 3 -t "$ANSWER_WAIT" answer || die "no Enter within $ANSWER_WAIT s: nothing was run or changed."
}

# ---------------------------------------------------------------- Hyprland: reads and requests

hypr_json() { timeout 5 hyprctl -j "$@" 2>/dev/null; }
# KIND is dispatch or eval; LUA is one expression in the form Omarchy 4.5's own
# commands send. The request and Hyprland's reply go to records/hyprland.txt;
# whether it worked is always read back from Hyprland's state, never from the reply.
hypr_request() {
  local reply
  reply=$(timeout 5 hyprctl "$1" "$2" 2>&1)
  printf '%s %s -> %s\n' "$1" "$2" "$reply" | record_more hyprland.txt
}
focus_window() { hypr_request dispatch "hl.dsp.focus({ window = \"address:$1\" })"; }
close_window() { hypr_request dispatch "hl.dsp.window.close({ window = \"address:$1\" })"; }
toggle_floating() { hypr_request dispatch "hl.dsp.window.float({ window = \"address:$1\", action = \"toggle\" })"; }
focus_workspace() { hypr_request dispatch "hl.dsp.focus({ workspace = \"$1\" })"; }

# "ID WINDOWS" of the workspace that has the focus.
active_workspace() { hypr_json activeworkspace | jq -r '"\(.id) \(.windows)"' 2>/dev/null; }
# "numbered" where the run can start, else "special" or "named": the run leaves for
# an empty workspace among the monitor's ten numbered ones and comes back by its
# number, and no picture is taken under a special workspace. Hyprland gives a
# named workspace an id below 1.
starting_workspace() {
  hypr_json monitors | jq -r '
    ([.[] | select(.focused)] | first) as $monitor
    | if $monitor == null then "unknown"
      elif ($monitor.specialWorkspace.name // "") != "" then "special"
      elif ($monitor.activeWorkspace.id | type) != "number" then "unknown"
      elif $monitor.activeWorkspace.id < 1 then "named"
      else "numbered"
      end' 2>/dev/null
}
# The first workspace without a window among the ten Omarchy gives the focused
# monitor (1-10 on the first monitor, 11-20 on the next), and never one that
# another monitor shows; nothing when all ten hold windows.
empty_workspace() {
  local monitors workspaces
  monitors=$(hypr_json monitors) && workspaces=$(hypr_json workspaces) || return 1
  jq -rn --argjson monitors "$monitors" --argjson workspaces "$workspaces" '
    ([$monitors[] | select(.focused)] | first) as $monitor
    | ($monitor.activeWorkspace.id // 0) as $now
    | if $now < 1 then empty
      else
        ((($now - 1) / 10 | floor) * 10) as $base
        | first(range($base + 1; $base + 11)
            | select(. as $id | all($workspaces[]; .id != $id or (.windows == 0 and .monitorID == $monitor.id))))
      end' 2>/dev/null
}
# What Hyprland reports as the app's window opacity: "ACTIVE/INACTIVE".
window_opacity() {
  local active inactive
  active=$(timeout 5 hyprctl getprop "address:$APP_ADDRESS" opacity 2>/dev/null | head -n 1)
  inactive=$(timeout 5 hyprctl getprop "address:$APP_ADDRESS" opacity_inactive 2>/dev/null | head -n 1)
  [[ $active =~ ^[0-9.]+$ ]] || active=unknown
  [[ $inactive =~ ^[0-9.]+$ ]] || inactive=unknown
  printf '%s/%s' "$active" "$inactive"
}

active_address() { hypr_json activewindow | jq -r '.address // empty' 2>/dev/null; }
window_exists() { hypr_json clients | jq -e --arg address "$1" 'any(.[]; .address == $address)' >/dev/null 2>&1; }
# FILTER applied to the window of the app this script started, found by its pid.
app_window() {
  hypr_json clients | jq -r --argjson pid "${APP_PID:-0}" "first(.[] | select(.pid == \$pid and .mapped)) | $1" 2>/dev/null
}
# What Hyprland reports for each monitor, without what identifies the panel.
monitor_lines() {
  hypr_json monitors | jq -r '.[] | "\(.name) \(.width)x\(.height)@\(.refreshRate) scale \(.scale) at \(.x),\(.y) transform \(.transform) vrr \(.vrr) format \(.currentFormat) mirror \(.mirrorOf)"' 2>/dev/null
}

# ---------------------------------------------------------------- pictures

# "ok X Y W H SCALE FLOATING" when a grab of the app's rectangle can hold nothing
# but its window (and, through Omarchy's window opacity, the wallpaper under it);
# otherwise "blocked" and why. No other window may overlap the rectangle, tiled
# or floating, since a translucent window shows what lies under it, and no other
# monitor may reach into it. While the run has a workspace of its own (ALONE),
# the only other window allowed on it at all is the terminal this script opened.
picture_target() {
  local clients monitors
  clients=$(hypr_json clients) && monitors=$(hypr_json monitors) || {
    echo "blocked Hyprland does not answer"
    return
  }
  jq -rn --argjson clients "$clients" --argjson monitors "$monitors" --argjson pid "${APP_PID:-0}" \
    --arg alone "$ALONE" --arg terminal "${TERMINAL_ADDRESS:-}" '
    def overlaps($a):
      .at[0] < ($a.at[0] + $a.size[0]) and (.at[0] + .size[0]) > $a.at[0]
      and .at[1] < ($a.at[1] + $a.size[1]) and (.at[1] + .size[1]) > $a.at[1];
    # A monitor as a rectangle of the layout: its mode over its scale, turned with it.
    def area:
      (if (.transform % 2) == 1 then [.height, .width] else [.width, .height] end) as $mode
      | {at: [.x, .y], size: [$mode[0] / .scale, $mode[1] / .scale]};
    ([$clients[] | select(.pid == $pid and .mapped)] | first) as $app
    | if $app == null then "blocked the app has no window"
      elif $app.hidden then "blocked the window is hidden"
      else
        ([$monitors[] | select(.id == $app.monitor)] | first) as $monitor
        | if $monitor == null then "blocked the window is on no monitor"
          elif $monitor.activeWorkspace.id != $app.workspace.id then "blocked the window is on a workspace that is not shown"
          elif ($monitor.specialWorkspace.name // "") != "" then "blocked a special workspace is shown over it"
          else
            [$clients[] | select(.address != $app.address and .mapped and (.hidden | not)
              and (.workspace.id == $app.workspace.id or (.pinned and .monitor == $app.monitor)))] as $others
            | if any($monitors[] | select(.id != $app.monitor and (.disabled | not) and (.mirrorOf // "none") == "none");
                     area | overlaps($app)) then "blocked another monitor overlaps the rectangle of the window"
              elif any($others[]; (.fullscreen // 0) != 0) then "blocked another window is fullscreen on its workspace"
              elif any($others[]; overlaps($app)) then "blocked another window overlaps it"
              elif $alone == "yes" and any($others[]; .address != $terminal) then "blocked another window is on the workspace of the run"
              else "ok \($app.at[0]) \($app.at[1]) \($app.size[0]) \($app.size[1]) \($monitor.scale) \($app.floating)"
              end
          end
      end' 2>/dev/null || echo "blocked Hyprland's answer could not be read"
}

# pictures/NAME.png: the app's own window rectangle, grabbed only while nothing
# else can be in it and discarded if that changed during the grab. A notification
# is drawn over windows and may show anything, so a picture is taken only while
# the shell itself says that notifications are silenced.
picture() {
  local name=$1 file=$PICTURES/$1.png target verdict rest x y w h scale floating focus=inactive
  if session_locked; then
    report "picture $name: not taken: the session is locked"
    return 1
  fi
  if [[ $(dnd_state) != on ]]; then
    report "picture $name: not taken: notification silencing could not be confirmed, and a notification would be drawn over the window"
    return 1
  fi
  target=$(picture_target)
  read -r verdict rest <<<"$target"
  if [[ $verdict != ok ]]; then
    report "picture $name: not taken: ${rest:-no answer}"
    return 1
  fi
  read -r x y w h scale floating <<<"$rest"
  if ! timeout 20 grim -g "$x,$y ${w}x$h" "$file" 2>>"$WORK/grim.log"; then
    rm -f -- "$file"
    report "picture $name: not taken: grim failed ($(tail -n 1 "$WORK/grim.log"))"
    return 1
  fi
  if [[ $(picture_target) != "$target" ]] || session_locked || [[ $(dnd_state) != on ]]; then
    rm -f -- "$file"
    report "picture $name: discarded: the desktop changed while it was taken"
    return 1
  fi
  [[ $(active_address) != "$APP_ADDRESS" ]] || focus=active
  [[ $floating == true ]] && floating=floating || floating=tiled
  report "picture $name: pictures/$name.png, window ${w}x$h at $x,$y, monitor scale $scale, $floating, $focus, opacity $(window_opacity) (active/inactive, as Hyprland reports)"
}

# ---------------------------------------------------------------- the restore registry

# A change is registered before it is made; its restore runs when its check ends
# (run_restore) or, after a stop, when the script exits (restore_all), last
# change first. Every restore is safe to run when its change never happened.
register_restore() { RESTORES+=("$1"); }
run_restore() {
  local name=$1 item
  local -a kept=()
  "$name"
  for item in "${RESTORES[@]}"; do [[ $item == "$name" ]] || kept+=("$item"); done
  RESTORES=("${kept[@]}")
}
restore_all() {
  while ((${#RESTORES[@]} > 0)); do run_restore "${RESTORES[-1]}"; done
}
# A restore that did not take: said at once, and a FAILED line of the restore
# check at the end, which also keeps the saved copies.
restore_failed() {
  RESTORE_FAILURES+=("$*")
  report "restore FAILED: $*"
}
# work/restore holds what a put-back needs, and its manifest.txt says what each
# saved copy is and what else this run made; work/ is never packed, so full paths
# may stand in it.
restore_note() {
  [[ -d $RESTORE ]] || mkdir -p -m 700 -- "$RESTORE" || return 1
  printf '%s\n' "$*" >>"$RESTORE/manifest.txt"
}
# SAVED over TARGET by a copy beside TARGET and a rename, so that TARGET is never
# missing or half written; false, with TARGET untouched, when that cannot be done.
put_back() {
  local saved=$1 target=$2 temporary
  temporary=$(mktemp -- "${target%/*}/.${target##*/}.gitturtle-check.XXXXXX" 2>/dev/null) || return 1
  if cp -p -- "$saved" "$temporary" 2>/dev/null && cmp -s -- "$saved" "$temporary" && mv -fT -- "$temporary" "$target" 2>/dev/null; then
    return 0
  fi
  rm -f -- "$temporary"
  return 1
}
# PATH becomes a link that reads TEXT again, by a rename over whatever replaced it.
put_back_link() {
  local text=$1 path=$2 temporary
  temporary=${path%/*}/.${path##*/}.gitturtle-check.$$
  if ln -s -- "$text" "$temporary" 2>/dev/null && mv -fT -- "$temporary" "$path" 2>/dev/null; then
    return 0
  fi
  rm -f -- "$temporary"
  return 1
}

# ---------------------------------------------------------------- desktop changes, each with its restore

# --- notifications: silenced, so that none is drawn over the window in a picture
dnd_state() { timeout 5 omarchy-shell notifications isDnd 2>/dev/null; } # prints on or off
# The state is named, on or off, as the shell's own setDnd takes it: a toggle
# would turn silencing off again if anything else had turned it on meanwhile.
# The indicator is refreshed as Omarchy's own toggle command does after a change.
set_dnd() {
  timeout 10 omarchy-shell notifications setDnd "$1" >/dev/null 2>&1
  timeout 5 omarchy-shell -q omarchy.indicators refresh >/dev/null 2>&1
}
silence_notifications() {
  [[ -n $START_DND ]] || return 1
  [[ $(dnd_state) == on ]] || set_dnd on
  [[ $(dnd_state) == on ]]
}
restore_notifications() {
  [[ -n $START_DND ]] || return 0
  [[ $(dnd_state) == "$START_DND" ]] || set_dnd "$START_DND"
  if [[ $(dnd_state) == "$START_DND" ]]; then
    report "restore notifications: silencing is $START_DND, as it was"
  else
    restore_failed "notifications: silencing is $(dnd_state), and it was $START_DND"
  fi
}

# --- idle: Omarchy starts its screensaver after 150 s without input and locks at 300 s
keep_awake() {
  [[ $START_AWAKE == no ]] || return 0
  timeout 10 omarchy-toggle-idle stay-awake >/dev/null 2>&1
  [[ -f $STAY_AWAKE ]]
}
restore_idle() {
  [[ $START_AWAKE == no ]] || return 0
  [[ ! -f $STAY_AWAKE ]] || timeout 10 omarchy-toggle-idle allow-idle >/dev/null 2>&1
  report "restore idle: stay-awake is $([[ -f $STAY_AWAKE ]] && echo on || echo off), was off"
}

# --- the workspace: an empty one for the run, so that no window of the owner's is
# beside, under or over the app's; false when the desktop is not on one afterwards
go_to_empty_workspace() {
  local now windows target
  read -r now windows <<<"$(active_workspace)"
  [[ $now == "$START_WORKSPACE" ]] || return 1
  if [[ $windows != 0 ]]; then
    target=$(empty_workspace)
    [[ $target =~ ^[0-9]+$ ]] || return 1
    focus_workspace "$target"
    nap 0.5
    read -r now windows <<<"$(active_workspace)"
    [[ $now == "$target" && $windows == 0 ]] || return 1
  fi
  ALONE=yes
  report "workspace: the run has workspace $now, which held no window; it started on $START_WORKSPACE"
}
restore_workspace() {
  local now
  ALONE=no
  [[ $START_WORKSPACE =~ ^[0-9]+$ ]] || return 0
  now=$(active_workspace)
  if [[ ${now%% *} != "$START_WORKSPACE" ]]; then
    focus_workspace "$START_WORKSPACE"
    nap 0.5
    now=$(active_workspace)
  fi
  if [[ ${now%% *} == "$START_WORKSPACE" ]]; then
    report "restore workspace: back on workspace $START_WORKSPACE, where the run started"
  else
    restore_failed "workspace: the desktop is on workspace ${now%% *}, and the run started on $START_WORKSPACE"
  fi
  START_WORKSPACE=""
}

# --- focus: the window that had it when the desktop part began
restore_focus() {
  [[ $START_FOCUS =~ ^0x[0-9a-f]+$ ]] || return 0
  if ! window_exists "$START_FOCUS"; then
    report "restore focus: the window that had it is gone"
    return 0
  fi
  [[ $(active_address) == "$START_FOCUS" ]] || focus_window "$START_FOCUS"
  if [[ $(active_address) == "$START_FOCUS" ]]; then
    report "restore focus: the window that had it has it again"
  else
    restore_failed "focus: the window that had the focus does not have it again"
  fi
}

# --- the app: the installed executable on the demo repository, with a new XDG_CONFIG_HOME
# The app reads fontconfig's user configuration from XDG_CONFIG_HOME, which is new
# for this launch, so the desktop's own ~/.config/fontconfig is linked into it for
# as long as that directory exists (omarchy-font-set creates it on a stock system).
mirror_fontconfig() {
  local link=$LAUNCH/config/fontconfig
  if [[ -d $HOME/.config/fontconfig ]]; then
    [[ -L $link ]] || ln -s "$HOME/.config/fontconfig" "$link"
  else
    rm -f -- "$link"
  fi
}
# HOME stays the session's, because the app finds Omarchy's theme at
# $HOME/.local/state/omarchy/current; preferences, sessions and drafts go to the
# new XDG_CONFIG_HOME, caches to a new XDG_CACHE_HOME, and Git sees a fixed
# identity instead of the owner's configuration (main has already dropped every
# GIT_* and GITTURTLE_* variable of the calling shell).
#
# One app at a time: an app this script started earlier is ended before another
# is started, and none is started while that one could not be ended (false then).
launch_app() {
  stop_app || return 1
  mkdir -p "$LAUNCH/config" "$LAUNCH/cache"
  printf '[user]\n\tname = Maya Chen\n\temail = maya@aurora.example.invalid\n' >"$LAUNCH/gitconfig"
  mirror_fontconfig
  # `env` becomes the app, so this pid is the app's own.
  "${SESSION_ENV[@]}" "XDG_CONFIG_HOME=$LAUNCH/config" "XDG_CACHE_HOME=$LAUNCH/cache" \
    "GIT_CONFIG_GLOBAL=$LAUNCH/gitconfig" GIT_CONFIG_NOSYSTEM=1 \
    "$INSTALLED_BINARY" "$DEMO" >>"$LAUNCH/app.log" 2>&1 </dev/null 3<&- &
  APP_PID=$!
}
# True while the process runs; one that has ended and waits to be collected has not.
process_running() {
  local stat
  stat=$(cat "/proc/$1/stat" 2>/dev/null) || return 1
  stat=${stat##*) }
  [[ ${stat%% *} != Z ]]
}
wait_for_exit() { # PID MILLISECONDS: true when the process ended within that time
  local deadline=$(($(now_ms) + $2))
  while process_running "$1"; do
    (($(now_ms) < deadline)) || return 1
    nap 0.02
  done
}
wait_for_app_window() { # true once Hyprland lists the app's window, within WINDOW_WAIT seconds
  local deadline=$(($(now_ms) + WINDOW_WAIT * 1000))
  while (($(now_ms) < deadline)); do
    APP_ADDRESS=$(app_window .address)
    [[ ! $APP_ADDRESS =~ ^0x[0-9a-f]+$ ]] || return 0
    process_running "$APP_PID" || return 1
    nap 0.1
  done
  return 1
}
panic_lines() {
  local count
  count=$(grep -cE 'panicked at|fatal runtime error|stack backtrace' "$LAUNCH/app.log" 2>/dev/null)
  echo "${count:-0}"
}
# True while PID is a running child of this script: a process it started from its
# own shell (never from a subshell), and not another one that the kernel gave the
# same number after that one had ended. From the moment of the fork, whatever the
# child has become since.
still_ours() {
  local stat
  stat=$(cat "/proc/$1/stat" 2>/dev/null) || return 1
  stat=${stat##*) } # "STATE PARENT ..."
  [[ ${stat%% *} != Z ]] || return 1
  stat=${stat#* }
  [[ ${stat%% *} == "$$" ]]
}
# Ends a process this script started, by its exact pid and only while it is still
# that process: TERM, a wait with a limit, then KILL and one more. False when the
# process outlived both; never waits longer.
stop_process() {
  still_ours "$1" || return 0
  kill "$1" 2>/dev/null
  wait_for_exit "$1" "$STOP_WAIT_MS" && return 0
  still_ours "$1" || return 0
  kill -KILL "$1" 2>/dev/null
  wait_for_exit "$1" "$STOP_WAIT_MS"
}
# Collects a child that has ended; one that still runs is left, never waited for.
reap() { process_running "$1" || wait "$1" 2>/dev/null; }
# Ends the app this script started, if it still runs: its own child, by its exact
# pid. False when it could not be ended.
stop_app() {
  local ended=yes
  [[ -n $APP_PID ]] || return 0
  if still_ours "$APP_PID"; then
    if stop_process "$APP_PID"; then
      report "the app was still running and was ended by its pid"
    else
      restore_failed "the app: its process still runs after TERM and KILL"
      ended=no
    fi
  fi
  reap "$APP_PID"
  [[ ! -f $LAUNCH/app.log ]] || record app-output.txt <"$LAUNCH/app.log"
  APP_PID="" APP_ADDRESS=""
  [[ $ended == yes ]]
}
# Every exit ends what this script started: whatever is still in the shell's own
# list of what it put in the background once the restores have run (an app or a
# terminal whose pid a stop came too early to note, the sleep of a wait that a
# stop cut short), each by its exact pid.
stop_leftovers() {
  local pid ended=0
  for pid in $(jobs -pr); do
    still_ours "$pid" || continue
    if stop_process "$pid"; then
      ended=$((ended + 1))
    else
      restore_failed "a process this script started still runs after TERM and KILL"
    fi
    reap "$pid"
  done
  ((ended == 0)) || report "the exit: $ended more process(es) this script had started still ran and were ended, each by its pid"
}

# --- the theme: Omarchy's own switch, and the wallpaper it brings
theme_now() { cat "$OMARCHY_CURRENT/theme.name" 2>/dev/null; }
theme_slug() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr ' ' '-'; } # as omarchy-theme-set names a directory
# omarchy-theme-set applies a directory without colours too and leaves the desktop without any.
theme_has_colors() {
  local directory
  for directory in "$OMARCHY_PATH/themes/$1" "$HOME/.config/omarchy/themes/$1"; do
    [[ -f $directory/colors.toml || -f $directory/alacritty.toml ]] && return 0
  done
  return 1
}
# Waits for Omarchy's retints too, and never reads a terminal: a switch typed in
# one would let Omarchy's browser-colour step ask for a password there.
# OMARCHY_THEME_SYNC_FROM is what Omarchy's mirror to herdr machines takes as "this
# theme came from another machine": it then sends nothing, whatever its toggle says.
set_theme() {
  "${SESSION_ENV[@]}" OMARCHY_THEME_WAIT=1 OMARCHY_THEME_SYNC_FROM=gitturtle-omarchy-check \
    timeout "$SWITCH_WAIT" omarchy-theme-set "$1" </dev/null >>"$WORK/theme-set.log" 2>&1 3<&-
}
# Omarchy's skip toggles, on for the run: with one on, omarchy-theme-set-vscode
# leaves that editor alone, so no extension is installed and no settings file is
# rewritten. A toggle the owner already has on stays theirs: only one this run
# makes is noted, before it is made, and only that one is removed afterwards.
skip_toggles_on() { # false when a switch could still reach an editor
  local toggle
  if [[ ! -d $TOGGLES ]]; then
    restore_note "directory	$TOGGLES	made by this run"
    TOGGLES_DIRECTORY_MADE=yes
    mkdir -p -- "$TOGGLES" || return 1
  fi
  for toggle in "${SKIP_TOGGLES[@]}"; do
    if [[ ! -e $TOGGLES/$toggle && ! -L $TOGGLES/$toggle ]]; then
      restore_note "toggle	$TOGGLES/$toggle	made by this run; remove it" || return 1
      SKIP_TOGGLES_MADE+=("$toggle")
      : >"$TOGGLES/$toggle"
    fi
    [[ -f $TOGGLES/$toggle ]] || return 1
  done
}
skip_toggles_all_on() {
  local toggle
  for toggle in "${SKIP_TOGGLES[@]}"; do [[ -f $TOGGLES/$toggle ]] || return 1; done
}
restore_skip_toggles() {
  local toggle
  local -a removed=()
  for toggle in "${SKIP_TOGGLES_MADE[@]}"; do
    if rm -f -- "$TOGGLES/$toggle" && [[ ! -e $TOGGLES/$toggle ]]; then
      removed+=("$toggle")
    else
      restore_failed "toggle: $toggle is still on, and this run turned it on"
    fi
  done
  [[ $TOGGLES_DIRECTORY_MADE == no ]] || rmdir -- "$TOGGLES" 2>/dev/null
  ((${#SKIP_TOGGLES[@]} == 0)) || report "restore skip toggles: removed ${removed[*]:-none}; the others were on before the run"
  SKIP_TOGGLES_MADE=() TOGGLES_DIRECTORY_MADE=no
}
wait_theme_intro() { # the wallpaper's intro, 15 s at most
  local id tries=60
  have omarchy-shell || return 0
  id=$(stat -Lc '%d:%i' "$OMARCHY_CURRENT/theme" 2>/dev/null) || return 0
  while ((tries-- > 0)) && [[ $(timeout 2 omarchy-shell shell themeIntroStatus "$id" 2>/dev/null) == pending ]]; do nap 0.25; done
}
wallpaper_now() { readlink -f "$OMARCHY_CURRENT/background" 2>/dev/null; }
restore_theme() {
  local as_found=yes
  [[ -n $START_THEME ]] || return 0
  if [[ $(theme_now) != "$START_THEME" || ! -f $OMARCHY_CURRENT/theme/colors.toml ]]; then
    set_theme "$START_THEME"
    wait_theme_intro
  fi
  if [[ -n $START_WALLPAPER && $(wallpaper_now) != "$START_WALLPAPER" && -f $START_WALLPAPER ]]; then
    timeout 30 omarchy-theme-bg-set "$START_WALLPAPER" >/dev/null 2>&1
  fi
  [[ $(wallpaper_now) == "$START_WALLPAPER" ]] || as_found=NO
  report "restore theme: the theme is $(theme_now), was $START_THEME; wallpaper as found: $as_found"
}

# --- the font: Omarchy's own command, and the files it rewrites
# omarchy-font-set writes through a link at kitty.conf and fonts.conf (sed
# --follow-symlinks, a redirection) and replaces a link with a file of its own at
# the other three (sed -i). So what is saved, compared and put back is the regular
# file a path leads to, wherever that lies under the home, and the link's own text.
# "absent", "file RESOLVED", or "unsafe WHY" for a path this script will not change.
font_path_state() {
  local path=$1 real
  if [[ ! -e $path && ! -L $path ]]; then
    real=$(realpath -m -- "${path%/*}" 2>/dev/null)
    if [[ $real == "$HOME_REAL"/* ]]; then echo absent; else echo "unsafe its directory leads outside your home"; fi
    return
  fi
  if ! real=$(realpath -e -- "$path" 2>/dev/null); then
    echo "unsafe it is a link that leads nowhere"
  elif [[ ! -f $real ]]; then
    echo "unsafe it leads to something that is not a regular file"
  elif [[ $real != "$HOME_REAL"/* ]]; then
    echo "unsafe it leads outside your home"
  else
    echo "file $real"
  fi
}
# Copies under work/restore, never packed, each named in manifest.txt. False, with
# FONT_PROBLEM saying why, when a path is unsafe or cannot be saved: nothing has
# been changed then.
save_font_files() {
  local file path state index=0
  FONT_PROBLEM=""
  for file in "${FONT_FILES[@]}"; do
    state=$(font_path_state "$HOME/.config/$file")
    if [[ $state == unsafe* ]]; then
      FONT_PROBLEM="~/.config/$file, which omarchy-font-set rewrites: ${state#unsafe }"
      return 1
    fi
  done
  for file in "${FONT_FILES[@]}"; do
    path=$HOME/.config/$file
    state=$(font_path_state "$path")
    FONT_DIRECTORY[index]=absent
    [[ ! -d ${path%/*} ]] || FONT_DIRECTORY[index]=present
    FONT_LINK[index]=""
    [[ ! -L $path ]] || FONT_LINK[index]=$(readlink -- "$path")
    FONT_REAL[index]="" FONT_SAVED[index]=absent
    if [[ $state == file* ]]; then
      FONT_REAL[index]=${state#file }
      FONT_SAVED[index]=copy
      if ! restore_note "font-$index	$path	the bytes of ${FONT_REAL[index]}${FONT_LINK[index]:+	$path was a link reading ${FONT_LINK[index]}}" ||
        ! cp -p -- "${FONT_REAL[index]}" "$RESTORE/font-$index" || ! cmp -s -- "${FONT_REAL[index]}" "$RESTORE/font-$index"; then
        FONT_PROBLEM="~/.config/$file could not be saved first"
        return 1
      fi
    else
      restore_note "absent	$path	did not exist; remove it" || return 1
    fi
    index=$((index + 1))
  done
}
# Byte for byte, by a copy and a rename; a link the change replaced is a link
# again; a file or directory the change created is removed. What cannot be put
# back is left as it is and becomes a FAILED line of the restore check.
put_back_font_files() {
  local file path saved index=0
  local -a done=()
  for file in "${FONT_FILES[@]}"; do
    path=$HOME/.config/$file saved=$RESTORE/font-$index
    case ${FONT_SAVED[index]:-} in
      copy)
        if [[ -n ${FONT_LINK[index]} ]] && [[ ! -L $path || $(readlink -- "$path") != "${FONT_LINK[index]}" ]]; then
          if put_back_link "${FONT_LINK[index]}" "$path"; then
            done+=("$file is a link again")
          else
            restore_failed "font: ~/.config/$file was a link and could not be made one again (work/restore/manifest.txt has its text)"
          fi
        fi
        if ! cmp -s -- "$saved" "${FONT_REAL[index]}"; then
          if put_back "$saved" "${FONT_REAL[index]}"; then
            done+=("$file put back")
          else
            restore_failed "font: ~/.config/$file could not be put back; its saved copy is work/restore/font-$index"
          fi
        fi
        ;;
      absent)
        if [[ -e $path || -L $path ]]; then
          if rm -f -- "$path"; then
            done+=("$file removed, it did not exist")
          else
            restore_failed "font: ~/.config/$file did not exist and could not be removed"
          fi
        fi
        [[ ${FONT_DIRECTORY[index]:-} != absent ]] || rmdir -- "${path%/*}" 2>/dev/null
        ;;
    esac
    index=$((index + 1))
  done
  ((${#done[@]} == 0)) || report "restore font: under ~/.config: $(printf '%s; ' "${done[@]}")"
}
restore_font() {
  [[ -n $START_FONT ]] || return 0
  if [[ $(timeout 5 omarchy-font-current 2>/dev/null) != "$START_FONT" ]]; then
    "${SESSION_ENV[@]}" timeout "$SWITCH_WAIT" omarchy-font-set "$START_FONT" </dev/null >>"$WORK/font-set.log" 2>&1 3<&-
  fi
  put_back_font_files
  mirror_fontconfig
  report "restore font: omarchy-font-current is $(timeout 5 omarchy-font-current 2>/dev/null), was $START_FONT"
}

# --- floating: the app's own window
restore_floating() {
  local now
  [[ -n $START_FLOATING ]] || return 0
  now=$(app_window .floating)
  [[ -z $now || $now == "$START_FLOATING" ]] || toggle_floating "$APP_ADDRESS"
  nap 0.5
  report "restore floating: the window's floating state is $(app_window .floating), was $START_FLOATING"
  START_FLOATING=""
}

# --- the monitor's scale: a rule built from the monitor's own mode and position
monitor_scale() { hypr_json monitors | jq -r --arg name "$MONITOR_NAME" '.[] | select(.name == $name) | .scale' 2>/dev/null; }
monitor_line() { monitor_lines | awk -v name="$MONITOR_NAME" '$1 == name'; }
same_scale() { awk -v a="$1" -v b="$2" 'BEGIN { d = a - b; if (d < 0) d = -d; exit !(d < 0.005) }'; }
# Hyprland accepts only scales that divide the mode into whole logical pixels, in
# steps of 1/120 (the rule Omarchy's own scaling command follows). For any other it
# picks a scale of its own and draws a warning over the screen for five seconds, so
# such a scale is never requested.
scale_divides_mode() { # SCALE WIDTH HEIGHT
  awk -v scale="$1" -v w="$2" -v h="$3" 'BEGIN { k = int(scale * 120 + 0.5); exit !((w * 120) % k == 0 && (h * 120) % k == 0) }'
}
set_scale() {
  hypr_request eval "hl.monitor({ output = \"$MONITOR_NAME\", mode = \"$MONITOR_MODE\", position = \"$MONITOR_POSITION\", scale = $1 })"
}
restore_scale() {
  [[ -n $MONITOR_NAME ]] || return 0
  if ! same_scale "$(monitor_scale)" "$MONITOR_START_SCALE"; then
    set_scale "$MONITOR_START_SCALE"
    nap 1
  fi
  if [[ $(monitor_line) != "$MONITOR_START_LINE" ]]; then
    # The rule above names only mode, position and scale: Hyprland's own
    # configuration gives the monitor back whatever else its rule there sets.
    timeout 10 hyprctl reload >/dev/null 2>&1
    nap 1
  fi
  report "restore scale: $(monitor_line)"
  [[ $(monitor_line) == "$MONITOR_START_LINE" ]] || report "restore scale: NOT as found, which was: $MONITOR_START_LINE"
}

# --- the terminal: Omarchy's default one, opened to take the focus
close_terminal() {
  local deadline
  if [[ $TERMINAL_ADDRESS =~ ^0x[0-9a-f]+$ ]] && window_exists "$TERMINAL_ADDRESS"; then
    close_window "$TERMINAL_ADDRESS"
    deadline=$(($(now_ms) + STOP_WAIT_MS))
    while (($(now_ms) < deadline)) && window_exists "$TERMINAL_ADDRESS"; do nap 0.1; done
    # A terminal that runs as one process for all its windows is the owner's
    # process, not this script's: its window is asked to close, and no more.
    ! window_exists "$TERMINAL_ADDRESS" ||
      restore_failed "the terminal: its window is still open ${STOP_WAIT_MS} ms after Hyprland was asked to close it (its command ends by itself within $TERMINAL_LIFE s)"
  fi
  if [[ -n $TERMINAL_PID ]]; then
    # This script's own child, by its exact pid: a moment to end with its window,
    # then TERM and KILL, each with a limit, so the restores after it never wait.
    wait_for_exit "$TERMINAL_PID" 1000 || stop_process "$TERMINAL_PID" ||
      restore_failed "the terminal: its process still runs after TERM and KILL"
    reap "$TERMINAL_PID"
  fi
  TERMINAL_ADDRESS="" TERMINAL_PID=""
}

# ---------------------------------------------------------------- checks: nothing on the desktop changes

report_monitors() {
  local line found=no
  while IFS= read -r line; do
    [[ -n $line ]] || continue
    report "monitor: $line"
    found=yes
  done < <(monitor_lines)
  [[ $found == yes ]] || report "monitors: Hyprland does not answer"
}
report_gpu() {
  local line found=no card driver
  if have lspci; then
    while IFS= read -r line; do
      report "GPU (lspci): $line"
      found=yes
    done < <(timeout 10 lspci 2>/dev/null | grep -Ei 'VGA compatible|3D controller|Display controller')
    [[ $found == yes ]] || report "GPU (lspci): no VGA, 3D or display controller on the PCI bus"
  else
    report "GPU (lspci): lspci is not installed"
  fi
  for card in /sys/class/drm/card[0-9]*; do
    [[ -e $card && ${card##*/} != *-* ]] || continue
    driver=$(readlink -f "$card/device/driver" 2>/dev/null)
    report "GPU (kernel): ${card##*/} uses the ${driver##*/} driver"
  done
}
report_tools() {
  local tool
  local -a present=() missing=()
  for tool in python3 git desktop-file-validate lspci ldd makepkg bsdtar hyprctl grim coredumpctl journalctl fc-match \
    xdg-terminal-exec wl-paste omarchy-version omarchy-theme-list omarchy-theme-set omarchy-font-list \
    omarchy-font-set omarchy-hyprland-session-locked omarchy-shell omarchy-toggle-idle; do
    if have "$tool"; then present+=("$tool"); else missing+=("$tool"); fi
  done
  report "tools found: ${present[*]}"
  report "tools missing: ${missing[*]:-none} (a check that needs one says so and is skipped)"
}

check_host() {
  local version
  section a host "this machine"
  report "architecture: $(uname -m)"
  report "kernel: $(uname -sr)"
  report "page size: $(getconf PAGESIZE 2>/dev/null || echo unknown) bytes"
  report "system: $(sed -n 's/^PRETTY_NAME="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' /etc/os-release 2>/dev/null | head -n 1), $(getconf GNU_LIBC_VERSION 2>/dev/null)"
  version=$(timeout 10 omarchy-version 2>/dev/null)
  if [[ -n $version ]]; then report "omarchy-version: $version"; else problem "omarchy-version gave no version: is this Omarchy?"; fi
  version=$(hypr_json version | jq -r '"\(.version) (commit \(.commit[0:12]))"' 2>/dev/null)
  report "Hyprland: ${version:-does not answer in this session}"
  report "session: ${XDG_SESSION_TYPE:-unknown}, desktop ${XDG_CURRENT_DESKTOP:-unknown}"
  report_monitors
  report_gpu
  report "theme: $(timeout 5 omarchy-theme-current 2>/dev/null || echo unknown)"
  report "font: $(timeout 5 omarchy-font-current 2>/dev/null || echo unknown)"
  report_tools
  result
}

# Prints why the archive's listing is not one that may be extracted: only plain
# files and directories, all under the one directory ARCHIVE_TOP.
archive_listing_problem() {
  if [[ ! -s $WORK/archive-listing.txt || ! -s $WORK/archive-names.txt ]]; then
    echo "tar cannot list it"
    return
  fi
  if grep -qv '^[-d]' "$WORK/archive-listing.txt"; then
    echo "it holds an entry that is neither a file nor a directory (a link or a device)"
    return
  fi
  if [[ ! $ARCHIVE_TOP =~ ^[A-Za-z0-9][A-Za-z0-9._+-]*$ ]]; then
    echo "unexpected directory name in it: $ARCHIVE_TOP"
    return
  fi
  if ! awk -v top="$ARCHIVE_TOP" '
      $0 != top && $0 != top "/" && index($0, top "/") != 1 { bad = 1 }
      /(^|\/)\.\.(\/|$)/ { bad = 1 }
      END { exit bad }' "$WORK/archive-names.txt"; then
    echo "it holds entries outside its one directory, $ARCHIVE_TOP/"
  fi
}

# The archive's own SHA256SUMS: every listed file matches, and no file is unlisted.
check_bundle_sums() {
  local sums=$BUNDLE/SHA256SUMS status listed matched unlisted
  if [[ ! -f $sums || -L $sums ]]; then
    problem "no SHA256SUMS in the archive"
    return 1
  fi
  sed -E 's/^[0-9a-f]{64} [ *]//' "$sums" | sort >"$WORK/sums-names.txt"
  if grep -qE '^/|(^|/)\.\.(/|$)' "$WORK/sums-names.txt"; then
    problem "SHA256SUMS names a file outside the archive"
    return 1
  fi
  (cd "$BUNDLE" && sha256sum --check --strict SHA256SUMS) >"$WORK/sums-check.txt" 2>&1
  status=$?
  record sha256sums-check.txt <"$WORK/sums-check.txt"
  listed=$(grep -c . "$sums")
  matched=$(grep -c ': OK$' "$WORK/sums-check.txt")
  unlisted=$(cd "$BUNDLE" && find . -type f ! -path ./SHA256SUMS -printf '%P\n' | sort | comm -13 "$WORK/sums-names.txt" - | head -n 5 | tr '\n' ' ')
  report "SHA256SUMS: $matched of $listed listed files match; files it does not list: ${unlisted:-none}"
  if ((status != 0)) || [[ $matched != "$listed" ]]; then
    problem "SHA256SUMS does not match the extracted files (records/sha256sums-check.txt)"
    return 1
  fi
  if [[ -n $unlisted ]]; then
    problem "the archive holds files its SHA256SUMS does not list: $unlisted"
    return 1
  fi
}

# Who built this, settled before anything from the archive executes: the source
# revision in the archive's own build-info.json, which --expect-revision, the
# manifest and the CI record beside the archive must all name. False, with
# IDENTITY_REFUSAL saying why, when they do not; nothing from the archive runs then.
establish_identity() {
  local info=$BUNDLE/build-info.json binary=$BUNDLE/bin/gitturtle manifest=$ARCHIVE.manifest.json
  local provenance=${ARCHIVE%/*}/ci-provenance.json other difference
  local -a differs=() agreed=()
  if [[ ! -f $info || -L $info ]] || ! jq -e 'type == "object"' "$info" >/dev/null 2>&1; then
    differs+=("the archive has no build-info.json that names its source revision")
  else
    BUNDLE_REVISION=$(jq -r '.compiled_identity.source_revision // ""' "$info" 2>/dev/null)
    [[ $BUNDLE_REVISION =~ ^[0-9a-f]{40}$ ]] || differs+=("build-info.json in the archive names no source revision")
    if [[ -f $binary && ! -L $binary ]]; then
      BUNDLE_BINARY_SHA256=$(sha256_of "$binary")
      [[ $(jq -r '.binary_sha256 // ""' "$info" 2>/dev/null) == "$BUNDLE_BINARY_SHA256" ]] ||
        differs+=("build-info.json in the archive gives another SHA-256 than bin/gitturtle has")
    fi
  fi
  if [[ -n $EXPECT_REVISION ]]; then
    if [[ -n $BUNDLE_REVISION && $BUNDLE_REVISION == "$EXPECT_REVISION"* ]]; then
      agreed+=("--expect-revision $EXPECT_REVISION")
    else
      differs+=("--expect-revision $EXPECT_REVISION, but the archive's build-info.json names ${BUNDLE_REVISION:-none}")
    fi
  fi
  if [[ -f $manifest && ! -L $manifest ]]; then
    other=$(jq -r '.package.compiled_identity.source_revision // ""' "$manifest" 2>/dev/null)
    if [[ -n $BUNDLE_REVISION && $other == "$BUNDLE_REVISION" ]]; then
      agreed+=("the manifest")
    else
      differs+=("the manifest names revision ${other:-none}, the archive's build-info.json ${BUNDLE_REVISION:-none}")
    fi
  fi
  if [[ -f $provenance && ! -L $provenance && $(jq -r '.archive // ""' "$provenance" 2>/dev/null) == "$ARCHIVE_NAME" ]]; then
    other=$(jq -r '.revision // ""' "$provenance" 2>/dev/null)
    if [[ -n $BUNDLE_REVISION && $other == "$BUNDLE_REVISION" ]]; then
      agreed+=("ci-provenance.json")
    else
      differs+=("ci-provenance.json names revision ${other:-none}, the archive's build-info.json ${BUNDLE_REVISION:-none}")
    fi
  fi
  if ((${#differs[@]} == 0)); then
    report "identity: source revision $BUNDLE_REVISION, read from build-info.json in the archive before anything from it ran; named also by: $(printf '%s, ' "${agreed[@]:-nothing else (no --expect-revision, manifest or CI record)}" | sed 's/, $//')"
    return 0
  fi
  for difference in "${differs[@]}"; do problem "identity: $difference"; done
  refuse_identity "${differs[0]}"
  return 1
}
# From here on nothing from the archive runs: not its executable, not its
# installer, not the package build, not the desktop part.
refuse_identity() {
  [[ -n $IDENTITY_REFUSAL ]] || IDENTITY_REFUSAL="identity not established: $1"
  ARCHIVE_TRUSTED=no
}

check_build_info() {
  local binary=$BUNDLE/bin/gitturtle status target
  if [[ ! -f $binary || ! -x $binary ]]; then
    problem "no executable bin/gitturtle in the archive"
    return
  fi
  report "executable: bin/gitturtle, $(stat -c %s -- "$binary") bytes, sha256 $BUNDLE_BINARY_SHA256"
  timeout 30 env "${DISPOSABLE_ENV[@]}" "$binary" --build-info >"$WORK/build-info.json" 2>"$WORK/build-info.err"
  status=$?
  if ((status != 0)) || ! jq -e 'type == "object"' "$WORK/build-info.json" >/dev/null 2>&1; then
    problem "bin/gitturtle --build-info did not answer (exit $status, this machine is $(uname -m)): $(head -n 1 "$WORK/build-info.err")"
    : >"$WORK/build-info.json"
    return
  fi
  record build-info.json <"$WORK/build-info.json"
  BUILD_REVISION=$(jq -r '.source_revision // ""' "$WORK/build-info.json")
  target=$(jq -r '.target // ""' "$WORK/build-info.json")
  report "--build-info: $(jq -r '"version \(.version), source_revision \(.source_revision), source_tree \(.source_tree), target \(.target), profile \(.profile), \(.rustc), build_unix_seconds \(.build_unix_seconds)"' "$WORK/build-info.json")"
  [[ $target == "$(uname -m)-"* ]] || problem "the executable targets $target, and this machine is $(uname -m)"
  # The executable has now spoken for itself, once and in the disposable home. If it
  # is not the build its archive said it was, nothing else from the archive runs.
  if [[ $BUILD_REVISION != "$BUNDLE_REVISION" ]]; then
    problem "identity: the executable reports revision ${BUILD_REVISION:-none}, the archive's build-info.json $BUNDLE_REVISION"
    refuse_identity "the executable reports another revision than its archive's build-info.json"
  elif [[ -n $EXPECT_REVISION ]]; then
    report "--expect-revision $EXPECT_REVISION: the executable's source revision"
  fi
  if ! jq -e --slurpfile compiled "$WORK/build-info.json" --arg sum "$BUNDLE_BINARY_SHA256" \
    '.compiled_identity == $compiled[0] and .binary_sha256 == $sum' "$BUNDLE/build-info.json" >/dev/null 2>&1; then
    problem "the archive's build-info.json does not describe this executable (its compiled_identity or binary_sha256 differs)"
    refuse_identity "the archive's build-info.json does not describe its executable"
  fi
}

check_libraries() {
  local missing
  if ! have ldd || ! timeout 30 ldd "$BUNDLE/bin/gitturtle" >"$WORK/ldd.txt" 2>&1; then
    problem "ldd could not read bin/gitturtle: $(head -n 1 "$WORK/ldd.txt" 2>/dev/null)"
    return
  fi
  record ldd.txt <"$WORK/ldd.txt"
  missing=$(grep -c 'not found' "$WORK/ldd.txt")
  report "ldd: $(grep -c . "$WORK/ldd.txt") lines, $missing \"not found\" (records/ldd.txt)"
  ((missing == 0)) || problem "ldd: $(grep 'not found' "$WORK/ldd.txt" | head -n 3 | tr -s '\t\n' '  ')"
}

check_manifest() {
  local manifest=$ARCHIVE.manifest.json line differs=no
  if [[ ! -f $manifest || -L $manifest ]]; then
    uncovered "the manifest's fields: no $ARCHIVE_NAME.manifest.json beside the archive (a file, not a link)"
    return
  fi
  if ! jq -e 'type == "object"' "$manifest" >/dev/null 2>&1; then
    problem "$ARCHIVE_NAME.manifest.json is not a JSON object"
    return
  fi
  record manifest.json <"$manifest"
  report "manifest: $(jq -r '"format \(.format), archive \(.archive), \(.archive_bytes) bytes, target \(.package.target), packaged_from_revision \(.package.packaged_from_revision) (\(.package.packaging_tree_status)), distribution \(.package.distribution), signing \(.package.signing), notices to review \(.package.license_review_required | length)"' "$manifest")"
  while IFS= read -r line; do
    [[ -n $line ]] || continue
    problem "manifest: $line"
    differs=yes
  done < <(jq -r --arg name "$ARCHIVE_NAME" --arg sum "$ARCHIVE_SHA256" --argjson bytes "$(stat -c %s -- "$ARCHIVE")" \
    --arg bundled "$(sha256_of "$BUNDLE/build-info.json")" --arg binary "$BUNDLE_BINARY_SHA256" \
    --slurpfile package "$BUNDLE/build-info.json" --slurpfile compiled "$WORK/build-info.json" '
      (if .archive != $name then "archive is \(.archive), not this file" else empty end),
      (if .archive_sha256 != $sum then "archive_sha256 is not the SHA-256 of the archive" else empty end),
      (if .archive_bytes != $bytes then "archive_bytes is \(.archive_bytes), and the file has \($bytes)" else empty end),
      (if .package_manifest_sha256 != $bundled then "package_manifest_sha256 is not the SHA-256 of build-info.json in the archive" else empty end),
      (if .package != $package[0] then "package is not build-info.json in the archive" else empty end),
      (if .package.binary_sha256 != $binary then "package.binary_sha256 is not the SHA-256 of bin/gitturtle" else empty end),
      (if ($compiled | length) == 1 and .package.compiled_identity != $compiled[0]
       then "package.compiled_identity is not what --build-info prints" else empty end)' "$manifest" 2>&1)
  [[ $differs == no ]] || return 0
  # The compiled identity is compared with what the executable says only when the
  # executable ran and answered; the line says which of the two happened.
  if [[ -s $WORK/build-info.json ]]; then
    report "manifest: its archive name, size and SHA-256, its package block, and the executable's SHA-256 and compiled identity equal what was extracted and run"
  else
    report "manifest: its archive name, size and SHA-256, its package block and the executable's SHA-256 equal what was extracted; its compiled identity was not compared with the executable's own answer, because the executable gave none (it was not run, or did not answer --build-info)"
  fi
}

# The CI job's own record, when it was downloaded with the archive.
check_provenance() {
  local provenance=${ARCHIVE%/*}/ci-provenance.json
  if [[ ! -f $provenance || -L $provenance ]] || ! jq -e 'type == "object"' "$provenance" >/dev/null 2>&1; then
    report "ci-provenance.json: not beside the archive (optional)"
    return
  fi
  if [[ $(jq -r '.archive // ""' "$provenance") != "$ARCHIVE_NAME" ]]; then
    report "ci-provenance.json: names another archive, not used"
    return
  fi
  record ci-provenance.json <"$provenance"
  report "ci-provenance.json: $(jq -r '"run \(.run_id) attempt \(.run_attempt), event \(.event), revision \(.revision), target \(.target)"' "$provenance")"
  [[ $(jq -r '.archive_sha256 // ""' "$provenance") == "$ARCHIVE_SHA256" ]] || problem "ci-provenance.json: archive_sha256 is not the SHA-256 of the archive"
  [[ $(jq -r '.input_binary_sha256 // ""' "$provenance") == "$BUNDLE_BINARY_SHA256" ]] || problem "ci-provenance.json: input_binary_sha256 is not the SHA-256 of bin/gitturtle"
  [[ -z $BUILD_REVISION || $(jq -r '.revision // ""' "$provenance") == "$BUILD_REVISION" ]] || problem "ci-provenance.json: revision is not the executable's source revision"
}

check_archive() {
  local unsafe unpacked entries
  section b archive "the archive, its checksums and what its executable says"
  : >"$WORK/build-info.json"
  report "archive: $ARCHIVE_NAME, $(stat -c %s -- "$ARCHIVE") bytes"
  report "sha256: $ARCHIVE_SHA256, the one in $ARCHIVE_NAME.sha256 (compared before anything else ran)"
  # Owners as numbers: a name can hold a space, which would move the size out of
  # the third column and let a member count as nothing.
  timeout 300 tar --numeric-owner -tzvf "$ARCHIVE" >"$WORK/archive-listing.txt" 2>/dev/null || : >"$WORK/archive-listing.txt"
  timeout 300 tar -tzf "$ARCHIVE" >"$WORK/archive-names.txt" 2>/dev/null || : >"$WORK/archive-names.txt"
  ARCHIVE_TOP=$(sed -n '1s|/.*||p' "$WORK/archive-names.txt")
  unsafe=$(archive_listing_problem)
  if [[ -n $unsafe ]]; then
    problem "not extracted: $unsafe"
    result
    return
  fi
  # A size counts only as a plain number of at most SIZE_DIGITS digits, and it is
  # held to that before anything is computed from it: awk stops at the first that
  # is not, and the shell compares the sum only once the sum is such a number too.
  # A member can list as 2^62 bytes for nothing (a sparse one is all hole), and two
  # of them add up to a total that 64-bit arithmetic reads as negative, and so as
  # within the limit. An archive refused here is not extracted, so nothing of it is
  # hashed or read afterwards either.
  unpacked=$(awk -v digits="$SIZE_DIGITS" '
    $2 !~ /^[0-9]+\/[0-9]+$/ || $3 !~ /^[0-9]+$/ || length($3) > digits { unread = 1; exit }
    { bytes += $3 }
    END { if (unread) print "unread"; else printf "%.0f", bytes }' "$WORK/archive-listing.txt")
  entries=$(grep -c . "$WORK/archive-names.txt")
  if [[ ! $unpacked =~ ^[0-9]+$ ]]; then
    problem "not extracted: a size in its listing is not a plain number of at most $SIZE_DIGITS digits, so what it would unpack to is not counted"
    result
    return
  fi
  if ((${#unpacked} > SIZE_DIGITS)) || ((unpacked > MAX_UNPACKED || entries > MAX_ENTRIES)); then
    problem "not extracted: it would unpack to $unpacked bytes in $entries entries, over the limit of $MAX_UNPACKED bytes and $MAX_ENTRIES entries"
    result
    return
  fi
  mkdir -p "$WORK/archive"
  if ! timeout 600 tar -xzf "$ARCHIVE" -C "$WORK/archive" --no-same-owner 2>"$WORK/tar.log"; then
    problem "tar could not extract the archive: $(tail -n 1 "$WORK/tar.log")"
    result
    return
  fi
  BUNDLE=$WORK/archive/$ARCHIVE_TOP
  report "extracted: $(find "$BUNDLE" -type f | grep -c .) files under $ARCHIVE_TOP/ ($unpacked bytes, within the limit of $MAX_UNPACKED)"
  if check_bundle_sums; then
    # The executable runs (--build-info) and is loaded (ldd) only once the archive
    # is the build it is expected to be; the manifest and the CI record are read
    # either way.
    if establish_identity; then
      ARCHIVE_TRUSTED=yes
      check_build_info
      check_libraries
    fi
    check_manifest
    check_provenance
  fi
  result
}

check_install() {
  local installer=$BUNDLE/install.py status entry installed validation
  section c install "the archive's installer, into a disposable home"
  if [[ $ARCHIVE_TRUSTED != yes ]]; then
    skipped "${IDENTITY_REFUSAL:-the archive did not pass its own checksums}, so nothing from it runs"
    return
  fi
  if ! have python3; then
    skipped "python3 is not installed, and the archive's installer is install.py"
    return
  fi
  if [[ ! -f $installer ]]; then
    problem "no install.py in the archive"
    result
    return
  fi
  mkdir -p "$DISPOSABLE_HOME" "$WORK/tmp"
  timeout 300 env "${DISPOSABLE_ENV[@]}" python3 "$installer" >"$WORK/install.txt" 2>&1
  status=$?
  record install.txt <"$WORK/install.txt"
  report "install.py: exit $status, with HOME and XDG_DATA_HOME under <results>/work/home (records/install.txt)"
  if ((status != 0)); then
    problem "install.py failed (exit $status): $(tail -n 1 "$WORK/install.txt")"
    result
    return
  fi
  (cd "$DISPOSABLE_HOME" && find . \( -type f -o -type l \) -printf '%P\n' | sort) >"$WORK/installed.txt"
  record installed-files.txt <"$WORK/installed.txt"
  report "installed: $(grep -c . "$WORK/installed.txt") files (records/installed-files.txt); outside gitturtle/licenses/:"
  while IFS= read -r entry; do report "  $entry"; done < <(grep -v '/licenses/' "$WORK/installed.txt")

  installed=$DISPOSABLE_HOME/.local/bin/gitturtle
  if [[ -f $installed && $(sha256_of "$installed") == "$BUNDLE_BINARY_SHA256" ]]; then
    INSTALLED_BINARY=$installed
    report "installed executable: .local/bin/gitturtle, sha256 $BUNDLE_BINARY_SHA256, the archive's"
  else
    problem "the installed .local/bin/gitturtle is not the archive's executable"
  fi
  entry=$DISPOSABLE_HOME/.local/share/applications/$APP_ID.desktop
  if [[ ! -f $entry ]]; then
    problem "no applications/$APP_ID.desktop was installed"
  elif ! have desktop-file-validate; then
    uncovered "the desktop entry's validation: desktop-file-validate is not installed"
  else
    validation=$(timeout 30 desktop-file-validate "$entry" 2>&1)
    status=$?
    if ((status == 0)) && [[ -z $validation ]]; then
      report "desktop-file-validate: passed for $APP_ID.desktop (Exec=$(sed -n 's/^Exec=//p' "$entry"))"
    else
      problem "desktop-file-validate: $validation"
    fi
  fi
  result
}

check_package() {
  local test=$REPO_ROOT/packaging/aur/test-local.sh tool status
  section d package "the gitturtle-bin package, built from this archive"
  skip_gate || return
  if [[ $ARCHIVE_TRUSTED != yes ]]; then
    skipped "${IDENTITY_REFUSAL:-the archive did not pass its own checksums}, so nothing from it runs"
    return
  fi
  if [[ ! -f $test ]]; then
    skipped "packaging/aur/test-local.sh is not beside this script: run it from a source checkout"
    return
  fi
  for tool in makepkg bsdtar python3 desktop-file-validate; do
    if ! have "$tool"; then
      skipped "$tool is not installed"
      return
    fi
  done
  # It installs nothing, downloads nothing and works only in the directory it is given.
  timeout 900 bash "$test" "$ARCHIVE" "$WORK/package" >"$WORK/package.txt" 2>&1
  status=$?
  record package.txt <"$WORK/package.txt"
  report "packaging/aur/test-local.sh: exit $status (records/package.txt); its last line:"
  report "  $(tail -n 1 "$WORK/package.txt")"
  ((status == 0)) || problem "packaging/aur/test-local.sh failed (exit $status)"
  result
}

# ---------------------------------------------------------------- checks: the desktop part

# The state the desktop part must leave as it found it, one "key value" line each.
# The wallpaper's path may name a private picture, so only its digest is kept.
# The bytes a path leads to, through a link too, and the link's own text.
file_mark() {
  local mark=absent
  [[ ! -f $1 ]] || mark=$(sha256sum <"$1" | cut -c1-16)
  [[ ! -L $1 ]] || mark+=" link:$(readlink -- "$1" | sha256sum | cut -c1-16)"
  printf '%s' "$mark"
}
desktop_snapshot() {
  local file toggle
  printf 'workspace %s\n' "$(active_workspace | cut -d' ' -f1)"
  for toggle in "${SKIP_TOGGLES[@]}"; do printf 'toggle %s %s\n' "$toggle" "$([[ -f $TOGGLES/$toggle ]] && echo on || echo off)"; done
  printf 'theme %s\n' "$(theme_now)"
  printf 'wallpaper %s\n' "$(wallpaper_now | sha256sum | cut -c1-16)"
  printf 'font %s\n' "$(timeout 5 omarchy-font-current 2>/dev/null)"
  printf 'notifications-silenced %s\n' "$(dnd_state)"
  printf 'stay-awake %s\n' "$([[ -f $STAY_AWAKE ]] && echo on || echo off)"
  monitor_lines | sed 's/^/monitor /'
  for file in "${FONT_FILES[@]}"; do printf 'file ~/.config/%s %s\n' "$file" "$(file_mark "$HOME/.config/$file")"; done
}

# True when the plan lets the named check run. A check the plan lets run can still
# skip itself, so what the text below says of it is conditional too.
planned() { ! skip_named "$1" && [[ $1 != themes || -z $THEME_BLOCKER ]]; }

# How to put back by hand what this run changes, with this run's own values: a
# step only for a change the plan lets the run make, in the script's own order,
# and each only where the desktop differs from the value given, which is how the
# run found it. A step taken where nothing was changed would itself change
# something: setting the current theme again moves on to its next wallpaper, and
# omarchy-font-set rewrites the font files and replaces a link at three of them.
# The skip toggles and the wallpaper are told by what manifest.txt lists: a
# toggle only if this run made it, and the wallpaper's path, which the records
# must not hold.
by_hand() {
  local theme font scales manifest=$RESTORE/manifest.txt
  local -a steps=()
  theme=$(sed -n 's/^theme //p' <<<"$DESKTOP_START")
  font=$(sed -n 's/^font //p' <<<"$DESKTOP_START")
  scales=$(sed -n 's/^monitor \([^ ]*\) .* scale \([^ ]*\) at .*/\1 at \2/p' <<<"$DESKTOP_START" | tr '\n' ',' | sed 's/,$//; s/,/, /g')
  if [[ $LEAVES_WORKSPACE != no && $START_WORKSPACE =~ ^[0-9]+$ ]]; then
    steps+=("  the workspace          only if the desktop is not on workspace $START_WORKSPACE, where the run
                         started: SUPER + $(((START_WORKSPACE - 1) % 10 + 1)), its number among this monitor's ten")
  fi
  if planned themes && [[ -n $theme ]]; then
    steps+=("  the theme              only if ~/.local/state/omarchy/current/theme.name does not
                         read \"$theme\" (setting the current theme again moves on
                         to its next wallpaper): omarchy-theme-set \"$theme\"")
    if grep -q $'^wallpaper\t' "$manifest" 2>/dev/null; then
      steps+=("  the wallpaper          after the theme, and only if ~/.local/state/omarchy/current/background
                         does not lead to the picture that the \"wallpaper\" line of
                         $manifest names:
                         omarchy-theme-bg-set and that picture's path")
    fi
    steps+=("  the skip toggles       only what $manifest
                         lists as \"toggle ... made by this run\": remove each such
                         file, and then the toggles directory, if a line there says
                         \"directory ... made by this run\" (rmdir: it goes only
                         when nothing else is in it). A toggle that was on before
                         the run is not listed, and stays on")
  fi
  if planned font && [[ -n $font ]]; then
    steps+=("  the font               only if omarchy-font-current does not print \"$font\"
                         (omarchy-font-set rewrites the font files, and replaces a
                         link at three of them, when the font is the same too):
                         omarchy-font-set \"$font\""
      "  the font files         after the font, because omarchy-font-set rewrites them, and
                         only where a file differs from its saved copy: for each font-N line of
                         $manifest, copy
                         $RESTORE/font-N over the file the line
                         names, and remove a file a line calls \"absent\". Before the
                         font check has saved the files there is no such line, and
                         the files are as found")
  fi
  if planned scales && [[ -n $scales ]]; then
    steps+=("  the monitor's scale    only if hyprctl monitors shows another scale than the run
                         found ($scales): hyprctl reload")
  fi
  if [[ $START_DND == off ]]; then
    steps+=("  notification silencing only if \"omarchy-shell notifications isDnd\" prints on (it
                         printed off): omarchy-shell notifications setDnd off")
  fi
  if [[ $START_AWAKE == no ]]; then
    steps+=("  stay-awake             only if ~/.local/state/omarchy/indicators/stay-awake exists
                         (it did not): omarchy-toggle-idle allow-idle")
  fi
  if ((${#steps[@]} == 0)); then
    echo "The script puts back what it changes itself, on every exit. This run changes"
    echo "nothing that would need putting back by hand after a kill it could not catch"
    echo "(kill -9) or a power loss."
  else
    echo "The script puts back what it changes itself, on every exit. After a kill it could"
    echo "not catch (kill -9) or a power loss, put back by hand what is listed here, which"
    echo "is what this run changes: in this order, and each only if it differs from the"
    echo "value given, which is how the run found it."
    printf '%s\n' "${steps[@]}"
  fi
  echo "A GitTurtle window or a terminal this run opened can simply be closed."
}
# The starting state, written where a hard stop still leaves it readable: in the
# records, which are packed, and beside the saved copies in work/restore. The
# terminal is told where both are and how to put the desktop back by hand.
record_desktop_start() {
  local wallpaper
  DESKTOP_START=$(desktop_snapshot)
  restore_note "# What this run saved or made: kind, path, note, separated by tabs. desktop-start.txt beside this file is the desktop as it was found."
  # The wallpaper's path may name a private picture, so it stays out of the
  # records, which are packed: the by-hand text points at this line instead.
  wallpaper=$(wallpaper_now)
  if planned themes && [[ -f $wallpaper && $wallpaper != *[[:cntrl:]]* ]]; then
    restore_note "wallpaper	$wallpaper	the wallpaper as found; omarchy-theme-bg-set with this path puts it back"
  fi
  {
    echo "The desktop as the desktop part found it ($(date -u +%FT%TZ)):"
    printf '%s\n' "$DESKTOP_START"
    echo
    by_hand
  } | tee -a "$RESTORE/desktop-start.txt" | record desktop-start.txt
  say ""
  say "The desktop as it is now is written in $RECORDS/desktop-start.txt and in"
  say "$RESTORE/desktop-start.txt; saved copies go to $RESTORE."
  by_hand | while IFS= read -r line; do say "$line"; done
  say ""
}

# Records the starting state, moves to a workspace of the run's own, then makes
# the two changes every picture needs: no notification, no screensaver.
begin_desktop_part() {
  local now windows=""
  # The plan stands: a part it said would not run stays off, whatever has changed
  # since (the charger plugged in during the package build). A change for the
  # worse still stops it.
  [[ -z $DESKTOP_REFUSAL ]] || return 0
  now=$(desktop_refusal)
  if [[ -n $now ]]; then
    DESKTOP_REFUSAL="$now (this changed after the plan was printed)"
    return 0
  fi
  # Without an installed executable of an established identity there is nothing to
  # show, so nothing on the desktop is changed.
  [[ -z $IDENTITY_REFUSAL && -n $INSTALLED_BINARY ]] || return 0
  START_FOCUS=$(active_address)
  read -r START_WORKSPACE windows <<<"$(active_workspace)"
  # The run leaves a workspace that holds a window, and stays on one that holds none.
  LEAVES_WORKSPACE=yes
  [[ $windows != 0 ]] || LEAVES_WORKSPACE=no
  START_DND=$(dnd_state)
  [[ $START_DND == on || $START_DND == off ]] || START_DND=""
  START_AWAKE=no
  [[ ! -f $STAY_AWAKE ]] || START_AWAKE=yes
  record_desktop_start
  register_restore restore_focus
  register_restore restore_workspace
  if ! go_to_empty_workspace; then
    run_restore restore_workspace
    DESKTOP_REFUSAL="the desktop could not be moved to a workspace without a window, so no picture could be kept clear of your windows"
    report "NOTE $DESKTOP_REFUSAL: the desktop part does not run"
    run_restore restore_focus
    return 0
  fi
  # Every picture needs notifications known to be silenced. Where the shell does
  # not confirm that now, no later check could take one, so the desktop part ends
  # here as it does above: what was set is undone, and nothing else is changed.
  register_restore restore_notifications
  if ! silence_notifications; then
    now=$(dnd_state | tr -cd 'a-z' | cut -c1-12)
    [[ -z $now ]] && now="gave no answer" || now="answered \"$now\", not \"on\""
    run_restore restore_notifications
    run_restore restore_workspace
    DESKTOP_REFUSAL="notification silencing could not be confirmed (asked whether notifications are silenced, Omarchy's shell $now), so a notification could be drawn over the window in a picture"
    report "NOTE $DESKTOP_REFUSAL: the desktop part does not run"
    run_restore restore_focus
    return 0
  fi
  register_restore restore_idle
  keep_awake || report "NOTE stay-awake could not be turned on: the screensaver may start during the run"
}

# The RESULT of a desktop check that cannot run at all; false then. An archive
# whose identity is not established shows nothing, whatever the desktop allows.
desktop_gate() {
  if [[ -n $IDENTITY_REFUSAL ]]; then
    skipped "$IDENTITY_REFUSAL, so nothing from the archive runs"
    return 1
  fi
  if [[ -n $DESKTOP_REFUSAL ]]; then
    skipped "$DESKTOP_REFUSAL"
    return 1
  fi
}
# The RESULT of a check that cannot run without the app's window; false then.
window_gate() {
  desktop_gate || return 1
  if session_locked; then
    skipped "the session is locked"
    return 1
  fi
  if [[ -z $APP_PID || -z $(app_window .address) ]]; then
    skipped "GitTurtle's window is not open (see the launch check)"
    return 1
  fi
  silence_notifications
  return 0
}

# The only repository the app is shown: invented people, fixed dates.
make_demo_repository() {
  local maker=$SCRIPT_DIR/create-demo-repo.py
  local -a run_git
  DEMO=$WORK/Aurora
  if have python3 && [[ -f $maker ]]; then
    if timeout 120 env PYTHONDONTWRITEBYTECODE=1 python3 "$maker" --output "$DEMO" >"$WORK/demo.txt" 2>&1; then
      report "demo repository: made by scripts/create-demo-repo.py"
      return 0
    fi
    report "demo repository: scripts/create-demo-repo.py failed ($(tail -n 1 "$WORK/demo.txt")); using plain git commands"
    DEMO=$WORK/Aurora-plain
  fi
  have git || return 1
  run_git=(env GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 "GIT_AUTHOR_NAME=Maya Chen" "GIT_COMMITTER_NAME=Maya Chen"
    GIT_AUTHOR_EMAIL=maya@aurora.example.invalid GIT_COMMITTER_EMAIL=maya@aurora.example.invalid
    GIT_AUTHOR_DATE=2026-01-05T10:00:00Z GIT_COMMITTER_DATE=2026-01-05T10:00:00Z git -C "$DEMO")
  mkdir -p "$DEMO/src" && "${run_git[@]}" init --quiet --initial-branch=main || return 1
  printf '# Aurora\n\nA demo repository.\n' >"$DEMO/README.md"
  "${run_git[@]}" add --all && "${run_git[@]}" commit --quiet --message "Start the Aurora demo" || return 1
  printf 'export function greet(name: string) {\n  return "Hello, " + name;\n}\n' >"$DEMO/src/greet.ts"
  "${run_git[@]}" add --all && "${run_git[@]}" commit --quiet --message "Add a greeting" || return 1
  printf '\n## Use\n\nCall greet with a name.\n' >>"$DEMO/README.md"
  "${run_git[@]}" add --all && "${run_git[@]}" commit --quiet --message "Describe how to use it" || return 1
  report "demo repository: made with three git commits (scripts/create-demo-repo.py or python3 is missing)"
}

check_launch() {
  local started appeared
  section e launch "first launch of the installed executable"
  desktop_gate || return
  if [[ -z $INSTALLED_BINARY ]]; then
    skipped "no installed executable (see the install check)"
    return
  fi
  if ! make_demo_repository; then
    problem "the demo repository could not be made"
    result
    return
  fi
  register_restore stop_app
  started=$(now_ms)
  if ! launch_app; then
    problem "an app this script started earlier could not be ended, so no other was started"
    run_restore stop_app
    result
    return
  fi
  if ! wait_for_app_window; then
    problem "no GitTurtle window within $WINDOW_WAIT s (the process $(process_running "$APP_PID" && echo "still runs" || echo "has ended"))"
    run_restore stop_app
    result
    return
  fi
  appeared=$(($(now_ms) - started))
  report "launch: XDG_CONFIG_HOME new and empty, HOME the session's (the app reads Omarchy's theme from ~/.local/state/omarchy/current)"
  report "window: listed by Hyprland $appeared ms after the launch: $(app_window '"class \(.class), xwayland \(.xwayland), floating \(.floating), \(.size[0])x\(.size[1]), workspace \(.workspace.id)"')"
  [[ $(app_window .class) == "$APP_ID" ]] || problem "the window's class is $(app_window .class), not $APP_ID"
  [[ $(readlink "/proc/$APP_PID/exe" 2>/dev/null) == "$INSTALLED_BINARY" ]] || problem "the process is not running the installed executable"
  nap "$SETTLE"
  picture first-launch || problem "no picture of the first launch"
  report_preferences "after the first launch"
  (($(panic_lines) == 0)) || problem "the app's output holds a panic line (records/app-output.txt at the end)"
  result
}
# What the app saved in its new configuration home: which theme and code font a
# first launch chose by itself.
report_preferences() {
  local saved=$LAUNCH/config/gitturtle/preferences.json
  if [[ -f $saved ]]; then
    record preferences.json <"$saved"
    report "preferences $1: $(jq -r '.settings | "theme \(.theme), system_code_font \(.system_code_font)"' "$saved" 2>/dev/null) (records/preferences.json)"
  else
    report "preferences $1: the app has saved none yet"
  fi
}

check_themes() {
  local name slug status started unread taken=0 passed=0
  local -a names=()
  section f themes "every Omarchy theme, live"
  skip_gate || return
  window_gate || return
  START_THEME=$(theme_now)
  if [[ -z $START_THEME ]] || ! have omarchy-theme-list || ! have omarchy-theme-set; then
    START_THEME=""
    skipped "Omarchy names no current theme, or omarchy-theme-list or omarchy-theme-set is missing"
    return
  fi
  # Asked again here: what was true for the plan may have changed since.
  [[ -n $THEME_BLOCKER ]] || THEME_BLOCKER=$(theme_blocker)
  if [[ -n $THEME_BLOCKER ]]; then
    START_THEME=""
    skipped "$THEME_BLOCKER"
    return
  fi
  register_restore restore_skip_toggles
  if ! skip_toggles_on; then
    run_restore restore_skip_toggles
    START_THEME=""
    skipped "an Omarchy skip toggle could not be turned on under ~/.local/state/omarchy/toggles, so a switch could reach an editor"
    return
  fi
  if ((${#THEME_UNKNOWNS[@]} == 0)); then
    unread="no hook of the owner's runs"
  else
    unread="run with --themes-anyway although every switch also runs, unread by this script: $(printf '%s; ' "${THEME_UNKNOWNS[@]}" | sed 's/; $//')"
  fi
  report "themes: Omarchy's skip toggles are on for the switches: ${SKIP_TOGGLES[*]:-none exist here} (made by this run: ${SKIP_TOGGLES_MADE[*]:-none}); theme sync to herdr machines is off; $unread; the browser colour goes through Omarchy's passwordless sudo rule"
  START_WALLPAPER=$(wallpaper_now)
  mapfile -t names < <(timeout 10 omarchy-theme-list 2>/dev/null | grep .)
  report "themes: omarchy-theme-list names ${#names[@]}; starting from $START_THEME; each picture is taken $SETTLE s after omarchy-theme-set returns"
  if ((${#names[@]} == 0)); then
    run_restore restore_skip_toggles
    START_THEME=""
    problem "omarchy-theme-list named no theme, so none was checked"
    result
    return
  fi
  register_restore restore_theme
  for name in "${names[@]}"; do
    slug=$(theme_slug "$name")
    if [[ ! $slug =~ ^[a-z0-9][a-z0-9._-]*$ ]]; then
      report "theme $slug: passed over: not a plain name"
      passed=$((passed + 1))
      continue
    fi
    if ! theme_has_colors "$slug"; then
      report "theme $slug: passed over: its directory has no colors.toml, and omarchy-theme-set would leave the desktop without colours"
      passed=$((passed + 1))
      continue
    fi
    if session_locked; then
      problem "the session locked before theme $slug"
      break
    fi
    if [[ $(theme_now) == "$slug" ]]; then
      report "theme $slug: already the current theme, not set again (that would move on to its next wallpaper)"
    elif ! skip_toggles_all_on; then
      problem "an Omarchy skip toggle was turned off during the run; the remaining themes were not tried"
      break
    else
      started=$(now_ms)
      set_theme "$slug"
      status=$?
      if [[ $(theme_now) != "$slug" || ! -f $OMARCHY_CURRENT/theme/colors.toml ]]; then
        problem "omarchy-theme-set $slug (exit $status) did not apply; the remaining themes were not tried"
        break
      fi
      wait_theme_intro
      report "theme $slug: omarchy-theme-set and its intro took $(($(now_ms) - started)) ms"
    fi
    nap "$SETTLE"
    report "theme $slug: $(grep -m1 '^background' "$OMARCHY_CURRENT/theme/colors.toml" | tr -d ' '), light.mode $([[ -f $OMARCHY_CURRENT/theme/light.mode ]] && echo present || echo absent)"
    if picture "theme-$slug"; then taken=$((taken + 1)); else problem "no picture of theme $slug"; fi
  done
  run_restore restore_theme
  run_restore restore_skip_toggles
  nap "$SETTLE"
  picture "theme-restored-$START_THEME" || problem "no picture after the starting theme was restored"
  [[ $(theme_now) == "$START_THEME" ]] || problem "the theme is $(theme_now), not the starting $START_THEME"
  [[ ! -f $WORK/theme-set.log ]] || record theme-set-output.txt <"$WORK/theme-set.log"
  report "themes: $taken pictured, $passed passed over, of ${#names[@]}"
  ((passed == 0)) || uncovered "$passed of ${#names[@]} themes: passed over, each with its reason above"
  result
}

plain_font_name() { [[ $1 =~ ^[A-Za-z0-9][A-Za-z0-9\ ._+-]{0,79}$ ]]; }
listed_font() { timeout 10 omarchy-font-list 2>/dev/null | grep -Fxq -- "$1"; }
# The first other family whose first word differs, so that the change can be seen.
other_font() {
  local family first=""
  while IFS= read -r family; do
    [[ -n $family && $family != "$1" ]] || continue
    [[ -n $first ]] || first=$family
    if [[ ${family%% *} != "${1%% *}" ]]; then
      printf '%s' "$family"
      return
    fi
  done < <(timeout 10 omarchy-font-list 2>/dev/null)
  printf '%s' "$first"
}

check_font() {
  local other status hook
  section g font "omarchy-font-set, live"
  skip_gate || return
  window_gate || return
  if ! have omarchy-font-set || ! have omarchy-font-current; then
    skipped "omarchy-font-set or omarchy-font-current is missing"
    return
  fi
  START_FONT=$(timeout 5 omarchy-font-current 2>/dev/null)
  other=${FONT_CHOICE:-$(other_font "$START_FONT")}
  if [[ -z $START_FONT || -z $other || $other == "$START_FONT" ]]; then
    START_FONT=""
    skipped "omarchy-font-list names no other font than the current one"
    return
  fi
  # Both names are passed to omarchy-font-set, which puts them into sed
  # expressions and configuration files: only plain names, and for the font that
  # is set, only one that omarchy-font-list prints.
  if ! plain_font_name "$START_FONT" || ! plain_font_name "$other" || ! listed_font "$other"; then
    START_FONT=""
    skipped "the font to set or the current font is not a plain family name that omarchy-font-list prints; nothing was changed"
    return
  fi
  hook=$(own_hooks font-set | head -n 1)
  if [[ -n $hook ]]; then
    START_FONT=""
    skipped "a font-set hook under ~/.config/omarchy/hooks would run on the change, and the script cannot know what it does ($hook): move it aside for the run"
    return
  fi
  if ! save_font_files; then
    START_FONT=""
    skipped "${FONT_PROBLEM:-a file omarchy-font-set rewrites could not be saved first}; nothing was changed"
    return
  fi
  report "font: $START_FONT -> $other; ~/.config/fontconfig $([[ -d $HOME/.config/fontconfig ]] && echo "exists" || echo "does not exist yet (omarchy-font-set creates it)")"
  register_restore restore_font
  "${SESSION_ENV[@]}" timeout "$SWITCH_WAIT" omarchy-font-set "$other" </dev/null >"$WORK/font-set.log" 2>&1 3<&-
  status=$?
  ((status == 0)) || problem "omarchy-font-set \"$other\" exit $status: $(tail -n 1 "$WORK/font-set.log")"
  [[ $(timeout 5 omarchy-font-current 2>/dev/null) == "$other" ]] || problem "omarchy-font-current is $(timeout 5 omarchy-font-current 2>/dev/null) after the set"
  mirror_fontconfig
  silence_notifications # omarchy-font-set restarts the Omarchy shell, which holds that state
  nap "$SETTLE"
  report "font: fc-match monospace, in the app's environment: $(XDG_CONFIG_HOME=$LAUNCH/config fc-match monospace -f '%{family}\n' 2>/dev/null | head -n 1)"
  picture "font-$(theme_slug "$other")" || problem "no picture with the other font"
  run_restore restore_font
  silence_notifications
  nap "$SETTLE"
  picture font-restored || problem "no picture after the starting font was restored"
  [[ $(timeout 5 omarchy-font-current 2>/dev/null) == "$START_FONT" ]] || problem "the font is not the starting $START_FONT"
  record font-set-output.txt <"$WORK/font-set.log"
  result
}

check_floating() {
  local now
  section h floating "tiled and floating"
  skip_gate || return
  window_gate || return
  START_FLOATING=$(app_window .floating)
  picture "window-as-opened" || problem "no picture of the window as it opened"
  register_restore restore_floating
  toggle_floating "$APP_ADDRESS"
  nap "$SETTLE"
  now=$(app_window .floating)
  if [[ $now == "$START_FLOATING" ]]; then
    problem "Hyprland did not change the window's floating state ($now)"
  else
    picture "window-$([[ $now == true ]] && echo floating || echo tiled)" || problem "no picture after the toggle"
  fi
  now=$START_FLOATING
  run_restore restore_floating
  [[ $(app_window .floating) == "$now" ]] || problem "the window's floating state was not restored"
  nap "$SETTLE"
  result
}

check_scales() {
  local facts width height refresh x y transform scale reported taken=0 fractional=0
  local -a impossible=()
  section i scales "fractional scales"
  skip_gate || return
  window_gate || return
  facts=$(hypr_json monitors | jq -r --argjson id "$(app_window .monitor)" '.[] | select(.id == $id) | "\(.name) \(.width) \(.height) \(.refreshRate) \(.x) \(.y) \(.transform) \(.scale)"' 2>/dev/null)
  read -r MONITOR_NAME width height refresh x y transform MONITOR_START_SCALE <<<"$facts"
  if [[ ! $MONITOR_NAME =~ ^[A-Za-z0-9._-]+$ || ! $width$height =~ ^[0-9]+$ ]]; then
    MONITOR_NAME=""
    skipped "the window's monitor could not be read as a plain name and mode"
    return
  fi
  if [[ $transform != 0 ]]; then
    MONITOR_NAME=""
    skipped "the monitor is rotated (transform $transform), and a rule naming only mode, position and scale would turn it back"
    return
  fi
  MONITOR_MODE=${width}x${height}@$refresh
  MONITOR_POSITION=${x}x$y
  MONITOR_START_LINE=$(monitor_line)
  report "monitor: $MONITOR_START_LINE"
  register_restore restore_scale
  for scale in "${SCALES[@]}"; do
    if session_locked; then
      problem "the session locked before scale $scale"
      break
    fi
    if ! scale_divides_mode "$scale" "$width" "$height"; then
      report "scale $scale: not requested: ${width}x$height does not divide into whole logical pixels at $scale, so Hyprland would refuse it and choose another"
      impossible+=("$scale")
      continue
    fi
    if same_scale "$(monitor_scale)" "$scale"; then
      report "scale $scale: already the monitor's scale, not set again"
    else
      set_scale "$scale"
      nap "$SETTLE"
    fi
    reported=$(monitor_scale)
    if ! same_scale "$reported" "$scale"; then
      report "scale $scale: Hyprland reports $reported: refused, no picture"
      impossible+=("$scale")
      continue
    fi
    report "scale $scale: Hyprland reports $reported"
    if picture "scale-$scale"; then
      taken=$((taken + 1))
      [[ $scale != *.* ]] || fractional=$((fractional + 1))
    else
      problem "no picture at scale $scale"
    fi
  done
  run_restore restore_scale
  [[ $(monitor_line) == "$MONITOR_START_LINE" ]] || problem "the monitor is not as it was found"
  nap "$SETTLE"
  ((taken > 0)) || problem "no scale was pictured"
  if ((fractional == 0)); then
    uncovered "fractional scales: neither 1.25 nor 1.5 is possible on this monitor's mode, ${width}x$height"
  elif ((${#impossible[@]} > 0)); then
    uncovered "scale ${impossible[*]}: not possible on this monitor's mode, ${width}x$height"
  fi
  result
}

check_inactive() {
  local title=gitturtle-omarchy-check-$$ before deadline
  section j inactive "readability while another window has the focus"
  skip_gate || return
  window_gate || return
  if ! have xdg-terminal-exec; then
    skipped "xdg-terminal-exec, which opens Omarchy's default terminal, is missing"
    return
  fi
  before=$(hypr_json clients | jq -c '[.[].address]' 2>/dev/null)
  register_restore close_terminal
  # The terminal shows nothing: its command names the window and waits.
  "${SESSION_ENV[@]}" xdg-terminal-exec --title="$title" -- sh -c "printf '\\033]2;%s\\007' '$title'; exec sleep $TERMINAL_LIFE" </dev/null >/dev/null 2>&1 3<&- &
  TERMINAL_PID=$!
  deadline=$(($(now_ms) + 10000))
  while (($(now_ms) < deadline)) && [[ -z $TERMINAL_ADDRESS ]]; do
    nap 0.2
    # A window that was not there before, and that carries this run's title or pid.
    TERMINAL_ADDRESS=$(hypr_json clients | jq -r --argjson before "${before:-[]}" --arg title "$title" --argjson pid "$TERMINAL_PID" \
      'first(.[] | select(.mapped and (.address as $a | all($before[]; . != $a)) and (.title == $title or .pid == $pid))) | .address' 2>/dev/null)
  done
  if [[ ! $TERMINAL_ADDRESS =~ ^0x[0-9a-f]+$ ]]; then
    problem "no terminal window appeared within 10 s"
    run_restore close_terminal
    result
    return
  fi
  [[ $(active_address) == "$TERMINAL_ADDRESS" ]] || focus_window "$TERMINAL_ADDRESS"
  nap "$SETTLE"
  report "opacity: Hyprland's getprop for the app's window: active $(timeout 5 hyprctl getprop "address:$APP_ADDRESS" opacity 2>&1), inactive $(timeout 5 hyprctl getprop "address:$APP_ADDRESS" opacity_inactive 2>&1); decoration:inactive_opacity $(hypr_json getoption decoration:inactive_opacity | jq -r '.float' 2>/dev/null)"
  if [[ $(active_address) == "$APP_ADDRESS" ]]; then
    problem "GitTurtle still has the focus: the terminal did not take it"
  else
    picture inactive || problem "no picture of the inactive window"
  fi
  run_restore close_terminal
  nap 1
  result
}

# utime + stime in clock ticks, and the thread count, from /proc/PID/stat.
cpu_and_threads() { sed 's/^.*) //' "/proc/$1/stat" 2>/dev/null | awk '{ print $12 + $13, $18 }'; }

check_rest() {
  local ticks_before ticks_after threads_before threads_after hz started status crashes
  section k rest "at rest, and the exit"
  skip_gate || return
  window_gate || return
  hz=$(getconf CLK_TCK 2>/dev/null || echo 100)
  read -r ticks_before threads_before <<<"$(cpu_and_threads "$APP_PID")"
  nap "$REST_SECONDS"
  read -r ticks_after threads_after <<<"$(cpu_and_threads "$APP_PID")"
  if [[ $ticks_before =~ ^[0-9]+$ && $ticks_after =~ ^[0-9]+$ ]]; then
    report "rest: $(((ticks_after - ticks_before) * 1000 / hz)) ms of CPU in $REST_SECONDS s without input (ticks $ticks_before -> $ticks_after at $hz Hz); threads $threads_before -> $threads_after; $(grep -m1 VmRSS "/proc/$APP_PID/status" 2>/dev/null | tr -s '\t ' ' ')"
  else
    problem "the process could not be read from /proc"
  fi
  started=$(now_ms)
  close_window "$APP_ADDRESS"
  if wait_for_exit "$APP_PID" "$EXIT_WAIT_MS"; then
    wait "$APP_PID" 2>/dev/null
    status=$?
    report "exit: the window was closed through Hyprland and the process was gone $(($(now_ms) - started)) ms later, exit status $status"
  else
    problem "the process still ran $EXIT_WAIT_MS ms after its window was closed through Hyprland"
  fi
  (($(panic_lines) == 0)) || problem "the app's output holds a panic line (records/app-output.txt)"
  report_preferences "at the exit"
  run_restore stop_app
  nap 2 # a core dump takes a moment to be registered
  report_crashes
  result
}

# Only lines that name GitTurtle are kept: the journal and the core dump list
# also hold the owner's other programs.
report_crashes() {
  local found
  if have coredumpctl; then
    found=$(timeout 30 coredumpctl list --no-pager --since "@$START_EPOCH" 2>/dev/null | grep -ci gitturtle)
    report "coredumpctl: $found entries naming gitturtle since the script started"
    ((found == 0)) || problem "coredumpctl lists $found core dumps of gitturtle"
  else
    uncovered "core dumps: coredumpctl is not installed"
  fi
  if have journalctl; then
    timeout 30 journalctl --user --no-pager --output=cat --since "@$START_EPOCH" 2>/dev/null |
      grep -i gitturtle | grep -Ei 'segfault|SIGSEGV|SIGABRT|SIGBUS|SIGILL|dumped core|panicked' >"$WORK/journal.txt"
    found=$(grep -c . "$WORK/journal.txt")
    report "user journal: $found crash lines naming gitturtle since the script started"
    if ((found > 0)); then
      record journal-crash-lines.txt <"$WORK/journal.txt"
      problem "the user journal holds $found crash lines for gitturtle (records/journal-crash-lines.txt)"
    fi
  else
    uncovered "crash lines in the user journal: journalctl is not installed"
  fi
}

# The keys Omarchy binds to DESCRIPTION, read from its own bindings file.
binding_keys() {
  sed -n "s/^o\\.bind(\"\\([^\"]*\\)\", \"$1\".*/\\1/p" "$OMARCHY_PATH/default/hypr/bindings/clipboard.lua" 2>/dev/null | head -n 1
}
# One prompt: what to do, Enter, a picture unless PICTURE is "-", then y or n.
# A prompt counts as done only with its answer and, where it takes one, its picture;
# otherwise it is a part the check did not cover (HANDS_DONE stays as it was).
ask() { # NUMBER PICTURE DO QUESTION
  local answer="" pictured=yes
  if [[ $HANDS_ABSENT == yes ]]; then
    uncovered "hands $1: not asked, the prompt before it went unanswered"
    return 1
  fi
  say ""
  say "[$1/4] $3"
  printf '      Press Enter here when that is done: '
  if ! read -r -u 3 -t "$ANSWER_WAIT" answer; then
    HANDS_ABSENT=yes
    uncovered "hands $1: no Enter within $ANSWER_WAIT s"
    return 1
  fi
  [[ $2 == - ]] || picture "$2" || pictured=no
  answer=""
  while [[ $answer != y && $answer != n ]]; do
    printf '      %s [y/n]: ' "$4"
    read -r -u 3 -t "$ANSWER_WAIT" answer || {
      HANDS_ABSENT=yes
      uncovered "hands $1: no answer within $ANSWER_WAIT s"
      return 1
    }
  done
  report "hands $1: $3 | $4 $answer"
  if [[ $answer != y ]]; then
    problem "hands $1: answered n: $4"
  elif [[ $pictured == no ]]; then
    uncovered "hands $1: answered y, but its picture could not be taken"
    return 1
  fi
  HANDS_DONE=$((HANDS_DONE + 1))
}
# Says only whether the clipboard holds a commit of the demo repository; nothing
# else that it may hold is ever written down.
report_clipboard() {
  local text
  have wl-paste || return 0
  text=$(timeout 3 wl-paste --no-newline 2>/dev/null | head -c 64)
  if [[ $text =~ ^[0-9a-f]{40}$ ]] && git -C "$DEMO" cat-file -e "$text^{commit}" 2>/dev/null; then
    report "hands 1: the clipboard holds the full hash of a commit of the demo repository"
  else
    report "hands 1: the clipboard does NOT hold the full hash of a commit of the demo repository"
  fi
}

check_hands() {
  local copy paste before new
  HANDS_DONE=0 HANDS_ABSENT=no
  section l hands "the owner's two minutes"
  skip_gate || return
  if [[ $HANDS == no ]]; then
    skipped "--hands was not given"
    return
  fi
  desktop_gate || return
  if [[ ! -t 3 ]]; then
    skipped "standard input is not a terminal to answer in"
    return
  fi
  if [[ -z $INSTALLED_BINARY || -z $DEMO ]]; then
    skipped "no installed executable or demo repository (see the install and launch checks)"
    return
  fi
  # The app of the earlier checks still runs when the rest check, which closes it,
  # was skipped: launch_app ends it before it starts the one for this check.
  register_restore stop_app
  if ! launch_app; then
    problem "the GitTurtle the earlier checks used could not be ended, so no second one was started"
    run_restore stop_app
    result
    return
  fi
  if ! wait_for_app_window; then
    problem "no GitTurtle window within $WINDOW_WAIT s"
    run_restore stop_app
    result
    return
  fi
  silence_notifications
  copy=$(binding_keys "Universal copy")
  paste=$(binding_keys "Universal paste")
  report "bindings, from $OMARCHY_PATH/default/hypr/bindings/clipboard.lua: copy \"${copy:-not found}\", paste \"${paste:-not found}\""
  report "hands: GitTurtle is open beside this terminal, on the workspace the run started from; a picture is taken only while no other window overlaps GitTurtle's"
  say ""
  say "Four things for you to do in the GitTurtle window that just opened. Step 1"
  say "replaces what your clipboard holds with a commit's hash."
  ask 1 hands-copy "Click a commit in History, then press ${copy:-Omarchy's copy binding} (Omarchy's Universal copy)." \
    "Did GitTurtle say that it copied?" && report_clipboard
  ask 2 hands-paste "Click History's search field, then press ${paste:-Omarchy's paste binding} (Omarchy's Universal paste)." \
    "Is the commit's hash in the search field?"
  before=$(hypr_json clients | jq -c '[.[].address]' 2>/dev/null)
  # No picture while the picker shows: it lists the owner's folders.
  ask 3 - "Press Ctrl+O in GitTurtle (Open Repository) and leave the folder picker open." \
    "Is the desktop's folder picker showing?"
  new=$(hypr_json clients | jq -r --argjson before "${before:-[]}" '[.[] | select(.address as $a | all($before[]; . != $a)) | .class] | join(", ")' 2>/dev/null)
  report "hands 3: windows opened since the prompt, by class: ${new:-none}"
  ask 4 hands-after-cancel "Press Cancel in the folder picker." \
    "Is GitTurtle as it was before Ctrl+O (same repository, same view)?"
  run_restore stop_app
  # Nothing answered and nothing failed: the check did not happen.
  if ((HANDS_DONE == 0 && ${#PROBLEMS[@]} == 0)); then
    skipped "none of the four prompts was answered with its picture (${PARTIALS[0]:-no answer})"
    return
  fi
  result
}

# ---------------------------------------------------------------- the end of a run

# The owner's own installation and settings, which no step may touch.
owner_install() {
  local path
  for path in .local/bin/gitturtle ".local/share/applications/$APP_ID.desktop" .local/share/gitturtle/build-info.json .config/gitturtle/preferences.json; do
    printf '~/%s: %s\n' "$path" "$(stat -c '%s bytes, changed %Z' -- "$HOME/$path" 2>/dev/null || echo absent)"
  done
}

write_restore_section() {
  local changes line
  CHECK=restore
  PROBLEMS=()
  report ""
  report "== restore: what was put back, and what is left"
  restore_all
  stop_leftovers
  # A restore that did not take, in any check or just now, fails this one.
  PROBLEMS=("${RESTORE_FAILURES[@]}")
  if [[ -z $DESKTOP_START ]]; then
    report "desktop: nothing on it was changed"
  else
    changes=$(diff <(printf '%s\n' "$DESKTOP_START") <(desktop_snapshot) | grep '^[<>]')
    if [[ -z $changes ]]; then
      report "desktop: as found: workspace, theme, wallpaper, Omarchy's skip toggles, font, notification silencing, stay-awake, every monitor, and the bytes of the files omarchy-font-set rewrites"
    else
      report "desktop: NOT as found (\"<\" as found, \">\" now; records/desktop-start.txt has the start):"
      while IFS= read -r line; do report "  $line"; done <<<"$changes"
      problem "the desktop is not as it was found"
    fi
    # The saved copies go only when everything is back; otherwise they are what
    # the owner puts the rest back with.
    if ((${#PROBLEMS[@]} > 0)); then
      KEEP_RESTORE=yes
      report "kept, and not packed: <results>/work/restore, with the desktop as it was found (desktop-start.txt), the saved copies and manifest.txt, which says what each is"
    fi
    report "left by Omarchy's own commands: its bookkeeping of the theme visits under ~/.local/state/omarchy (each theme's remembered wallpaper, theme.generation, its logs)"
  fi
  if [[ $(owner_install) == "$OWNER_INSTALL_START" ]]; then
    report "your own GitTurtle: unchanged (~/.local/bin/gitturtle, its desktop entry, build-info.json and ~/.config/gitturtle/preferences.json, by size and change time)"
  else
    report "your own GitTurtle: these differ from the start of the run:"
    while IFS= read -r line; do report "  $line"; done < <(diff <(printf '%s\n' "$OWNER_INSTALL_START") <(owner_install) | grep '^[<>]')
    problem "a file of the owner's own GitTurtle changed during the run"
  fi
  result
}

# Removes work/, except work/restore while something is not back as it was found.
remove_work() {
  [[ -n $OUT && $WORK == "$OUT/work" && -d $WORK ]] || return 0
  chmod -R u+w -- "$WORK" 2>/dev/null
  if [[ $KEEP_RESTORE == yes && -d $RESTORE ]]; then
    find "$WORK" -mindepth 1 -maxdepth 1 ! -name restore -exec rm -rf -- {} +
  else
    rm -rf -- "$WORK"
  fi
}
say_what_was_kept() {
  [[ $KEEP_RESTORE == yes && -d $RESTORE ]] || return 0
  say ""
  say "NOT everything is as it was found. Kept, and not packed, to put the rest back with:"
  say "  $RESTORE"
  say "  desktop-start.txt is the desktop as it was found; manifest.txt says which saved"
  say "  copy (font-0 to font-4) belongs to which file, what this run made and, where"
  say "  the themes were to be switched, which picture the wallpaper was."
}

# The names of the records that name the home directory, the user or the host.
# False, printing why, when the scan could not vouch for every record: one that
# is not text, one that cannot be read, a scan that fails. Nothing is packed then.
privacy_scan() {
  local file status
  local -a files=()
  mapfile -d '' -t files < <(find "$REPORT" "$RECORDS" -type f -print0 2>/dev/null)
  if ((${#files[@]} == 0)) || [[ ! -f $REPORT ]]; then
    echo "the report and the records could not be listed"
    return 1
  fi
  for file in "${files[@]}"; do
    [[ -s $file ]] || continue
    if ! grep -qI '' -- "$file" 2>/dev/null; then
      echo "${file#"$OUT"/} cannot be read as text"
      return 1
    fi
    grep -qE -e "$SCAN_PATTERN" -- "$file" 2>/dev/null
    status=$?
    if ((status == 0)); then
      printf '%s ' "${file#"$OUT"/}"
    elif ((status != 1)); then
      echo "the scan of ${file#"$OUT"/} failed (grep exit $status)"
      return 1
    fi
  done
}

# Packs report.txt, records/ and pictures/ with numeric, neutral owners, and only
# when every record was read and none names the home directory, the user or the host.
pack_results() {
  local name=${OUT##*/} leaks
  if [[ $KEEP_WORK == yes ]]; then
    say "Kept, and not packed: $WORK (it holds full paths)."
  else
    remove_work
  fi
  if ! leaks=$(privacy_scan); then
    say "privacy scan: it could not be completed: $leaks"
    say "NOT packed: a record that was not read is not sent. The results are in $OUT."
    return 1
  fi
  if [[ -n $leaks ]]; then
    report "privacy scan: the home directory, the user or the host is named in: $leaks"
    report "NOT packed. Look at those files, then send the directory or not."
    return 1
  fi
  report "privacy scan: every record was read as text, and none names the home directory, the user or the host"
  report "finished: $(date -u +%FT%TZ); $COUNT_OK ok, $COUNT_PARTIAL partial, $COUNT_FAILED FAILED, $COUNT_SKIPPED skipped"
  if ! tar -czf "$OUT.tar.gz" --owner=0 --group=0 --numeric-owner --exclude="$name/work" -C "${OUT%/*}" "$name"; then
    say "The results could not be packed; they are in $OUT."
    return 1
  fi
  say ""
  say "send this file back: $OUT.tar.gz"
}

# The first stop is the only one: from its first line on, no other signal is
# heard. Left to finish, that came too late: a second, different signal arriving
# after this one's exit and before finish's own first lines ran this again inside
# the EXIT trap, where an exit ends the run at once, before the restores.
on_signal() {
  trap '' INT TERM HUP QUIT
  trap : PIPE
  STOPPED_BY=$1
  exit "$2"
}
finish() {
  local status=$?
  trap '' INT TERM HUP QUIT # nothing cuts the restore short,
  trap : PIPE               # nor does a terminal or a pipe that no longer reads
  trap - EXIT
  [[ -n $OUT_CREATED ]] || exit "$status"
  [[ -z $STOPPED_BY ]] || report "STOPPED by $STOPPED_BY during the $CHECK check: restoring, then packing what was done"
  write_restore_section
  pack_results || status=1
  say_what_was_kept
  ((status != 0 || COUNT_FAILED == 0)) || status=1
  exit "$status"
}

# ---------------------------------------------------------------- main

main() {
  local variable
  parse_arguments "$@"
  # The prompts read the terminal through descriptor 3; everything this script
  # runs gets an empty standard input, so nothing it starts can wait on a key.
  exec 3<&0 </dev/null
  # Nothing it runs inherits a Git or GitTurtle setting from the calling shell.
  for variable in $(compgen -e); do
    [[ $variable != GIT_* && $variable != GITTURTLE_* ]] || unset "$variable"
  done
  [[ $EUID -ne 0 ]] || die "run this as yourself: never as root, never with sudo."
  [[ $(uname -s) == Linux ]] || die "this checks a Linux archive on an Omarchy machine."
  refuse_missing_tools
  refuse_unverified_archive
  choose_results_directory
  build_redaction
  # With the desktop part asked for, the results directory must name no one:
  # said now, before the archive checks, rather than after them.
  if [[ $DESKTOP == yes ]] && path_names_someone "$OUT"; then
    die "the results directory lies under your home or names you or the host, and GitTurtle's header shows the demo repository's path in every picture: use --out /var/tmp/${OUT##*/}, or --no-desktop."
  fi
  # The plan is decided here, once, and printed; nothing later in the run starts
  # a part it says will not run.
  DESKTOP_REFUSAL=$(desktop_refusal)
  if [[ -z $DESKTOP_REFUSAL ]]; then
    [[ -z $FONT_CHOICE ]] || listed_font "$FONT_CHOICE" || die "--font $FONT_CHOICE is not a family that omarchy-font-list prints."
    # A themes check that --skip names asks nothing either, sudo included.
    if ! skip_named themes; then
      mapfile -t SKIP_TOGGLES < <(theme_skip_toggles)
      mapfile -t THEME_UNKNOWNS < <(theme_unknowns)
      THEME_BLOCKER=$(theme_blocker)
      ! sudo_question_asked "$THEME_BLOCKER" || SUDO_ASKED=yes
    fi
  fi
  show_plan_and_wait

  trap finish EXIT
  trap 'on_signal INT 130' INT
  trap 'on_signal TERM 143' TERM
  trap 'on_signal HUP 129' HUP
  trap 'on_signal QUIT 131' QUIT
  # Output that is no longer read (the run piped into a program that has ended)
  # is a stop like the others, not an end without the restores.
  trap 'on_signal PIPE 141' PIPE
  mkdir -- "$OUT" || die "cannot create $OUT"
  OUT_CREATED=yes
  mkdir -- "$WORK" "$RECORDS" "$PICTURES"
  START_EPOCH=$(date +%s)
  OWNER_INSTALL_START=$(owner_install)
  CHECK=start
  report "GitTurtle on Omarchy: check of a CI-built archive"
  report "started: $(date -u +%FT%TZ)"
  report "script: scripts/omarchy-verify.sh, sha256 $(sha256_of "${BASH_SOURCE[0]}"), checkout $(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  report "options: desktop part $([[ -z $DESKTOP_REFUSAL ]] && echo yes || echo "no ($DESKTOP_REFUSAL)"), hands $HANDS, expected revision ${EXPECT_REVISION:-none}, skipped by --skip: ${SKIPS[*]:-none}, --themes-anyway $THEMES_ANYWAY"

  check_host
  check_archive
  check_install
  check_package
  begin_desktop_part
  check_launch
  check_themes
  check_font
  check_floating
  check_scales
  check_inactive
  check_rest
  # The prompts are answered in the owner's terminal, so the hands part runs
  # beside it: the run's own workspace is left first.
  run_restore restore_workspace
  check_hands
  exit 0
}

[[ ${BASH_SOURCE[0]} != "$0" ]] || main "$@"
