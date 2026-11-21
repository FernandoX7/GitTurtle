#!/usr/bin/env python3
"""Install Ubuntu packages from one selected apt source file, each apt command under a deadline (stdlib only).

The calling step writes the selection once and the package names. The hosts that
the selected source file makes apt read are looked up first, each under a bound:
when none resolves the step fails and names them, and when some do not, one
warning names those and the step goes on. `apt-get update --error-on=any` and
`apt-get install` then run as root with that one selection, each under a deadline
that `timeout` holds on the root side, where it may signal apt. Nothing is run a
second time and no file is written. The deadlines are constants of this file: no
option, input or environment variable sets one. See docs/ci.md.
"""

import argparse
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


# In 110 hosted Ubuntu job logs of 2026-11-16 to 2026-11-19 the fetch of
# `apt-get update` took 2 s at the median, 18 s at p95 and 49 s at most, and the
# fetch of `apt-get install` 5 s, 292 s and 640 s, that maximum for 19.2 MB from a
# mirror serving about 30 kB/s. A job's own limit is 30 minutes or more.
UPDATE_SECONDS = 300
INSTALL_SECONDS = 900
# `timeout` sends TERM at the deadline and KILL this long after it.
KILL_AFTER_SECONDS = 10
LOOKUP_SECONDS = 5
MAX_SOURCE_BYTES = 1 << 20

SOURCE_OPTION = "Dir::Etc::sourcelist="
PARTS_OPTION = "Dir::Etc::sourceparts=-"
LABEL = r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?"
HOSTNAME = re.compile(rf"(?=.{{1,253}}\Z){LABEL}(?:\.{LABEL})*")
# No final hyphen: apt-get reads `name-` as "remove name".
PACKAGE = re.compile(r"[a-z0-9](?:[a-z0-9+.-]*[a-z0-9+.])?")
# A deb822 field: a name of printable ASCII without a colon, then its value.
FIELD = re.compile(r"([!-9;-~]+):(.*)")
NETWORK_URI = re.compile(r"(?:mirror\+https?|mirror|https?)://([^/?#]*)")
MIRROR_LIST = re.compile(r"mirror\+file:(?://)?(/[^?#]*)")
LOCAL_URI = re.compile(r"(?:file|copy|cdrom):")
# apt separates lines at a newline and words at ASCII white space, and nothing else.
SPACE = " \t\r\v\f"
WORD = re.compile(rf"[^{SPACE}\n]+")
# What apt reads as false in a paragraph's `Enabled` field.
DISABLED = {"no", "false", "without", "off", "disable", "0"}
# `timeout`'s own statuses: the command timed out, `timeout` failed, the command
# could not be run, the command was not found.
TIMED_OUT, TIMEOUT_FAILED = 124, (125, 126, 127)
# getent's statuses for a key it found and one it did not.
FOUND, NOT_FOUND = 0, 2


class AptError(Exception):
    """A failure that ends the step with one line."""


def shown(text):
    """A value as printable ASCII that starts no workflow command, wherever it stands in a line."""
    return "".join(character if " " <= character <= "~" else character.encode("unicode_escape").decode("ascii")
                   for character in str(text)).replace("##[", "#\\x23[")


def selected_source(options):
    """The one source file a selection names. A selection is the two options that
    make apt read that file and no other, so nothing else reaches an apt command line."""
    sources = [option for option in options if option.startswith(SOURCE_OPTION)]
    if len(sources) != 1 or sorted(options) != sorted([sources[0], PARTS_OPTION]):
        raise AptError(f"the selection must be `-o {SOURCE_OPTION}<file> -o {PARTS_OPTION}` and nothing else")
    path = sources[0].removeprefix(SOURCE_OPTION)
    if not path.startswith("/"):
        raise AptError(f"the selected source file must be an absolute path: {shown(path)}")
    return Path(path)


