from __future__ import annotations

import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

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
        for key in ("XDG_CONFIG_HOME", "HOME", "WAYLAND_DISPLAY", "GIT_DIR", "DISPLAY", "GPUI_X11_SCALE_FACTOR",
                    "XDG_RUNTIME_DIR", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS"):
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

    def test_commit_run_directory_refuses_home_roots_that_resolve_elsewhere(self) -> None:
        # macOS resolves /home to /System/Volumes/Data/home.
        resolve = Path.resolve

        def firmlinked(path: Path, strict: bool = False) -> Path:
            if path == Path("/home") or Path("/home") in path.parents:
                return Path("/System/Volumes/Data") / path.relative_to("/")
            return resolve(path, strict)

        with mock.patch.object(Path, "resolve", firmlinked), self.assertRaises(runenv.Refusal):
            runenv.check_commit_run_dir(Path("/home/someone/run"))


class ReadOnlyTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.dirs = runenv.prepare(self.root / "run", b'{"version": 6}', home=self.root / "operator")
        self.themes = self.dirs.preferences.parent  # config/gitturtle
        self.themes.chmod(0o755)
        self.dirs.preferences.chmod(0o664)

    def tearDown(self) -> None:
        for path in (self.themes, self.root / "run" / "config" / "moved"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o755)
        self.scratch.cleanup()

    def test_paths_are_validated(self) -> None:
        for good in ("config/gitturtle", "config", "data/gitturtle/themes", "state/x", "cache/gitturtle/a.json"):
            with self.subTest(good=good):
                self.assertIsNone(runenv.read_only_problem(good))
        for bad in ("", "/config/gitturtle", "config/../home", "../config", "config//gitturtle", "config/./x",
                    "config/", "home/.config", "home", "captures/x", "config\\gitturtle", "other/x", None, 5,
                    ["config"], "config/" + "x" * 300):
            with self.subTest(bad=bad):
                self.assertIsNotNone(runenv.read_only_problem(bad))
        with self.assertRaisesRegex(runenv.Refusal, "must start with one of config, data, cache, state"):
            runenv.lock_read_only(self.dirs.root, "home/.gitconfig")

    def test_write_bits_go_until_the_restore_puts_the_old_mode_back(self) -> None:
        store = runenv.lock_read_only(self.dirs.root, "config/gitturtle/preferences.json")
        directory = runenv.lock_read_only(self.dirs.root, "config/gitturtle")
        self.assertEqual((directory.old_mode, directory.new_mode), (0o755, 0o500))
        self.assertEqual((store.old_mode, store.new_mode), (0o664, 0o444))
        self.assertEqual(self.themes.stat().st_mode & 0o7777, 0o500)
        self.assertEqual(self.dirs.preferences.stat().st_mode & 0o7777, 0o444)
        if os.geteuid() != 0:  # root writes regardless of the mode
            with self.assertRaises(PermissionError):
                (self.themes / "themes.json").write_text("{}")
        runenv.restore_mode(directory)
        runenv.restore_mode(store)
        self.assertEqual(self.themes.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(self.dirs.preferences.stat().st_mode & 0o7777, 0o664)
        (self.themes / "themes.json").write_text("{}")

    def test_a_failure_after_the_mode_changed_puts_it_back(self) -> None:
        real, calls = os.fchmod, []

        def interrupted(fd, mode):
            calls.append(mode)
            real(fd, mode)
            if len(calls) == 1:
                raise KeyboardInterrupt  # an asynchronous exception just after the mode changed

        with mock.patch.object(runenv.os, "fchmod", interrupted), self.assertRaises(KeyboardInterrupt):
            runenv.lock_read_only(self.dirs.root, "config/gitturtle")
        self.assertEqual(calls, [0o500, 0o755])
        self.assertEqual(self.themes.stat().st_mode & 0o7777, 0o755)

    def test_the_restore_changes_the_locked_entry_not_what_now_has_its_path(self) -> None:
        # A regular file, because macOS refuses to move a directory without write permission on it.
        outside = self.root / "outside.json"
        outside.write_bytes(b"{}")
        outside.chmod(0o600)
        locked = runenv.lock_read_only(self.dirs.root, "config/gitturtle/preferences.json")
        moved = self.themes / "moved.json"
        self.dirs.preferences.rename(moved)
        self.dirs.preferences.symlink_to(outside)
        runenv.restore_mode(locked)
        self.assertEqual(moved.stat().st_mode & 0o7777, 0o664)
        self.assertEqual(outside.stat().st_mode & 0o7777, 0o600)

    def test_a_link_a_missing_path_or_another_file_type_is_refused_with_nothing_changed(self) -> None:
        outside = self.root / "outside"
        (outside / "gitturtle").mkdir(parents=True)
        for path in (outside, outside / "gitturtle"):
            path.chmod(0o755)
        (self.root / "run" / "data" / "link").symlink_to(outside)
        (self.root / "run" / "cache" / "store.json").symlink_to(self.dirs.preferences)
        os.mkfifo(self.root / "run" / "state" / "pipe")
        for relative, reason in (("data/link", "data/link is a symbolic link"),
                                 ("data/link/gitturtle", "data/link is a symbolic link"),
                                 ("cache/store.json", "cache/store.json is a symbolic link"),
                                 ("config/absent", "config/absent does not exist"),
                                 ("config/gitturtle/absent/x", "config/gitturtle/absent does not exist"),
                                 ("config/gitturtle/preferences.json/x", "preferences.json is not a directory"),
                                 ("state/pipe", "state/pipe is not a directory or regular file")):
            with self.subTest(relative=relative), self.assertRaisesRegex(runenv.Refusal, reason):
                runenv.lock_read_only(self.dirs.root, relative)
        self.assertEqual((outside / "gitturtle").stat().st_mode & 0o7777, 0o755)
        self.assertEqual(outside.stat().st_mode & 0o7777, 0o755)
        self.assertEqual(self.dirs.preferences.stat().st_mode & 0o7777, 0o664)


OMARCHY = {".local/state/omarchy/current/theme/colors.toml": b'accent = "#7aa2f7"\n',
           ".local/state/omarchy/current/theme.name": b"tokyo-night\n",
           ".local/state/omarchy/current/theme/light.mode": b""}


class HomeFilesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.operator = self.root / "operator"
        self.operator.mkdir()

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_paths_are_validated(self) -> None:
        for good in (".local/state/omarchy/current/theme/colors.toml", "notes.txt", ".config/x/y", "a..b/c"):
            with self.subTest(good=good):
                self.assertIsNone(runenv.home_file_problem(good))
        for bad in ("", "/etc/passwd", "../outside", "a/../../b", ".", "a/./b", "a//b", "a/", "a\\b", ".gitconfig",
                    ".gitconfig/x", "x\0y", None, 3, "x" * 301):
            with self.subTest(bad=bad):
                self.assertIsNotNone(runenv.home_file_problem(bad))
        self.assertIsNone(runenv.home_overlap(["a/b", "a/c", "ab"]))
        self.assertEqual(runenv.home_overlap(["a/b/c", "a/b"]), "'a/b' is a file, but 'a/b/c' needs it as a directory")

    def test_prepare_seeds_the_files_before_the_store(self) -> None:
        dirs = runenv.prepare(self.root / "run", b'{"version": 6}', home=self.operator, home_files=OMARCHY)
        home = dirs.paths["HOME"]
        for relative, data in OMARCHY.items():
            with self.subTest(relative=relative):
                path = home / relative
                self.assertTrue(path.is_file() and not path.is_symlink())
                self.assertEqual(path.read_bytes(), data)
                self.assertEqual(path.stat().st_mode & 0o777, 0o644 & ~self.umask())
        self.assertTrue((home / ".gitconfig").is_file())
        self.assertEqual(dirs.preferences.read_bytes(), b'{"version": 6}')

    @staticmethod
    def umask() -> int:
        mask = os.umask(0)
        os.umask(mask)
        return mask

    def test_seed_home_returns_each_digest(self) -> None:
        home = self.root / "home"
        home.mkdir()
        digests = runenv.seed_home(home, OMARCHY)
        self.assertEqual(digests, {path: stores.sha256(data) for path, data in OMARCHY.items()})
        self.assertEqual(sorted(digests), sorted(OMARCHY))

    def test_bad_files_are_refused_before_anything_is_created(self) -> None:
        for files, reason in (({"../escape": b"x"}, "without empty, . or .. parts"),
                              ({"/abs": b"x"}, "relative POSIX path"),
                              ({".gitconfig": b"[user]"}, "Git identity"),
                              ({"a": b"x", "a/b": b"y"}, "'a' is a file, but 'a/b' needs it as a directory"),
                              ({"big": b"x" * (runenv.HOME_FILE_BYTES + 1)}, "at most"),
                              ({f"f{i}": b"" for i in range(runenv.HOME_FILES + 1)}, "at most 64")):
            with self.subTest(reason=reason), self.assertRaisesRegex(runenv.Refusal, reason):
                runenv.prepare(self.root / "run", b"{}", home=self.operator, home_files=files)
            self.assertFalse((self.root / "run").exists())

    def test_a_link_or_an_existing_file_in_home_is_refused_not_followed(self) -> None:
        outside = self.root / "outside"
        outside.mkdir()
        home = self.root / "home"
        (home / ".local").mkdir(parents=True)
        (home / ".local" / "state").symlink_to(outside)
        with self.assertRaisesRegex(runenv.Refusal, r"\.local/state is not a plain directory"):
            runenv.seed_home(home, {".local/state/omarchy/current/theme.name": b"x"})
        self.assertEqual(list(outside.iterdir()), [])
        (home / "note").symlink_to(outside / "note")
        with self.assertRaisesRegex(runenv.Refusal, "HOME file note: cannot create it"):
            runenv.seed_home(home, {"note": b"x"})
        self.assertFalse((outside / "note").exists())
        (home / "plain").write_text("kept")
        with self.assertRaisesRegex(runenv.Refusal, "HOME file plain: cannot create it"):
            runenv.seed_home(home, {"plain": b"replaced"})
        self.assertEqual((home / "plain").read_text(), "kept")
        linked = self.root / "linked-home"
        linked.symlink_to(home)
        with self.assertRaises(OSError):
            runenv.seed_home(linked, {"x": b"x"})  # HOME itself is never reached through a link


DRAFTS = {"recovery-drafts.json": b'{"version": 1, "drafts": []}', "activity.json": b""}


class AppConfigFilesTest(unittest.TestCase):
    """Files seeded beside the preference store, in the app's configuration directory."""

    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.operator = self.root / "operator"
        self.operator.mkdir()

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_names_are_validated(self) -> None:
        for good in ("recovery-drafts.json", "activity.json", ".hidden", "a..b", "x" * runenv.NAME_MAX):
            with self.subTest(good=good):
                self.assertIsNone(runenv.app_config_file_problem(good))
        for bad in ("", ".", "..", "a/b", "/abs", "a/", "a\\b", "x\0y", None, 3, "x" * (runenv.NAME_MAX + 1)):
            with self.subTest(bad=bad):
                self.assertIsNotNone(runenv.app_config_file_problem(bad))
        self.assertIn("the preference store", runenv.app_config_file_problem("preferences.json"))

    def test_prepare_seeds_the_files_beside_the_store(self) -> None:
        dirs = runenv.prepare(self.root / "run", b'{"version": 6}', home=self.operator, app_config_files=DRAFTS)
        self.assertEqual(dirs.app_config, dirs.paths["XDG_CONFIG_HOME"] / "gitturtle")
        self.assertEqual(dirs.preferences.parent, dirs.app_config)
        for name, data in DRAFTS.items():
            with self.subTest(name=name):
                path = dirs.app_config / name
                self.assertTrue(path.is_file() and not path.is_symlink())
                self.assertEqual(path.read_bytes(), data)
                self.assertEqual(path.stat().st_mode & 0o777, 0o644 & ~HomeFilesTest.umask())
        self.assertEqual(dirs.preferences.read_bytes(), b'{"version": 6}')
        self.assertEqual(sorted(p.name for p in dirs.app_config.iterdir()), sorted([*DRAFTS, "preferences.json"]))
        self.assertTrue(dirs.captures.is_dir())

    def test_bad_files_are_refused_before_anything_is_created(self) -> None:
        for files, reason in (({"preferences.json": b"{}"}, "app configuration files: 'preferences.json' is the "
                                                            "preference store"),
                              ({"a/b": b"x"}, "must be a plain file name"),
                              ({"..": b"x"}, "must be a plain file name"),
                              ({"big": b"x" * (runenv.APP_CONFIG_FILE_BYTES + 1)}, "big: expected at most 1000000"),
                              ({f"f{i}": b"" for i in range(runenv.APP_CONFIG_FILES + 1)}, "65 files; at most 64")):
            with self.subTest(reason=reason), self.assertRaisesRegex(runenv.Refusal, reason):
                runenv.prepare(self.root / "run", b"{}", home=self.operator, app_config_files=files)
            self.assertFalse((self.root / "run").exists())

    def test_a_link_or_an_existing_file_is_refused_not_followed(self) -> None:
        outside = self.root / "outside"
        outside.mkdir()
        directory = self.root / "gitturtle"
        directory.mkdir()
        self.assertEqual(runenv.seed_app_config(directory, DRAFTS),
                         {name: stores.sha256(data) for name, data in DRAFTS.items()})
        (directory / "linked.json").symlink_to(outside / "linked.json")
        with self.assertRaisesRegex(runenv.Refusal, "app configuration file linked.json: cannot create it"):
            runenv.seed_app_config(directory, {"linked.json": b"x"})
        self.assertFalse((outside / "linked.json").exists())
        (directory / "preferences.json").write_text("kept")
        with self.assertRaisesRegex(runenv.Refusal, "'preferences.json' is the preference store"):
            runenv.seed_app_config(directory, {"preferences.json": b"replaced"})
        with self.assertRaisesRegex(runenv.Refusal, "app configuration file activity.json: cannot create it"):
            runenv.seed_app_config(directory, {"activity.json": b"replaced"})
        self.assertEqual((directory / "activity.json").read_bytes(), b"")
        self.assertEqual((directory / "preferences.json").read_text(), "kept")
        linked = self.root / "linked-config"
        linked.symlink_to(directory)
        with self.assertRaises(OSError):
            runenv.seed_app_config(linked, {"x": b"x"})  # the directory itself is never reached through a link
        self.assertFalse((directory / "x").exists())


class SnapshotTest(unittest.TestCase):
    """`stores.snapshot` on a real file, with a clock that advances one poll per sleep."""

    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.store = self.root / "config" / "gitturtle" / "preferences.json"
        self.store.parent.mkdir(parents=True)
        self.now, self.sleeps, self.writes = 0.0, 0, {}

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def clock(self) -> float:
        return self.now

    def sleep(self, seconds: float) -> None:
        self.now += seconds
        self.sleeps += 1
        if self.sleeps in self.writes:  # the app saves again at this poll
            data = self.writes[self.sleeps]
            partial = self.store.with_name(".preferences.json.tmp")
            partial.write_bytes(data)
            os.replace(partial, self.store)  # a new inode, as an atomic save makes

    def snap(self, relative: str = stores.PREFERENCES, quiet: float = 1.0, within: float = 10.0) -> dict:
        return stores.snapshot(self.root, relative, quiet=quiet, within=within, poll=0.25, clock=self.clock,
                               sleep=self.sleep)

    def test_a_settled_store_is_read_with_its_digest_size_and_json(self) -> None:
        data = b'{"version": 6, "settings": {"theme": "omarchy"}}'
        self.store.write_bytes(data)
        entry = self.snap()
        self.assertEqual((entry["stable"], entry["exists"], entry["sha256"], entry["bytes"]),
                         (True, True, stores.sha256(data), len(data)))
        self.assertEqual(entry["json"], {"version": 6, "settings": {"theme": "omarchy"}})
        self.assertEqual(entry["inode"], self.store.stat().st_ino)
        self.assertEqual(entry["waited_s"], 1.0)

    def test_it_waits_until_the_saves_stop(self) -> None:
        self.store.write_bytes(b'{"version": 6}')
        # Saves at 0.75, 1.5 and 2.25 s, each within the quiet second of the one before.
        self.writes = {3: b'{"version": 6, "a": 1}', 6: b'{"version": 6, "a": 2}', 9: b'{"version": 6, "a": 2}'}
        entry = self.snap()
        self.assertTrue(entry["stable"])
        self.assertEqual(entry["json"], {"version": 6, "a": 2})
        # The last save (same bytes, a new inode) still restarts the wait: quiet 1 s after it.
        self.assertEqual(entry["waited_s"], 3.25)

    def test_a_store_that_never_settles_is_returned_unstable(self) -> None:
        self.store.write_bytes(b"{}")
        self.writes = {n: f'{{"n": {n}}}'.encode() for n in range(1, 200, 2)}  # a save every half second
        entry = self.snap(quiet=1.0, within=3.0)
        self.assertFalse(entry["stable"])
        self.assertEqual(entry["waited_s"], 3.0)

    def test_an_absent_store_and_one_that_is_not_json(self) -> None:
        absent = self.snap()
        self.assertEqual((absent["stable"], absent["exists"], absent["sha256"], absent["json"]),
                         (True, False, None, None))
        self.store.write_bytes(b"{not json")
        garbled = self.snap()
        self.assertIsNone(garbled["json"])
        self.assertIn("json_error", garbled)
        self.assertEqual(garbled["bytes"], 9)

    def test_a_link_anywhere_on_the_way_is_refused_never_followed(self) -> None:
        outside = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, outside)
        (outside / "preferences.json").write_text('{"outside": true}')
        self.store.symlink_to(outside / "preferences.json")
        with self.assertRaisesRegex(OSError, "config/gitturtle/preferences.json is not a regular file"):
            self.snap()
        self.store.unlink()
        (self.root / "config" / "elsewhere.json").write_text("{}")
        self.store.symlink_to(self.root / "config" / "elsewhere.json")
        with self.assertRaisesRegex(OSError, "is not a regular file"):
            self.snap()  # inside the run directory, but still a link: never followed
        # A directory on the way that is a link, whether it leads out of the run directory or stays inside it.
        self.store.unlink()
        self.store.parent.rmdir()
        for target in (outside, self.root / "data"):
            target.mkdir(exist_ok=True)
            (target / "preferences.json").write_text("{}")
            self.store.parent.symlink_to(target)
            with self.subTest(target=target), \
                    self.assertRaisesRegex(OSError, r"config/gitturtle is not a plain directory"):
                self.snap()
            self.store.parent.unlink()
        for bad in ("../outside.json", "/etc/passwd", "config//x", "config/./x"):
            with self.subTest(bad=bad), self.assertRaisesRegex(OSError, "not a relative path"):
                self.snap(bad)
        # A directory on the way that does not exist yet is an absent store, not an error.
        self.assertEqual(self.snap("state/gitturtle/absent.json")["exists"], False)


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
