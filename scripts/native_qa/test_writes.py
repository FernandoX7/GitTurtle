"""Scenarios that write: per-launch fixture copies, the declared changes and their verdict, on real Git repositories.

No display, bus or app: a launch's writes are made by `git` in the copy, as the app's Delete branch and Push do.
"""

from __future__ import annotations

import contextlib
import copy
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from native_qa import evidence, play, recipe, runenv, scenario, writes

QA = Path(__file__).resolve().parent / "qa.py"
# Each test's global Git configuration, as scripts/operator/test_land.py gives its own: automatic maintenance stays
# off, so no detached Git child is still writing a temporary repository when the test removes it. A file, unlike
# GIT_CONFIG_COUNT, also reaches the receive-pack of a local push.
GIT_CONFIG = ("[user]\n\tname = GitTurtle QA\n\temail = qa@example.invalid\n[maintenance]\n\tauto = false\n"
              "[gc]\n\tauto = 0\n\tautoDetach = false\n")
# The delete-then-push route's fixture: `scoped` is already in main, so a safe delete accepts it, and main is one
# commit ahead of origin/main, which it tracks, so a Push really writes.
RECIPE = {
    "name": "vanished-scope",
    "description": "a scoped branch already in main; main one commit ahead of its local bare origin",
    "clock": {"start": "2026-09-01T09:00:00Z", "step": 60},
    "operations": [
        {"commit": "Start", "files": {"README.md": "# Fixture\n"}},
        {"commit": "Scoped work", "files": {"notes.md": "scoped\n"}},
        {"branch": "scoped", "at": "@1"},
        {"branch": "keep", "at": "@0"},
        {"remote": "origin", "bare": "remotes/origin.git"},
        {"push": "origin", "refs": ["main"], "set_upstream": True},
        {"commit": "Ahead of origin", "files": {"later.md": "ahead\n"}},
        {"worktree": "worktrees/side", "new_branch": "side", "at": "@0"},
    ],
}
WRITES = {"refs": {"refs/heads/scoped": "deleted", "refs/remotes/origin/main": "refs/heads/main"},
          "remotes": {"origin": {"refs": {"refs/heads/main": "refs/heads/main"}}}}
SPEC = {
    "version": 1, "task": "history-vanished-scope-refresh",
    "variants": [{"palette": "midnight", "text_size": 13}, {"palette": "porcelain", "text_size": 18}],
    "fixture": RECIPE, "writes": WRITES,
    "steps": [{"key": "r", "mods": ["Control_L"], "note": "Refresh"}, {"capture": "after", "commit": False}],
}


def spec(**changes) -> dict:
    data = copy.deepcopy(SPEC)
    data.update(changes)
    return data


