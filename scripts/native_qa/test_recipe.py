from __future__ import annotations

import contextlib
import io
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from native_qa import recipe, scenario

EXAMPLE = Path(__file__).resolve().parents[2] / "docs" / "evidence" / "tab-reveals-branch-and-tag-rows" / "scenario.json"
RECIPE = {
    "name": "reflog-mix",
    "description": "every operation once",
    "clock": {"start": "2026-09-01T09:00:00Z", "step": 60},
    "operations": [
        {"commit": "Start", "files": {"README.md": "# Fixture\n", "bin/data.bin": {"base64": "AAEC"}}},
        {"commit": "Second", "files": {"app/paging.py": "PAGE = 20\n"}, "date": "2026-09-03T12:00:00+02:00"},
        {"branch": "topic", "at": "@0"},
        {"tag": "v1", "at": "@1", "message": "Release 1"},
        {"tag": "light", "at": "@0"},
        {"checkout": "feature/x", "create": True},
        {"commit": "On the feature", "files": {"README.md": None, "notes.md": "notes\n"}},
        {"checkout": "main"},
        {"merge": "feature/x", "message": "Merge feature/x"},
        {"commit": "Empty", "allow_empty": True},
        {"reset": "@3", "mode": "hard"},
        {"remote": "origin", "bare": "remotes/origin.git"},
        {"push": "origin", "refs": ["main", "v1"]},
        {"worktree": "worktrees/alpha", "new_branch": "alpha", "at": "@1"},
        {"worktree": "worktrees/detached", "at": "@0"},
    ],
}


def normalised(data: dict) -> dict:
    return scenario.fixture_recipe(data, "$.fixture")


@unittest.skipUnless(shutil.which("git"), "no git")
class RecipeTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def git(self, repo: Path, *args: str) -> str:
        return recipe.read(repo, *args).decode().strip()

    def test_every_operation_builds_with_fixed_identity_dates_and_reflog(self) -> None:
        manifest = recipe.build(normalised(RECIPE), self.root / "a")
        repo = Path(manifest["repository"])
        self.assertFalse(manifest["reused"])
        self.assertEqual(len(manifest["commits"]), 5)  # four commits and the merge
        self.assertEqual(manifest["branch"], "main")
        self.assertEqual(manifest["head"], manifest["commits"][3])  # reset to the merge
        refs = manifest["refs"]
        for name in ("refs/heads/main", "refs/heads/topic", "refs/heads/feature/x", "refs/heads/alpha",
                     "refs/tags/v1", "refs/tags/light", "refs/remotes/origin/main"):
            self.assertIn(name, refs)
        self.assertEqual(self.git(repo, "cat-file", "-t", "v1"), "tag")
        self.assertEqual(self.git(repo, "cat-file", "-t", "light"), "commit")
        self.assertEqual(set(self.git(repo, "log", "--all", "--format=%an <%ae>").splitlines()),
                         {"GitTurtle QA <qa@example.invalid>"})
        # The clock dates every operation; an explicit date keeps its offset.
        self.assertEqual(self.git(repo, "log", "-1", "--format=%ad", "--date=iso-strict", manifest["commits"][0]),
                         "2026-09-01T09:00:00Z")
        self.assertEqual(self.git(repo, "log", "-1", "--format=%ad", "--date=iso-strict", manifest["commits"][1]),
                         "2026-09-03T12:00:00+02:00")
        self.assertEqual(recipe.read(repo, "cat-file", "-p", f"{manifest['commits'][0]}:bin/data.bin"), b"\x00\x01\x02")
        self.assertEqual(len(manifest["head_reflog"]), 8)  # commits, checkouts, merge and reset
        self.assertEqual(sum(1 for line in manifest["worktrees"] if line.startswith("worktree ")), 3)
        self.assertEqual(recipe.state(repo), manifest["state"])
        self.assertTrue((self.root / "a" / "reflog-mix" / "remotes" / "origin.git" / "refs" / "tags" / "v1").exists())
        self.assertIn("5 commits", recipe.summary(manifest))
        self.assertIn("2 linked worktrees", recipe.summary(manifest))

    def test_the_same_recipe_gives_the_same_objects_elsewhere(self) -> None:
        one = recipe.build(normalised(RECIPE), self.root / "one")
        two = recipe.build(normalised(RECIPE), self.root / "two")
        self.assertEqual(one["commits"], two["commits"])
        self.assertEqual(one["refs"], two["refs"])
        self.assertEqual(one["recipe_sha256"], two["recipe_sha256"])

    def test_reuse_only_an_unchanged_build_of_the_same_recipe(self) -> None:
        fixtures = self.root / "fixtures"
        first = recipe.build(normalised(RECIPE), fixtures)
        again = recipe.build(normalised(RECIPE), fixtures)
        self.assertTrue(again["reused"])
        self.assertEqual(again["head"], first["head"])
        edited = json.loads(json.dumps(RECIPE))
        edited["operations"][0]["commit"] = "Begin"
        with self.assertRaisesRegex(SystemExit, "another recipe"):
            recipe.build(normalised(edited), fixtures)
        subprocess.run(["git", "-C", first["repository"], "branch", "stray"], check=True, capture_output=True)
        with self.assertRaisesRegex(SystemExit, "changed since it was built.*refs_sha256"):
            recipe.build(normalised(RECIPE), fixtures)
        subprocess.run(["git", "-C", first["repository"], "branch", "-D", "stray"], check=True, capture_output=True)
        recipe.build(normalised(RECIPE), fixtures)  # as built again
        alpha = fixtures / "reflog-mix" / "worktrees" / "alpha"
        (alpha / "staged.txt").write_text("staged in a linked worktree\n")
        subprocess.run(["git", "-C", str(alpha), "add", "staged.txt"], check=True, capture_output=True)
        with self.assertRaisesRegex(SystemExit, "changed since it was built.*linked_worktrees"):
            recipe.build(normalised(RECIPE), fixtures)
        (fixtures / "half").mkdir()
        half = dict(RECIPE, name="half")
        with self.assertRaisesRegex(SystemExit, "no finished fixture"):
            recipe.build(normalised(half), fixtures)

    def test_expect_pins_object_ids(self) -> None:
        wrong = dict(RECIPE, expect={"@0": "0" * 40})
        with self.assertRaisesRegex(SystemExit, "object IDs differ"):
            recipe.build(normalised(wrong), self.root / "f")
        self.assertFalse((self.root / "f" / "reflog-mix" / recipe.MANIFEST).exists())
        with self.assertRaisesRegex(SystemExit, "no finished fixture"):
            recipe.build(normalised(wrong), self.root / "f")

    def test_the_example_reproduces_the_evidence_fixture(self) -> None:
        spec = scenario.load(EXAMPLE)
        with contextlib.redirect_stdout(io.StringIO()):
            manifest = recipe.build(spec["fixture"], self.root)
        self.assertEqual({rev: check["found"] for rev, check in manifest["expect"].items()},
                         spec["fixture"]["expect"])
        self.assertEqual(sum(1 for name in manifest["refs"] if name.startswith("refs/heads/")), 30)
        self.assertEqual(sum(1 for name in manifest["refs"] if name.startswith("refs/tags/")), 30)


