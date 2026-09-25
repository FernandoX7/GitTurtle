from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from native_qa import runenv, stores


class RunEnvironmentTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.home = self.root / "operator"  # stands in for the account's real home
        self.home.mkdir()
        self.run_dir = self.root / "run"

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def prepare(self, preferences: bytes = b'{"version": 6}') -> runenv.RunDirs:
        return runenv.prepare(self.run_dir, preferences, home=self.home)

    def test_relative_paths_are_refused_before_anything_is_created(self) -> None:
        for what in ("run", "./run", "relative/config"):
            with self.subTest(what=what), self.assertRaises(runenv.Refusal) as caught:
                runenv.prepare(what, b"{}", home=self.home)
            self.assertIn("must be an absolute path", str(caught.exception.code))
        self.assertFalse(Path("run").exists())
        with self.assertRaises(runenv.Refusal):
            runenv.check_fixture("fixture", for_commit=False)

    def test_non_empty_run_directory_is_refused(self) -> None:
        self.run_dir.mkdir()
        (self.run_dir / "captures").mkdir()
        with self.assertRaises(runenv.Refusal) as caught:
            self.prepare()
        self.assertIn("not empty", str(caught.exception.code))

    def test_operator_directories_are_refused(self) -> None:
        for target in (self.home, self.home / ".config" / "qa", self.home / ".cache" / "run",
                       self.home / ".local" / "share" / "run"):
            with self.subTest(target=target), self.assertRaises(runenv.Refusal):
                runenv.check_run_dir(target, home=self.home)
        runenv.check_run_dir(self.home / "evidence" / "run", home=self.home)  # under HOME is allowed

    def test_prepare_seeds_identity_store_and_layout(self) -> None:
        store = stores.store_text("porcelain")
        dirs = self.prepare(store)
        self.assertEqual((dirs.paths["HOME"] / ".gitconfig").read_text(),
                         "[user]\n\tname = GitTurtle QA\n\temail = qa@example.invalid\n")
        self.assertEqual(dirs.preferences.read_bytes(), store)
        self.assertEqual(dirs.preferences, self.run_dir / "config" / "gitturtle" / "preferences.json")
        self.assertTrue((self.run_dir / "RUN-STARTED").is_file())
        self.assertEqual(sorted(p.name for p in self.run_dir.iterdir()),
                         ["RUN-STARTED", "cache", "captures", "config", "data", "home", "state"])
        for name, path in dirs.paths.items():
            self.assertTrue(path.is_absolute() and path.is_dir(), name)

    def test_launch_environment_is_isolated(self) -> None:
        dirs = self.prepare()
        inherited = {"WAYLAND_DISPLAY": "wayland-0", "DISPLAY": ":0", "GIT_DIR": "/elsewhere",
                     "GIT_CONFIG_GLOBAL": "/elsewhere/config", "HOME": str(self.home),
                     "XDG_CONFIG_HOME": "config", "DBUS_SESSION_BUS_ADDRESS": "unix:path=/bus", "PATH": "/usr/bin"}
        env = runenv.launch_env(dirs, base=inherited, home=self.home)
        self.assertNotIn("WAYLAND_DISPLAY", env)
        self.assertFalse([key for key in env if key.startswith("GIT_")])
        self.assertEqual((env["DISPLAY"], env["GPUI_X11_SCALE_FACTOR"]), (":1", "1"))
        self.assertEqual(env["DBUS_SESSION_BUS_ADDRESS"], "unix:path=/bus")
        for name in runenv.LAYOUT:
            self.assertEqual(Path(env[name]), dirs.paths[name])
            self.assertTrue(Path(env[name]).is_relative_to(self.run_dir))
        extra = runenv.launch_env(dirs, base={}, extra={"GITTURTLE_GITHUB_FIXTURE": "review"}, home=self.home)
        self.assertEqual(extra["GITTURTLE_GITHUB_FIXTURE"], "review")

    def test_extra_variables_cannot_undo_the_isolation(self) -> None:
        for key in ("XDG_CONFIG_HOME", "HOME", "WAYLAND_DISPLAY", "GIT_DIR"):
            with self.subTest(key=key), self.assertRaises(runenv.Refusal):
                runenv.check_extra({key: "x"})

    def test_verify_refuses_a_relative_or_foreign_store(self) -> None:
        dirs = self.prepare()
        env = runenv.launch_env(dirs, base={}, home=self.home)
        for name, value in (("XDG_CONFIG_HOME", "config"), ("XDG_CONFIG_HOME", str(self.home / ".config")),
                            ("HOME", str(self.home)), ("XDG_STATE_HOME", str(self.root / "other"))):
            with self.subTest(name=name, value=value), self.assertRaises(runenv.Refusal):
                runenv.verify({**env, name: value}, dirs, home=self.home)
        with self.assertRaises(runenv.Refusal):
            runenv.verify({**env, "WAYLAND_DISPLAY": "wayland-0"}, dirs, home=self.home)
        dirs.preferences.unlink()
        with self.assertRaises(runenv.Refusal) as caught:
            runenv.verify(env, dirs, home=self.home)
        self.assertIn("seeded store missing", str(caught.exception.code))

    def test_fixture_policy_warns_or_refuses_outside_the_evidence_root(self) -> None:
        outside = self.root / "fixture"
        fixture, warnings = runenv.check_fixture(outside, for_commit=False)
        self.assertEqual(fixture, outside)
        self.assertTrue(any("outside /tmp/gitturtle-evidence" in warning for warning in warnings))
        with self.assertRaises(runenv.Refusal):
            runenv.check_fixture(outside, for_commit=True)
        _, warnings = runenv.check_fixture(runenv.EVIDENCE_ROOT / "demo", for_commit=True)
        self.assertFalse([w for w in warnings if "outside" in w])

    def test_commit_run_directory_avoids_home_roots(self) -> None:
        for root in runenv.HOME_ROOTS:
            with self.subTest(root=root), self.assertRaises(runenv.Refusal):
                runenv.check_commit_run_dir(root / "someone" / "run")
        runenv.check_commit_run_dir(runenv.EVIDENCE_ROOT / "runs" / "one")


