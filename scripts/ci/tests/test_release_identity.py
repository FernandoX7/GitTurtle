"""Local source preflight fixtures; these do not establish a hosted release."""

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
from dataclasses import FrozenInstanceError
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch


PROJECT = Path(__file__).resolve().parents[3]
MODULE = PROJECT / "scripts/release/identity.py"
SPEC = importlib.util.spec_from_file_location("release_identity", MODULE)
identity = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = identity
SPEC.loader.exec_module(identity)
LINUX = "x86_64-unknown-linux-gnu"
MACOS = "aarch64-apple-darwin"


class Repository(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="gitturtle-release-identity-")
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
            GIT_AUTHOR_NAME="Release Fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
            GIT_COMMITTER_NAME="Release Fixture", GIT_COMMITTER_EMAIL="fixture@example.invalid",
        )
        self.git("init", "--quiet")
        for name in ("Cargo.toml", "Cargo.lock", *(f"{member}/Cargo.toml" for member in identity.MEMBERS)):
            target = self.repo / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(PROJECT / name, target)
        (self.repo / "ordinary.txt").write_text("original source\n")
        (self.repo / ".gitignore").write_text("build/\n")
        self.commit = self.save("Fixture source")
        self.git("tag", "v0.1.0")

    def git(self, *args):
        return subprocess.run(
            ["git", "-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false", *args],
            cwd=self.repo, env=self.env, check=True, capture_output=True, timeout=10,
        ).stdout.decode().strip()

    def save(self, message):
        self.git("add", "--all")
        self.git("commit", "--quiet", "-m", message)
        return self.git("rev-parse", "HEAD")

    def verify(self, **kwargs):
        args = dict(tag="v0.1.0", version="0.1.0", commit=self.commit, platforms=[LINUX])
        args.update(kwargs)
        return identity.verify_release_identity(self.repo, **args)

    def retag(self):
        self.commit = self.save("Changed fixture")
        self.git("tag", "--force", "v0.1.0")

    def replace(self, path, old, new):
        file = self.repo / path
        original = file.read_text()
        self.assertIn(old, original)
        file.write_text(original.replace(old, new))

    def test_lightweight_identity_checks_actual_project_manifests_and_preserves_source(self):
        before = {
            name: (self.repo / ".git" / name).read_bytes()
            for name in ("HEAD", "index", "config", "refs/tags/v0.1.0")
        }
        verified = self.verify()
        self.assertEqual((verified.version, verified.commit, verified.tag_object), ("0.1.0", self.commit, self.commit))
        self.assertEqual(verified.platforms, (LINUX,))
        self.assertEqual(verified.cargo_lock_sha256, hashlib.sha256((self.repo / "Cargo.lock").read_bytes()).hexdigest())
        with self.assertRaises(FrozenInstanceError):
            verified.commit = "0" * 40
        for name, original in before.items():
            self.assertEqual((self.repo / ".git" / name).read_bytes(), original)
        self.assertFalse((self.repo / ".git/index.lock").exists())

    def test_annotated_tag_pins_both_tag_object_and_peeled_commit(self):
        self.git("tag", "--force", "-a", "v0.1.0", "-m", "Reviewed release tag")
        result = self.verify(platforms=[LINUX, MACOS])
        self.assertNotEqual(result.tag_object, result.commit)
        self.assertEqual(result.tag_object, self.git("rev-parse", "refs/tags/v0.1.0"))
        self.assertEqual(result, self.verify(platforms=[MACOS, LINUX], expected_tag_object=result.tag_object))

    def test_version_tag_without_prefix_is_explicitly_supported(self):
        self.git("tag", "0.1.0")
        self.assertEqual(self.verify(tag="0.1.0").tag, "0.1.0")

    def test_branch_with_same_name_does_not_substitute_for_missing_tag(self):
        self.git("tag", "--delete", "v0.1.0")
        self.git("branch", "v0.1.0")
        with self.assertRaises(identity.IdentityError):
            self.verify()

    def test_tag_moved_to_another_commit_is_refused(self):
        (self.repo / "ordinary.txt").write_text("next source\n")
        next_commit = self.save("Next commit")
        self.git("tag", "--force", "v0.1.0", next_commit)
        with self.assertRaisesRegex(identity.IdentityError, "peel"):
            self.verify()

    def test_annotated_tag_rewritten_at_same_commit_is_refused_when_pinned(self):
        self.git("tag", "--force", "-a", "v0.1.0", "-m", "First annotation")
        prior = self.verify()
        self.git("tag", "--force", "-a", "v0.1.0", "-m", "Different annotation")
        with self.assertRaisesRegex(identity.IdentityError, "tag object changed"):
            self.verify(expected_tag_object=prior.tag_object)

    def test_tag_to_a_tree_is_not_a_release_commit(self):
        self.git("tag", "--force", "v0.1.0", self.git("rev-parse", "HEAD^{tree}"))
        with self.assertRaises(identity.IdentityError):
            self.verify()

    def test_different_checkout_head_is_refused(self):
        (self.repo / "ordinary.txt").write_text("next source\n")
        self.save("Different checkout")
        with self.assertRaisesRegex(identity.IdentityError, "HEAD"):
            self.verify()

    def test_dirty_tracked_untracked_and_staged_source_are_refused(self):
        for kind in ("tracked", "untracked", "staged"):
            with self.subTest(kind=kind):
                target = self.repo / ("new.txt" if kind == "untracked" else "ordinary.txt")
                target.write_text("uncommitted\n")
                if kind == "staged":
                    self.git("add", "--", "ordinary.txt")
                with self.assertRaisesRegex(identity.IdentityError, "dirty"):
                    self.verify()
                if kind == "untracked":
                    target.unlink()
                else:
                    self.git("restore", "--staged", "--worktree", "--", "ordinary.txt")

    def test_ignored_build_outputs_match_native_build_info_clean_semantics(self):
        (self.repo / "build").mkdir()
        (self.repo / "build/package.tar.gz").write_bytes(b"not a release artifact")
        self.assertEqual(self.verify().commit, self.commit)

    def test_permissive_stat_config_cannot_hide_changed_source_bytes(self):
        self.git("config", "core.trustctime", "false")
        self.git("config", "core.checkStat", "minimal")
        target = self.repo / "ordinary.txt"
        # Avoid Git's racy-clean timestamp exception without a wall-clock sleep.
        past = time.time_ns() - 5_000_000_000
        os.utime(target, ns=(past, past))
        self.git("update-index", "--refresh")
        before = target.stat()
        target.write_text("modified source\n")
        os.utime(target, ns=(before.st_atime_ns, before.st_mtime_ns))
        self.assertEqual(target.stat().st_size, before.st_size)
        self.assertEqual(self.git("status", "--porcelain"), "")
        with self.assertRaisesRegex(identity.IdentityError, "dirty|tracked source bytes"):
            self.verify()

    def test_local_filemode_config_cannot_hide_changed_executable_mode(self):
        self.git("config", "core.filemode", "false")
        target = self.repo / "ordinary.txt"
        target.chmod(0o755)
        self.assertEqual(self.git("status", "--porcelain"), "")
        with self.assertRaisesRegex(identity.IdentityError, "dirty"):
            self.verify()

    def test_committed_symlink_target_is_checked_without_following_it(self):
        (self.repo / "link").symlink_to("/nonexistent/external-target")
        self.retag()
        self.assertEqual(self.verify().commit, self.commit)

    def test_git_clean_eol_transformation_is_refused_as_different_release_bytes(self):
        (self.repo / ".gitattributes").write_text("ordinary.txt text eol=crlf\n")
        self.retag()
        (self.repo / "ordinary.txt").unlink()
        self.git("restore", "--", "ordinary.txt")
        self.assertEqual((self.repo / "ordinary.txt").read_bytes(), b"original source\r\n")
        self.assertEqual(self.git("status", "--porcelain"), "")
        with self.assertRaisesRegex(identity.IdentityError, "tracked source bytes"):
            self.verify()

    def test_raw_source_verification_has_a_byte_budget(self):
        with patch.object(identity, "MAX_SOURCE_BYTES", 1):
            with self.assertRaisesRegex(identity.IdentityError, "byte limit"):
                self.verify()

    def test_mismatched_root_version_is_refused(self):
        self.git("tag", "v0.2.0")
        with self.assertRaisesRegex(identity.IdentityError, "workspace version"):
            self.verify(tag="v0.2.0", version="0.2.0")

    def test_index_flags_cannot_hide_uncommitted_source(self):
        for flag in ("--assume-unchanged", "--skip-worktree"):
            with self.subTest(flag=flag):
                self.git("update-index", flag, "Cargo.toml")
                self.replace("Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"')
                with self.assertRaisesRegex(identity.IdentityError, "sparse/assumed"):
                    self.verify()
                self.git("update-index", "--no-assume-unchanged", "Cargo.toml")
                self.git("update-index", "--no-skip-worktree", "Cargo.toml")
                self.git("restore", "--", "Cargo.toml")

    def test_tag_version_disagreement_is_refused_before_reading_git(self):
        with patch.object(identity._Git, "read", side_effect=AssertionError("must validate first")):
            with self.assertRaisesRegex(identity.IdentityError, "tag must equal"):
                self.verify(tag="v0.2.0")

    def test_hardcoded_core_version_mismatch_is_refused(self):
        self.replace("crates/git-core/Cargo.toml", 'version = "0.1.0"', 'version = "0.2.0"')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "gitturtle-core version"):
            self.verify()

    def test_app_literal_version_is_resolved_and_not_assumed_inherited(self):
        self.replace("crates/app/Cargo.toml", "version.workspace = true", 'version = "0.2.0"')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "gitturtle version"):
            self.verify()

    def test_matching_literal_app_and_inherited_core_versions_are_supported(self):
        self.replace("crates/app/Cargo.toml", "version.workspace = true", 'version = "0.1.0"')
        self.replace("crates/git-core/Cargo.toml", 'version = "0.1.0"', "version.workspace = true")
        self.retag()
        self.assertEqual(self.verify().version, "0.1.0")

    def test_false_or_malformed_version_inheritance_is_refused(self):
        for value in ("false", "1", '"true"'):
            with self.subTest(value=value):
                path = self.repo / "crates/preview/Cargo.toml"
                text = (PROJECT / "crates/preview/Cargo.toml").read_text()
                path.write_text(text.replace("version.workspace = true", f"version.workspace = {value}"))
                self.retag()
                with self.assertRaisesRegex(identity.IdentityError, "gitturtle-preview version"):
                    self.verify()

    def test_missing_and_stale_workspace_lock_versions_are_refused(self):
        self.replace("Cargo.lock", 'name = "gitturtle-core"\nversion = "0.1.0"',
                     'name = "gitturtle-core"\nversion = "0.2.0"')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "Cargo.lock"):
            self.verify()
        self.replace("Cargo.lock", 'name = "gitturtle-core"', 'name = "other"')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "Cargo.lock"):
            self.verify()

    def test_unreviewed_workspace_and_implicit_path_members_fail_clearly(self):
        self.replace("Cargo.toml", '"crates/preview"]', '"crates/*"]')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "workspace membership"):
            self.verify()
        shutil.copyfile(PROJECT / "Cargo.toml", self.repo / "Cargo.toml")
        self.replace("crates/app/Cargo.toml", '[dependencies]', '[dependencies]\nother = { path = "../other" }')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "local dependency/workspace"):
            self.verify()

    def test_explicit_other_workspace_is_refused(self):
        self.replace("crates/app/Cargo.toml", "[package]", '[package]\nworkspace = "../../elsewhere"')
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "package/workspace"):
            self.verify()

    def test_bad_toml_and_symlink_manifest_are_refused(self):
        file = self.repo / "crates/preview/Cargo.toml"
        file.write_bytes(b"[invalid\xff")
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "UTF-8 TOML"):
            self.verify()
        file.unlink()
        file.symlink_to("../app/Cargo.toml")
        self.retag()
        with self.assertRaisesRegex(identity.IdentityError, "regular file"):
            self.verify()

    def test_linux_prerelease_is_exact_and_macos_mapping_remains_explicit(self):
        version = "0.1.0-rc.1+test.5"
        for name in ("Cargo.toml", "crates/git-core/Cargo.toml", "Cargo.lock"):
            self.replace(name, '"0.1.0"', f'"{version}"')
        self.commit = self.save("Prerelease fixture")
        self.git("tag", "v" + version)
        self.assertEqual(self.verify(tag="v" + version, version=version).version, version)
        with self.assertRaisesRegex(identity.IdentityError, "plist mapping"):
            self.verify(tag="v" + version, version=version, platforms=[MACOS, LINUX])

    def test_inherited_git_targeting_and_replacement_objects_are_ignored(self):
        self.git("replace", self.commit, self.commit)
        with patch.dict(os.environ, {"GIT_DIR": "/missing", "GIT_WORK_TREE": "/wrong",
                                     "GIT_INDEX_FILE": "/wrong-index", "GIT_CONFIG_COUNT": "1",
                                     "GIT_CONFIG_KEY_0": "core.bare", "GIT_CONFIG_VALUE_0": "true"}):
            self.assertEqual(self.verify().commit, self.commit)

    def test_status_does_not_run_clean_filters_or_fsmonitor(self):
        sentinel = self.repo / ".git/filter-ran"
        command = "touch " + str(sentinel)
        self.git("config", "filter.probe.clean", command)
        self.git("config", "filter.probe.required", "true")
        self.git("config", "core.fsmonitor", command)
        (self.repo / ".gitattributes").write_text("ordinary.txt filter=probe\n")
        # Commit only attributes before exercising an invalidated ordinary.txt stat.
        self.git("-c", "core.fsmonitor=false", "add", "--", ".gitattributes")
        self.git("-c", "core.fsmonitor=false", "commit", "--quiet", "-m", "Fixture filter config")
        self.commit = self.git("rev-parse", "HEAD")
        self.git("tag", "--force", "v0.1.0")
        sentinel.unlink(missing_ok=True)
        (self.repo / "ordinary.txt").write_text("changed input\n")
        with self.assertRaisesRegex(identity.IdentityError, "dirty"):
            self.verify()
        self.assertFalse(sentinel.exists())

    def test_detects_tag_change_during_preflight(self):
        original = identity._versions
        def changed(*args):
            result = original(*args)
            self.git("tag", "--force", "-a", "v0.1.0", "-m", "Changed during preflight")
            return result
        with patch.object(identity, "_versions", side_effect=changed):
            with self.assertRaisesRegex(identity.IdentityError, "changed during preflight"):
                self.verify()

    def test_cli_outputs_identity_only_after_all_declared_platforms_match(self):
        args = ["--repo", str(self.repo), "--tag", "v0.1.0", "--version", "0.1.0",
                "--commit", self.commit, "--platform", LINUX]
        output, error = io.StringIO(), io.StringIO()
        with redirect_stdout(output), redirect_stderr(error):
            result = identity.main(args + ["--available-platform", LINUX])
        self.assertEqual(result, 0)
        self.assertEqual(json.loads(output.getvalue())["commit"], self.commit)
        self.assertEqual(error.getvalue(), "")
        output, error = io.StringIO(), io.StringIO()
        with redirect_stdout(output), redirect_stderr(error):
            result = identity.main(args + ["--platform", MACOS, "--available-platform", LINUX])
        self.assertEqual(result, 2)
        self.assertEqual(output.getvalue(), "")
        self.assertIn("missing=['aarch64-apple-darwin']", error.getvalue())


