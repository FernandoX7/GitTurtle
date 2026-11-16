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
# The archive's installer and executable run only after its checksums match, and
# only with a disposable home under the results directory. Nothing here runs as
# root or asks the network for anything, and the only processes ended are the two
# this script starts itself (the app and one terminal).

set -u

# ---------------------------------------------------------------- settings

readonly APP_ID=com.gitturtle.GitTurtle
readonly SETTLE=3           # seconds a window gets to draw after a change, before its picture
readonly WINDOW_WAIT=20     # seconds for the app's window to appear
readonly SWITCH_WAIT=180    # seconds for one omarchy-theme-set or omarchy-font-set
readonly REST_SECONDS=30    # the rest check's interval without input
readonly EXIT_WAIT_MS=2000  # the process must be gone this long after its window closes
readonly TERMINAL_LIFE=90   # seconds after which the inactive check's terminal ends by itself
readonly ANSWER_WAIT=600    # seconds a prompt waits for Enter or an answer
readonly MIN_BATTERY=30     # percent; below it, discharging, the desktop part does not start
readonly SCALES=(1 1.25 1.5 2)
# The files `omarchy-font-set` (Omarchy 4.5) rewrites under ~/.config; it creates
# kitty/ and fontconfig/ when they are missing.
readonly FONT_FILES=(alacritty/alacritty.toml kitty/kitty.conf ghostty/config foot/foot.ini fontconfig/fonts.conf)
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

# Parsing below reads English messages and dot decimals. What this script starts
# for the desktop (the app, the terminal, Omarchy's commands) gets the session's
# own locale back: SESSION_ENV goes in front of NAME=value pairs and the command.
if [[ -n ${LC_ALL+set} ]]; then SESSION_ENV=(env "LC_ALL=$LC_ALL"); else SESSION_ENV=(env -u LC_ALL); fi
export LC_ALL=C

# ---------------------------------------------------------------- state

ARCHIVE_ARG="" OUT_ARG="" EXPECT_REVISION="" FONT_CHOICE=""
DESKTOP=yes ASSUME_YES=no HANDS=no KEEP_WORK=no
ARCHIVE="" ARCHIVE_NAME="" ARCHIVE_SHA256="" ARCHIVE_TOP="" ARCHIVE_TRUSTED=no
OUT="" OUT_CREATED="" WORK="" RECORDS="" PICTURES="" REPORT=""
BUNDLE="" BUNDLE_BINARY_SHA256="" BUILD_REVISION="" DISPOSABLE_HOME="" INSTALLED_BINARY=""
DISPOSABLE_ENV=()
HOME_REAL="" USER_NAME="" HOST_NAME="" REDACT=() SCAN_PATTERN=""
CHECK="" PROBLEMS=() COUNT_OK=0 COUNT_FAILED=0 COUNT_SKIPPED=0
START_EPOCH=0 STOPPED_BY="" OWNER_INSTALL_START=""
DESKTOP_REFUSAL="" DESKTOP_START="" RESTORES=()
START_FOCUS="" START_DND="" START_AWAKE=""
LAUNCH="" DEMO="" APP_PID="" APP_ADDRESS=""
START_THEME="" START_WALLPAPER=""
START_FONT="" FONT_SAVED=() FONT_DIRECTORY=()
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
  --expect-revision SHA  fail unless the executable reports this source revision
  --no-desktop           run only the checks that change nothing (host to package)
  --yes                  do not wait for Enter before starting
  --hands                end with four prompts for you: Omarchy's copy and paste
                         bindings and the folder picker (about two minutes)
  --font NAME            the font the font check sets (default: the first other
                         family that omarchy-font-list names)
  --keep-work            keep DIR/work (the extraction, the disposable home, the demo
                         repository); it holds full paths and is never packed
  -h, --help             this text

Checks, in this order. report.txt gets one section each, appended as it finishes and
ending in "RESULT <check> ok", "RESULT <check> FAILED: <why>" or "RESULT <check>
skipped: <why>"; one failure does not stop the next check.
  host      architecture, kernel, page size, Omarchy and Hyprland versions, each
            monitor's name, mode and scale, the GPU, the current theme and font
  archive   the .sha256, the extraction, SHA256SUMS inside, bin/gitturtle
            --build-info, ldd, the manifest's fields
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

