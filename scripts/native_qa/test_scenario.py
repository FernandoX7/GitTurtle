from __future__ import annotations

import contextlib
import copy
import importlib.util
import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from native_qa import evidence, play, scenario

HAVE_PIL = importlib.util.find_spec("PIL") is not None
HERE = Path(__file__).resolve().parent
QA = HERE / "qa.py"
EXAMPLE = HERE.parents[1] / "docs" / "evidence" / "tab-reveals-branch-and-tag-rows" / "scenario.json"
SURFACE, RING = (16, 21, 31), (117, 224, 187)
PANEL = [10, 10, 110, 60]
FOCUS_RECT = [20, 20, 100, 50]
SPEC = {
    "version": 1, "task": "demo-task", "summary": "the panel shows its focus", "limitations": "Linux only",
    "variants": {"palettes": ["midnight", "porcelain"]},
    "crops": {"panel": PANEL},
    "steps": [
        {"mark": "page"},
        {"key": "Tab", "repeat": 2, "await_change": 1.0},
        {"guard": {"kind": "compare", "a": "page", "b": "@now", "min_pixels": 1}, "on_fail": [{"key": "Escape"}]},
        {"capture": "rest", "commit": False},
        {"capture": "focus", "crop": "panel", "shows": {"base": "no ring", "cand": "the ring"}},
        {"capture": "only-base", "crop": "panel", "roles": ["base"], "shows": "the base only"},
    ],
    "analyses": [
        {"name": "ring", "kind": "ring", "frame": "focus", "rect": FOCUS_RECT, "min_contrast": 3.0,
         "expect": {"base": False, "cand": True}},
        {"name": "rest-same", "kind": "compare", "a": "base:rest", "b": "cand:rest", "masks": ["status-timing"]},
    ],
}
INFO = {"application": "GitTurtle", "version": "0.1.0", "source_tree": "clean", "target": "x86_64-unknown-linux-gnu",
        "profile": "release", "rustc": "rustc 1.98.0", "build_unix_seconds": "1"}
BASE_SHA, CAND_SHA, REBUILT_SHA = "a" * 40, "b" * 40, "c" * 40


def spec(**changes) -> dict:
    data = copy.deepcopy(SPEC)
    data.update(changes)
    return data


def build(path: str, revision: str) -> dict:
    return dict(path=path, sha256=revision[0] * 64, build_info=dict(INFO, source_revision=revision))


