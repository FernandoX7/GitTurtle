"""Contract validation and scope checks without a checkout or model process."""

import json
from pathlib import Path
import tempfile
import unittest

from agent_loop.task_spec import (
    MAX_SPEC_BYTES, load_spec, parse_spec, path_allowed, required_evidence, select_ready,
)


def task(task_id="first", **changes):
    result = {
        "id": task_id,
        "title": "Repair the documented workflow",
        "description": "Make the specified workflow usable and document its evidence.",
        "depends_on": [],
        "scope": ["docs/**/*.md"],
        "acceptance": [{"id": "workflow", "description": "The documented command succeeds."}],
        "profiles": ["docs"],
        "commit": "docs: repair the documented workflow",
    }
    result.update(changes)
    return result


def spec(*tasks):
    return {"version": 1, "tasks": list(tasks)}


class TaskSpecTests(unittest.TestCase):
    def test_empty_backlog_and_independent_contract_copy(self):
        self.assertEqual(parse_spec(spec()), [])
        data = spec(task())
        parsed = parse_spec(data)
        data["tasks"][0]["acceptance"][0]["description"] = "Changed later"
        self.assertNotEqual(parsed[0].acceptance[0]["description"], "Changed later")
        self.assertEqual(parsed[0].depends_on, ())

    def test_unknown_or_missing_fields_and_status_are_rejected(self):
        for level, extra in (("spec", "status"), ("task", "passes"), ("criterion", "result")):
            data = spec(task())
            target = {"spec": data, "task": data["tasks"][0],
                      "criterion": data["tasks"][0]["acceptance"][0]}[level]
            target[extra] = True
            with self.subTest(level=level), self.assertRaises(ValueError):
                parse_spec(data)
        data = spec(task())
        del data["tasks"][0]["scope"]
        with self.assertRaises(ValueError):
            parse_spec(data)

    def test_bounds_types_and_empty_values(self):
        invalid = [
            {"version": True, "tasks": []},
            {"version": 1.0, "tasks": []},
            {"version": 2, "tasks": []},
            {"version": 1, "tasks": "not an array"},
            spec(*[task(str(index)) for index in range(201)]),
        ]
        for field, value in (
            ("title", " "), ("title", "x" * 161), ("description", "x" * 4097),
            ("scope", []), ("scope", ["docs/*"] * 51), ("profiles", []),
            ("acceptance", []), ("acceptance", [task()["acceptance"][0]] * 51),
            ("depends_on", "first"), ("id", "../first"), ("id", "A"),
            ("id", "x" * 65), ("description", "bad\x00text"),
        ):
            invalid.append(spec(task(**{field: value})))
        for data in invalid:
            with self.subTest(data=str(data)[:120]), self.assertRaises(ValueError):
                parse_spec(data)

    def test_malformed_graphs_fail_before_selection(self):
        invalid = [
            spec(task(), task()),
            spec(task(depends_on=["missing"])),
            spec(task(depends_on=["first"])),
            spec(task("a", depends_on=["b"]), task("b", depends_on=["a"])),
            spec(task("a", depends_on=["b"]), task("b", depends_on=["c"]),
                 task("c", depends_on=["a"])),
            spec(task("a"), task("b", depends_on=["a", "a"])),
        ]
        for data in invalid:
            with self.subTest(data=data), self.assertRaises(ValueError):
                parse_spec(data)

    def test_complete_bounded_dependency_chain_is_valid(self):
        tasks = [task(f"t{index}", depends_on=[f"t{index - 1}"] if index else [])
                 for index in range(200)]
        self.assertEqual(len(parse_spec(spec(*tasks))), 200)

    def test_ready_selection_skips_unready_head_and_preserves_order(self):
        tasks = parse_spec(spec(
            task("later", depends_on=["base"]), task("blocked"), task("base"), task("independent"),
        ))
        self.assertEqual([item.id for item in select_ready(tasks, set(), {"blocked"})],
                         ["base", "independent"])
        self.assertEqual([item.id for item in select_ready(tasks, {"base"}, {"blocked"})],
                         ["later", "independent"])
        self.assertEqual(select_ready(tasks, {item.id for item in tasks}, set()), [])

    def test_blocked_dependency_does_not_block_unrelated_work(self):
        tasks = parse_spec(spec(task("base"), task("dependent", depends_on=["base"]), task("free")))
        self.assertEqual([item.id for item in select_ready(tasks, set(), {"base"})], ["free"])

    def test_criteria_and_profiles_are_specific_and_unique(self):
        invalid = [
            task(acceptance=[task()["acceptance"][0]] * 2),
            task(acceptance=[{"id": "a", "description": ""}]),
            task(profiles=["docs", "docs"]), task(profiles=["unchecked"]),
            task(scope=["docs/**", "docs/**"]),
        ]
        for value in invalid:
            with self.subTest(value=value), self.assertRaises(ValueError):
                parse_spec(spec(value))

    def test_commit_subjects_are_conventional_bounded_single_lines(self):
        for subject in ("fix: restore focus", "feat(history)!: change navigation", "revert: undo the change"):
            self.assertEqual(parse_spec(spec(task(commit=subject)))[0].commit, subject)
        for subject in ("fix: ", "fix: update\n\nbody", "fix: update\r", "--amend", "fix: x\x1b[0m",
                        "fix: " + "x" * 68, "update the code", "fix(): update", "fix: update "):
            with self.subTest(subject=subject), self.assertRaises(ValueError):
                parse_spec(spec(task(commit=subject)))

    def test_profiles_require_evidence_but_do_not_claim_completion(self):
        for profile in ("docs", "tooling", "rust"):
            self.assertEqual(required_evidence(parse_spec(spec(task(profiles=[profile])))[0]), set())
        for profile in ("native", "performance", "package", "vendor"):
            self.assertEqual(required_evidence(parse_spec(spec(task(profiles=[profile])))[0]), {profile})
        combined = parse_spec(spec(task(profiles=["rust", "native", "performance", "package", "vendor"])))[0]
        self.assertEqual(required_evidence(combined), {"native", "performance", "package", "vendor"})

    def test_loading_rejects_duplicate_keys_invalid_json_and_excess_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tasks.json"
            path.write_text(json.dumps(spec(task())), encoding="utf-8")
            self.assertEqual(load_spec(path)[0].id, "first")
            for content in (
                b'{"version": 1, "version": 1, "tasks": []}',
                b'{"version": 1, "tasks": [{"id": "a", "id": "b"}]}',
                b'{"version":', b"\xff", b" " * (MAX_SPEC_BYTES + 1),
            ):
                path.write_bytes(content)
                with self.subTest(content=content[:70]), self.assertRaises(ValueError):
                    load_spec(path)