def read_bounded(path, what):
    try:
        with open(path, "rb") as stream:
            data = stream.read(MAX_SOURCE_BYTES + 1)
        if len(data) > MAX_SOURCE_BYTES:
            raise AptError(f"{what} {shown(path)} is larger than {MAX_SOURCE_BYTES} bytes")
        return data.decode("utf-8")
    except FileNotFoundError:
        raise AptError(f"{what} {shown(path)} is missing; no other source is read in its place") from None
    except (OSError, UnicodeDecodeError) as error:
        raise AptError(f"{what} {shown(path)} cannot be read: {type(error).__name__}") from None


def enabled_uris(text, path):
    """The URIs of every enabled paragraph of a deb822 source file, in the file's order."""
    uris, fields, name = [], {}, None
    for number, line in enumerate([*text.split("\n"), ""], 1):
        if line.startswith("#"):
            continue
        if not line.strip(SPACE):
            if fields.get("enabled", "yes").lower() not in DISABLED:
                uris += WORD.findall(fields.get("uris", ""))
            fields, name = {}, None
        elif line[0] in " \t" and name:
            fields[name] = f"{fields[name]} {line.strip(SPACE)}"
        elif field := FIELD.fullmatch(line):
            name = field.group(1).lower()
            fields[name] = field.group(2).strip(SPACE)
        else:
            raise AptError(f"line {number} of {shown(path)} is not a field of a deb822 source file")
    return uris


def uri_host(uri, path):
    """The host of one URI, checked against the hostname pattern; None when apt reads it from this machine."""
    if LOCAL_URI.match(uri):
        return None
    network = NETWORK_URI.match(uri)
    if not network:
        # The scheme alone: a URI can carry a user and a password.
        scheme = re.match(r"[A-Za-z][A-Za-z0-9+.-]*(?=:)", uri)
        kind = f"its scheme is {scheme.group(0)}" if scheme else "it has no scheme"
        raise AptError(f"{shown(path)} names a source whose host this step cannot read: {kind}")
    # user:password@host:port
    host = re.sub(r":[0-9]*\Z", "", network.group(1).rpartition("@")[2])
    if not HOSTNAME.fullmatch(host):
        raise AptError(f"{shown(path)} names a host that is not a hostname: {shown(host)}")
    return host.lower()


def uri_hosts(uri, path):
    """The hosts apt reads for one URI of the source file: its own, or those of the `mirror+file:` list it names."""
    listed = MIRROR_LIST.fullmatch(uri)
    if not listed:
        return [uri_host(uri, path)]
    mirrors = [WORD.findall(line)[0] for line in read_bounded(listed.group(1), "the mirror list").split("\n")
               if line.strip(SPACE) and not line.startswith("#")]
    if not mirrors:
        raise AptError(f"the mirror list {shown(listed.group(1))} that {shown(path)} names is empty")
    return [uri_host(mirror, listed.group(1)) for mirror in mirrors]


def source_hosts(source):
    """Every host apt reads with the selected source file, each once, in the order the file names them.

    Every name has matched the hostname pattern before this returns, so one
    that does not never reaches a command line.
    """
    uris = enabled_uris(read_bounded(source, "the selected source file"), source)
    hosts = list(dict.fromkeys(host for uri in uris for host in uri_hosts(uri, source) if host))
    if not hosts:
        raise AptError(f"the selected source file {shown(source)} names no host for apt to read")
    return hosts


def bounded(command, seconds, *, root=False):
    """`command` under `timeout`. As root the deadline is held by a process that may signal apt."""
    return [*(["sudo"] if root else []), "timeout", f"--kill-after={KILL_AFTER_SECONDS}", str(seconds), *command]


def status(command, *, quiet=False):
    """Run one command with this step's own output; its exit status and the seconds it took."""
    sys.stdout.flush()
    start = time.monotonic()
    try:
        code = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL if quiet else None,
                              check=False).returncode
    except OSError as error:
        raise AptError(f"{shown(command[0])} cannot be run: {type(error).__name__}") from None
    return code, time.monotonic() - start


