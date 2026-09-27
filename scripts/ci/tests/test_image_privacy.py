from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "image_privacy.py"
SPEC = importlib.util.spec_from_file_location("ci_image_privacy", SCRIPT)
image_privacy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(image_privacy)
sys.path.insert(0, str(SCRIPT.parents[1]))

from native_qa import privacy  # noqa: E402

HAVE_PIL = importlib.util.find_spec("PIL") is not None


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
