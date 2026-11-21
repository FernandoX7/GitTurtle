"""Ubuntu package installation: one selected source file, a bounded lookup of its hosts, a deadline for each apt command; no network required."""

import contextlib
import importlib.util
import io
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[3]
HELPER = ROOT / "scripts/ci/apt_install.py"
SPEC = importlib.util.spec_from_file_location("ci_apt_install", HELPER)
apt_install = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(apt_install)

PARTS = "Dir::Etc::sourceparts=-"
# Ubuntu 24.04's own source file as the arm64 image has it; the x64 image's
# build replaces both URIs with the mirror list (runner-images' configure-apt-sources.sh).
ARM64_SOURCES = textwrap.dedent("""\
    ## Ubuntu distribution repository
    ##
    ## Types: Append deb-src to enable the fetching of source package.
    Types: deb
    URIs: http://ports.ubuntu.com/ubuntu-ports/
    Suites: noble noble-updates noble-backports
    Components: main universe restricted multiverse
    Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg

    ## Ubuntu security updates. Aside from URIs and Suites,
    ## this should mirror your choices in the previous section.
    Types: deb
    URIs: http://ports.ubuntu.com/ubuntu-ports/
    Suites: noble-security
    Components: main universe restricted multiverse
    Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
    """)
X64_MIRRORS = ("http://azure.archive.ubuntu.com/ubuntu/\tpriority:1\n"
               "https://archive.ubuntu.com/ubuntu/\tpriority:2\n"
               "https://security.ubuntu.com/ubuntu/\tpriority:3\n")
X64_HOSTS = ["azure.archive.ubuntu.com", "archive.ubuntu.com", "security.ubuntu.com"]
RESOLVED, NOT_FOUND, NO_ANSWER = (0, 0.1), (2, 0.1), (124, 5.0)


class Commands:
    """Every command the helper would run, answered with (status, seconds) and no process: getent by host, apt-get by phase."""

    def __init__(self, hosts=None, **phases):
        self.hosts, self.phases, self.ran = hosts or {}, {"update": (0, 3.0), "install": (0, 20.0), **phases}, []

    def __call__(self, command, *, quiet=False):
        self.ran.append(list(command))
        if "getent" in command:
            return self.hosts.get(command[-1], RESOLVED)
        return self.phases[next(word for word in command if word in self.phases)]

    def apt(self):
        return [command for command in self.ran if "apt-get" in command]


class HelperFixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="apt-test-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.source, self.mirrors = self.base / "ubuntu.sources", self.base / "apt-mirrors.txt"

    def x64(self, mirrors=X64_MIRRORS):
        self.source.write_text(ARM64_SOURCES.replace("http://ports.ubuntu.com/ubuntu-ports/", f"mirror+file:{self.mirrors}"))
        if mirrors is not None:
            self.mirrors.write_text(mirrors)

    def selection(self):
        return ["-o", f"Dir::Etc::sourcelist={self.source}", "-o", PARTS]

    def step(self, commands, arguments=None, packages=("pkg-config", "libdav1d-dev"), **deadlines):
        """The helper as a step calls it: its status and the lines it printed."""
        output = io.StringIO()
        with mock.patch.object(apt_install, "status", commands), contextlib.redirect_stdout(output):
            status = apt_install.main([*(self.selection() if arguments is None else arguments), "--", *packages], **deadlines)
        return status, output.getvalue().splitlines()

    def failure(self, commands, **keywords):
        """The one error line of a step that failed, which is the last line it printed."""
        status, lines = self.step(commands, **keywords)
        self.assertEqual(status, 1)
        errors = [line for line in lines if line.startswith("::error::")]
        self.assertEqual(len(errors), 1, lines)
        self.assertEqual(lines[-1], errors[0])
        return errors[0].removeprefix("::error::")


