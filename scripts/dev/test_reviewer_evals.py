"""Validate offline reviewer evaluation fixtures and mechanical invariant detection."""
from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT / "scripts") not in sys.path:
    sys.path.insert(0, str(ROOT / "scripts"))

from dev import check_invariants  # noqa: E402

EVAL_DIR = Path(__file__).resolve().parent / "reviewer_evals"


def _added_snippet(diff_text: str) -> tuple[str, str]:
    target_path = ""
    lines: list[str] = []
    for line in diff_text.splitlines():
        if line.startswith("+++ b/"):
            target_path = line[len("+++ b/"):]
        elif line.startswith("+") and not line.startswith("+++"):
            lines.append(line[1:])
        elif line.startswith(" "):
            lines.append(line[1:])
    return target_path, "\n".join(lines) + "\n"


class ReviewerEvalsTests(unittest.TestCase):
    def test_manifest_references_valid_roles_and_unified_diffs(self) -> None:
        manifest = json.loads((EVAL_DIR / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(manifest["version"], 1)
        cases = manifest["cases"]
        self.assertGreaterEqual(len(cases), 6)
        role_dir = ROOT / ".agents" / "roles"
        seen_ids: set[str] = set()
        for case in cases:
            self.assertNotIn(case["id"], seen_ids)
            seen_ids.add(case["id"])
            diff_path = EVAL_DIR / case["diff"]
            self.assertTrue(diff_path.is_file(), f"missing diff fixture {diff_path}")
            diff_text = diff_path.read_text(encoding="utf-8")
            self.assertIn("diff --git ", diff_text)
            self.assertIn("@@ ", diff_text)
            self.assertIn(case["expected_severity"], ("blocker", "major", "minor", "none"))
            for role in case["roles"]:
                self.assertTrue((role_dir / f"{role}.md").is_file(), f"unknown role {role!r}")

    def test_mechanical_fixtures_match_check_invariants(self) -> None:
        manifest = json.loads((EVAL_DIR / "manifest.json").read_text(encoding="utf-8"))
        for case in manifest["cases"]:
            expected_rule = case["mechanical_rule"]
            if expected_rule is None and case["expected_severity"] != "none":
                continue
            target_rel, body = _added_snippet((EVAL_DIR / case["diff"]).read_text(encoding="utf-8"))
            with tempfile.TemporaryDirectory() as tmp:
                repo = Path(tmp)
                (repo / "crates/git-core/src").mkdir(parents=True)
                (repo / "crates/preview/src").mkdir(parents=True)
                (repo / "crates/app/src").mkdir(parents=True)
                (repo / "crates/git-core/src/lib.rs").write_text(
                    "#![forbid(unsafe_code)]\n#![cfg_attr(not(test), deny(clippy::unwrap_used))]\n",
                    encoding="utf-8",
                )
                dest = repo / target_rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                if expected_rule == "ui-render-nonblocking":
                    dest.write_text("impl View {\n" + body + "}\n", encoding="utf-8")
                else:
                    dest.write_text(body, encoding="utf-8")
                violations = check_invariants.check_repo(repo)
                if expected_rule is None:
                    self.assertEqual(violations, [], f"control case {case['id']} triggered {violations}")
                else:
                    self.assertIn(
                        expected_rule,
                        [v.rule for v in violations],
                        f"fixture {case['id']} did not trigger {expected_rule}: {violations}",
                    )


if __name__ == "__main__":
    unittest.main()