class SpecTest(unittest.TestCase):
    def test_text_sizes_match_the_app(self) -> None:
        source = HERE.parents[1] / "crates" / "app" / "src" / "appearance.rs"
        if not source.is_file():
            self.skipTest("app source not present")
        found = re.search(r"INTERFACE_TEXT_RANGE: std::ops::RangeInclusive<u8> = (\d+)..=(\d+);", source.read_text())
        self.assertEqual((int(found[1]), int(found[2]) + 1), (scenario.TEXT_SIZES.start, scenario.TEXT_SIZES.stop))

    def test_the_example_is_valid_and_names_the_committed_crops(self) -> None:
        loaded = scenario.load(EXAMPLE)
        names = [crop.name for crop in scenario.committed(loaded)]
        self.assertEqual(len(names), 20)
        # Exactly the crops committed beside the spec, so its recheck finds every one under its name.
        self.assertEqual(sorted(names), sorted(path.name for path in EXAMPLE.parent.glob("*.png")))
        self.assertIn("candidate-midnight-1000x680-branch-chooser-tab-down.png", names)
        self.assertIn("base-porcelain-1000x680-tags-shift-tab-confirm.png", names)
        self.assertNotIn("candidate-porcelain-1000x680-tags-shift-tab-confirm.png", names)
        self.assertEqual(len(scenario.committed(loaded, roles=("cand",))), 8)
        self.assertEqual(len(loaded["sha256"]), 64)

    def test_every_session_step_names_exactly_one_action(self) -> None:
        # An option named like an action (a capture's `stable`) once made Session.run take a capture for a wait.
        from native_qa import session

        loaded = scenario.load(EXAMPLE)
        for step in loaded["steps"]:
            if "guard" in step:
                continue
            for role in loaded["roles"]:
                sent = play.session_step(step, role)
                with self.subTest(step=step["index"]):
                    self.assertEqual(len([key for key in session.STEP_KEYS if key in sent]), 1, sent)
        self.assertFalse({option for options in scenario.STEPS.values() for option in options} & set(scenario.STEPS))

    def test_defaults_and_planning(self) -> None:
        loaded = scenario.validate(spec())
        self.assertEqual(loaded["window"], (1000, 680))
        self.assertEqual(loaded["roles"], ["base", "cand"])
        self.assertEqual([v.id for v in loaded["variants"]], ["midnight", "porcelain"])
        capture = loaded["steps"][4]
        self.assertEqual((capture["stable_within"], capture["quiet"], capture["keep_pointer"], capture["commit"]),
                         (scenario.CAPTURE_STABLE, scenario.CAPTURE_QUIET, False, True))
        ring, same = loaded["analyses"]
        self.assertEqual(ring["expect"], {"base": False, "cand": True})
        self.assertEqual((same["roles"], same["expect"], same["max_pixels"]), (None, True, 0))
        runs = scenario.analysis_runs(loaded)
        self.assertEqual([(entry["name"], role, variant.id) for entry, role, variant in runs],
                         [("ring", "base", "midnight"), ("ring", "cand", "midnight"), ("ring", "base", "porcelain"),
                          ("ring", "cand", "porcelain"), ("rest-same", None, "midnight"),
                          ("rest-same", None, "porcelain")])

    def test_crop_names(self) -> None:
        loaded = scenario.validate(spec(window=[1200, 800], variants=[
            {"palette": "midnight", "text_size": 13}, {"palette": "porcelain"},
            {"palette": "midnight", "text_size": 18, "id": "big"}]))
        crops = scenario.committed(loaded)
        self.assertEqual([c.name for c in crops], [
            "base-midnight-13pt-1200x800-focus.png", "candidate-midnight-13pt-1200x800-focus.png",
            "base-midnight-13pt-1200x800-only-base.png",
            "base-porcelain-1200x800-focus.png", "candidate-porcelain-1200x800-focus.png",
            "base-porcelain-1200x800-only-base.png",
            "base-midnight-18pt-1200x800-focus.png", "candidate-midnight-18pt-1200x800-focus.png",
            "base-midnight-18pt-1200x800-only-base.png"])
        self.assertEqual((crops[1].shows, crops[1].box, crops[1].crop, crops[6].variant.id),
                         ("the ring", tuple(PANEL), "panel", "big"))
        self.assertEqual(scenario.store_settings(loaded, crops[6].variant), {"interface_text_size": 18})
        steps = copy.deepcopy(SPEC["steps"][:5])
        steps[4]["shows"] = "the ring"
        only_cand = scenario.validate(spec(roles=["cand"], steps=steps, analyses=[dict(SPEC["analyses"][0], expect=True)]))
        for changes, match in ((dict(steps=SPEC["steps"]), r"shows: unknown key\(s\) base"),
                               (dict(steps=steps + SPEC["steps"][5:]), "roles: 'base' does not match"),
                               (dict(steps=steps, analyses=SPEC["analyses"][1:]), "'base' is not one of the roles")):
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                scenario.validate(spec(roles=["cand"], **changes))
        self.assertEqual([c.name for c in scenario.committed(only_cand)],
                         ["candidate-midnight-1000x680-focus.png", "candidate-porcelain-1000x680-focus.png"])

    def test_a_variant_carries_its_own_store_settings(self) -> None:
        steps = SPEC["steps"] + [{"key": "Escape", "when": {"variant": ["midnight-13pt-code-18pt"]}}]
        variants = [{"palette": "midnight", "text_size": 13},
                    {"palette": "midnight", "text_size": 13, "id": "midnight-13pt-code-18pt",
                     "settings": {"code_text_size": 18}},
                    {"palette": "porcelain", "id": "porcelain-code-18pt",
                     "settings": {"code_text_size": 18, "reopen_last": False}}]
        loaded = scenario.validate(spec(settings={"code_text_size": 12, "system_code_font": False}, steps=steps,
                                        variants=variants))
        plain, code, light = loaded["variants"]
        # The spec's settings, the variant's own over them, then the variant's text size.
        self.assertEqual(scenario.store_settings(loaded, plain),
                         {"code_text_size": 12, "system_code_font": False, "interface_text_size": 13})
        self.assertEqual(scenario.store_settings(loaded, code),
                         {"code_text_size": 18, "system_code_font": False, "interface_text_size": 13})
        self.assertEqual(scenario.store_settings(loaded, light),
                         {"code_text_size": 18, "system_code_font": False, "reopen_last": False})
        # A variant with settings commits under its id; a plain one keeps its palette and size.
        self.assertEqual([c.name for c in scenario.committed(loaded, roles=("cand",))], [
            "candidate-midnight-13pt-1000x680-focus.png", "candidate-midnight-13pt-code-18pt-1000x680-focus.png",
            "candidate-porcelain-code-18pt-1000x680-focus.png"])
        self.assertEqual((plain.describe(), code.describe()),
                         ({"id": "midnight-13pt", "palette": "midnight", "text_size": 13},
                          {"id": "midnight-13pt-code-18pt", "palette": "midnight", "text_size": 13,
                           "settings": {"code_text_size": 18}}))
        # `when` names a variant by its id.
        self.assertEqual(scenario.steps_for(loaded, code)[-1].get("key"), "Escape")
        self.assertEqual([len(scenario.steps_for(loaded, v)) for v in (plain, code, light)], [6, 7, 6])

    def test_steps_filtered_by_variant(self) -> None:
        steps = spec()["steps"] + [{"key": "Escape", "when": {"palette": ["porcelain"]}},
                                   {"wait": 1, "when": {"variant": ["midnight"], "text_size": [None]}}]
        loaded = scenario.validate(spec(steps=steps))
        midnight, porcelain = loaded["variants"]
        self.assertEqual([s.get("key") or s.get("wait") for s in scenario.steps_for(loaded, midnight)][-1], 1)
        self.assertEqual(scenario.steps_for(loaded, porcelain)[-1]["key"], "Escape")

    def test_a_read_only_step_takes_a_note_and_a_filter(self) -> None:
        step = {"read_only": "config/gitturtle", "note": "force a save error", "when": {"palette": ["midnight"]}}
        loaded = scenario.validate(spec(steps=[step] + SPEC["steps"]))
        midnight, porcelain = loaded["variants"]
        self.assertEqual(scenario.steps_for(loaded, midnight)[0]["read_only"], "config/gitturtle")
        self.assertNotIn("read_only", scenario.steps_for(loaded, porcelain)[0])
        self.assertEqual(play.session_step(loaded["steps"][0], "cand"),
                         {"read_only": "config/gitturtle", "note": "force a save error"})

    def test_bad_specs(self) -> None:
        def step(index, **changes):
            steps = copy.deepcopy(SPEC["steps"])
            steps[index] = dict(steps[index], **changes)
            return steps

        def without(index, key):
            steps = copy.deepcopy(SPEC["steps"])
            del steps[index][key]
            return steps

        analysis = copy.deepcopy(SPEC["analyses"][0])
        cases = [
            (dict(version=2), r"\$\.version: this tooling reads version 1"),
            (dict(colour="red"), r"\$: unknown key\(s\) colour"),
            (dict(task="Bad Task"), r"\$\.task: 'Bad Task' does not match"),
            (dict(window=[1000]), r"\$\.window: expected a list of 2"),
            (dict(roles=["base", "head"]), r"\$\.roles: expected distinct roles"),
            (dict(variants=[{"palette": "midnight"}, {"palette": "midnight"}]), "variant id\\(s\\) midnight repeat"),
            (dict(variants=[{"palette": "midnight", "text_size": 30}]), r"text_size: 30 is outside 11..18"),
            (dict(crops={"panel": [10, 10, 5, 60]}), r"\$\.crops\.panel: .* is empty"),
            (dict(crops={"panel": [10, 10, 1100, 60]}), "reaches outside the 1000x680 window"),
            (dict(settings={"theme": "porcelain"}), r"\$\.settings: the palette and text size come from the variants"),
            (dict(settings={"interface_text_size": 14}), r"\$\.settings: the palette and text size come from"),
            (dict(settings=["code_text_size"]), r"\$\.settings: expected an object, got list"),
            (dict(settings={"follow_system": False}),
             r"\$\.settings\.follow_system: the generated store turns Follow system off so frames do not depend on "
             r"the host's appearance"),
            # A variant's settings are refused exactly as the spec's are.
            (dict(variants=[{"palette": "midnight", "id": "midnight-x", "settings": {"theme": "porcelain"}}]),
             r"\$\.variants\[0\]\.settings: the palette and text size come from the variants"),
            (dict(variants=[{"palette": "midnight"}, {"palette": "midnight", "text_size": 13, "id": "midnight-13pt-x",
                                                     "settings": {"interface_text_size": 18}}]),
             r"\$\.variants\[1\]\.settings: the palette and text size come from the variants"),
            (dict(variants=[{"palette": "midnight", "id": "midnight-x", "settings": ["code_text_size"]}]),
             r"\$\.variants\[0\]\.settings: expected an object, got list"),
            (dict(variants=[{"palette": "midnight"}, {"palette": "midnight", "id": "midnight-x",
                                                     "settings": {"code_text_size": 18, "follow_system": True}}]),
             r"\$\.variants\[1\]\.settings\.follow_system: the generated store turns Follow system off"),
            (dict(variants=[{"palette": "midnight", "id": "midnight-x", "settings": {}}]),
             r"\$\.variants\[0\]\.settings: no settings; leave \"settings\" out"),
            (dict(variants=[{"palette": "midnight", "settings": {"code_text_size": 18}}]),
             r"\$\.variants\[0\]\.id: a variant with settings needs an \"id\" that extends 'midnight-'"),
            (dict(variants=[{"palette": "midnight", "text_size": 13, "id": "code-18pt",
                             "settings": {"code_text_size": 18}}]),
             r"\$\.variants\[0\]\.id: .* extends 'midnight-13pt-'"),
            (dict(variants=[{"palette": "midnight", "id": "midnight-", "settings": {"code_text_size": 18}}]),
             r"\$\.variants\[0\]\.id: a variant with settings needs"),
            (dict(variants={"palettes": ["midnight"], "settings": {"code_text_size": 18}}),
             r"\$\.variants: unknown key\(s\) settings"),
            (dict(env={"GITTURTLE_GITHUB_FIXTURE": 1}), r"\$\.env\.GITTURTLE_GITHUB_FIXTURE: expected a non-empty"),
            (dict(env={"GPUI_X11_SCALE_FACTOR": "2"}), r"\$\.env\.GPUI_X11_SCALE_FACTOR: the run sets this itself"),
            (dict(env={"DISPLAY": ":5"}), r"\$\.env\.DISPLAY: the run sets this itself"),
            (dict(env={"XDG_RUNTIME_DIR": "/x"}), r"\$\.env\.XDG_RUNTIME_DIR: the run sets this itself"),
            (dict(steps=step(2, on_fail=[{"key": "Escape", "mods": "Shift_L"}])),
             r"on_fail\[0\]\.mods: expected a list"),
            (dict(steps=step(1, type="abc")), r"\$\.steps\[1\]: needs exactly one action"),
            (dict(steps=step(1, hold=True)), r"\$\.steps\[1\]: unknown key\(s\) hold"),
            (dict(steps=step(1, repeat=0)), r"\$\.steps\[1\]\.repeat: 0 is outside 1..200"),
            (dict(steps=step(1, key="Ta b")), r"\$\.steps\[1\]\.key: 'Ta b' does not match"),
            (dict(steps=step(4, crop="dialog")), r"\$\.steps\[4\]\.crop: no crop box named 'dialog'"),
            (dict(steps=without(4, "shows")), r"\$\.steps\[4\]: a committed capture needs \"shows\""),
            (dict(steps=step(4, shows={"base": "no ring"})), "one line for each committed role"),
            (dict(steps=step(4, capture="rest")), "capture 'rest' is taken twice"),
            (dict(steps=step(2, guard={"kind": "compare", "a": "later", "b": "@now"})),
             "the guard reads 'later', which is not marked or captured before it"),
            (dict(steps=step(2, guard={"kind": "glow", "a": "page", "b": "@now"})), r"guard\.kind: expected one of"),
            (dict(steps=step(1, when={"palette": ["solarized"]})), "matches no variant"),
            (dict(steps=step(0, mark="Page")), r"\$\.steps\[0\]\.mark: 'Page' does not match"),
            (dict(steps=[{"wheel": [10, 10], "steps": 0}]), "0 steps sends nothing"),
            (dict(steps=[{"click": [1000, 10]}]), "outside the 1000x680 window"),
            (dict(steps=[{"read_only": "/config/gitturtle"}]), r"\$\.steps\[0\]\.read_only: '/config/gitturtle' must "
                                                                "be a relative POSIX path"),
            (dict(steps=[{"read_only": "config/../home"}]), r"\$\.steps\[0\]\.read_only: .* without empty, \. or \.\."),
            (dict(steps=[{"read_only": "home/.config"}]), r"\$\.steps\[0\]\.read_only: .* must start with one of "
                                                          "config, data, cache, state"),
            (dict(steps=[{"read_only": "captures/x"}]), r"\$\.steps\[0\]\.read_only: 'captures/x' must start with"),
            (dict(steps=[{"read_only": ""}]), r"\$\.steps\[0\]\.read_only: expected a non-empty path"),
            (dict(steps=[{"read_only": ["config"]}]), r"\$\.steps\[0\]\.read_only: expected a non-empty path"),
            (dict(steps=[{"read_only": "config/gitturtle", "mode": 0}]), r"\$\.steps\[0\]: unknown key\(s\) mode"),
            (dict(steps=SPEC["steps"] + [{"capture": "focus-2", "crop": "panel", "shows": "x", "roles": ["head"]}]),
             r"roles: 'head' does not match"),
            (dict(analyses=[dict(analysis, frame="missing")]), "reads 'missing', which variant midnight never captures"),
            (dict(analyses=[dict(analysis, frame="head:focus")]), "'head' is not one of the roles"),
            (dict(analyses=[dict(analysis, sides=["top", "top"])]), "distinct sides"),
            (dict(analyses=[dict(analysis, rect=[20, 20, 100, 700])]), "reaches outside"),
            (dict(analyses=[analysis, dict(analysis)]), "analysis names repeat"),
            (dict(analyses=[{"name": "c", "kind": "clearance", "frame": "focus", "rect": FOCUS_RECT, "side": "top",
                             "surface": [0, 0, 0]}]), "needs min_px or max_px"),
            (dict(analyses=[{"name": "f", "kind": "fill", "frame": "focus", "region": PANEL,
                             "reference": {"colour": [0, 0, 300]}, "min_contrast": 1.2}]), r"0\.\.255"),
            (dict(analyses=[{"name": "x", "kind": "compare", "a": "base:rest", "b": "cand:rest",
                             "roles": ["base"]}]), "runs once per variant"),
            (dict(analyses=[{"name": "x", "kind": "compare", "a": "rest", "b": "focus", "masks": ["timing"]}]),
             "no named mask 'timing'"),
            (dict(analyses=[{"name": "x", "kind": "compare", "a": "rest", "b": "focus", "crop": "panel",
                             "region": PANEL}]), "a region or a crop, not both"),
        ]
        for changes, match in cases:
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                scenario.validate(spec(**changes))
        # Two variants that would commit one name, plain or through a settings variant's id.
        for variants, clash in (([{"palette": "midnight", "id": "a"}, {"palette": "midnight", "id": "b"}],
                                 "a and b would both commit base-midnight-1000x680-focus.png"),
                                ([{"palette": "midnight-code", "id": "plain"},
                                  {"palette": "midnight", "id": "midnight-code", "settings": {"code_text_size": 18}}],
                                 "plain and midnight-code would both commit base-midnight-code-1000x680-focus.png")):
            with self.subTest(clash=clash), \
                    self.assertRaisesRegex(scenario.SpecError, rf"\$\.variants: variants {clash} \(step 4\)"):
                scenario.validate(spec(variants=variants))

    def test_load_reports_json_errors(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "s.json"
            path.write_text("{")
            with self.assertRaisesRegex(scenario.SpecError, "is not JSON"):
                scenario.load(path)

    def test_cli_check(self) -> None:
        ok = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(EXAMPLE)],
                            capture_output=True, text=True)
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertIn("20 committed crops", ok.stdout)
        self.assertEqual(sorted(re.findall(r"^    (\S+\.png)  \(", ok.stdout, re.M)),
                         sorted(path.name for path in EXAMPLE.parent.glob("*.png")))
        with tempfile.TemporaryDirectory() as scratch:
            bad = Path(scratch) / "bad.json"
            bad.write_text(json.dumps(spec(steps=[{"park": False}])))
            result = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(bad)],
                                    capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("$.steps[0].park: expected true", result.stderr)