class SourceHosts(HelperFixture):
    def test_the_x64_layout_gives_the_hosts_of_the_mirror_list_in_its_order(self):
        self.x64()
        self.assertEqual(apt_install.source_hosts(self.source), X64_HOSTS)
        # Metadata after a mirror, a comment and an empty line are not hosts.
        self.x64("# failover\n\nhttp://us-east-2.ec2.archive.ubuntu.com/ubuntu/\tpriority:1\tarch:amd64\n" + X64_MIRRORS)
        self.assertEqual(apt_install.source_hosts(self.source), ["us-east-2.ec2.archive.ubuntu.com", *X64_HOSTS])

    def test_the_arm64_layout_gives_the_host_the_file_names_itself(self):
        self.source.write_text(ARM64_SOURCES)
        self.assertEqual(apt_install.source_hosts(self.source), ["ports.ubuntu.com"])
        # A port, credentials and letter case are not part of the name that is looked up.
        self.source.write_text("X-Repolib_Name: Ubuntu\nTypes: deb\nURIs: https://user:secret@Mirror.Example.COM:8443/ubuntu\n"
                               "  http://second.example/ubuntu\nSuites: noble\n")
        self.assertEqual(apt_install.source_hosts(self.source), ["mirror.example.com", "second.example"])

    def test_only_what_apt_reads_over_the_network_is_a_host(self):
        # apt skips a paragraph that is not enabled and reads a file: source from this machine.
        self.source.write_text(ARM64_SOURCES.replace("Suites: noble-security", "Suites: noble-security\nEnabled: no")
                               .replace("ports.ubuntu.com/ubuntu-ports/\nSuites: noble-security", "off.example/ubuntu/\nSuites: noble-security")
                               + "\nTypes: deb\nURIs: file:/var/local/archive\nSuites: local\n")
        self.assertEqual(apt_install.source_hosts(self.source), ["ports.ubuntu.com"])
        self.source.write_text("Types: deb\nURIs: file:/var/local/archive\nSuites: local\n")
        self.assertIn("names no host for apt to read", self.failure(Commands()))

    def test_a_name_outside_the_hostname_pattern_fails_before_any_command_runs(self):
        hostile = ("http://mirror.example;reboot/ubuntu", "http://-o/ubuntu", "http://$(id)/ubuntu", "http://a..b/ubuntu",
                   "http://[2001:db8::1]/ubuntu", "http:///ubuntu", f"http://{'a' * 64}.example/ubuntu",
                   "http://mirror.example ::error::x/ubuntu", "http://##[error]x/ubuntu")
        for where in ("source", "list"):
            for uri in hostile:
                with self.subTest(where=where, uri=uri):
                    if where == "source":
                        self.source.write_text(f"Types: deb\nURIs: http://ports.ubuntu.com/ubuntu-ports/ {uri}\nSuites: noble\n")
                    else:
                        self.x64(X64_MIRRORS + f"{uri}\tpriority:4\n")
                    commands = Commands()
                    message = self.failure(commands)
                    self.assertIn("names a host that is not a hostname: ", message)
                    self.assertEqual(commands.ran, [])
                    self.assertTrue(message.isascii() and message.isprintable(), message)
                    self.assertNotIn("##[", message)

    def test_an_empty_mirror_list_fails_by_name(self):
        for text in ("", "\n# no mirror\n\n"):
            with self.subTest(text=text):
                self.x64(text)
                commands = Commands()
                self.assertEqual(self.failure(commands), f"the mirror list {self.mirrors} that {self.source} names is empty")
                self.assertEqual(commands.ran, [])

    def test_a_missing_source_file_fails_by_name_and_no_other_source_is_read(self):
        commands = Commands()
        self.assertEqual(self.failure(commands), f"the selected source file {self.source} is missing; no other source is read in its place")
        # Neither a lookup nor an apt command that would read every listed source.
        self.assertEqual(commands.ran, [])
        self.x64(None)
        self.assertEqual(self.failure(commands), f"the mirror list {self.mirrors} is missing; no other source is read in its place")
        self.assertEqual(commands.ran, [])

    def test_a_source_file_or_mirror_list_over_the_size_bound_fails_by_name_and_is_never_parsed_cut_short(self):
        bound = apt_install.MAX_SOURCE_BYTES
        # A valid mirror list padded with a comment to one byte over the bound: read cut
        # short it would still parse, so only the size check can refuse it.
        padded = X64_MIRRORS + "#" * (bound + 1 - len(X64_MIRRORS.encode()))
        self.x64(padded)
        self.assertEqual(self.mirrors.stat().st_size, bound + 1)
        commands = Commands()
        self.assertEqual(self.failure(commands), f"the mirror list {self.mirrors} is larger than {bound} bytes")
        self.assertEqual(commands.ran, [])
        # Exactly the bound is read whole.
        self.mirrors.write_text(padded[:-1])
        self.assertEqual(len(apt_install.read_bounded(self.mirrors, "the mirror list")), bound)
        # The selected source file has the same bound.
        self.source.write_text(ARM64_SOURCES + "#" * (bound + 1 - len(ARM64_SOURCES.encode())))
        self.assertEqual(self.failure(commands), f"the selected source file {self.source} is larger than {bound} bytes")
        self.assertEqual(commands.ran, [])

    def test_a_source_whose_hosts_cannot_be_read_fails_by_name(self):
        cases = {"URIs: ftp://ports.ubuntu.com/ubuntu-ports/": f"{self.source} names a source whose host this step cannot read: its scheme is ftp",
                 # Only the scheme is printed: a URI can carry a user and a password.
                 "URIs: s3://KEY:SECRET@bucket/ubuntu/": f"{self.source} names a source whose host this step cannot read: its scheme is s3",
                 "URIs: ?KEY:SECRET@bucket": f"{self.source} names a source whose host this step cannot read: it has no scheme",
                 "deb http://ports.ubuntu.com/ubuntu-ports/ noble main": f"line 2 of {self.source} is not a field of a deb822 source file"}
        for line, expected in cases.items():
            with self.subTest(line=line):
                self.source.write_text(f"Types: deb\n{line}\nSuites: noble\n")
                commands = Commands()
                message = self.failure(commands)
                self.assertIn(expected, message)
                self.assertNotIn("SECRET", message)
                self.assertEqual(commands.ran, [])
        # A list names mirrors, never another list.
        self.x64(f"mirror+file:{self.mirrors}\tpriority:1\n")
        self.assertIn("names a source whose host this step cannot read: its scheme is mirror+file", self.failure(Commands()))

    def test_a_word_apt_would_read_as_more_than_a_package_to_install_is_refused(self):
        # apt-get reads `name-` as "remove name"; an option or a path is no name either.
        for word in ("git-", "-o", "--allow-unauthenticated", "Dir::Etc::sourceparts=/tmp", "a b", "/tmp/x.deb", "pkg=1.0", ""):
            with self.subTest(word=word):
                self.x64()
                commands = Commands()
                status, lines = self.step(commands, packages=("pkg-config", word))
                self.assertEqual(status, 1, lines)
                self.assertEqual(sum(line.startswith("::error::not a package name: ") for line in lines), 1, lines)
                self.assertEqual(commands.ran, [])
        for word in ("g++", "libstdc++-14-dev", "python3.12", "a"):
            self.assertTrue(apt_install.PACKAGE.fullmatch(word), word)


