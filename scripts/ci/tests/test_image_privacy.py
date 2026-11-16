from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import queue
import re
import signal
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / "image_privacy.py"
QUALITY = SCRIPT.parents[2] / ".github/workflows/quality.yml"
SPEC = importlib.util.spec_from_file_location("ci_image_privacy", SCRIPT)
image_privacy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(image_privacy)
sys.path.insert(0, str(SCRIPT.parents[1]))

from native_qa import privacy  # noqa: E402
import test_changes  # noqa: E402  (the gate's complete needs, as its own tests build them)

HAVE_PIL = importlib.util.find_spec("PIL") is not None
SHARDS = range(1, image_privacy.SHARDS + 1)

# Stands in for `qa.py privacy scan --redacted`: the same lines and exit statuses, no pixels.
STAND_IN_SCANNER = """\
import sys

images = sys.argv[sys.argv.index("--") + 1:]
matched = sum("leaky" in image for image in images)
for image in images:
    print(f"{image}: {'MATCH' if 'leaky' in image else 'clean'}")
print(f"privacy scan: {len(images)} image(s), {matched} matched a template")
sys.exit(1 if matched else 0)
"""

# A scanner that prints its first verdict and then waits to be released, for at most a minute.
HELD_SCANNER = """\
import json
import os
import sys
import time
from pathlib import Path

images = sys.argv[sys.argv.index("--") + 1:]
release = Path(os.environ["STAND_IN_RELEASE"])
Path(os.environ["STAND_IN_SAW"]).write_text(json.dumps({"argv": sys.argv, "environment": dict(os.environ)}))
print(f"{images[0]}: clean", flush=True)
deadline = time.monotonic() + 60
while not release.exists() and time.monotonic() < deadline:
    time.sleep(0.01)
for image in images[1:]:
    print(f"{image}: clean", flush=True)
print("::error title=Injected::a scanner line that looks like a workflow command", flush=True)
print(f"privacy scan: {len(images)} image(s), 0 matched a template")
"""

# A scanner that prints its first verdict and then outlasts any test.
SLEEPING_SCANNER = """\
import os
import sys
import time
from pathlib import Path

Path(os.environ["STAND_IN_PID"]).write_text(str(os.getpid()))
print(f"{sys.argv[sys.argv.index('--') + 1]}: clean", flush=True)
time.sleep(600)
"""

# `image_privacy.py ARGUMENTS` as its own process, writing to a real pipe, with the scanner replaced.
DRIVER = """\
import importlib.util
import sys
from pathlib import Path

spec = importlib.util.spec_from_file_location("ci_image_privacy", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.QA = Path(sys.argv[2])
sys.exit(module.main(sys.argv[3:]))
"""


def names(count: int) -> list[bytes]:
    return [f"docs/evidence/frame-{number:04}.png".encode() for number in range(count)]


def nul_list(paths: list[bytes]) -> bytes:
    return b"".join(path + b"\0" for path in paths)


def run_main(*arguments, env=None) -> tuple[int, str]:
    """`image_privacy.py ARGUMENTS` in this process, without the secret unless `env` gives it: (status, stdout)."""
    environment = {key: value for key, value in os.environ.items() if key != image_privacy.SECRET}
    printed = io.StringIO()
    with mock.patch.dict(os.environ, {**environment, **(env or {})}, clear=True), contextlib.redirect_stdout(printed):
        status = image_privacy.main([str(argument) for argument in arguments])
    return status, printed.getvalue()


def step_outputs(path: Path) -> dict[str, str]:
    """What one step appended to its GITHUB_OUTPUT."""
    return dict(line.split("=", 1) for line in path.read_text().splitlines()) if path.exists() else {}


class PlanTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name) / "repo"
        self.repo.mkdir()
        self.git("init", "-b", "main")
        self.git("config", "user.name", "CI Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        for name in ("docs/kept.png", "docs/changed.jpg", "docs/deleted.png"):
            self.write(name, b"before")
        self.base = self.commit("base")

    def git(self, *args):
        env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        return subprocess.run(["git", *args], cwd=self.repo, env=env, check=True,
                              capture_output=True).stdout.decode().strip()

    def write(self, path, content):
        file = self.repo / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(content)

    def commit(self, message):
        self.git("add", "--all")
        self.git("-c", "commit.gpgsign=false", "commit", "-m", message)
        return self.git("rev-parse", "HEAD")

    def change_images(self):
        self.write("docs/changed.jpg", b"after")
        self.write("docs/new frame.PNG", b"new")
        self.write("docs/notes.md", b"text")
        (self.repo / "docs/deleted.png").unlink()
        self.git("mv", "docs/kept.png", "docs/moved.webp")
        return self.commit("images")

    def pull_request(self, configured=True):
        head = self.git("rev-parse", "HEAD")
        self.git("checkout", "-b", "pull-request-base", self.base)
        self.git("-c", "commit.gpgsign=false", "merge", "--no-ff", "-m", "PR merge fixture", head)
        merge = self.git("rev-parse", "HEAD")
        event = {"pull_request": {"base": {"sha": self.base}, "head": {"sha": head}}}
        return image_privacy.plan(self.repo, "pull_request", event, merge, configured)

    def test_pull_request_lists_added_changed_and_moved_images_only(self):
        self.change_images()
        self.assertEqual(self.pull_request(), ("scan", [b"docs/changed.jpg", b"docs/moved.webp", b"docs/new frame.PNG"]))

    def test_without_the_secret_the_plan_is_unavailable_never_a_pass(self):
        self.change_images()
        status, images = self.pull_request(configured=False)
        self.assertEqual((status, len(images)), ("unavailable", 3))

    def test_no_images_and_events_without_a_comparison(self):
        self.write("docs/notes.md", b"text")
        head = self.commit("docs only")
        push = {"before": self.base, "after": head, "deleted": False, "ref": "refs/heads/main"}
        self.assertEqual(image_privacy.plan(self.repo, "push", push, head, True), ("no-images", []))
        self.assertEqual(image_privacy.plan(self.repo, "workflow_dispatch", {}, head, True), ("no-comparison", []))
        created = {**push, "before": "0" * 40}
        self.assertEqual(image_privacy.plan(self.repo, "push", created, head, True), ("no-comparison", []))

    def test_main_push_compares_before_and_after(self):
        head = self.change_images()
        push = {"before": self.base, "after": head, "deleted": False, "ref": "refs/heads/main"}
        self.assertEqual(image_privacy.plan(self.repo, "push", push, head, True)[1],
                         [b"docs/changed.jpg", b"docs/moved.webp", b"docs/new frame.PNG"])

    def test_inconsistent_identity_fails_when_templates_are_configured(self):
        head = self.change_images()
        for event_name, event in [("push", {"before": self.base, "after": self.base, "deleted": False,
                                            "ref": "refs/heads/main"}),
                                  ("pull_request", {"pull_request": {"base": {"sha": self.base}, "head": {"sha": head}}}),
                                  ("release", {})]:
            with self.subTest(event=event_name):
                with self.assertRaises(image_privacy.changes.PolicyError):
                    image_privacy.plan(self.repo, event_name, event, head, True)
                self.assertEqual(image_privacy.plan(self.repo, event_name, event, head, False), ("unavailable", []))

    def test_unprintable_names_fail_instead_of_reaching_the_log(self):
        self.write("docs/line\nbreak.png", b"new")
        head = self.commit("odd name")
        push = {"before": self.base, "after": head, "deleted": False, "ref": "refs/heads/main"}
        with self.assertRaises(image_privacy.changes.PolicyError):
            image_privacy.plan(self.repo, "push", push, head, True)

    def run_cli(self, *args, env=None):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), *args], cwd=self.repo, capture_output=True,
                              text=True, env={**os.environ, **(env or {})})

    def test_plan_cli_writes_fixed_outputs_a_notice_and_a_nul_list(self):
        head = self.change_images()
        event = Path(self.temporary.name) / "event.json"
        event.write_text(json.dumps({"before": self.base, "after": head, "deleted": False, "ref": "refs/heads/main"}))
        output, summary, listing = (Path(self.temporary.name) / name for name in ("output", "summary", "images"))
        base_env = {"GITHUB_EVENT_PATH": str(event), "GITHUB_EVENT_NAME": "push", "GITHUB_SHA": head}
        for configured, status in [("false", "unavailable"), ("true", "scan")]:
            with self.subTest(configured=configured):
                output.write_text("")
                completed = self.run_cli("plan", "--list", str(listing), "--output", str(output), "--summary",
                                         str(summary), env={**base_env, "TEMPLATES_CONFIGURED": configured})
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertEqual(output.read_text(), f"status={status}\nimages=3\n")
                self.assertEqual(listing.read_bytes(), b"docs/changed.jpg\0docs/moved.webp\0docs/new frame.PNG\0")
                self.assertNotIn("docs/", completed.stdout)
                self.assertEqual("::notice title=Image privacy scan skipped::" in completed.stdout,
                                 status == "unavailable")
        self.assertIn("NOT template-scanned", summary.read_text())
        self.assertIn("This is not a pass", summary.read_text())