class WritesSpecTest(unittest.TestCase):
    def test_one_declaration_serves_every_role(self) -> None:
        loaded = scenario.validate(spec())
        declared = {"refs": WRITES["refs"], "config": {},
                    "remotes": {"origin": {"refs": {"refs/heads/main": "refs/heads/main"}, "config": {}}}}
        self.assertEqual(loaded["writes"], {"base": declared, "cand": declared})
        self.assertIsNone(scenario.validate({k: v for k, v in spec().items() if k != "writes"})["writes"])

    def test_a_declaration_per_role(self) -> None:
        loaded = scenario.validate(spec(writes={"base": {"refs": {"refs/heads/scoped": "deleted"}}, "cand": WRITES}))
        self.assertEqual(loaded["writes"]["base"], {"refs": {"refs/heads/scoped": "deleted"}, "config": {},
                                                    "remotes": {}})
        self.assertEqual(loaded["writes"]["cand"]["remotes"]["origin"]["refs"], {"refs/heads/main": "refs/heads/main"})
        only = scenario.validate(spec(roles=["cand"], writes={"cand": {}}))
        self.assertEqual(only["writes"], {"cand": {"refs": {}, "config": {}, "remotes": {}}})

    def test_config_is_declared_key_by_key_as_git_names_it(self) -> None:
        loaded = scenario.validate(spec(writes={
            "config": {"Branch.Keep.Remote": "origin", "branch.keep.merge": ["refs/heads/keep"],
                       "branch.scoped.merge": "deleted", "remote.origin.push": ["deleted"]},
            "remotes": {"origin": {"config": {"receive.denyDeletes": "true"}}}}))["writes"]["cand"]
        self.assertEqual(loaded["config"], {"branch.Keep.remote": ["origin"], "branch.keep.merge": ["refs/heads/keep"],
                                            "branch.scoped.merge": "deleted", "remote.origin.push": ["deleted"]})
        self.assertEqual(loaded["remotes"]["origin"], {"refs": {}, "config": {"receive.denydeletes": ["true"]}})

    def test_set_upstream_changes_no_earlier_recipe(self) -> None:
        plain = copy.deepcopy(RECIPE)
        del plain["operations"][5]["set_upstream"]
        normalised = scenario.fixture_recipe(plain, "$.fixture")
        self.assertNotIn("set_upstream", normalised["operations"][5])
        tracking = scenario.fixture_recipe(copy.deepcopy(RECIPE), "$.fixture")
        self.assertIs(tracking["operations"][5]["set_upstream"], True)
        self.assertNotEqual(recipe.recipe_sha256(normalised), recipe.recipe_sha256(tracking))
        plain["operations"][5]["set_upstream"] = False
        self.assertEqual(recipe.recipe_sha256(scenario.fixture_recipe(plain, "$.fixture")),
                         recipe.recipe_sha256(normalised))

    def test_bad_writes(self) -> None:
        no_fixture = {key: value for key, value in spec().items() if key != "fixture"}
        upstream = copy.deepcopy(RECIPE)
        upstream["operations"][5]["set_upstream"] = "yes"
        for data, match in (
                (no_fixture, r"\$\.writes: a scenario that writes needs a fixture recipe"),
                (spec(writes=[]), r"\$\.writes: expected an object"),
                (spec(writes={"refs": {}, "colour": 1}), "unknown key"),
                (spec(writes={"refs": {"heads/scoped": "deleted"}}),
                 r"\$\.writes\.refs: 'heads/scoped' does not match"),
                (spec(writes={"refs": {"refs/heads/scoped": "gone"}}), "does not match"),
                (spec(writes={"refs": {"refs/heads/scoped": 3}}), "non-empty string"),
                (spec(writes={"refs": {"refs/heads/scoped": "@9"}}), "@9 names a commit the recipe never makes"),
                (spec(writes={"remotes": {"upstream": {"refs/heads/main": "@2"}}}),
                 "'upstream' is not a remote the recipe defines"),
                (spec(writes={"remotes": {"origin": {"main": "@2"}}}),
                 r"\$\.writes\.remotes\.origin: unknown key\(s\) main"),
                (spec(writes={"remotes": {"origin": {"refs": {"main": "@2"}}}}),
                 r"\$\.writes\.remotes\.origin\.refs: 'main' does not match"),
                (spec(writes={"config": {"branch": "x"}}), r"\$\.writes\.config: 'branch' does not match"),
                (spec(writes={"config": {"branch.main.remote": 1}}), "non-empty string"),
                (spec(writes={"config": {"branch.main.remote": []}}), "expected a list of 1 to 16"),
                (spec(writes={"config": {"Gc.Auto": "1", "gc.auto": "0"}}), "gc.auto is declared twice"),
                (spec(writes={"remotes": {"origin": {"config": {"gc auto": "0"}}}}),
                 r"\$\.writes\.remotes\.origin\.config: 'gc auto' does not match"),
                (spec(writes={"base": WRITES}), "exactly the scenario's roles: base, cand"),
                (spec(writes={"refs": {"refs/heads/scoped": "deleted",
                                       "refs/remotes/origin/main": "refs/heads/scoped"}}),
                 "declared deleted, so nothing can equal it"),
                (spec(writes={"remotes": {"origin": {"refs": {"refs/heads/main": "refs/heads/scoped"}}},
                              "refs": {"refs/heads/scoped": "deleted"}}), "declared deleted"),
                (spec(writes={"refs": {"refs/heads/main": "refs/heads/main"}}), "equal to itself"),
                (spec(fixture=upstream), r"operations\[5\]\.set_upstream: expected true or false"),
                (spec(fixture=dict(RECIPE, operations=RECIPE["operations"][:1] + [
                    {"remote": "o", "bare": ".gitturtle-fixture-copy.json"}])),
                 "'.gitturtle-fixture-copy.json' overlaps '.gitturtle-fixture-copy.json'"),
                (spec(fixture=dict(RECIPE, operations=RECIPE["operations"][:1] + [
                    {"worktree": ".fixture-manifest.json.partial/w", "at": "@0"}])), "overlaps"),
        ):
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                scenario.validate(data)

    def test_a_scenario_that_writes_never_copies_a_supplied_fixture(self) -> None:
        with self.assertRaisesRegex(SystemExit, "a --fixture is never copied or written to"):
            writes.plan(scenario.validate(spec()), None)
        self.assertIsNone(writes.plan(scenario.validate({k: v for k, v in spec().items() if k != "writes"}), None))

    def test_cli_check_lists_the_declared_writes(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            good, bad = Path(scratch) / "good.json", Path(scratch) / "bad.json"
            good.write_text(json.dumps(spec()))
            bad.write_text(json.dumps(spec(writes={"remotes": {"upstream": {}}})))
            ok = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(good)], capture_output=True,
                                text=True)
            refused = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(bad)],
                                     capture_output=True, text=True)
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertIn("writes: every launch opens its own copy of the recipe build, <fixtures>/vanished-scope_copy/",
                      ok.stdout)
        self.assertIn("cand: refs/heads/scoped deleted, refs/remotes/origin/main -> refs/heads/main, "
                      "origin:refs/heads/main -> refs/heads/main; everything else unchanged", ok.stdout)
        self.assertEqual(refused.returncode, 2)
        self.assertIn("$.writes.remotes: 'upstream' is not a remote the recipe defines", refused.stderr)