class RecipeValidationTest(unittest.TestCase):
    def test_refusals(self) -> None:
        ops = RECIPE["operations"]
        for bad, match in (
                (dict(RECIPE, operations=[{"branch": "x", "at": "@0"}]), "only 0 precede"),
                (dict(RECIPE, operations=[{"commit": "a", "files": {"../escape": "x"}}]), "relative POSIX"),
                (dict(RECIPE, operations=[{"commit": "a", "files": {".git/config": "x"}}]), "outside .git"),
                (dict(RECIPE, operations=[{"commit": "a"}]), "allow_empty"),
                (dict(RECIPE, operations=[{"commit": "a", "branch": "b"}]), "exactly one of"),
                (dict(RECIPE, operations=[{"commit": "a", "allow_empty": True, "date": "2026-09-01T09:00:00"}]),
                 "no offset"),
                (dict(RECIPE, operations=[{"commit": "a", "allow_empty": True, "date": "yesterday"}]), "ISO 8601"),
                (dict(RECIPE, operations=ops[:1] + [{"branch": "-delete", "at": "@0"}]), "does not match"),
                (dict(RECIPE, operations=ops[:1] + [{"remote": "o", "bare": "repo/o.git"}]), "overlaps 'repo'"),
                (dict(RECIPE, operations=ops[:1] + [{"push": "origin"}]), "refs: expected a list"),
                (dict(RECIPE, operations=ops[:1] + [{"push": "../elsewhere", "refs": ["main"]}]),
                 "not a remote this recipe defined"),
                (dict(RECIPE, operations=ops[:1] + [{"remote": "up/stream", "bare": "r.git"}]), "does not match"),
                (dict(RECIPE, operations=ops[:1] + [{"remote": "o", "bare": "a.git"}, {"remote": "o", "bare": "b.git"}]),
                 "already defined"),
                (dict(RECIPE, operations=[{"commit": "a", "files": {"sub/.GIT/hooks/x": "x"}}]), "outside .git"),
                (dict(RECIPE, operations=ops[:1] + [{"remote": "origin", "bare": "o.git"},
                                                    {"push": "origin", "refs": ["@0"]}]), "does not match"),
                (dict(RECIPE, operations=ops[:1] + [{"checkout": "x", "at": "@0"}]), "start point"),
                (dict(RECIPE, operations=ops[:1] + [{"reset": "@0", "mode": "keep"}]), "soft, mixed or hard"),
                (dict(RECIPE, expect={"@9": "0" * 40}), "never makes"),
                (dict(RECIPE, expect={"HEAD": "abc"}), "does not match"),
                (dict(RECIPE, colour="blue"), "unknown key"),
        ):
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                normalised(bad)


if __name__ == "__main__":
    unittest.main()