def frame(ring: bool = False, patch: bool = False):
    """A window frame: the surface, the panel's ring with `ring`, and a noisy patch (private text) with `patch`."""
    from PIL import Image, ImageDraw

    image = Image.new("RGB", (1000, 680), SURFACE)
    if ring:
        ImageDraw.Draw(image).rectangle(FOCUS_RECT[:2] + [FOCUS_RECT[2] - 1, FOCUS_RECT[3] - 1], outline=RING,
                                        width=2)
    if patch:
        for x in range(60, 76):
            for y in range(30, 42):
                image.putpixel((x, y), ((x * 37 + y * 91) % 256, (x * 11) % 256, (y * 53) % 256))
    return image


@unittest.skipUnless(HAVE_PIL, "Pillow is not installed")
class BundleTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.spec = scenario.validate(spec(), "f" * 64)

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def captures(self, root: Path, roles=("base", "cand"), patch: bool = False) -> None:
        for role in roles:
            for variant in self.spec["variants"]:
                captures = root / role / variant.id / "captures"
                captures.mkdir(parents=True)
                frame().save(captures / "rest.png")
                frame(ring=role == "cand", patch=patch).save(captures / "focus.png")
                frame().save(captures / "only-base.png")

    def test_crops_and_manifest(self) -> None:
        self.captures(self.root)
        entries = evidence.write_crops(self.root, scenario.committed(self.spec), self.root / "commit", (1000, 680))
        self.assertEqual(sorted(p.name for p in (self.root / "commit").iterdir()), sorted(e["name"] for e in entries))
        first = entries[1]
        self.assertEqual(first["name"], "candidate-midnight-1000x680-focus.png")
        self.assertEqual((first["role"], first["capture"], first["crop"], first["box"], first["size"]),
                         ("cand", "focus", "panel", PANEL, [100, 50]))
        self.assertEqual(first["variant"], {"id": "midnight", "palette": "midnight", "text_size": None})
        self.assertEqual(first["shows"], "the ring")
        self.assertEqual(first["source"], "cand/midnight/captures/focus.png")
        data = (self.root / "commit" / first["name"]).read_bytes()
        self.assertEqual((first["bytes"], first["sha256"]), (len(data), evidence.identity.sha256_bytes(data)))
        manifest = evidence.commit_manifest(self.spec, entries)
        self.assertEqual((manifest["task"], manifest["scenario_sha256"], manifest["window"], len(manifest["frames"])),
                         ("demo-task", "f" * 64, [1000, 680], 6))
        # The same pixels crop to the same bytes.
        again = self.root / "again.png"
        evidence.cut(self.root / "cand" / "midnight" / "captures" / "focus.png", PANEL, again)
        self.assertEqual(again.read_bytes(), data)

    def test_a_frame_of_another_size_is_never_cropped(self) -> None:
        from PIL import Image

        self.captures(self.root)
        Image.new("RGB", (900, 600), SURFACE).save(self.root / "cand" / "porcelain" / "captures" / "focus.png")
        with self.assertRaisesRegex(SystemExit, "is 900x600, not the scenario's 1000x680 window"):
            evidence.write_crops(self.root, scenario.committed(self.spec), self.root / "commit", (1000, 680))

    def test_analyses_meet_their_expectations(self) -> None:
        self.captures(self.root)
        report = evidence.run_analyses(self.spec, self.root)
        self.assertEqual((report["total"], report["as_expected"], report["unexpected"]), (6, 6, []))
        ring = [r for r in report["results"] if r["name"] == "ring" and r["role"] == "cand"][0]
        self.assertTrue(ring["passed"])
        self.assertEqual(ring["result"]["box"], FOCUS_RECT)
        frame(ring=True).save(self.root / "base" / "porcelain" / "captures" / "focus.png")
        report = evidence.run_analyses(self.spec, self.root)
        self.assertEqual(report["unexpected"], ["ring (base, porcelain): passed, expected fail"])

    def test_recheck_comparison(self) -> None:
        from PIL import Image

        crops = scenario.committed(self.spec, roles=("cand",))
        self.captures(self.root, roles=("cand",))
        evidence.write_crops(self.root, crops, self.root / "commit", (1000, 680))
        committed = self.root / "committed"
        shutil.copytree(self.root / "commit", committed)
        result = evidence.compare_committed(crops, self.root / "commit", committed)
        self.assertEqual(result["verdict"], "identical")
        self.assertEqual([r["result"] for r in result["results"]], ["identical", "identical"])
        # Same pixels in other bytes, one pixel changed, a missing file, and a committed crop the spec lacks.
        midnight, porcelain = (committed / crop.name for crop in crops)
        with Image.open(midnight) as image:
            image.copy().save(midnight, compress_level=1)
        with Image.open(porcelain) as image:
            changed = image.copy()
        changed.putpixel((50, 25), (255, 0, 0))
        changed.save(porcelain)
        result = evidence.compare_committed(crops, self.root / "commit", committed)
        self.assertEqual(result["verdict"], "different")
        self.assertEqual([r["result"] for r in result["results"]], ["pixels-identical", "different"])
        self.assertEqual((result["results"][1]["differing_pixels"], result["results"][1]["regions"]),
                         (1, [[50, 25, 51, 26]]))
        porcelain.unlink()
        shutil.copy(midnight, committed / "candidate-midnight-1000x680-old.png")
        result = evidence.compare_committed(crops, self.root / "commit", committed)
        self.assertEqual([(r["name"], r["result"]) for r in result["results"]][1:], [
            ("candidate-porcelain-1000x680-focus.png", "missing"),
            ("candidate-midnight-1000x680-old.png", "not-in-spec")])

    def write_bundle(self, bundle: Path, verdict: str = "pass") -> None:
        (bundle).mkdir()
        (bundle / "scenario.json").write_text(json.dumps(spec()))
        loaded = scenario.load(bundle / "scenario.json")
        self.captures(bundle)
        entries = evidence.write_crops(bundle, scenario.committed(loaded), bundle / "commit", loaded["window"])
        evidence.write_json(bundle / "commit-manifest.json", evidence.commit_manifest(loaded, entries))
        evidence.write_json(bundle / "analysis.json", evidence.run_analyses(loaded, bundle))
        launch = dict(started_utc="2026-10-01T22:33:10Z", ended_utc="2026-10-01T22:40:59Z", fixture_unchanged=True)
        evidence.write_json(bundle / "run.json", dict(
            scenario_sha256=loaded["sha256"], builds={"base": build("/x/base", BASE_SHA), "cand": build("/x/cand", CAND_SHA)},
            host=dict(description="Ubuntu 26.04 LTS, GNOME 50, XWayland :0, scale factor 1, window 1000x680",
                      input="mutter"),
            fixture=dict(summary="/tmp/gitturtle-evidence/fixtures/demo/repo (5 commits)"),
            launches=[launch, dict(launch, started_utc="2026-10-01T22:41:00Z", ended_utc="2026-10-01T22:44:00Z")],
            privacy=dict(frames=6, matched=0, verdicts={e["name"]: "clean" for e in entries},
                         scanned_sha256={e["name"]: e["sha256"] for e in entries}),
            verdict=dict(result=verdict)))

    def test_attestation(self) -> None:
        bundle = self.root / "bundle"
        self.write_bundle(bundle)
        payload = evidence.attestation(bundle, "demo-task", CAND_SHA, "e" * 40, evidence_commit="d" * 40)
        self.assertEqual((payload["task"], payload["kind"], payload["candidate"], payload["base"]),
                         ("demo-task", "native", CAND_SHA, "e" * 40))
        self.assertEqual(payload["attested_executable"]["source_revision"], CAND_SHA)
        self.assertEqual(payload["attested_executable"]["profile"], "release")
        self.assertEqual(payload["comparison_builds"], {"base": {"commit": BASE_SHA, "sha256": "a" * 64}})
        self.assertEqual(payload["sessions"], [dict(
            utc="2026-10-01 22:33-22:44",
            what="base aaaaaaa vs candidate bbbbbbb: the panel shows its focus; 6 crops privacy-scanned clean "
                 "(6 scanned, 0 matched); 6 of 6 analyses as the scenario expects")])
        self.assertTrue(payload["input"].startswith("qa.py scenario run through Mutter RemoteDesktop"))
        self.assertEqual(payload["fixtures"], "/tmp/gitturtle-evidence/fixtures/demo/repo (5 commits), "
                                              "unchanged by every launch")
        self.assertEqual(payload["committed_frames"], "docs/evidence/demo-task/ (6 crops) and its scenario.json")
        self.assertEqual(len(payload["frames"]), 6)
        self.assertEqual((payload["evidence_commit"], payload["limitations"]), ("d" * 40, "Linux only"))
        self.assertEqual(payload["scenario"]["committed_path"], "docs/evidence/demo-task/scenario.json")
        # A rebuilt candidate is attested through its re-check.
        recheck = self.root / "recheck"
        recheck.mkdir()
        checked = dict(scenario_sha256=payload["scenario"]["sha256"], verdict="identical",
                       executable=build("/x/rebuilt", REBUILT_SHA), committed="docs/evidence/demo-task",
                       launches=[dict(started_utc="2026-10-02T00:17:00Z", ended_utc="2026-10-02T00:21:30Z")],
                       results=[dict(name=f["name"], result="identical", committed_sha256=f["sha256"])
                                for f in payload["frames"] if f["name"].startswith("candidate-")])
        evidence.write_json(recheck / "recheck.json", checked)
        rebuilt = evidence.attestation(bundle, "demo-task", REBUILT_SHA, "e" * 40, recheck=recheck,
                                       what="the focus ring", limitations="none")
        self.assertEqual(rebuilt["attested_executable"]["path"], "/x/rebuilt")
        self.assertEqual(rebuilt["comparison_builds"]["first_candidate"]["commit"], CAND_SHA)
        self.assertEqual(rebuilt["sessions"][1], dict(
            utc="2026-10-02 00:17-00:21", what="re-check of ccccccc: the 2 candidate crops re-captured "
                                               "byte-identical (sha256) to the committed "
                                               "docs/evidence/demo-task/candidate-*.png"))
        self.assertIn("the focus ring", rebuilt["sessions"][0]["what"])
        self.assertIn("(re-check in", rebuilt["bundle"])
        for kwargs, match in ((dict(task="other-task"), "is for demo-task"),
                              (dict(candidate=REBUILT_SHA), "built from b"),
                              (dict(candidate=CAND_SHA, recheck=recheck), "built from c")):
            arguments = dict(task="demo-task", candidate=CAND_SHA, base="e" * 40)
            arguments.update(kwargs)
            with self.subTest(match=match), self.assertRaisesRegex(SystemExit, match):
                evidence.attestation(bundle, **arguments)
        evidence.write_json(recheck / "recheck.json", dict(checked, verdict="different"))
        with self.assertRaisesRegex(SystemExit, "re-check found differences"):
            evidence.attestation(bundle, "demo-task", REBUILT_SHA, "e" * 40, recheck=recheck)
        # A re-check of other crops, or of other bytes, does not attest this bundle's candidate.
        other = [dict(r, committed_sha256="0" * 64) if i == 0 else r for i, r in enumerate(checked["results"])]
        for results in (other, checked["results"][:1], checked["results"] + [dict(name="x.png", result="identical")]):
            evidence.write_json(recheck / "recheck.json", dict(checked, results=results))
            with self.subTest(results=len(results)), self.assertRaisesRegex(SystemExit, "exactly this bundle's"):
                evidence.attestation(bundle, "demo-task", REBUILT_SHA, "e" * 40, recheck=recheck)
        # Every file in the bundle must come from its own scenario.json, and the scan must cover its crops.
        for name, change in (("analysis.json", dict(scenario_sha256="0" * 64)),
                             ("commit-manifest.json", dict(scenario_sha256="0" * 64)),
                             ("run.json", dict(privacy=dict(scanned_sha256={}, verdicts={})))):
            original = (bundle / name).read_text()
            evidence.write_json(bundle / name, dict(json.loads(original), **change))
            with self.subTest(name=name), self.assertRaisesRegex(SystemExit, "refusing: (analysis|commit|the priv)"):
                evidence.attestation(bundle, "demo-task", CAND_SHA, "e" * 40)
            (bundle / name).write_text(original)
        failed = self.root / "failed"
        self.write_bundle(failed, verdict="fail")
        with self.assertRaisesRegex(SystemExit, "did not pass"):
            evidence.attestation(failed, "demo-task", CAND_SHA, "e" * 40)

    def test_attestation_cli_never_overwrites(self) -> None:
        bundle = self.root / "bundle"
        self.write_bundle(bundle)
        out = self.root / "attestation.json"
        command = [sys.executable, "-B", str(QA), "attestation", "--bundle", str(bundle), "--task", "demo-task",
                   "--candidate", CAND_SHA, "--base", "e" * 40, "--out", str(out)]
        first = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(json.loads(out.read_text())["candidate"], CAND_SHA)
        second = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(second.returncode, 2)
        self.assertIn("never overwritten", second.stderr)
        short = subprocess.run(command[:-6] + ["--candidate", "bbbbbbb", "--base", "e" * 40, "--out", str(out)],
                               capture_output=True, text=True)
        self.assertEqual(short.returncode, 2)