class ScopeTests(unittest.TestCase):
    def test_single_segment_and_recursive_globs(self):
        self.assertTrue(path_allowed("docs/readme.md", ["docs/*.md"]))
        self.assertFalse(path_allowed("docs/nested/readme.md", ["docs/*.md"]))
        self.assertTrue(path_allowed("docs/readme.md", ["docs/**/*.md"]))
        self.assertTrue(path_allowed("docs/nested/deep/readme.md", ["docs/**/*.md"]))
        self.assertFalse(path_allowed("docs/nested/readme.txt", ["docs/**/*.md"]))
        self.assertTrue(path_allowed("crates/app/src/main.rs", ["docs/**", "crates/app/**"]))
        self.assertFalse(path_allowed("crates/app2/src/main.rs", ["crates/app/**"]))
        self.assertFalse(path_allowed("README.md", ["readme.md"]))
        self.assertTrue(path_allowed("docs/a1.md", ["docs/[ab]?.md"]))

    def test_unsafe_scope_and_concrete_paths(self):
        unsafe = [
            "/tmp/file", "../file", "docs/../../file", "docs/./file", "docs//file", "docs/",
            "C:/file", "C:\\file", "docs\\..\\file", ".git/config", "docs/.git/config",
            ".local/state.json", "docs/.LOCAL/state", "docs/bad\nfile", "docs/bad\x00file",
            "docs/\u202efile", " file", "file ", "",
        ]
        for value in unsafe:
            with self.subTest(value=value):
                self.assertFalse(path_allowed(value, ["**"]))
                with self.assertRaises(ValueError):
                    parse_spec(spec(task(scope=[value])))

    def test_obfuscated_metadata_scopes_are_rejected(self):
        for value in (".g*/**", "[.]git/**", "**/.loc?l/**", "docs/[.][.]/**", "docs/foo**/bar"):
            with self.subTest(value=value):
                self.assertFalse(path_allowed("docs/file.md", [value]))
                with self.assertRaises(ValueError):
                    parse_spec(spec(task(scope=[value])))
        self.assertFalse(path_allowed(".git/config", ["**/*"]))
        self.assertFalse(path_allowed(".local/run.json", ["**"]))
        self.assertFalse(path_allowed("docs/a.md", "docs/*.md"))

    def test_deep_bounded_path_does_not_depend_on_python_recursion(self):
        self.assertTrue(path_allowed("/".join(["a"] * 1500), ["**"]))
        self.assertFalse(path_allowed("a" * 4097, ["**"]))


if __name__ == "__main__":
    unittest.main()