class InputsAndBounds(unittest.TestCase):
    def test_missing_duplicate_extra_and_unknown_platforms_are_refused(self):
        for expected, available in (([LINUX, MACOS], [LINUX]), ([LINUX], [LINUX, MACOS]),
                                    ([], [LINUX]), ([LINUX], []), ([LINUX], [LINUX, LINUX]),
                                    ([LINUX], ["windows"]), ([LINUX], LINUX)):
            with self.subTest(expected=expected, available=available):
                with self.assertRaises(identity.IdentityError):
                    identity.require_complete_platforms(expected, available)
        identity.require_complete_platforms([LINUX], [LINUX])
        identity.require_complete_platforms([LINUX, MACOS], [MACOS, LINUX])

    def test_malformed_untrusted_inputs_fail_before_git(self):
        good = dict(tag="v0.1.0", version="0.1.0", commit="a" * 40, platforms=[LINUX])
        bad = {
            "tag": ["", "--help", "refs/tags/v0.1.0", "$(touch marker)", "v0.1.0\n", None],
            "version": ["0.1", "01.1.0", "0.1.0-01", "0.1.0+", "0.1.0-", "1" * 129, 1],
            "commit": ["main", "a" * 39, "a" * 41, "A" * 40, "a" * 40 + "^{commit}", None],
            "platforms": [[], [LINUX, LINUX], ["linux"], LINUX, [None], None],
            "expected_tag_object": ["main", "a" * 39],
        }
        with patch.object(identity._Git, "read", side_effect=AssertionError("must validate first")):
            for field, values in bad.items():
                for value in values:
                    with self.subTest(field=field, value=value):
                        with self.assertRaises(identity.IdentityError):
                            identity.verify_release_identity("/missing", **(good | {field: value}))

    def test_git_output_is_bounded_while_produced(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "git"
            fake.write_text(f"#!{sys.executable}\nimport os\nwhile True: os.write(1, b'x' * 8192)\n")
            fake.chmod(0o700)
            with patch.dict(os.environ, {"PATH": directory}), patch.object(identity, "MAX_OUTPUT_BYTES", 10000):
                with self.assertRaisesRegex(identity.IdentityError, "output limit"):
                    identity._Git(Path(directory)).read(("rev-parse", "HEAD"))

    def test_git_timeout_covers_a_pipe_holding_descendant(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "git"
            fake.write_text(
                f"#!{sys.executable}\nimport os, time\n"
                "if os.fork() == 0: time.sleep(30)\nelse: os._exit(0)\n"
            )
            fake.chmod(0o700)
            start = time.monotonic()
            with patch.dict(os.environ, {"PATH": directory}), patch.object(identity, "GIT_TIMEOUT_SECONDS", 0.1):
                with self.assertRaisesRegex(identity.IdentityError, "time limit"):
                    identity._Git(Path(directory)).read(("rev-parse", "HEAD"))
            self.assertLess(time.monotonic() - start, 3)


if __name__ == "__main__":
    unittest.main()