@unittest.skipUnless(shutil.which("git"), "no git")
class GitCase(unittest.TestCase):
    def setUp(self) -> None:
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name).resolve()
        config = self.root / "gitconfig"
        config.write_text(GIT_CONFIG)
        environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(config))
        patcher = mock.patch.dict(os.environ, environment, clear=True)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.fixtures = self.root / "fixtures"
        self.spec = scenario.validate(spec(), "f" * 64)
        self.manifest = recipe.build(self.spec["fixture"], self.fixtures)
        self.source = Path(self.manifest["root"])
        self.plan = writes.plan(self.spec, self.manifest)

    def git(self, repo: Path, *args: str) -> str:
        """Git as the app runs it in the copy: the test's isolated environment."""
        return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                              text=True).stdout.strip()

    def route(self, repo: Path, push: bool = True) -> None:
        """What the route does through the app: delete the scoped branch safely, then push main to origin."""
        self.git(repo, "branch", "-d", "scoped")
        if push:
            self.git(repo, "push", "--porcelain", "origin", "refs/heads/main:refs/heads/main")


class CopyTest(GitCase):
    def test_a_copy_is_isolated_and_its_remotes_and_worktrees_point_into_it(self) -> None:
        self.assertEqual(self.plan.root, self.fixtures / "vanished-scope_copy")
        made = writes.fresh_copy(self.plan, "cand")
        repo, root = made.repository, self.plan.root
        self.assertEqual(repo, root / "repo")
        self.assertEqual(self.git(repo, "config", "remote.origin.url"), str(root / "remotes" / "origin.git"))
        self.assertEqual(self.git(self.source / "repo", "config", "remote.origin.url"),
                         str(self.source / "remotes" / "origin.git"))
        self.assertEqual(self.git(repo, "config", "branch.main.remote"), "origin")  # set_upstream in the recipe
        self.assertEqual(self.git(root / "remotes" / "origin.git", "config", "receive.autogc"), "false")
        writes.local_remotes(repo, root)
        with self.assertRaisesRegex(SystemExit, "outside"):
            writes.local_remotes(repo, self.source)  # nothing in the copy names the build any more
        listing = self.git(repo, "worktree", "list", "--porcelain").splitlines()
        self.assertEqual([line[9:] for line in listing if line.startswith("worktree ")],
                         [str(repo), str(root / "worktrees" / "side")])
        self.assertEqual(self.git(root / "worktrees" / "side", "rev-parse", "--path-format=absolute",
                                  "--git-common-dir"), str(repo / ".git"))
        self.assertEqual(made.before.record["refs"], self.manifest["refs"])
        self.assertEqual(json.loads((root / writes.MARKER).read_text())["role"], "cand")
        self.assertEqual(self.git(repo, "rev-list", "--count", "origin/main..main"), "1")
        # The route and a commit in the copy's linked worktree write to the copy alone.
        self.route(repo)
        self.git(root / "worktrees" / "side", "commit", "--allow-empty", "-q", "-m", "in the copy's worktree")
        self.assertEqual(writes.ref_map(root / "remotes" / "origin.git"),
                         {"refs/heads/main": self.manifest["commits"][2]})
        self.assertEqual(writes.ref_map(self.source / "remotes" / "origin.git"),
                         {"refs/heads/main": self.manifest["commits"][1]})
        self.assertEqual(recipe.build_state(self.manifest), self.manifest["state"])
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertTrue(recipe.build(self.spec["fixture"], self.fixtures)["reused"])
        # One path for every launch: a second copy waits until the first is gone, which only its token removes.
        with self.assertRaisesRegex(SystemExit, "vanished-scope_copy exists .*a launch in progress, cand"):
            writes.fresh_copy(self.plan, "base")
        with self.assertRaisesRegex(OSError, "not the copy this launch made"):
            writes.remove(root, "0" * 32)
        writes.remove(root, made.token)
        self.assertFalse(root.exists())

    def test_the_declared_route_passes_and_its_copy_is_removed(self) -> None:
        made = writes.fresh_copy(self.plan, "cand")
        scoped, main, pushed = (self.manifest["refs"]["refs/heads/scoped"], self.manifest["commits"][2],
                                self.manifest["commits"][1])
        self.route(made.repository)
        entry = writes.conclude(made, writes.EXITED)
        verdict = entry["verdict"]
        self.assertEqual(verdict["result"], "pass", verdict["problems"])
        self.assertEqual(verdict["changes"], [
            f"refs/heads/scoped: {scoped[:12]} -> absent",
            f"refs/remotes/origin/main: {pushed[:12]} -> {main[:12]}",
            "reflog refs/heads/scoped: removed with its ref",
            "reflog refs/remotes/origin/main: 1 entries appended",
            f"origin:refs/heads/main: {pushed[:12]} -> {main[:12]}",
        ])
        self.assertEqual(entry["before"]["refs"]["refs/heads/scoped"], scoped)
        self.assertNotIn("refs/heads/scoped", entry["after"]["refs"])
        self.assertEqual(entry["after"]["remotes"]["origin"]["refs"], {"refs/heads/main": main})
        # The app's safe delete of a branch without configuration and its plain Push of a branch that already tracks
        # origin change no configuration and no special ref, here or in the bare remote.
        for key in ("config", "config_files", "special_refs"):
            self.assertEqual(entry["after"][key], entry["before"][key])
            self.assertEqual(entry["after"]["remotes"]["origin"][key], entry["before"]["remotes"]["origin"][key])
        self.assertEqual(entry["after"]["config"]["branch.main.merge"], ["refs/heads/main"])
        self.assertFalse(entry["kept"])
        self.assertFalse(self.plan.root.exists())
        json.dumps(entry)
        # The next launch starts again from the build, with the branch the last one deleted.
        again = writes.fresh_copy(self.plan, "base")
        self.assertEqual(again.before.record["refs"]["refs/heads/scoped"], scoped)
        self.assertFalse(writes.conclude(again, writes.EXITED)["kept"])  # untouched: removed, though not as declared

    def test_a_mismatch_names_each_unexpected_and_missing_change_and_keeps_the_copy(self) -> None:
        made = writes.fresh_copy(self.plan, "cand")
        repo = made.repository
        self.route(repo, push=False)
        self.git(repo, "branch", "stray", "main")
        self.git(repo, "branch", "-f", "keep", "main")
        self.git(repo, "branch", "-f", "keep", self.manifest["commits"][0])  # the same ref value, a longer reflog
        entry = writes.conclude(made, writes.EXITED)
        problems = entry["verdict"]["problems"]
        self.assertEqual(entry["verdict"]["result"], "fail")
        main, pushed = self.manifest["commits"][2][:12], self.manifest["commits"][1][:12]
        self.assertEqual(problems, [
            f"refs/heads/stray: unexpected change absent -> {main}",
            f"refs/remotes/origin/main: missing change to {main} (refs/heads/main), found {pushed}",
            "reflog refs/heads/keep: unexpected change (only a declared ref's reflog may change)",
            "reflog refs/heads/stray: unexpected change (only a declared ref's reflog may change)",
            f"origin:refs/heads/main: missing change to {main} (refs/heads/main), found {pushed}",
        ])
        self.assertEqual((entry["kept"], entry["kept_because"]), (True, "its changes differ from the declaration"))
        self.assertEqual(json.loads((self.plan.root / writes.MARKER).read_text())["kept_because"],
                         "its changes differ from the declaration")
        for attempt in (lambda: writes.plan(self.spec, self.manifest), lambda: writes.fresh_copy(self.plan, "cand")):
            with self.assertRaisesRegex(SystemExit, "exists \\(its changes differ from the declaration, cand"):
                attempt()
        self.assertEqual(recipe.build_state(self.manifest), self.manifest["state"])

    def test_configuration_and_special_refs_change_only_as_declared(self) -> None:
        made = writes.fresh_copy(self.plan, "cand")
        self.addCleanup(writes.remove, made.root, made.token)
        repo, bare = made.repository, made.remotes["origin"]
        url = self.git(repo, "config", "remote.origin.url")
        # A Push of a branch without an upstream records one (the app passes --set-upstream then).
        self.git(repo, "push", "--porcelain", "--set-upstream", "origin", "refs/heads/keep:refs/heads/keep")
        self.git(repo, "config", "remote.origin.url", f"{url}.moved")
        self.git(repo, "update-ref", "ORIG_HEAD", "HEAD")
        self.git(bare, "config", "receive.denyDeletes", "true")
        after = writes.snapshot(repo, made.remotes)
        declared = {"refs": {"refs/remotes/origin/keep": "refs/heads/keep"},
                    "remotes": {"origin": {"refs": {"refs/heads/keep": "refs/heads/keep"}}}}
        self.assertEqual(writes.compare(declared, made.before, after, made.commits)["problems"], [
            "special ref ORIG_HEAD: unexpected change (created; it cannot be declared)",
            "config branch.keep.merge: unexpected change unset -> 'refs/heads/keep'",
            "config branch.keep.remote: unexpected change unset -> 'origin'",
            f"config remote.origin.url: unexpected change {url!r} -> '{url}.moved'",
            "origin:config receive.denydeletes: unexpected change unset -> 'true'",
        ])
        declared["config"] = {"branch.keep.merge": ["refs/heads/keep"], "branch.keep.remote": ["origin"],
                              "remote.origin.url": [f"{url}.moved"]}
        declared["remotes"]["origin"]["config"] = {"receive.denydeletes": ["true"]}
        verdict = writes.compare(declared, made.before, after, made.commits)
        self.assertEqual(verdict["problems"], ["special ref ORIG_HEAD: unexpected change (created; it cannot be "
                                               "declared)"])
        self.assertIn("config branch.keep.remote: unset -> 'origin'", verdict["changes"])
        declared["config"]["branch.main.merge"] = "deleted"
        declared["config"]["branch.main.remote"] = ["origin"]
        self.assertEqual(writes.compare(declared, made.before, after, made.commits)["problems"][1:], [
            "config branch.main.merge: missing deletion, still 'refs/heads/main'",
            "config branch.main.remote: already 'origin' before the launch, so the declared change cannot be told "
            "apart from no write",
        ])

    def test_a_build_whose_bare_remote_changed_is_refused(self) -> None:
        self.assertEqual(set(self.manifest["state"]["remotes"]), {"origin"})
        path = self.source / recipe.MANIFEST
        recorded = path.read_text()
        older = json.loads(recorded)
        del older["state"]["remotes"]  # as built before the bare remotes' state was recorded
        path.write_text(json.dumps(older))
        with self.assertRaisesRegex(SystemExit, "built before its bare remotes' state was recorded"):
            recipe.build(self.spec["fixture"], self.fixtures)
        path.write_text(recorded)
        bare = self.source / "remotes" / "origin.git"
        for change in (("update-ref", "refs/heads/stray", self.manifest["commits"][0]),
                       ("config", "receive.denyDeletes", "true")):
            with self.subTest(change=change[0]):
                self.git(bare, *change)
                with self.assertRaisesRegex(SystemExit, r"changed since it was built \(remotes\)"):
                    recipe.build(self.spec["fixture"], self.fixtures)
                with self.assertRaisesRegex(SystemExit, "recipe build .* changed since it was built"):
                    writes.fresh_copy(self.plan, "cand")
                self.assertFalse(self.plan.root.exists())

    def test_head_and_its_reflog_follow_only_a_declared_branch(self) -> None:
        made = writes.fresh_copy(self.plan, "cand")
        repo = made.repository
        self.addCleanup(writes.remove, made.root, made.token)
        # A commit with HEAD's own tree, moved through HEAD: main, HEAD and both reflogs change, the checkout does not.
        made_by_app = self.git(repo, "commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "made by the app")
        self.git(repo, "update-ref", "-m", "app", "HEAD", made_by_app)
        after = writes.snapshot(repo, made.remotes)
        none = writes.compare({"refs": {}, "remotes": {}}, made.before, after, made.commits)
        self.assertEqual([problem.split(":")[0] for problem in none["problems"]],
                         ["refs/heads/main", "HEAD", "reflog HEAD", "reflog refs/heads/main"])
        declared = writes.compare({"refs": {"refs/heads/main": made_by_app}, "remotes": {}}, made.before, after,
                                  made.commits)
        self.assertEqual(declared["result"], "pass", declared["problems"])
        # A declared ref's reflog may grow, never be rewritten.
        log = repo / ".git" / "logs" / "refs" / "heads" / "main"
        log.write_bytes(log.read_bytes().splitlines(keepends=True)[-1])
        rewritten = writes.compare({"refs": {"refs/heads/main": made_by_app}, "remotes": {}}, made.before,
                                   writes.snapshot(repo, made.remotes), made.commits)
        self.assertEqual(rewritten["problems"], ["reflog refs/heads/main: rewritten; a write only appends to a reflog"])

    def test_a_declared_change_must_be_seen(self) -> None:
        made = writes.fresh_copy(self.plan, "cand")
        self.addCleanup(writes.remove, made.root, made.token)
        declared = {"refs": {"refs/heads/keep": "@0", "refs/heads/ghost": "deleted",
                             "refs/heads/new": "refs/heads/gone", "refs/heads/scoped": "deleted"}, "remotes": {}}
        verdict = writes.compare(declared, made.before, made.before, made.commits)
        scoped = self.manifest["refs"]["refs/heads/scoped"][:12]
        self.assertEqual(verdict["problems"], [
            "refs/heads/ghost: declared deleted, but it did not exist before the launch",
            f"refs/heads/keep: already at {self.manifest['commits'][0][:12]} (@0) before the launch, so the declared "
            "change cannot be told apart from no write",
            "refs/heads/new: declared equal to refs/heads/gone, which does not exist after the launch",
            f"refs/heads/scoped: missing deletion, still at {scoped}",
        ])

    def test_remote_urls_outside_the_fixture_are_refused(self) -> None:
        repo = self.source / "repo"
        original = self.git(repo, "config", "remote.origin.url")
        outside = self.root / "elsewhere.git"
        (self.source / "escape.git").symlink_to(outside)
        for url, match in (
                ("https://example.invalid/fixture.git", "not an absolute local path"),
                ("ssh://example.invalid/fixture.git", "not an absolute local path"),
                ("git@example.invalid:fixture.git", "not an absolute local path"),
                (f"file://{self.source}/remotes/origin.git", "not an absolute local path"),
                ("remotes/origin.git", "not an absolute local path"),
                (str(outside), "outside"),
                (f"{self.source}/../elsewhere.git", "outside"),
                (str(self.source / "escape.git"), "resolves to"),
        ):
            with self.subTest(url=url):
                self.git(repo, "config", "remote.origin.url", url)
                with self.assertRaisesRegex(SystemExit, f"remote.origin.url of {repo} is .*{match}.*never a network"):
                    writes.plan(self.spec, self.manifest)
        self.git(repo, "config", "remote.origin.url", original)
        self.git(repo, "config", "remote.origin.pushurl", "https://example.invalid/fixture.git")
        with self.assertRaisesRegex(SystemExit, "remote.origin.pushurl .*not an absolute local path"):
            writes.plan(self.spec, self.manifest)
        self.git(repo, "config", "--unset", "remote.origin.pushurl")
        self.git(repo, "config", "url.https://example.invalid/.insteadOf", str(self.source))
        with self.assertRaisesRegex(SystemExit, "rewrites remote URLs \\(url.https://example.invalid/.insteadof\\)"):
            writes.plan(self.spec, self.manifest)
        self.git(repo, "config", "--remove-section", "url.https://example.invalid/")
        self.assertFalse(self.plan.root.exists())
        self.assertEqual(writes.plan(self.spec, self.manifest).root, self.plan.root)


class LaunchTest(GitCase):
    """`play.launch` with a session that writes as the app would: the copy it opens and the record it leaves."""

    def launch(self, run_dir: Path, behaviour: str = "route") -> tuple[dict, list[Path]]:
        opened: list[Path] = []
        test = self

        class FakeSession:
            def __init__(self, binary, fixture, run_dir, store, **kwargs) -> None:
                if behaviour == "bad session":
                    raise ValueError("no such display")
                opened.append(fixture)
                self.fixture, self.proc = fixture, None
                run_dir.mkdir(parents=True)
                self.dirs = SimpleNamespace(root=run_dir)
                # The session's own check reads the copy; a writing launch never relies on it.
                self.log = dict(header=dict(started_utc="2026-10-02T10:00:00Z", ended_utc="2026-10-02T10:01:00Z"),
                                captures=[], fixture_unchanged=False)
                self.restore_failures = []

            def launch(self) -> None:  # as Session.launch: nothing is spawned while the desktop is locked
                if behaviour == "locked":
                    raise runenv.Refusal("the desktop is locked before the launch; nothing sent")
                self.proc = object()

            def run(self, steps) -> None:  # once per step: the route goes with the first
                if not getattr(self, "routed", False):
                    self.routed = True
                    test.route(self.fixture, push=behaviour != "no push")
                    if behaviour == "touches the build":
                        test.git(test.source / "remotes" / "origin.git", "update-ref", "refs/heads/stray",
                                 test.manifest["commits"][0])

            def close(self) -> int | None:
                if behaviour == "close fails":
                    raise OSError("flow log unwritable")
                if self.proc is not None:  # as Session.close, which records an exit only for a spawned app
                    survived = "pid 1 still running 15s after SIGTERM; not killed"
                    self.log["exit"] = survived if behaviour == "survives" else -15
                (self.dirs.root / "flow-log.json").write_text(json.dumps(self.log))
                return self.log["exit"] if isinstance(self.log.get("exit"), int) else None

        with mock.patch("native_qa.session.Session", FakeSession), contextlib.redirect_stdout(io.StringIO()):
            record = play.launch(self.spec, "cand", self.spec["variants"][0], Path("/x/cand"),
                                 self.source / "repo", run_dir,
                                 dict(display=":1", settle=0, backend="xtest", writes=self.plan,
                                      remotes=recipe.bare_remotes(self.manifest["recipe"], self.source)))
        return record, opened

    def test_a_writing_launch_opens_its_own_copy_and_records_the_verdict(self) -> None:
        record, opened = self.launch(self.root / "run" / "one")
        self.assertEqual(opened, [self.plan.root / "repo"])
        self.assertIsNone(record["error"])
        self.assertTrue(record["fixture_unchanged"])  # the recipe build
        self.assertEqual(record["writes"]["verdict"]["result"], "pass")
        log = json.loads((self.root / "run" / "one" / "flow-log.json").read_text())
        self.assertEqual(log["fixture_writes"], json.loads(json.dumps(record["writes"])))
        self.assertEqual(set(log["fixture_writes"]), {"copy", "repository", "app", "declared", "before", "after",
                                                      "verdict", "kept"})
        self.assertEqual(log["fixture_writes"]["app"], "exited")
        self.assertFalse(self.plan.root.exists())
        # The next launch starts from the build again, so the same route passes again.
        again, _ = self.launch(self.root / "run" / "two")
        self.assertIsNone(again["error"])

    def test_a_launch_whose_copy_differs_fails_and_keeps_it(self) -> None:
        record, _ = self.launch(self.root / "run" / "one", "no push")
        main, pushed = self.manifest["commits"][2][:12], self.manifest["commits"][1][:12]
        self.assertEqual(record["error"], "the fixture copy's changes differ from the declaration: "
                                          f"refs/remotes/origin/main: missing change to {main} (refs/heads/main), "
                                          f"found {pushed}; origin:refs/heads/main: missing change to {main} "
                                          f"(refs/heads/main), found {pushed}")
        self.assertFalse(record["refusal"])
        self.assertTrue(record["writes"]["kept"])
        self.assertTrue(self.plan.root.is_dir())
        refused, opened = self.launch(self.root / "run" / "two")
        self.assertEqual((refused["refusal"], opened), (True, []))
        self.assertIn("vanished-scope_copy exists (its changes differ from the declaration, cand", refused["error"])
        self.assertFalse((self.root / "run" / "two").exists())

    def test_a_copy_whose_app_outlived_sigterm_is_kept(self) -> None:
        record, _ = self.launch(self.root / "run" / "one", "survives")
        self.assertIn("the app exited with 'pid 1 still running", record["error"])
        self.assertEqual((record["writes"]["verdict"]["result"], record["writes"]["kept_because"]),
                         ("pass", "the app may still be running in it"))
        self.assertTrue(self.plan.root.is_dir())

    def test_a_launch_refused_before_the_app_started_removes_its_untouched_copy(self) -> None:
        record, opened = self.launch(self.root / "run" / "one", "locked")
        self.assertEqual(opened, [self.plan.root / "repo"])
        self.assertEqual((record["error"], record["refusal"]),
                         ("refusing: the desktop is locked before the launch; nothing sent", True))
        self.assertEqual((record["writes"]["app"], record["writes"]["kept"]), ("never started", False))
        self.assertFalse(self.plan.root.exists())
        again, _ = self.launch(self.root / "run" / "two")
        self.assertIsNone(again["error"])

    def test_any_failure_to_set_up_the_copy_or_the_session_is_a_refusal(self) -> None:
        with mock.patch("native_qa.writes.shutil.copytree", side_effect=OSError(28, "No space left on device")):
            record, opened = self.launch(self.root / "run" / "one")
        self.assertEqual((record["refusal"], opened), (True, []))
        self.assertEqual(record["error"], "OSError(28, 'No space left on device')")
        self.assertFalse(self.plan.root.exists())
        record, opened = self.launch(self.root / "run" / "two", "bad session")
        self.assertEqual((record["refusal"], record["error"], opened), (True, "ValueError('no such display')", []))
        self.assertFalse(self.plan.root.exists())

    def test_a_launch_whose_session_fails_to_close_restores_isenabled_then_concludes_its_copy(self) -> None:
        self.spec = scenario.validate(spec(atspi=True), "f" * 64)
        events: list[str] = []

        @contextlib.contextmanager
        def enabled(record):
            record.update(original=False, set=True)
            try:
                yield record
            finally:
                events.append("IsEnabled restored")
                record["restored"] = False

        def conclude(made, app):
            events.append(f"copy concluded ({app})")
            return real_conclude(made, app)

        real_conclude = writes.conclude
        with mock.patch("native_qa.a11y.enabled", enabled), mock.patch.object(writes, "conclude", conclude), \
                self.assertRaisesRegex(OSError, "flow log unwritable"):
            self.launch(self.root / "run" / "one", "close fails")
        self.assertEqual(events, ["IsEnabled restored", "copy concluded (may still be running)"])
        self.assertEqual(json.loads((self.plan.root / writes.MARKER).read_text())["kept_because"],
                         "the app may still be running in it")

    def test_a_launch_that_changes_the_builds_bare_remote_fails(self) -> None:
        record, _ = self.launch(self.root / "run" / "one", "touches the build")
        self.assertEqual(record["error"], "the fixture's state changed during the launch")
        self.assertFalse(record["fixture_unchanged"])

    def test_the_attestation_says_each_launch_wrote_to_its_own_copy(self) -> None:
        record, _ = self.launch(self.root / "run" / "one")
        summary = "/tmp/gitturtle-evidence/fixtures/vanished-scope/repo (3 commits)"
        run = dict(fixture=dict(summary=summary, writes=self.plan.record()), launches=[record])
        self.assertEqual(evidence.fixture_line(run), f"{summary}, unchanged by every launch; each launch wrote only to "
                                                     f"its own fresh copy at {self.plan.root}, which changed exactly "
                                                     "as the scenario declares")
        self.assertEqual(evidence.fixture_line(dict(fixture=dict(summary=summary), launches=[record])),
                         f"{summary}, unchanged by every launch")


if __name__ == "__main__":
    unittest.main()