class Lookup(HelperFixture):
    def test_each_host_is_one_argument_of_getent_under_timeout(self):
        self.x64()
        commands = Commands()
        status, lines = self.step(commands)
        self.assertEqual(status, 0, lines)
        self.assertEqual(commands.ran[:3], [["timeout", "--kill-after=10", "5", "getent", "ahosts", host] for host in X64_HOSTS])
        # The bound and the names are printed, and every answer.
        self.assertEqual(lines[:4], [f"Hosts apt reads, each looked up within 5 s: {', '.join(X64_HOSTS)}",
                                     *(f"  {host}: resolved in 0.1 s" for host in X64_HOSTS)])

    def test_no_host_resolving_fails_by_name_before_apt_runs(self):
        self.x64()
        commands = Commands({"azure.archive.ubuntu.com": NOT_FOUND, "archive.ubuntu.com": NO_ANSWER, "security.ubuntu.com": NOT_FOUND})
        self.assertEqual(self.failure(commands), f"none of the hosts {self.source} names resolved within 5 s: {', '.join(X64_HOSTS)}")
        self.assertEqual(len(commands.ran), 3)
        self.assertEqual(commands.apt(), [])

    def test_some_hosts_not_resolving_is_one_warning_and_the_step_goes_on(self):
        self.x64()
        # The first mirror is the one that did not resolve on 2026-11-11.
        for missing, unanswered in ((X64_HOSTS[:1], NOT_FOUND), (X64_HOSTS[:2], NO_ANSWER), (X64_HOSTS[2:], NOT_FOUND)):
            with self.subTest(missing=missing):
                commands = Commands({host: unanswered for host in missing})
                status, lines = self.step(commands)
                self.assertEqual(status, 0, lines)
                self.assertEqual([line for line in lines if line.startswith("::")],
                                 [f"::warning::{len(missing)} of the 3 hosts apt reads did not resolve within 5 s: {', '.join(missing)}; "
                                  "apt-get goes on under its deadlines"])
                self.assertEqual([command[command.index("apt-get") + 5] for command in commands.apt()], ["update", "install"])

    def test_every_host_resolving_prints_no_warning(self):
        self.x64()
        commands = Commands()
        status, lines = self.step(commands)
        self.assertEqual((status, [line for line in lines if line.startswith("::")]), (0, []))
        self.assertEqual(len(commands.apt()), 2)

    def test_a_lookup_that_could_not_be_made_is_not_a_name_that_did_not_resolve(self):
        self.source.write_text(ARM64_SOURCES)
        for code in (1, 3, 125, 126, 127):
            with self.subTest(status=code):
                commands = Commands({"ports.ubuntu.com": (code, 0.0)})
                self.assertEqual(self.failure(commands), f"the lookup of ports.ubuntu.com could not be made: getent or timeout exited with status {code}")
                self.assertEqual(commands.apt(), [])