class StoreTest(unittest.TestCase):
    def test_store_version_matches_the_app(self) -> None:
        source = Path(__file__).resolve().parents[2] / "crates" / "app" / "src" / "preferences.rs"
        if not source.is_file():
            self.skipTest("app source not present")
        self.assertIn(f"const STORE_VERSION: u32 = {stores.STORE_VERSION};", source.read_text())

    def test_generated_store(self) -> None:
        store = json.loads(stores.store_text("porcelain", ["/tmp/gitturtle-evidence/a=Alpha", "/tmp/gitturtle-evidence/b"]))
        self.assertEqual(store["settings"], {"theme": "porcelain", "follow_system": False})
        self.assertEqual(store["recent_repositories"], ["/tmp/gitturtle-evidence/a", "/tmp/gitturtle-evidence/b"])
        self.assertEqual(store["project_library"][0], {"project": {"path": "/tmp/gitturtle-evidence/a"}})
        self.assertEqual(store["project_names"], [{"path": "/tmp/gitturtle-evidence/a", "name": "Alpha"}])
        self.assertNotIn("project_names", json.loads(stores.store_text()))
        with self.assertRaises(SystemExit):
            stores.store_text(projects=["relative/repo"])

    def test_supplied_store_is_kept_byte_for_byte(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "preferences.json"
            path.write_bytes(b'{"version": 6, "settings": {"theme": "midnight"}}')
            self.assertEqual(stores.load_file(path), path.read_bytes())
            path.write_text("[1, 2]")
            with self.assertRaises(SystemExit):
                stores.load_file(path)
            path.write_text("not json")
            with self.assertRaises(SystemExit):
                stores.load_file(path)


if __name__ == "__main__":
    unittest.main()