The desktop part (launch to hands) opens GitTurtle and one terminal window, and it
changes, with Omarchy's and Hyprland's own commands:
  - the theme (omarchy-theme-set: the wallpaper and all that Omarchy retints on a
    switch, the browser colour through Omarchy's own passwordless rule included);
  - the monospace font (omarchy-font-set: the terminal and fontconfig files it
    rewrites, and its restart of the Omarchy shell);
  - the scale of the monitor that holds the window; the window's floating state;
    the focused window;
  - notification silencing (on, so that none lands in a picture) and stay-awake
    (on, so that neither the screensaver nor the lock starts during the run).
Each is recorded before it is changed and put back when its check ends and on every
exit, Ctrl+C included (a stop takes effect when the step in progress returns). The
script prints this plan and waits for Enter. It refuses the desktop part when
Hyprland is not the compositor, when the session is locked, on battery below 30 %,
or when the results directory lies under your home or names you or the host:
GitTurtle's header shows the demo repository's path, so run it from /var/tmp.

Privacy: report.txt and the records name no host, no user and no home directory
(written "~"); pictures are grabs of GitTurtle's own window rectangle, never of the
screen, and the app shows only the demo repository made for the run. The script makes
no network request.
Safety: it never runs as root and never calls sudo; it writes only under the results
directory; your installed GitTurtle and its settings are never touched (the installer
gets a disposable home, the app a new XDG_CONFIG_HOME); it ends only the processes it
started; every wait has a limit; a symlinked or missing archive, a missing .sha256 or
a checksum that does not match stops it before anything from the archive runs.

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
  PROBLEMS=()
  report ""
  report "== $1. $2: $3"
}
problem() {
  PROBLEMS+=("$*")
  report "PROBLEM $*"
}
# The check's last line: ok, or FAILED with its first problem.
result() {
  local more=""
  if ((${#PROBLEMS[@]} == 0)); then
    report "RESULT $CHECK ok"
    COUNT_OK=$((COUNT_OK + 1))
    return
  fi
  ((${#PROBLEMS[@]} > 1)) && more=" (and $((${#PROBLEMS[@]} - 1)) more above)"
  report "RESULT $CHECK FAILED: ${PROBLEMS[0]}$more"
  COUNT_FAILED=$((COUNT_FAILED + 1))
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
        [[ -n ${2:-} ]] || die "--font needs a font family."
        FONT_CHOICE=$2
        shift
        ;;
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
  DISPOSABLE_HOME=$WORK/home LAUNCH=$WORK/launch
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

# Exit 0 locked, 1 unlocked, 2 undetermined: Omarchy's own test, which its
# callers read as unlocked unless it says locked.
session_locked() {
  have omarchy-hyprland-session-locked || return 1
  timeout 5 omarchy-hyprland-session-locked
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
    echo "the session is locked"
    return
  fi
  low=$(battery_low)
  if [[ -n $low ]]; then
    echo "$low"
    return
  fi
  if path_names_someone "$OUT"; then
    echo "the results directory lies under your home or names you or the host, and GitTurtle's header shows the demo repository's path: use --out /var/tmp/${OUT##*/}"
  fi
}

show_plan_and_wait() {
  local themes="?" answer=""
  say "GitTurtle on Omarchy: check of $ARCHIVE_NAME (its .sha256 matches)."
  say "Results: $OUT, packed at the end as ${OUT##*/}.tar.gz beside it."
  say ""
  say "First, nothing on the desktop changes: this host, the archive, its installer into a"
  say "disposable home under the results directory, and the gitturtle-bin package build."
  if [[ -n $DESKTOP_REFUSAL ]]; then
    say "The desktop part will NOT run: $DESKTOP_REFUSAL."
  else
    have omarchy-theme-list && themes=$(timeout 10 omarchy-theme-list 2>/dev/null | grep -c .)
    say "Then the desktop part, about six minutes; leave the keyboard and pointer alone:"
    say "  - GitTurtle opens on a demo repository; notifications are silenced and the"
    say "    session is kept awake;"
    say "  - each of $themes Omarchy themes is set, then one other monospace font;"
    say "  - the window floats; its monitor goes through the scales ${SCALES[*]};"
    say "  - one terminal opens and closes; GitTurtle is closed."
    say "Everything is put back as it was found: theme and wallpaper, font and the files"
    say "omarchy-font-set rewrites, scale, tiling, focus, notifications and idle. A picture"
    say "of GitTurtle's own window is taken at each step, never of the screen."
    [[ $HANDS == no ]] || say "At the end it asks four things of you (--hands)."
  fi
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
# but its window (and, through Omarchy's window opacity, a trace of what lies
# under it); otherwise "blocked" and why.
picture_target() {
  local clients monitors
  clients=$(hypr_json clients) && monitors=$(hypr_json monitors) || {
    echo "blocked Hyprland does not answer"
    return
  }
  jq -rn --argjson clients "$clients" --argjson monitors "$monitors" --argjson pid "${APP_PID:-0}" '
    def overlaps($a):
      .at[0] < ($a.at[0] + $a.size[0]) and (.at[0] + .size[0]) > $a.at[0]
      and .at[1] < ($a.at[1] + $a.size[1]) and (.at[1] + .size[1]) > $a.at[1];
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
            | if any($others[]; (.fullscreen // 0) != 0) then "blocked another window is fullscreen on its workspace"
              elif any($others[]; (.floating or .pinned) and overlaps($app)) then "blocked a floating window overlaps it"
              else "ok \($app.at[0]) \($app.at[1]) \($app.size[0]) \($app.size[1]) \($monitor.scale) \($app.floating)"
              end
          end
      end' 2>/dev/null || echo "blocked Hyprland's answer could not be read"
}

# pictures/NAME.png: the app's own window rectangle, grabbed only while nothing
# else can be in it and discarded if that changed during the grab.
picture() {
  local name=$1 file=$PICTURES/$1.png target verdict rest x y w h scale floating focus=inactive
  if session_locked; then
    report "picture $name: not taken: the session is locked"
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
  if [[ $(picture_target) != "$target" ]] || session_locked; then
    rm -f -- "$file"
    report "picture $name: discarded: the desktop changed while it was taken"
    return 1
  fi
  [[ $(active_address) != "$APP_ADDRESS" ]] || focus=active
  [[ $floating == true ]] && floating=floating || floating=tiled
  report "picture $name: pictures/$name.png, window ${w}x$h at $x,$y, monitor scale $scale, $floating, $focus"
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

# ---------------------------------------------------------------- desktop changes, each with its restore

# --- notifications: silenced, so that none is drawn over the window in a picture
dnd_state() { timeout 5 omarchy-shell notifications isDnd 2>/dev/null; } # prints on or off
silence_notifications() {
  [[ -n $START_DND ]] || return 1
  [[ $(dnd_state) == on ]] || timeout 10 omarchy-toggle-notification-silencing >/dev/null 2>&1
  [[ $(dnd_state) == on ]]
}
restore_notifications() {
  [[ -n $START_DND ]] || return 0
  [[ $(dnd_state) == "$START_DND" ]] || timeout 10 omarchy-toggle-notification-silencing >/dev/null 2>&1
  report "restore notifications: silencing is $(dnd_state), was $START_DND"
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

# --- focus: the window that had it when the desktop part began
restore_focus() {
  [[ $START_FOCUS =~ ^0x[0-9a-f]+$ ]] || return 0
  if ! window_exists "$START_FOCUS"; then
    report "restore focus: the window that had it is gone"
    return 0
  fi
  [[ $(active_address) == "$START_FOCUS" ]] || focus_window "$START_FOCUS"
  report "restore focus: the window that had it $([[ $(active_address) == "$START_FOCUS" ]] && echo "has it again" || echo "does NOT have it")"
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
launch_app() {
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
# Ends the app this script started, if it still runs: its own child, by its exact
# pid, and only while that pid still runs the installed executable.
stop_app() {
  [[ -n $APP_PID ]] || return 0
  if process_running "$APP_PID" && [[ $(readlink "/proc/$APP_PID/exe" 2>/dev/null) == "$INSTALLED_BINARY"* ]]; then
    kill "$APP_PID" 2>/dev/null
    wait_for_exit "$APP_PID" 3000 || kill -KILL "$APP_PID" 2>/dev/null
    report "the app was still running and was ended by its pid"
  fi
  wait "$APP_PID" 2>/dev/null
  [[ ! -f $LAUNCH/app.log ]] || record app-output.txt <"$LAUNCH/app.log"
  APP_PID="" APP_ADDRESS=""
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
set_theme() {
  "${SESSION_ENV[@]}" OMARCHY_THEME_WAIT=1 timeout "$SWITCH_WAIT" omarchy-theme-set "$1" </dev/null >>"$WORK/theme-set.log" 2>&1 3<&-
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
same_file() {
  if [[ -L $1 || -L $2 ]]; then
    [[ -L $1 && -L $2 && $(readlink -- "$1") == "$(readlink -- "$2")" ]]
  else
    cmp -s -- "$1" "$2"
  fi
}
save_font_files() { # copies under work/, never packed; false when one cannot be saved
  local file index=0
  mkdir -p -m 700 "$WORK/restore" || return 1
  for file in "${FONT_FILES[@]}"; do
    FONT_DIRECTORY[index]=absent
    [[ ! -d $HOME/.config/${file%/*} ]] || FONT_DIRECTORY[index]=present
    if [[ -e $HOME/.config/$file || -L $HOME/.config/$file ]]; then
      cp -a -- "$HOME/.config/$file" "$WORK/restore/font-$index" || return 1
      FONT_SAVED[index]=copy
    else
      FONT_SAVED[index]=absent
    fi
    index=$((index + 1))
  done
}
put_back_font_files() { # byte for byte; a file or directory the change created is removed
  local file path index=0
  local -a done=()
  for file in "${FONT_FILES[@]}"; do
    path=$HOME/.config/$file
    case ${FONT_SAVED[index]:-} in
      copy)
        if ! same_file "$WORK/restore/font-$index" "$path"; then
          rm -f -- "$path"
          cp -a -- "$WORK/restore/font-$index" "$path" && done+=("$file put back")
        fi
        ;;
      absent)
        if [[ -e $path || -L $path ]]; then
          rm -f -- "$path" && done+=("$file removed, it did not exist")
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
  if [[ $TERMINAL_ADDRESS =~ ^0x[0-9a-f]+$ ]] && window_exists "$TERMINAL_ADDRESS"; then
    close_window "$TERMINAL_ADDRESS"
  fi
  if [[ -n $TERMINAL_PID ]]; then
    # This script's own child, by its exact pid; its command also ends by itself.
    wait_for_exit "$TERMINAL_PID" 3000 || kill "$TERMINAL_PID" 2>/dev/null
    wait "$TERMINAL_PID" 2>/dev/null
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

check_build_info() {
  local binary=$BUNDLE/bin/gitturtle status target
  if [[ ! -f $binary || ! -x $binary ]]; then
    problem "no executable bin/gitturtle in the archive"
    return
  fi
  BUNDLE_BINARY_SHA256=$(sha256_of "$binary")
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
  if [[ -n $EXPECT_REVISION ]]; then
    if [[ $BUILD_REVISION == "$EXPECT_REVISION"* ]]; then
      report "--expect-revision $EXPECT_REVISION: the executable's source revision"
    else
      problem "--expect-revision $EXPECT_REVISION, but the executable reports $BUILD_REVISION"
    fi
  fi
  if ! jq -e --slurpfile compiled "$WORK/build-info.json" --arg sum "$BUNDLE_BINARY_SHA256" \
    '.compiled_identity == $compiled[0] and .binary_sha256 == $sum' "$BUNDLE/build-info.json" >/dev/null 2>&1; then
    problem "the archive's build-info.json does not describe this executable (its compiled_identity or binary_sha256 differs)"
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
    problem "no $ARCHIVE_NAME.manifest.json beside the archive (a file, not a link)"
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
  [[ $differs == yes ]] || report "manifest: its archive name, size and SHA-256, its package block, and the executable's SHA-256 and compiled identity equal what was extracted and run"
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
  local unsafe
  section b archive "the archive, its checksums and what its executable says"
  : >"$WORK/build-info.json"
  report "archive: $ARCHIVE_NAME, $(stat -c %s -- "$ARCHIVE") bytes"
  report "sha256: $ARCHIVE_SHA256, the one in $ARCHIVE_NAME.sha256 (compared before anything else ran)"
  tar -tzvf "$ARCHIVE" >"$WORK/archive-listing.txt" 2>/dev/null
  tar -tzf "$ARCHIVE" >"$WORK/archive-names.txt" 2>/dev/null
  ARCHIVE_TOP=$(sed -n '1s|/.*||p' "$WORK/archive-names.txt")
  unsafe=$(archive_listing_problem)
  if [[ -n $unsafe ]]; then
    problem "not extracted: $unsafe"
    result
    return
  fi
  mkdir -p "$WORK/archive"
  if ! tar -xzf "$ARCHIVE" -C "$WORK/archive" --no-same-owner 2>"$WORK/tar.log"; then
    problem "tar could not extract the archive: $(tail -n 1 "$WORK/tar.log")"
    result
    return
  fi
  BUNDLE=$WORK/archive/$ARCHIVE_TOP
  report "extracted: $(find "$BUNDLE" -type f | grep -c .) files under $ARCHIVE_TOP/"
  if check_bundle_sums; then
    ARCHIVE_TRUSTED=yes
    check_build_info
    check_libraries
    check_manifest
    check_provenance
  fi
  result
}

check_install() {
  local installer=$BUNDLE/install.py status entry installed validation
  section c install "the archive's installer, into a disposable home"
  if [[ $ARCHIVE_TRUSTED != yes ]]; then
    skipped "the archive did not pass its own checksums, so nothing from it runs"
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
    report "desktop-file-validate: not installed, the entry was not validated"
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
  if [[ $ARCHIVE_TRUSTED != yes ]]; then
    skipped "the archive did not pass its own checksums, so nothing from it runs"
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
file_mark() {
  if [[ -L $1 ]]; then
    printf 'link:%s' "$(readlink -- "$1" | sha256sum | cut -c1-16)"
  elif [[ -f $1 ]]; then
    sha256sum <"$1" | cut -c1-16
  else
    printf absent
  fi
}
desktop_snapshot() {
  local file
  printf 'theme %s\n' "$(theme_now)"
  printf 'wallpaper %s\n' "$(wallpaper_now | sha256sum | cut -c1-16)"
  printf 'font %s\n' "$(timeout 5 omarchy-font-current 2>/dev/null)"
  printf 'notifications-silenced %s\n' "$(dnd_state)"
  printf 'stay-awake %s\n' "$([[ -f $STAY_AWAKE ]] && echo on || echo off)"
  monitor_lines | sed 's/^/monitor /'
  for file in "${FONT_FILES[@]}"; do printf 'file ~/.config/%s %s\n' "$file" "$(file_mark "$HOME/.config/$file")"; done
}

# Records the starting state where a hard stop still leaves it readable, then
# makes the two changes every picture needs: no notification, no screensaver.
begin_desktop_part() {
  [[ $DESKTOP == no ]] || DESKTOP_REFUSAL=$(desktop_refusal)
  [[ -z $DESKTOP_REFUSAL ]] || return 0
  START_FOCUS=$(active_address)
  START_DND=$(dnd_state)
  [[ $START_DND == on || $START_DND == off ]] || START_DND=""
  START_AWAKE=no
  [[ ! -f $STAY_AWAKE ]] || START_AWAKE=yes
  DESKTOP_START=$(desktop_snapshot)
  {
    echo "The desktop as the desktop part found it ($(date -u +%FT%TZ)):"
    printf '%s\n' "$DESKTOP_START"
    echo
    echo "The script puts all of it back. If it was killed outright, by hand:"
    echo "  omarchy-theme-set <theme>      omarchy-font-set \"<font>\"      hyprctl reload   (the monitor's scale)"
    echo "  omarchy-toggle-notification-silencing   (if silencing is on and was off)"
    echo "  omarchy-toggle-idle allow-idle          (if stay-awake is on and was off)"
  } | record desktop-start.txt
  register_restore restore_focus
  register_restore restore_notifications
  silence_notifications || report "NOTE notifications could not be silenced: look at every picture before sending the file"
  register_restore restore_idle
  keep_awake || report "NOTE stay-awake could not be turned on: the screensaver may start during the run"
}

# The RESULT of a check that cannot run without the app's window; false then.
window_gate() {
  if [[ -n $DESKTOP_REFUSAL ]]; then
    skipped "$DESKTOP_REFUSAL"
    return 1
  fi
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
  if [[ -n $DESKTOP_REFUSAL ]]; then
    skipped "$DESKTOP_REFUSAL"
    return
  fi
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
  launch_app
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
  local name slug status taken=0 passed=0
  local -a names=()
  section f themes "every Omarchy theme, live"
  window_gate || return
  START_THEME=$(theme_now)
  if [[ -z $START_THEME ]] || ! have omarchy-theme-list || ! have omarchy-theme-set; then
    START_THEME=""
    skipped "Omarchy names no current theme, or omarchy-theme-list or omarchy-theme-set is missing"
    return
  fi
  START_WALLPAPER=$(wallpaper_now)
  mapfile -t names < <(timeout 10 omarchy-theme-list 2>/dev/null)
  report "themes: omarchy-theme-list names ${#names[@]}; starting from $START_THEME; each picture is taken $SETTLE s after omarchy-theme-set returns"
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
    else
      set_theme "$slug"
      status=$?
      if [[ $(theme_now) != "$slug" || ! -f $OMARCHY_CURRENT/theme/colors.toml ]]; then
        problem "omarchy-theme-set $slug (exit $status) did not apply; the remaining themes were not tried"
        break
      fi
      wait_theme_intro
    fi
    nap "$SETTLE"
    report "theme $slug: $(grep -m1 '^background' "$OMARCHY_CURRENT/theme/colors.toml" | tr -d ' '), light.mode $([[ -f $OMARCHY_CURRENT/theme/light.mode ]] && echo present || echo absent)"
    if picture "theme-$slug"; then taken=$((taken + 1)); else problem "no picture of theme $slug"; fi
  done
  run_restore restore_theme
  nap "$SETTLE"
  picture "theme-restored-$START_THEME" || problem "no picture after the starting theme was restored"
  [[ $(theme_now) == "$START_THEME" ]] || problem "the theme is $(theme_now), not the starting $START_THEME"
  [[ ! -f $WORK/theme-set.log ]] || record theme-set-output.txt <"$WORK/theme-set.log"
  report "themes: $taken pictured, $passed passed over, of ${#names[@]}"
  result
}

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
  local other status
  section g font "omarchy-font-set, live"
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
  if ! save_font_files; then
    START_FONT=""
    skipped "a file omarchy-font-set rewrites could not be saved first"
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
  local facts width height refresh x y transform scale reported taken=0
  section i scales "fractional scales"
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
      continue
    fi
    report "scale $scale: Hyprland reports $reported"
    if picture "scale-$scale"; then taken=$((taken + 1)); else problem "no picture at scale $scale"; fi
  done
  run_restore restore_scale
  [[ $(monitor_line) == "$MONITOR_START_LINE" ]] || problem "the monitor is not as it was found"
  nap "$SETTLE"
  ((taken > 0)) || problem "no scale was pictured"
  result
}

check_inactive() {
  local title=gitturtle-omarchy-check-$$ before deadline
  section j inactive "readability while another window has the focus"
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
    report "coredumpctl: not installed"
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
    report "journalctl: not installed"
  fi
}

# The keys Omarchy binds to DESCRIPTION, read from its own bindings file.
binding_keys() {
  sed -n "s/^o\\.bind(\"\\([^\"]*\\)\", \"$1\".*/\\1/p" "$OMARCHY_PATH/default/hypr/bindings/clipboard.lua" 2>/dev/null | head -n 1
}
# One prompt: what to do, Enter, a picture unless PICTURE is "-", then y or n.
ask() { # NUMBER PICTURE DO QUESTION
  local answer=""
  say ""
  say "[$1/4] $3"
  printf '      Press Enter here when that is done: '
  if ! read -r -u 3 -t "$ANSWER_WAIT" answer; then
    report "hands $1: no Enter within $ANSWER_WAIT s"
    return 1
  fi
  [[ $2 == - ]] || picture "$2"
  answer=""
  while [[ $answer != y && $answer != n ]]; do
    printf '      %s [y/n]: ' "$4"
    read -r -u 3 -t "$ANSWER_WAIT" answer || {
      report "hands $1: no answer within $ANSWER_WAIT s"
      return 1
    }
  done
  report "hands $1: $3 | $4 $answer"
  [[ $answer == y ]] || problem "hands $1: answered n: $4"
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
  section l hands "the owner's two minutes"
  if [[ $HANDS == no ]]; then
    skipped "--hands was not given"
    return
  fi
  if [[ -n $DESKTOP_REFUSAL ]]; then
    skipped "$DESKTOP_REFUSAL"
    return
  fi
  if [[ ! -t 3 ]]; then
    skipped "standard input is not a terminal to answer in"
    return
  fi
  if [[ -z $INSTALLED_BINARY || -z $DEMO ]]; then
    skipped "no installed executable or demo repository (see the install and launch checks)"
    return
  fi
  register_restore stop_app
  launch_app
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
  if [[ -z $DESKTOP_START ]]; then
    report "desktop: nothing on it was changed"
  else
    changes=$(diff <(printf '%s\n' "$DESKTOP_START") <(desktop_snapshot) | grep '^[<>]')
    if [[ -z $changes ]]; then
      report "desktop: as found: theme, wallpaper, font, notification silencing, stay-awake, every monitor, and the files omarchy-font-set rewrites"
    else
      report "desktop: NOT as found (\"<\" as found, \">\" now; records/desktop-start.txt has the start):"
      while IFS= read -r line; do report "  $line"; done <<<"$changes"
      problem "the desktop is not as it was found"
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

remove_work() {
  [[ -n $OUT && $WORK == "$OUT/work" && -d $WORK ]] || return 0
  chmod -R u+w -- "$WORK" 2>/dev/null
  rm -rf -- "$WORK"
}

# Packs report.txt, records/ and pictures/ with numeric, neutral owners, and only
# when no record names the home directory, the user or the host.
pack_results() {
  local name=${OUT##*/} leaks
  if [[ $KEEP_WORK == yes ]]; then
    say "Kept, and not packed: $WORK (it holds full paths)."
  else
    remove_work
  fi
  leaks=$(grep -rIlE -e "$SCAN_PATTERN" "$REPORT" "$RECORDS" 2>/dev/null | sed "s|^$OUT/||" | tr '\n' ' ')
  if [[ -n $leaks ]]; then
    report "privacy scan: the home directory, the user or the host is named in: $leaks"
    report "NOT packed. Look at those files, then send the directory or not."
    return 1
  fi
  report "privacy scan: no record names the home directory, the user or the host"
  report "finished: $(date -u +%FT%TZ); $COUNT_OK ok, $COUNT_FAILED FAILED, $COUNT_SKIPPED skipped"
  if ! tar -czf "$OUT.tar.gz" --owner=0 --group=0 --numeric-owner --exclude="$name/work" -C "${OUT%/*}" "$name"; then
    say "The results could not be packed; they are in $OUT."
    return 1
  fi
  say ""
  say "send this file back: $OUT.tar.gz"
}

on_signal() {
  STOPPED_BY=$1
  exit "$2"
}
finish() {
  local status=$?
  trap '' INT TERM HUP # nothing cuts the restore short
  trap - EXIT
  [[ -n $OUT_CREATED ]] || exit "$status"
  [[ -z $STOPPED_BY ]] || report "STOPPED by $STOPPED_BY during the $CHECK check: restoring, then packing what was done"
  write_restore_section
  pack_results || status=1
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
  DESKTOP_REFUSAL=$(desktop_refusal)
  show_plan_and_wait

  trap finish EXIT
  trap 'on_signal INT 130' INT
  trap 'on_signal TERM 143' TERM
  trap 'on_signal HUP 129' HUP
  mkdir -- "$OUT" || die "cannot create $OUT"
  OUT_CREATED=yes
  mkdir -- "$WORK" "$RECORDS" "$PICTURES"
  START_EPOCH=$(date +%s)
  OWNER_INSTALL_START=$(owner_install)
  CHECK=start
  report "GitTurtle on Omarchy: check of a CI-built archive"
  report "started: $(date -u +%FT%TZ)"
  report "script: scripts/omarchy-verify.sh, sha256 $(sha256_of "${BASH_SOURCE[0]}"), checkout $(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  report "options: desktop part $([[ -z $DESKTOP_REFUSAL ]] && echo yes || echo "no ($DESKTOP_REFUSAL)"), hands $HANDS, expected revision ${EXPECT_REVISION:-none}"

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
  check_hands
  exit 0
}

[[ ${BASH_SOURCE[0]} != "$0" ]] || main "$@"