class SliceTests(unittest.TestCase):
    def test_the_slices_are_disjoint_in_order_and_together_the_list(self):
        for count in (0, 1, 7, 8, 9, 735):
            with self.subTest(images=count):
                images = names(count)
                slices = [image_privacy.slice_of(images, shard, image_privacy.SHARDS) for shard in SHARDS]
                # Every eighth image by index: image i is shard (i mod 8) + 1's, and nobody else's.
                for shard, mine in zip(SHARDS, slices):
                    self.assertEqual(mine, [image for index, image in enumerate(images)
                                            if index % image_privacy.SHARDS == shard - 1])
                    self.assertEqual(mine, sorted(mine))
                together = [image for mine in slices for image in mine]
                self.assertEqual(len(together), len(set(together)))
                self.assertEqual(sorted(together), images)
                self.assertLessEqual(len(slices[0]) - len(slices[-1]), 1)

    def test_a_shard_or_a_count_the_workflow_does_not_have_is_refused(self):
        images = names(20)
        for shard, shards in [(0, 8), (9, 8), (-1, 8), (True, 8), ("3", 8), (3.0, 8), (None, 8),
                              (1, 4), (4, 4), (1, 7), (1, 9), (1, 16), (1, 0), (1, "8"), (1, None)]:
            with self.subTest(shard=shard, shards=shards), self.assertRaises(image_privacy.changes.PolicyError):
                image_privacy.slice_of(images, shard, shards)

    def test_a_list_that_is_not_the_sorted_plan_is_refused(self):
        # Shards agree on a partition only because each sorts the same set the same way.
        for images in ([b"b.png", b"a.png"], [b"a.png", b"a.png"], names(9)[::-1]):
            with self.subTest(images=images[:2]), self.assertRaises(image_privacy.changes.PolicyError):
                image_privacy.slice_of(images, 1, image_privacy.SHARDS)

    def slice_cli(self, images: list[bytes], shard, shards=image_privacy.SHARDS):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        listing, sliced, output, summary = (Path(temporary.name) / name
                                            for name in ("images", "work/slice", "output", "summary"))
        listing.write_bytes(nul_list(images))
        status, printed = run_main("slice", "--list", listing, "--slice", sliced, "--shard", shard, "--shards", shards,
                                   "--output", output, "--summary", summary)
        return status, printed, sliced, output, summary

    def test_slice_cli_writes_the_shards_images_and_fixed_outputs_only(self):
        images = names(19)
        status, printed, sliced, output, summary = self.slice_cli(images, 3)
        self.assertEqual(status, 0, printed)
        self.assertEqual(sliced.read_bytes(), nul_list([images[2], images[10], images[18]]))
        self.assertEqual(output.read_text(), "slice=scan\n")
        self.assertEqual(json.loads(printed), {"shard": 3, "shards": 8, "images": 19, "slice": 3})
        self.assertEqual(summary.read_text(), "**Image privacy:** shard 3 of 8 scans 3 of 19 added or changed image(s).\n")
        self.assertNotIn("docs/", printed + summary.read_text())

    def test_an_empty_slice_of_a_list_is_scanned_without_the_scanner_or_the_secret(self):
        started = AssertionError("the slice step started a process")
        with mock.patch.object(subprocess, "run", side_effect=started), \
                mock.patch.object(subprocess, "Popen", side_effect=started):
            status, printed, sliced, output, summary = self.slice_cli(names(3), 4)  # run_main withholds the secret
        self.assertEqual(status, 0, printed)
        self.assertEqual(sliced.read_bytes(), b"")
        # The workflow runs the install and scan steps only for slice=scan, so nothing maps the secret here.
        self.assertEqual(output.read_text(), "slice=empty\nstatus=scanned\n")
        self.assertIn("shard 4 of 8 has none of the 3 added or changed image(s) to scan", summary.read_text())

    def test_slice_cli_refuses_an_empty_list_and_shards_the_workflow_does_not_have(self):
        for images, shard, shards in [([], 1, 8), (names(3), 9, 8), (names(3), 0, 8), (names(3), 1, 4),
                                      ([b"b.png", b"a.png"], 1, 8)]:
            with self.subTest(images=len(images), shard=shard, shards=shards):
                status, printed, sliced, output, summary = self.slice_cli(images, shard, shards)
                self.assertEqual(status, 2)
                self.assertTrue(printed.startswith("::error title=Image privacy::"), printed)
                self.assertFalse(sliced.exists() or output.exists() or summary.exists())

    def test_slice_cli_refuses_a_missing_list(self):
        with tempfile.TemporaryDirectory() as scratch:
            status, printed = run_main("slice", "--list", Path(scratch) / "absent", "--slice", Path(scratch) / "slice",
                                       "--shard", 1, "--shards", 8, "--output", Path(scratch) / "output")
            self.assertEqual((status, (Path(scratch) / "output").exists()), (2, False), printed)