class Deadlines(HelperFixture):
    def setUp(self):
        super().setUp()
        self.x64()
        self.first_missing = {"azure.archive.ubuntu.com": NOT_FOUND}
        self.hosts = "azure.archive.ubuntu.com (did not resolve), archive.ubuntu.com (resolved), security.ubuntu.com (resolved)"

    def test_update_and_install_each_run_once_as_root_under_a_deadline_with_the_one_selection(self):
        commands = Commands()
        status, lines = self.step(commands)
        self.assertEqual(status, 0, lines)
        selection = self.selection()
        self.assertEqual(commands.ran[3:], [
            ["sudo", "timeout", "--kill-after=10", "300", "apt-get", *selection, "update", "-o", "Acquire::Languages=none", "--error-on=any"],
            ["sudo", "timeout", "--kill-after=10", "900", "apt-get", *selection, "install", "-y", "--no-install-recommends",
             "pkg-config", "libdav1d-dev"]])
        self.assertEqual(lines[4:], ["apt-get update, deadline 300 s", "apt-get update ended in 3 s",
                                     "apt-get install, deadline 900 s", "apt-get install ended in 20 s"])

    def test_the_deadlines_are_constants_that_nothing_widens(self):
        self.assertEqual((apt_install.UPDATE_SECONDS, apt_install.INSTALL_SECONDS), (300, 900))
        # Together, each with its time to be stopped, they are stricter than the limit of every job that
        # installs packages, by the helper's own call or through the setup action. The shortest limit is the
        # package transfer job's, which installs a handful of runtime packages and builds nothing.
        deadlines = apt_install.UPDATE_SECONDS + apt_install.INSTALL_SECONDS + 2 * apt_install.KILL_AFTER_SECONDS
        self.assertEqual(deadlines, 1220)
        limits = {}
        for name in ("quality.yml", "release.yml"):
            jobs = (ROOT / ".github/workflows" / name).read_text().split("\njobs:\n", 1)[1]
            for job in re.split(r"\n(?=  [A-Za-z0-9_-]+:\n)", jobs):
                if "scripts/ci/apt_install.py" in job or "uses: ./.github/actions/setup-rust" in job:
                    (minutes,) = re.findall(r"^    timeout-minutes: (\d+)$", job, re.MULTILINE)
                    limits[f"{name} {job.strip().split(':', 1)[0]}"] = int(minutes)
        self.assertEqual(limits, {"quality.yml rust-debug": 60, "quality.yml rust-clippy": 45, "quality.yml rust-release": 45,
                                  "quality.yml rust-arch": 60, "quality.yml image-privacy": 90,
                                  "quality.yml package-transfer": 30, "release.yml build": 60})
        for job, minutes in limits.items():
            with self.subTest(job=job):
                self.assertLess(deadlines, 60 * minutes)
        for wider in ({"update_seconds": 301}, {"install_seconds": 901}, {"update_seconds": 0}, {"install_seconds": -1}):
            with self.subTest(deadlines=wider):
                commands = Commands()
                with self.assertRaises(ValueError):
                    self.step(commands, **wider)
                self.assertEqual(commands.ran, [])
        # No option of the command line and no variable of the environment sets one.
        for option in ("--update-seconds=3600", "--install-seconds=3600", "--timeout=3600"):
            with self.subTest(option=option):
                commands = Commands()
                with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                    self.step(commands, arguments=[*self.selection(), option])
                self.assertEqual(commands.ran, [])
        variables = {name: "3600" for name in ("UPDATE_SECONDS", "INSTALL_SECONDS", "APT_UPDATE_SECONDS", "APT_INSTALL_SECONDS", "TIMEOUT")}
        commands = Commands()
        with mock.patch.dict(os.environ, variables):
            self.step(commands)
        self.assertEqual([command[3] for command in commands.apt()], ["300", "900"])
        self.assertNotRegex(HELPER.read_text(), r"\bos\.|\benviron\b|getenv")

    def test_a_deadline_that_ends_update_is_one_line_with_the_phase_the_bound_and_the_hosts(self):
        commands = Commands(self.first_missing, update=(124, 300.0))
        self.assertEqual(self.failure(commands),
                         f"apt-get update did not end within its 300 s deadline and was stopped; hosts: {self.hosts}")
        # Nothing runs after it: neither the install nor the update again.
        self.assertEqual([command[command.index("apt-get") + 5] for command in commands.apt()], ["update"])

    def test_a_deadline_that_ends_install_names_install_and_its_own_bound(self):
        commands = Commands(install=(124, 900.0))
        self.assertEqual(self.failure(commands), "apt-get install did not end within its 900 s deadline and was stopped; hosts: "
                         "azure.archive.ubuntu.com (resolved), archive.ubuntu.com (resolved), security.ubuntu.com (resolved)")
        self.assertEqual([command[command.index("apt-get") + 5] for command in commands.apt()], ["update", "install"])

    def test_a_command_that_took_kill_at_the_deadline_is_named_as_the_deadline(self):
        # `timeout` answers 128+9 once it has sent KILL, and sudo may end by that signal itself.
        for code in (137, -9):
            with self.subTest(status=code):
                self.assertEqual(self.failure(Commands(self.first_missing, update=(code, 310.2))),
                                 f"apt-get update did not end within its 300 s deadline and was stopped; hosts: {self.hosts}")
        # The same status long before the deadline is apt's own end, not the deadline's.
        self.assertEqual(self.failure(Commands(update=(137, 4.2))), "apt-get update failed with status 137 after 4 s")

    def test_a_lowered_deadline_reaches_the_command_and_the_line(self):
        commands = Commands(update=(124, 20.0))
        self.assertIn("apt-get update did not end within its 20 s deadline", self.failure(commands, update_seconds=20, install_seconds=60))
        self.assertEqual(commands.apt()[0][:4], ["sudo", "timeout", "--kill-after=10", "20"])
        commands = Commands()
        self.step(commands, update_seconds=20, install_seconds=60)
        self.assertEqual([command[3] for command in commands.apt()], ["20", "60"])

    def test_a_failed_apt_command_fails_the_step_and_nothing_follows_it(self):
        # `apt-get update --error-on=any` answers 100 where the plain command warned and answered 0.
        commands = Commands(update=(100, 1.4))
        self.assertEqual(self.failure(commands), "apt-get update failed with status 100 after 1 s")
        self.assertEqual(len(commands.apt()), 1)
        commands = Commands(install=(100, 31.0))
        self.assertEqual(self.failure(commands), "apt-get install failed with status 100 after 31 s")
        self.assertEqual(len(commands.apt()), 2)
        self.assertEqual(self.failure(Commands(update=(127, 0.0))),
                         "apt-get update could not be run under its deadline: timeout exited with status 127")

    def test_only_the_selection_of_one_source_file_and_package_names_reach_apt(self):
        source = f"Dir::Etc::sourcelist={self.source}"
        refused = ([["-o", source]], [["-o", PARTS]], [["-o", source, "-o", "Dir::Etc::sourceparts=/etc/apt/sources.list.d"]],
                   [["-o", source, "-o", PARTS, "-o", "Acquire::AllowInsecureRepositories=true"]],
                   [["-o", source, "-o", source, "-o", PARTS]], [["-o", "Dir::Etc::sourcelist=ubuntu.sources", "-o", PARTS]],
                   [self.selection(), ("--allow-unauthenticated",)], [self.selection(), ("pkg-config", "-oAPT::Get::AllowUnauthenticated=true")],
                   [self.selection(), ("pkg-config=1.8",)], [self.selection(), ("Pkg-Config",)], [self.selection(), ("pkg-config", "")])
        for arguments, *packages in refused:
            with self.subTest(arguments=arguments, packages=packages):
                commands = Commands()
                self.failure(commands, arguments=arguments, **({"packages": packages[0]} if packages else {}))
                self.assertEqual(commands.ran, [])
        # Either order of the two options is the same selection, passed on as written.
        commands = Commands()
        self.assertEqual(self.step(commands, arguments=["-o", PARTS, "-o", source])[0], 0)
        self.assertEqual(commands.apt()[0][5:9], ["-o", PARTS, "-o", source])