@unittest.skipUnless(shutil.which("git"), "no git")
class LaunchRecordTest(unittest.TestCase):
    def launch(self, loaded: dict, variant: scenario.Variant, restore_failure: str | None = None):
        """`play.launch` through a session that keeps its store and runs nothing; the record and that session."""
        sessions = []

        class FakeSession:
            """Runs the steps and, on close, reports `restore_failure` as a read_only path it could not restore."""

            def __init__(self, binary, fixture, run_dir, store, **kwargs) -> None:
                self.store = store
                self.log = dict(header=dict(started_utc="2026-10-02T10:00:00Z", ended_utc="2026-10-02T10:01:00Z"),
                                captures=[], fixture_unchanged=True)
                self.restore_failures, self.steps = [], []
                sessions.append(self)

            def launch(self) -> None:
                pass

            def run(self, steps) -> None:
                self.steps += steps

            def close(self) -> int:
                if restore_failure is not None:
                    self.restore_failures.append(restore_failure)
                self.log["exit"] = -15
                return -15

        with tempfile.TemporaryDirectory() as scratch:
            fixture = Path(scratch) / "fixture"
            subprocess.run(["git", "init", "-q", str(fixture)], check=True)
            with mock.patch("native_qa.session.Session", FakeSession), contextlib.redirect_stdout(io.StringIO()):
                record = play.launch(loaded, "cand", variant, Path("/x/cand"), fixture, Path(scratch) / "run",
                                     dict(display=":1", settle=0, backend="xtest"))
        return record, sessions[0]

    def test_a_mode_the_launch_could_not_restore_fails_it(self) -> None:
        loaded = scenario.validate(spec(steps=[{"read_only": "config/gitturtle"}, {"wait": 0}], analyses=[]), "f" * 64)
        failure = "could not restore mode 0755 of config/gitturtle in /run: denied"
        record, _ = self.launch(loaded, loaded["variants"][0], failure)
        self.assertEqual((record["error"], record["refusal"]), (failure, False))

    def test_a_launch_seeds_its_variants_merged_store_and_records_its_digest(self) -> None:
        loaded = scenario.validate(spec(settings={"code_text_size": 12, "system_code_font": False}, variants=[
            {"palette": "midnight", "text_size": 13},
            {"palette": "porcelain", "text_size": 13, "id": "porcelain-13pt-code-18pt",
             "settings": {"code_text_size": 18}}], steps=[{"wait": 0}], analyses=[]), "f" * 64)
        plain, code = loaded["variants"]
        record, session = self.launch(loaded, code)
        self.assertEqual(json.loads(session.store)["settings"],
                         {"theme": "porcelain", "follow_system": False, "code_text_size": 18,
                          "system_code_font": False, "interface_text_size": 13})
        self.assertEqual(session.log["scenario"]["variant"], code.describe())
        self.assertEqual(record["store_sha256"], evidence.identity.sha256_bytes(session.store))
        plain_record, plain_session = self.launch(loaded, plain)
        self.assertEqual(json.loads(plain_session.store)["settings"],
                         {"theme": "midnight", "follow_system": False, "code_text_size": 12,
                          "system_code_font": False, "interface_text_size": 13})
        self.assertNotIn("settings", plain_session.log["scenario"]["variant"])
        self.assertEqual(plain_record["store_sha256"], evidence.identity.sha256_bytes(plain_session.store))