class ShardedRunTests(unittest.TestCase):
    """Every shard's steps after the plan, in the workflow's order, with a stand-in for the scanner."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        scanner = self.root / "stand_in_qa.py"
        scanner.write_text(STAND_IN_SCANNER)
        replaced = mock.patch.object(image_privacy, "QA", scanner)
        replaced.start()
        self.addCleanup(replaced.stop)

    def shard(self, number: int, listing: Path) -> tuple[int, str, list[str]]:
        """(the job's exit status, its `status` output, the verdict lines it printed)."""
        work = listing.parent / f"shard-{number}"
        sliced, slice_output, scan_output = work / "slice", work / "slice-output", work / "scan-output"
        status, printed = run_main("slice", "--list", listing, "--slice", sliced, "--shard", number,
                                   "--shards", len(SHARDS), "--output", slice_output)
        self.assertEqual(status, 0, printed)
        sliced_outputs, printed = step_outputs(slice_output), ""
        if sliced_outputs.get("slice") == "scan":  # the workflow's condition on its install and scan steps
            status, printed = run_main("scan", "--list", sliced, "--directory", work / "temp", "--output", scan_output,
                                       env={image_privacy.SECRET: "packed"})
        # The job's output: `steps.scan.outputs.status || steps.slice.outputs.status || steps.plan.outputs.status`.
        job_status = step_outputs(scan_output).get("status") or sliced_outputs.get("status") or "scan"
        return status, job_status, [line for line in printed.splitlines() if line.endswith((": clean", ": MATCH"))]

    def run_shards(self, images: list[bytes]):
        listing = Path(tempfile.mkdtemp(dir=self.root)) / "images"  # one run's runners share nothing but the plan
        listing.write_bytes(nul_list(images))
        return [self.shard(number, listing) for number in SHARDS]

    def test_a_match_in_one_slice_fails_that_shard_and_it_never_writes_scanned(self):
        images = names(20)
        images[10] = b"docs/evidence/frame-0010-leaky.png"  # index 10 is shard 3's
        shards = self.run_shards(images)
        self.assertEqual([status for status, _, _ in shards], [0, 0, 1, 0, 0, 0, 0, 0])
        self.assertEqual([job_status for _, job_status, _ in shards], ["scanned"] * 2 + ["scan"] + ["scanned"] * 5)
        # A shard that did not finish clean reports no status the gate accepts, whatever its result: needs
        # that pass with a clean shard's status are refused, for that status, with the failed shard's.
        gate = image_privacy.changes.gate
        needs = test_changes.needs_for(image_privacy.changes.classify_paths([b"docs/evidence/frame.png"]))
        self.assertEqual(gate({**needs, **test_changes.privacy(shards[0][1])})[-1],
                         "image-privacy: added or changed images template-scanned; no match")
        with self.assertRaises(image_privacy.changes.PolicyError) as refused:
            gate({**needs, **test_changes.privacy(shards[2][1])})
        self.assertEqual(str(refused.exception),
                         "image-privacy: required result was not successful ('success', 'scan')")
        verdicts = sorted(line for _, _, lines in shards for line in lines)
        self.assertEqual(verdicts, sorted(f"{os.fsdecode(image)}: {'MATCH' if b'leaky' in image else 'clean'}"
                                          for image in images))

    def test_every_image_is_scanned_once_and_every_clean_shard_reports_the_same_status(self):
        for count in (1, 3, 8, 9, 43):
            with self.subTest(images=count):
                images = names(count)
                shards = self.run_shards(images)
                # The gate reads the last shard to set the output, so all of them must say the same true thing.
                self.assertEqual({(status, job_status) for status, job_status, _ in shards}, {(0, "scanned")})
                self.assertEqual([len(lines) for _, _, lines in shards],
                                 [len(images[shard - 1::len(SHARDS)]) for shard in SHARDS])
                self.assertEqual(sorted(line for _, _, lines in shards for line in lines),
                                 [f"{os.fsdecode(image)}: clean" for image in images])


class PassThroughTests(unittest.TestCase):
    """The scan step's log: each scanner line as it is read, inside one stop-commands bracket."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.listing, self.output = self.root / "images", self.root / "output"
        self.images = ["docs/a.png", "::warning::docs/b.png"]
        self.listing.write_bytes(nul_list([image.encode() for image in self.images]))
        self.arguments = ["scan", "--list", str(self.listing), "--directory", str(self.root / "runner-temp"),
                          "--output", str(self.output), "--repository", str(self.root)]

    def test_a_verdict_is_out_while_the_scan_still_runs_and_no_scanner_line_is_a_command(self):
        driver, scanner = self.root / "driver.py", self.root / "held_qa.py"
        driver.write_text(DRIVER)
        scanner.write_text(HELD_SCANNER)
        release, saw = self.root / "release", self.root / "saw.json"
        env = {**os.environ, image_privacy.SECRET: "packed-templates", "STAND_IN_RELEASE": str(release),
               "STAND_IN_SAW": str(saw)}
        process = subprocess.Popen([sys.executable, "-B", str(driver), str(SCRIPT), str(scanner), *self.arguments],
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=env)
        lines = queue.Queue()

        def read():
            for line in process.stdout:
                lines.put(line.rstrip("\n"))
            lines.put(None)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        received = []
        try:
            received.append(lines.get(timeout=30))  # the bracket opens
            received.append(lines.get(timeout=30))  # the first image's verdict
            held = process.poll() is None and not release.exists()  # and the scanner still waits for its release
        except queue.Empty:
            held = False
        finally:
            release.write_text("")
            process.wait(timeout=120)
            reader.join(timeout=120)
            process.stdout.close()
        received += iter(lines.get_nowait, None)
        self.assertTrue(held, f"the first verdict was not out before the scan ended: {received}")
        token = received[0].removeprefix("::stop-commands::")
        self.assertRegex(token, r"\A[0-9a-f]{32}\Z")
        self.assertEqual(received, [f"::stop-commands::{token}", "docs/a.png: clean", "::warning::docs/b.png: clean",
                                    "::error title=Injected::a scanner line that looks like a workflow command",
                                    "privacy scan: 2 image(s), 0 matched a template", f"::{token}::"])
        self.assertEqual((process.returncode, self.output.read_text()), (0, "status=scanned\n"))
        # The scanner cannot end the bracket itself: it is given neither the token nor the secret.
        given = json.loads(saw.read_text())
        self.assertFalse(any(token in value for value in [*given["argv"], *given["environment"].values()]))
        self.assertNotIn(image_privacy.SECRET, given["environment"])
        self.assertEqual(list((self.root / "runner-temp/scan").iterdir()), [])

    def test_a_scanner_that_fails_mid_line_still_resumes_commands_before_the_error(self):
        scanner = self.root / "failing_qa.py"
        scanner.write_text('import sys\nsys.stdout.write("docs/a.png: clean\\n::error::half a line")\nsys.exit(3)\n')
        with mock.patch.object(image_privacy, "QA", scanner):
            status, printed = run_main(*self.arguments, env={image_privacy.SECRET: "packed-templates"})
        lines = printed.splitlines()
        token = lines[0].removeprefix("::stop-commands::")
        self.assertEqual(lines, [f"::stop-commands::{token}", "docs/a.png: clean", "::error::half a line", f"::{token}::",
                                 "::error title=Image privacy::the scan could not complete (exit 3)."])
        self.assertEqual((status, self.output.exists()), (3, False))

    def test_an_interrupted_pass_through_kills_the_scanner_and_still_cleans_up(self):
        scanner = self.root / "sleeping_qa.py"
        scanner.write_text(SLEEPING_SCANNER)

        def gone(pid: int) -> bool:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return True
            return False

        def end(pid: int) -> None:
            if not gone(pid):
                os.kill(pid, signal.SIGKILL)

        for interruption in (KeyboardInterrupt, BrokenPipeError):
            with self.subTest(interruption=interruption.__name__):
                pid_file = self.root / f"pid-{interruption.__name__}"
                raised = []

                class Log(io.StringIO):
                    """Takes the first verdict and then fails, as Ctrl-C or a closed log would in the print."""

                    def write(self, text):
                        written = super().write(text)
                        if text.endswith(": clean\n"):
                            raise interruption()
                        return written

                def scan():
                    try:
                        image_privacy.main(self.arguments)
                    except BaseException as error:  # KeyboardInterrupt is one of the cases
                        raised.append(error)

                printed, worker = Log(), threading.Thread(target=scan, daemon=True)
                with mock.patch.object(image_privacy, "QA", scanner), contextlib.redirect_stdout(printed), \
                        mock.patch.dict(os.environ, {image_privacy.SECRET: "packed-templates",
                                                     "STAND_IN_PID": str(pid_file)}):
                    worker.start()
                    worker.join(timeout=20)  # the scanner sleeps ten minutes: waiting for it fails here
                    waited = worker.is_alive()
                    pid = int(pid_file.read_text())
                    self.addCleanup(end, pid)
                    left_running = not gone(pid)
                    end(pid)
                    worker.join(timeout=20)
                self.assertFalse(waited, "the interruption waited for the scanner instead of killing it")
                self.assertFalse(left_running, "the scanner outlived the interruption")
                self.assertEqual([type(error) for error in raised], [interruption])
                opened, verdict, resumed = printed.getvalue().splitlines()
                self.assertEqual((verdict, resumed), ("docs/a.png: clean", f"::{opened.removeprefix('::stop-commands::')}::"))
                self.assertEqual(list((self.root / "runner-temp/scan").iterdir()), [])
                self.assertFalse(self.output.exists())

    def test_a_scanner_that_cannot_start_leaves_no_templates_and_no_open_bracket(self):
        printed = io.StringIO()
        with mock.patch.object(subprocess, "Popen", side_effect=OSError("no interpreter")), \
                mock.patch.dict(os.environ, {image_privacy.SECRET: "packed-templates"}), \
                contextlib.redirect_stdout(printed), self.assertRaises(OSError):
            image_privacy.main(self.arguments)
        opened, resumed = printed.getvalue().splitlines()
        self.assertEqual(resumed, f"::{opened.removeprefix('::stop-commands::')}::")
        self.assertEqual(list((self.root / "runner-temp/scan").iterdir()), [])
        self.assertFalse(self.output.exists())


class WorkflowTests(unittest.TestCase):
    """The image-privacy job in quality.yml, read as text (no YAML dependency)."""

    def setUp(self):
        self.text = QUALITY.read_text(encoding="utf-8")
        parts = re.split(r"^  ([a-z][a-z0-9-]*):\n", self.text.split("\njobs:\n", 1)[1], flags=re.M)
        self.jobs = dict(zip(parts[1::2], parts[2::2]))
        self.header, *self.steps = re.split(r"^      - ", self.jobs["image-privacy"], flags=re.M)

    def step(self, marker: str) -> str:
        found = [step for step in self.steps if marker in step]
        self.assertEqual(len(found), 1, marker)
        return found[0]

    def test_the_job_is_a_matrix_of_the_scripts_shards_under_the_id_the_gate_needs(self):
        self.assertEqual(image_privacy.changes.PRIVACY_JOB, "image-privacy")
        needs = re.search(r"^    needs: \[(.*)\]$", self.jobs["quality"], re.M).group(1).split(", ")
        self.assertIn("image-privacy", needs)
        self.assertIn("    name: Image privacy · ${{ matrix.shard }}/8\n", self.header)
        self.assertIn("    timeout-minutes: 90\n", self.header)
        self.assertIn("    strategy:\n      fail-fast: false\n      matrix:\n"
                      f"        shard: {list(SHARDS)}\n", self.header)
        self.assertEqual(len(SHARDS), 8)
        self.assertIn("    outputs:\n      status: ${{ steps.scan.outputs.status || steps.slice.outputs.status "
                      "|| steps.plan.outputs.status }}\n", self.header)
        # Either would let a shard that failed, or never ran, count as a success.
        for unwanted in ("continue-on-error", "\n    if:", "include:", "exclude:"):
            self.assertNotIn(unwanted, self.jobs["image-privacy"])

    def test_every_shard_slices_the_same_plan_and_scans_only_a_slice_that_has_images(self):
        plan, sliced, install, scan = (self.step(marker) for marker in
                                       ("id: plan", "id: slice", "Install Pillow", "id: scan"))
        self.assertEqual([self.steps.index(step) for step in (plan, sliced, install, scan)], [1, 2, 3, 4])
        self.assertEqual(len(self.steps), 5)
        self.assertNotIn("        if:", plan)
        self.assertIn(' plan --list "$RUNNER_TEMP/image-privacy/images" --output "$GITHUB_OUTPUT" ', plan)
        self.assertIn("        if: steps.plan.outputs.status == 'scan'\n", sliced)
        # The matrix's real job count, not a second literal: the script refuses any count but its own.
        self.assertIn("        env:\n          SHARD: ${{ matrix.shard }}\n          SHARDS: ${{ strategy.job-total }}\n",
                      sliced)
        self.assertIn(' slice --list "$RUNNER_TEMP/image-privacy/images" --slice "$RUNNER_TEMP/image-privacy/slice" '
                      '--shard "$SHARD" --shards "$SHARDS" --output "$GITHUB_OUTPUT" ', sliced)
        for step in (install, scan):
            self.assertIn("        if: steps.slice.outputs.slice == 'scan'\n", step)
        self.assertIn(' scan --list "$RUNNER_TEMP/image-privacy/slice" ', scan)
        self.assertIn(' --output "$GITHUB_OUTPUT" ', scan)

    def test_the_secret_is_mapped_into_the_scan_step_alone(self):
        plan, scan = self.step("id: plan"), self.step("id: scan")
        self.assertIn("          TEMPLATES_CONFIGURED: ${{ secrets.NATIVE_QA_PRIVACY_TEMPLATES != '' }}\n", plan)
        self.assertIn("        env:\n          NATIVE_QA_PRIVACY_TEMPLATES: ${{ secrets.NATIVE_QA_PRIVACY_TEMPLATES }}\n",
                      scan)
        # Those two are the workflow's only uses of any secret.
        self.assertEqual(self.text.count("secrets."), 2)
        self.assertNotIn("secrets:", self.text)
        for chunk in (self.header, *(step for step in self.steps if step is not scan)):
            self.assertNotIn(f"{image_privacy.SECRET}:", chunk)
        self.assertNotIn("permissions:", self.jobs["image-privacy"])
        self.assertEqual(re.findall(r"uses: (\S+)", self.jobs["image-privacy"]),
                         ["actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"])
        self.assertIn("          persist-credentials: false\n", self.steps[0])


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class ScanTests(unittest.TestCase):
    def test_scan_matches_with_packed_templates_and_prints_no_template_data(self):
        from PIL import Image, ImageDraw, ImageFont

        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            repo = root / "repo"
            (repo / "docs").mkdir(parents=True)
            text = Image.new("L", (80, 14), 255)
            ImageDraw.Draw(text).text((2, 1), "QA-TEMPLATE", font=ImageFont.load_default(), fill=0)
            leaky = Image.new("L", (160, 60), 230)
            leaky.paste(text, (40, 20))
            leaky.convert("RGB").save(repo / "docs/-leaky.png")
            Image.new("RGB", (160, 60), (230, 230, 230)).save(repo / "docs/clean.png")
            listing = root / "images"
            listing.write_bytes(b"docs/-leaky.png\0docs/clean.png\0")
            packed = privacy.pack_templates({"QA-TEMPLATE": privacy.gray(text)})
            output, summary = root / "output", root / "summary"
            env = {**os.environ, image_privacy.SECRET: packed, "XDG_CACHE_HOME": str(root / "cache")}
            completed = subprocess.run([sys.executable, "-B", str(SCRIPT), "scan", "--list", str(listing),
                                        "--directory", str(root / "runner-temp"), "--jobs", "2", "--output",
                                        str(output), "--summary", str(summary), "--repository", str(repo)],
                                       capture_output=True, text=True, env=env)
            self.assertEqual(completed.returncode, 1, completed.stdout + completed.stderr)
            lines = completed.stdout.splitlines()
            self.assertTrue(lines[0].startswith("::stop-commands::"), lines)
            self.assertEqual(lines[1:4], ["docs/-leaky.png: MATCH", "docs/clean.png: clean",
                                          "privacy scan: 2 image(s), 1 matched a template"])
            self.assertEqual(lines[4], f"::{lines[0].rsplit('::', 1)[1]}::")
            self.assertTrue(lines[5].startswith("::error title=Image privacy::"))
            self.assertFalse(output.exists())
            self.assertEqual(list((root / "runner-temp/scan").iterdir()), [])
            for secret in ("QA-TEMPLATE", packed[:24]):
                self.assertNotIn(secret, completed.stdout + completed.stderr + summary.read_text())

            listing.write_bytes(b"docs/clean.png\0")
            clean = subprocess.run([sys.executable, "-B", str(SCRIPT), "scan", "--list", str(listing), "--directory",
                                    str(root / "runner-temp"), "--output", str(output), "--repository", str(repo)],
                                   capture_output=True, text=True, env=env)
            self.assertEqual(clean.returncode, 0, clean.stdout + clean.stderr)
            self.assertEqual(output.read_text(), "status=scanned\n")

    def test_scan_refuses_without_the_secret(self):
        with tempfile.TemporaryDirectory() as scratch:
            listing = Path(scratch) / "images"
            listing.write_bytes(b"docs/a.png\0")
            env = {key: value for key, value in os.environ.items() if key != image_privacy.SECRET}
            completed = subprocess.run([sys.executable, "-B", str(SCRIPT), "scan", "--list", str(listing),
                                        "--directory", scratch], capture_output=True, text=True, env=env)
            self.assertEqual(completed.returncode, 2)
            self.assertIn("::error", completed.stdout)


if __name__ == "__main__":
    unittest.main()