def timed_out(code, elapsed, seconds):
    """Whether `timeout` ended the command: with TERM it answers 124; when it took
    KILL it answers 128+9, and sudo then ends by that signal itself."""
    return code == TIMED_OUT or (code in (128 + signal.SIGKILL, -signal.SIGKILL) and elapsed >= seconds)


def lookup(hosts):
    """{host: whether it resolved}. Each name is one argument of `getent ahosts` under `timeout`."""
    print(f"Hosts apt reads, each looked up within {LOOKUP_SECONDS} s: {', '.join(hosts)}")
    resolved = {}
    for host in hosts:
        code, elapsed = status(bounded(["getent", "ahosts", host], LOOKUP_SECONDS), quiet=True)
        if code not in (FOUND, NOT_FOUND) and not timed_out(code, elapsed, LOOKUP_SECONDS):
            raise AptError(f"the lookup of {host} could not be made: getent or timeout exited with status {code}")
        resolved[host] = code == FOUND
        answer = "resolved" if resolved[host] else "did not resolve" if code == NOT_FOUND else "gave no answer"
        print(f"  {host}: {answer} in {elapsed:.1f} s")
    return resolved


def described(resolved):
    return ", ".join(f"{host} ({'resolved' if found else 'did not resolve'})" for host, found in resolved.items())


def verdict(resolved, source):
    """Fail when no host resolved; otherwise the one warning that names those that did not, or None."""
    missing = [host for host, found in resolved.items() if not found]
    if len(missing) == len(resolved):
        raise AptError(f"none of the hosts {shown(source)} names resolved within {LOOKUP_SECONDS} s: {', '.join(missing)}")
    if missing:
        return (f"::warning::{len(missing)} of the {len(resolved)} hosts apt reads did not resolve within {LOOKUP_SECONDS} s: "
                f"{', '.join(missing)}; apt-get goes on under its deadlines")
    return None


def phase(name, arguments, seconds, options, resolved):
    """One apt command as root under its deadline, once, with the selection. Its output is this step's own."""
    print(f"apt-get {name}, deadline {seconds} s")
    selection = [word for option in options for word in ("-o", option)]
    code, elapsed = status(bounded(["apt-get", *selection, name, *arguments], seconds, root=True))
    if timed_out(code, elapsed, seconds):
        raise AptError(f"apt-get {name} did not end within its {seconds} s deadline and was stopped; "
                       f"hosts: {described(resolved)}")
    if code in TIMEOUT_FAILED:
        raise AptError(f"apt-get {name} could not be run under its deadline: timeout exited with status {code}")
    if code:
        raise AptError(f"apt-get {name} failed with status {code} after {elapsed:.0f} s")
    print(f"apt-get {name} ended in {elapsed:.0f} s")


def install(options, packages, *, update_seconds=UPDATE_SECONDS, install_seconds=INSTALL_SECONDS):
    """Look the hosts up, then update and install with one selection.

    The two deadlines are arguments so that a test or a reproduction can lower
    them; one that is wider than this file's constant is refused.
    """
    if not (0 < update_seconds <= UPDATE_SECONDS and 0 < install_seconds <= INSTALL_SECONDS):
        raise ValueError("a deadline is only ever lowered")
    source = selected_source(options)
    for package in packages:
        if not PACKAGE.fullmatch(package):
            raise AptError(f"not a package name: {shown(package)}")
    resolved = lookup(source_hosts(source))
    if warning := verdict(resolved, source):
        print(warning)
    phase("update", ["--error-on=any"], update_seconds, options, resolved)
    phase("install", ["-y", "--no-install-recommends", *packages], install_seconds, options, resolved)


def main(argv=None, **deadlines):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
                                     allow_abbrev=False)
    parser.add_argument("-o", dest="options", action="append", required=True, metavar="OPTION",
                        help=f"the selection, both used by update and install: {SOURCE_OPTION}<file> and {PARTS_OPTION}")
    parser.add_argument("packages", nargs="+", metavar="PACKAGE", help="a package name")
    args = parser.parse_args(argv)
    try:
        install(args.options, args.packages, **deadlines)
    except AptError as error:
        print(f"::error::{error}", flush=True)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