@unittest.skipUnless(HAVE_PIL and shutil.which("git"), "Pillow or git is missing")
class RunWithoutDisplayTest(unittest.TestCase):
    """`play.run` and `play.recheck` end to end, with each launch replaced by frames written to disk."""

    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name).resolve()
        self.spec_path = self.root / "scenario.json"
        self.spec_path.write_text(json.dumps(spec()))
        self.fixture = self.root / "fixture"
        subprocess.run(["git", "init", "-q", str(self.fixture)], check=True)
        self.templates = self.root / "templates"
        self.templates.mkdir()
        from PIL import Image

        Image.new("L", (6, 6), 0).save(self.templates / "flat.png")  # matches nothing: flat windows are skipped
        self.patch = False

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def fake_launch(self, spec_, role, variant, binary, fixture, run_dir, options):
        captures = run_dir / "captures"
        captures.mkdir(parents=True)
        frame().save(captures / "rest.png")
        frame(ring=role == "cand", patch=self.patch).save(captures / "focus.png")
        frame().save(captures / "only-base.png")
        return dict(role=role, variant=variant.id, run_dir=str(run_dir), error=None, refusal=False,
                    started_utc="2026-10-01T22:33:10Z", ended_utc="2026-10-01T22:35:00Z", exit=-15,
                    fixture_unchanged=True, captures=3, guards=1)

    def patched(self):
        stack = contextlib.ExitStack()
        builds = {"/x/base": build("/x/base", BASE_SHA), "/x/cand": build("/x/cand", CAND_SHA),
                  "/x/same": build("/x/same", BASE_SHA)}
        stack.enter_context(mock.patch.object(play, "launch", self.fake_launch))
        stack.enter_context(mock.patch.object(play.identity, "describe", lambda path: builds[str(path)]))
        stack.enter_context(mock.patch("native_qa.mutter.choose_input", lambda choice, argvs: "xtest"))
        stack.enter_context(mock.patch.object(play, "host", lambda *args: dict(description="test host",
                                                                                input="xtest")))
        stack.enter_context(mock.patch("native_qa.privacy.helper", lambda build=True: None))
        stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        stack.enter_context(contextlib.redirect_stderr(io.StringIO()))
        return stack

    def run_bundle(self, name: str, **builds) -> tuple[int, Path]:
        out = self.root / name
        with self.patched():
            code = play.run(self.spec_path, {role: Path(path) for role, path in builds.items()}, out, self.fixture,
                            templates=self.templates, jobs=1)
        return code, out

    def test_a_bundle_from_launches_to_attestation(self) -> None:
        code, out = self.run_bundle("bundle", base="/x/base", cand="/x/cand")
        self.assertEqual(code, 0)
        run = json.loads((out / "run.json").read_text())
        self.assertEqual(run["verdict"], {"result": "pass", "exit": 0, "findings": []})
        self.assertEqual([(l["role"], l["variant"]) for l in run["launches"]],
                         [("base", "midnight"), ("cand", "midnight"), ("base", "porcelain"), ("cand", "porcelain")])
        manifest = json.loads((out / "commit-manifest.json").read_text())
        self.assertEqual(len(manifest["frames"]), 6)
        # The privacy scan read exactly the committed bytes.
        self.assertEqual(run["privacy"]["scanned_sha256"], {f["name"]: f["sha256"] for f in manifest["frames"]})
        self.assertEqual(set(run["privacy"]["verdicts"].values()), {"clean"})
        self.assertEqual((out / "scenario.json").read_bytes(), self.spec_path.read_bytes())
        self.assertEqual(json.loads((out / "analysis.json").read_text())["unexpected"], [])
        payload = evidence.attestation(out, "demo-task", CAND_SHA, "e" * 40)
        self.assertEqual(payload["host"], "test host")
        # A later build re-checks the committed candidate crops by command.
        committed = self.root / "docs-evidence"
        committed.mkdir()
        for entry in manifest["frames"]:
            shutil.copy(out / "commit" / entry["name"], committed / entry["name"])
        with self.patched():
            self.assertEqual(play.recheck(self.spec_path, Path("/x/cand"), committed, self.root / "re",
                                          self.fixture), 0)
        checked = json.loads((self.root / "re" / "recheck.json").read_text())
        self.assertEqual((checked["verdict"], len(checked["results"])), ("identical", 2))
        self.patch = True
        with self.patched():
            self.assertEqual(play.recheck(self.spec_path, Path("/x/cand"), committed, self.root / "re2",
                                          self.fixture), 1)
        payload = evidence.attestation(out, "demo-task", CAND_SHA, "e" * 40, recheck=self.root / "re")
        self.assertEqual(len(payload["sessions"]), 2)

    def test_a_privacy_match_fails_the_bundle(self) -> None:
        self.patch = True
        frame(patch=True).crop((60, 30, 76, 42)).convert("L").save(self.templates / "private.png")
        code, out = self.run_bundle("bundle", base="/x/base", cand="/x/cand")
        self.assertEqual(code, 1)
        run = json.loads((out / "run.json").read_text())
        self.assertEqual(run["verdict"]["result"], "fail")
        self.assertEqual(run["privacy"]["matched"], 4)  # the focus crops; only-base has no patch
        with self.assertRaisesRegex(SystemExit, "did not pass"):
            evidence.attestation(out, "demo-task", CAND_SHA, "e" * 40)

    def test_a_committed_capture_must_come_from_a_settled_window(self) -> None:
        from types import SimpleNamespace

        steps = {step["capture"]: step for step in scenario.validate(spec())["steps"] if "capture" in step}
        unsettled = SimpleNamespace(log={"captures": [{"stable_s": None}]})
        settled = SimpleNamespace(log={"captures": [{"stable_s": 1.2}]})
        with self.assertRaisesRegex(SystemExit, "inconclusive: capture focus .* did not settle within 8.0 s"):
            play.require_settled(unsettled, steps["focus"], "cand")
        play.require_settled(settled, steps["focus"], "cand")
        play.require_settled(unsettled, steps["rest"], "cand")       # not committed
        play.require_settled(unsettled, steps["only-base"], "cand")  # committed for the base only
        with self.assertRaises(SystemExit):
            play.require_settled(unsettled, steps["only-base"], "base")

    def test_refusals_before_any_launch(self) -> None:
        with self.patched():
            for builds, match in ((dict(cand="/x/cand"), "give exactly one --build"),
                                  (dict(base="/x/same", cand="/x/base"), "same source_revision")):
                with self.subTest(match=match), self.assertRaisesRegex(SystemExit, match):
                    play.run(self.spec_path, {r: Path(p) for r, p in builds.items()}, self.root / "out",
                             self.fixture, templates=self.templates)
            with self.assertRaisesRegex(SystemExit, "no privacy templates"):
                play.run(self.spec_path, {"base": Path("/x/base"), "cand": Path("/x/cand")}, self.root / "out",
                         self.fixture, templates=self.root / "absent")
            (self.root / "empty").mkdir()
            with self.assertRaisesRegex(SystemExit, "no PNG or JPEG templates"):
                play.run(self.spec_path, {"base": Path("/x/base"), "cand": Path("/x/cand")}, self.root / "out",
                         self.fixture, templates=self.root / "empty")
            with self.assertRaisesRegex(SystemExit, "must be an absolute path"):
                play.run(self.spec_path, {"base": Path("/x/base"), "cand": Path("/x/cand")}, Path("relative"),
                         self.fixture, templates=self.templates)
        self.assertFalse((self.root / "out").exists())


if __name__ == "__main__":
    unittest.main()