def helper_calls(text):
    """Every call of the helper in a workflow or action file, as its words."""
    return [call.replace("\\\n", " ").split() for call in re.findall(r"python3 scripts/ci/apt_install\.py ((?:[^\n]*\\\n)*[^\n]*)\n", text)]


class Callers(unittest.TestCase):
    SELECTION = ["-o", "Dir::Etc::sourcelist=/etc/apt/sources.list.d/ubuntu.sources", "-o", PARTS, "--"]

    def test_the_action_image_privacy_and_package_transfer_install_through_the_helper_with_the_image_ubuntu_source_file(self):
        action = (ROOT / ".github/actions/setup-rust/action.yml").read_text()
        quality = (ROOT / ".github/workflows/quality.yml").read_text()
        privacy = quality.split("\n  image-privacy:\n", 1)[1].split("\n  website:\n", 1)[0]
        transfer = quality.split("\n  package-transfer:\n", 1)[1].split("\n  quality:\n", 1)[0]
        (build, build_minimal), (pillow,), (runtime,) = helper_calls(action), helper_calls(privacy), helper_calls(transfer)
        # The selection is written once in each step, and nothing else is passed: no deadline, no other option.
        self.assertEqual(build[:5], self.SELECTION)
        self.assertEqual(build_minimal[:5], self.SELECTION)
        self.assertEqual(pillow, [*self.SELECTION, "python3-pil"])
        self.assertEqual([word for word in build[5:] if not apt_install.PACKAGE.fullmatch(word)], [])
        self.assertEqual([word for word in build_minimal[5:] if not apt_install.PACKAGE.fullmatch(word)], [])
        self.assertEqual(set(build[5:]) - set(build_minimal[5:]), {"python3-pil", "desktop-file-utils", "xdg-utils"})
        # The package transfer job runs a built executable and its installer and builds nothing: the libraries
        # the executable names and the package with the installer's two desktop-entry commands.
        self.assertEqual(runtime, [*self.SELECTION, "desktop-file-utils", "libdav1d7", "libxcb1", "libxkbcommon0",
                                   "libxkbcommon-x11-0"])
        # Runtime packages only: no headers, compiler or build tool, so nothing the build job installs to
        # compile with. The one name the two lists share is the desktop-entry tools'.
        self.assertEqual([name for name in runtime[5:] if name.endswith("-dev")], [])
        self.assertEqual(sorted(set(runtime[5:]) & set(build[5:])), ["desktop-file-utils"])
        # The job has no setup action, so that list is everything it installs.
        self.assertNotIn("./.github/actions/", transfer)
        self.assertEqual(helper_calls(quality), [pillow, runtime])
        # Neither runs an apt command of its own beside the helper, and the release workflow installs nothing itself.
        command = re.compile(r"^(?![ \t]*#).*(?<![\w`-])apt(?:-get)?\s.*", re.MULTILINE)
        for name, text in (("action.yml", action), ("quality.yml", quality),
                           ("release.yml", (ROOT / ".github/workflows/release.yml").read_text())):
            with self.subTest(file=name):
                self.assertEqual(command.findall(text), [])
        self.assertEqual(helper_calls((ROOT / ".github/workflows/release.yml").read_text()), [])


class CommandLine(unittest.TestCase):
    """The helper as a process, with stand-ins on PATH for sudo, timeout, getent and apt-get."""

    STAND_INS = {
        "sudo": 'exec "$@"',
        # A stand-in apt-get that answers 99 asks for the deadline's status.
        "timeout": 'shift 2\n"$@"\nstatus=$?\nif [ "$status" -eq 99 ]; then exit 124; fi\nexit "$status"',
        "getent": 'case "$2" in azure.*) exit 2 ;; esac\necho "192.0.2.7 STREAM $2"',
        "apt-get": 'echo "apt-get $*"\ncase " $* " in *" update "*) echo "Get:1 http://archive.ubuntu.com/ubuntu noble InRelease"\n'
                   'exit "$(cat "$0.update" 2>/dev/null || echo 0)" ;; esac',
    }

    def setUp(self):
        (ROOT / ".local").mkdir(exist_ok=True)
        temporary = tempfile.TemporaryDirectory(prefix="apt-test-", dir=ROOT / ".local")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name)
        for name, body in self.STAND_INS.items():
            (self.base / name).write_text(f"#!/bin/sh\n{body}\n")
            (self.base / name).chmod(0o755)
        self.source = self.base / "ubuntu.sources"
        self.source.write_text(ARM64_SOURCES.replace("http://ports.ubuntu.com/ubuntu-ports/", f"mirror+file:{self.base / 'apt-mirrors.txt'}"))
        (self.base / "apt-mirrors.txt").write_text(X64_MIRRORS)

    def run_helper(self):
        selection = f"-o Dir::Etc::sourcelist={self.source} -o {PARTS}"
        environment = {**os.environ, "PATH": f"{self.base}{os.pathsep}{os.environ.get('PATH', os.defpath)}"}
        result = subprocess.run([sys.executable, "-B", str(HELPER), *selection.split(), "--", "pkg-config"], env=environment,
                                capture_output=True, text=True, timeout=60)
        return result.returncode, result.stdout.splitlines(), f"apt-get {selection}"

    def test_a_healthy_run_prints_the_lookup_the_warning_and_what_each_command_printed(self):
        status, lines, apt = self.run_helper()
        self.assertEqual(status, 0, lines)
        # These seconds are measured on real processes, so the lines hold their form and never a
        # value: a hosted macOS runner took 0.1 s for one stand-in lookup. A lookup prints tenths
        # and a command whole seconds.
        tenths, whole = re.compile(r" in [0-9]+\.[0-9] s\Z"), re.compile(r" ended in [0-9]+ s\Z")
        formed = [tenths.sub(" in <tenths> s", line) if 1 <= index <= 3 else whole.sub(" ended in <whole> s", line)
                  for index, line in enumerate(lines)]
        self.assertEqual(formed, [
            f"Hosts apt reads, each looked up within 5 s: {', '.join(X64_HOSTS)}",
            "  azure.archive.ubuntu.com: did not resolve in <tenths> s", "  archive.ubuntu.com: resolved in <tenths> s",
            "  security.ubuntu.com: resolved in <tenths> s",
            "::warning::1 of the 3 hosts apt reads did not resolve within 5 s: azure.archive.ubuntu.com; apt-get goes on under its deadlines",
            "apt-get update, deadline 300 s", f"{apt} update -o Acquire::Languages=none --error-on=any", "Get:1 http://archive.ubuntu.com/ubuntu noble InRelease",
            "apt-get update ended in <whole> s", "apt-get install, deadline 900 s", f"{apt} install -y --no-install-recommends pkg-config",
            "apt-get install ended in <whole> s"])

    def test_a_deadline_prints_what_apt_had_printed_and_then_its_one_line(self):
        (self.base / "apt-get.update").write_text("99\n")
        status, lines, apt = self.run_helper()
        self.assertEqual(status, 1, lines)
        self.assertEqual(lines[5:], [
            "apt-get update, deadline 300 s", f"{apt} update -o Acquire::Languages=none --error-on=any", "Get:1 http://archive.ubuntu.com/ubuntu noble InRelease",
            "::error::apt-get update did not end within its 300 s deadline and was stopped; hosts: "
            "azure.archive.ubuntu.com (did not resolve), archive.ubuntu.com (resolved), security.ubuntu.com (resolved)"])


if __name__ == "__main__":
    unittest.main()
