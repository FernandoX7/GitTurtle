from __future__ import annotations

import contextlib
import copy
import importlib.util
import io
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

from native_qa import evidence, play, scenario, session
from native_qa.test_analysis import list_frame
from native_qa.test_session import FakeClock, ScriptedScreen

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


LIST = [100, 100, 400, 300]
PROBE_STEP = {"probe": "rows", "send": "Tab", "repeat": 2, "region": "list", "quiet": 0.051, "timeout": 0.5,
              "stable_within": 0.2, "note": "Tab past the list's lower edge, then Shift+Tab"}
PROBE_ANALYSES = [
    {"name": "rows-ring", "kind": "probe_ring", "probe": "rows", "clip": "list", "min_contrast": 3.0,
     "expect": {"base": "record", "cand": True}},
    {"name": "rows-endpoints", "kind": "probe_endpoints", "probe": "rows", "clip": "list",
     "expect": {"base": "record", "cand": True}},
]


def spec(**changes) -> dict:
    data = copy.deepcopy(SPEC)
    data.update(changes)
    return data


def probe_spec(**changes) -> dict:
    """SPEC with a list crop, a probe of two presses in it and both probe analyses, the base's only recorded."""
    data = spec(crops={"panel": PANEL, "list": LIST}, steps=SPEC["steps"] + [PROBE_STEP],
                analyses=SPEC["analyses"] + PROBE_ANALYSES)
    data.update(copy.deepcopy(changes))
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
        # A variant with settings commits under its id; a plain one keeps its palette and size. A size after the
        # settings' own part (`code-18pt`) is theirs, not the interface's.
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

    def test_a_theme_key_with_underscores_takes_every_variant_form(self) -> None:
        # 11 of the 20 built-in theme keys have one; ids and committed names keep it, as the palette label does.
        steps = SPEC["steps"] + [{"key": "Escape", "when": {"variant": ["solarized_dark-18pt-code-14pt"]}}]
        loaded = scenario.validate(spec(steps=steps, variants=[
            {"palette": "solarized_dark", "text_size": 18},
            {"palette": "solarized_dark", "text_size": 18, "id": "solarized_dark-18pt-code-14pt",
             "settings": {"code_text_size": 14}}]))
        plain, code = loaded["variants"]
        self.assertEqual((plain.id, code.id), ("solarized_dark-18pt", "solarized_dark-18pt-code-14pt"))
        self.assertEqual([c.name for c in scenario.committed(loaded, roles=("cand",))], [
            "candidate-solarized_dark-18pt-1000x680-focus.png",
            "candidate-solarized_dark-18pt-code-14pt-1000x680-focus.png"])
        self.assertEqual(scenario.store_settings(loaded, code), {"code_text_size": 14, "interface_text_size": 18})
        self.assertEqual(scenario.steps_for(loaded, code)[-1].get("key"), "Escape")
        combined = scenario.validate(spec(variants={"palettes": ["solarized_dark", "rose_pine_dawn"],
                                                    "text_sizes": [13]}))
        self.assertEqual([v.id for v in combined["variants"]], ["solarized_dark-13pt", "rose_pine_dawn-13pt"])

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
            # The id cannot claim an interface size the launch never sets.
            (dict(variants=[{"palette": "midnight", "id": "midnight-18pt-code", "settings": {"code_text_size": 18}}]),
             r"\$\.variants\[0\]\.id: 'midnight-18pt-code' names an interface size after 'midnight-' that only "
             r"\"text_size\" sets"),
            (dict(variants=[{"palette": "midnight", "id": "midnight-18pt", "settings": {"code_text_size": 18}}]),
             r"\$\.variants\[0\]\.id: 'midnight-18pt' names an interface size"),
            (dict(variants=[{"palette": "midnight", "text_size": 13, "id": "midnight-13pt-18pt-code",
                             "settings": {"code_text_size": 18}}]),
             r"\$\.variants\[0\]\.id: 'midnight-13pt-18pt-code' names an interface size after 'midnight-13pt-'"),
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

    def test_home_files_are_seeded_from_the_spec_and_each_variant(self) -> None:
        colors = ".local/state/omarchy/current/theme/colors.toml"
        home = {colors: 'accent = "#7aa2f7"\n', ".local/state/omarchy/current/theme.name": "tokyo-night\n",
                ".local/state/omarchy/current/theme/light.mode": "",
                "bin.dat": {"base64": "AAEC/w=="}}
        loaded = scenario.validate(spec(home=home, variants=[
            {"palette": "midnight"},
            {"palette": "midnight", "id": "midnight-catppuccin", "home": {colors: 'accent = "#89b4fa"\n'}},
            {"palette": "porcelain", "text_size": 13, "id": "porcelain-13pt-extra", "home": {"extra/x": "x"}}]))
        plain, other, extra = loaded["variants"]
        self.assertEqual(scenario.home_files(loaded, plain)[colors], b'accent = "#7aa2f7"\n')
        self.assertEqual(scenario.home_files(loaded, plain)["bin.dat"], b"\x00\x01\x02\xff")
        self.assertEqual(scenario.home_files(loaded, plain)[".local/state/omarchy/current/theme/light.mode"], b"")
        self.assertEqual(scenario.home_files(loaded, other)[colors], b'accent = "#89b4fa"\n')  # the variant's own
        self.assertEqual(sorted(scenario.home_files(loaded, extra)), sorted([*home, "extra/x"]))
        # A variant with its own HOME files commits under its id, and describes them by path only.
        self.assertEqual([c.name for c in scenario.committed(loaded, roles=("cand",))], [
            "candidate-midnight-1000x680-focus.png", "candidate-midnight-catppuccin-1000x680-focus.png",
            "candidate-porcelain-13pt-extra-1000x680-focus.png"])
        self.assertEqual(other.describe(), dict(id="midnight-catppuccin", palette="midnight", text_size=None,
                                                home=[colors]))
        self.assertEqual(scenario.validate(spec())["home"], {})

    def test_bad_home_files(self) -> None:
        good = {"notes.txt": "x"}
        for changes, match in (
                (dict(home={}), r"\$\.home: no files"),
                (dict(home=["notes.txt"]), r"\$\.home: expected an object"),
                (dict(home={"/etc/passwd": "x"}), r"\$\.home: '/etc/passwd' must be a relative POSIX path under HOME"),
                (dict(home={"../escape": "x"}), r"\$\.home: '\.\./escape' must be a relative POSIX path"),
                (dict(home={"a/./b": "x"}), "without empty, . or .. parts"),
                (dict(home={"a\\b": "x"}), "relative POSIX path"),
                (dict(home={".gitconfig": "[user]"}), "the run's own Git identity"),
                (dict(home={"a": "x", "a/b": "y"}), r"\$\.home: 'a' is a file, but 'a/b' needs it as a directory"),
                (dict(home={"a": 7}), r"\$\.home\['a'\]: expected an object, got int"),
                (dict(home={"a": {"base64": "!!"}}), r"\$\.home\['a'\]\.base64: not base64"),
                (dict(home={"a": {"base64": 7}}), r"\$\.home\['a'\]\.base64: expected a base64 string"),
                (dict(home={"a": {"text": "x"}}), r"\$\.home\['a'\]: unknown key\(s\) text"),
                (dict(home={"a": "\ud800"}), "not valid UTF-8"),
                (dict(home={f"f{i}": "" for i in range(65)}), "65 files; at most 64"),
                (dict(home={"big": "x" * 1_000_001}), "1000001 bytes; at most 1000000"),
                (dict(variants=[{"palette": "midnight", "home": good}]),
                 r"\$\.variants\[0\]\.id: a variant with HOME files needs an \"id\" that extends 'midnight-'"),
                (dict(variants=[{"palette": "midnight", "id": "midnight-13pt", "home": good}]),
                 r"names an interface size after 'midnight-' that only \"text_size\" sets; extend 'midnight-' with "
                 r"what the HOME files change"),
                (dict(home={"a": "x"}, variants=[{"palette": "midnight", "id": "midnight-x", "home": {"a/b": "y"}}]),
                 r"\$\.variants\[0\]\.home: with the spec's own HOME files, 'a' is a file, but 'a/b' needs it"),
                # Within the cap apart, over it merged: refused here, not by the launch's Session mid-run.
                (dict(home={f"spec-{i}": "" for i in range(40)}, variants=[
                    {"palette": "midnight"},
                    {"palette": "midnight", "id": "midnight-more", "home": {f"own-{i}": "" for i in range(30)}}]),
                 r"\$\.variants\[1\]\.home: with the spec's own HOME files, 70 files; at most 64")):
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                scenario.validate(spec(**changes))

    def test_readings_and_their_analyses(self) -> None:
        steps = SPEC["steps"] + [
            {"atspi_focus": "switch", "note": "the Switch holds focus"},
            {"store_snapshot": "store-before"},
            {"key": "space"},
            {"store_snapshot": "store-after", "path": "config/gitturtle/preferences.json", "quiet": 0.5,
             "within": 5},
            {"atspi_focus": "after-space", "within": 2}]
        analyses = SPEC["analyses"] + [
            {"name": "switch-focused", "kind": "atspi_focus", "focus": "switch", "node": "Follow system appearance",
             "role": "toggle button", "states": ["focused"], "not_states": ["pressed"]},
            {"name": "space-keeps-it", "kind": "atspi_focus", "focus": "after-space", "same_as": "switch",
             "expect": {"base": False, "cand": True}},
            {"name": "store-same", "kind": "store_compare", "a": "store-before", "b": "store-after"},
            {"name": "theme-same", "kind": "store_compare", "a": "store-before", "b": "store-after",
             "keys": ["settings.theme", "settings.follow_system", "recent_repositories.0"]},
            {"name": "stores-across-builds", "kind": "store_compare", "a": "base:store-after", "b": "cand:store-after"}]
        loaded = scenario.validate(spec(atspi=True, steps=steps, analyses=analyses))
        self.assertTrue(loaded["atspi"])
        self.assertFalse(scenario.validate(spec())["atspi"])
        focus, before, _, after, space = loaded["steps"][-5:]
        self.assertEqual((focus["within"], before["path"], before["quiet"], before["within"]),
                         (5.0, "config/gitturtle/preferences.json", 1.0, 10.0))
        self.assertEqual((after["quiet"], after["within"], space["within"]), (0.5, 5.0, 2.0))
        named = {entry["name"]: entry for entry in loaded["analyses"]}
        self.assertEqual(named["switch-focused"]["states"], ["focused"])
        self.assertEqual(named["space-keeps-it"]["expect"], {"base": False, "cand": True})
        self.assertEqual(named["theme-same"]["keys"], ["settings.theme", "settings.follow_system",
                                                       "recent_repositories.0"])
        self.assertIsNone(named["stores-across-builds"]["roles"])  # both frames name a role: once per variant
        runs = [(entry["name"], role) for entry, role, variant in scenario.analysis_runs(loaded)
                if variant.id == "midnight"]
        self.assertIn(("switch-focused", "base"), runs)
        self.assertIn(("stores-across-builds", None), runs)
        # Every reading step reaches the session as exactly one action.
        from native_qa import session

        for step in loaded["steps"][-5:]:
            sent = play.session_step(step, "cand")
            self.assertEqual(len([key for key in session.STEP_KEYS if key in sent]), 1, sent)
        self.assertEqual(set(scenario.READING_STEPS), set(session.READING_STEPS))

    def test_bad_readings(self) -> None:
        steps = SPEC["steps"] + [{"atspi_focus": "switch"}, {"store_snapshot": "store"}]
        focus = {"name": "f", "kind": "atspi_focus", "focus": "switch", "states": ["focused"]}
        compare = {"name": "s", "kind": "store_compare", "a": "store", "b": "store-2"}
        later = steps + [{"store_snapshot": "store-2"}]
        for changes, match in (
                (dict(steps=steps), r"\$\.steps\[6\]\.atspi_focus: needs \"atspi\": true"),
                (dict(atspi="yes", steps=steps), r"\$\.atspi: expected true or false"),
                (dict(atspi=True, steps=steps + [{"atspi_focus": "switch"}]), "reading 'switch' is taken twice"),
                (dict(atspi=True, steps=steps + [{"atspi_focus": "Switch"}]), r"\.atspi_focus: 'Switch' does not match"),
                (dict(atspi=True, steps=steps + [{"atspi_focus": "x", "within": 0}]), r"\.within: 0 is outside"),
                (dict(atspi=True, steps=steps + [{"atspi_focus": "x", "quiet": 1}]), r"unknown key\(s\) quiet"),
                (dict(atspi=True, steps=steps + [{"store_snapshot": "x", "path": "home/.gitconfig"}]),
                 r"\$\.steps\[8\]\.path: 'home/\.gitconfig' must start with one of config, data, cache, state"),
                (dict(atspi=True, steps=steps + [{"store_snapshot": "x", "path": "config/../../x"}]),
                 r"\.path: .* without empty, \. or \.\. parts"),
                (dict(atspi=True, steps=steps + [{"store_snapshot": "x", "quiet": 5, "within": 2}]),
                 r"\.within: 2\.0 s is shorter than quiet \(5\.0 s\)"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, states=["glowing"])]),
                 r"\$\.analyses\[0\]\.states\[0\]: 'glowing' is not a state a reading reports"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, states=[], not_states=[])]),
                 "needs node, role, states, not_states or same_as to pass or fail"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, not_states=["focused"])]),
                 "both required and refused"),
                (dict(atspi=True, steps=steps + [{"guard": {"kind": "atspi_focus", "focus": "@now", "node": "x"}}]),
                 r"\$\.steps\[8\]\.guard\.focus: @now is a frame; give a reading's label"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, focus="@now")]), r"\.focus: '@now' is not a"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, focus="gone")]),
                 r"\$\.analyses\[0\]: reads reading 'gone', which is never taken in variant midnight"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, focus="store")]),
                 r"reads 'store', which comes from store_snapshot, but atspi_focus reads only atspi_focus readings"),
                (dict(atspi=True, steps=steps, analyses=[dict(focus, focus="focus")]),
                 "reads reading 'focus', which is never taken"),  # a capture is not a reading
                (dict(atspi=True, steps=steps, analyses=[dict(compare, b="store")]), "a and b name the same snapshot"),
                (dict(atspi=True, steps=steps, analyses=[compare]), "reads reading 'store-2', which is never taken"),
                (dict(atspi=True, steps=later, analyses=[dict(compare, keys=["settings..theme"])]),
                 r"\.keys\[0\]: 'settings\.\.theme' does not match"),
                (dict(atspi=True, steps=later, analyses=[dict(compare, keys=[])]), r"\.keys: expected a list of 1"),
                (dict(atspi=True, steps=later, analyses=[dict(compare, a="head:store")]),
                 "'head' is not one of the roles"),
                (dict(atspi=True, steps=SPEC["steps"] + [
                    {"guard": {"kind": "atspi_focus", "focus": "switch", "states": ["focused"]}},
                    {"atspi_focus": "switch"}]),
                 r"\$\.steps\[6\]: reads reading 'switch', which is not taken before it in variant midnight")):
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                scenario.validate(spec(**changes))
        # A guard may read a reading taken before it.
        guarded = scenario.validate(spec(atspi=True, steps=steps + [
            {"guard": {"kind": "atspi_focus", "focus": "switch", "node": "Follow system appearance"},
             "on_fail": [{"key": "Escape"}]}]))
        self.assertEqual(guarded["steps"][-1]["guard"]["focus"], "switch")

    def test_a_probe_step_and_its_analyses(self) -> None:
        loaded = scenario.validate(probe_spec())
        step = loaded["steps"][-1]
        self.assertEqual({key: step[key] for key in ("probe", "send", "mods", "repeat", "region", "quiet", "timeout",
                                                     "stable_within", "keep_pointer")},
                         dict(probe="rows", send="Tab", mods=[], repeat=2, region=LIST, quiet=0.051, timeout=0.5,
                              stable_within=0.2, keep_pointer=False))
        defaults = scenario.validate(spec(steps=[{"probe": "rows", "send": "Tab", "mods": ["Shift_L"]}], analyses=[]))
        self.assertEqual({key: defaults["steps"][0][key] for key in ("region", "quiet", "timeout", "stable_within")},
                         dict(region=[0, 0, 1000, 680], quiet=session.PROBE_QUIET, timeout=session.PROBE_TIMEOUT,
                              stable_within=session.PROBE_STABLE))
        ring, ends = loaded["analyses"][2:]
        self.assertEqual((ring["clip"], ring["colour"], ring["surface"], ring["min_contrast"], ring["expect"]),
                         (tuple(LIST), "detect", None, 3.0, {"base": "record", "cand": True}))
        self.assertEqual((ends["masks"], ends["max_pixels"], ends["roles"]), ([], 0, ["base", "cand"]))
        recorded = scenario.validate(probe_spec(analyses=[dict(PROBE_ANALYSES[0], expect="record",
                                                               surface={"at": [101, 101]}, colour=[117, 224, 187])]))
        self.assertEqual(recorded["analyses"][0]["expect"], {"base": "record", "cand": "record"})
        self.assertEqual(recorded["analyses"][0]["surface"], {"at": (101, 101)})
        # A probe commits nothing, so the crops (and what recheck compares) are those of the spec without it.
        self.assertEqual([c.name for c in scenario.committed(loaded)],
                         [c.name for c in scenario.committed(scenario.validate(spec()))])
        sent = play.session_step(step, "cand")
        self.assertEqual([key for key in session.STEP_KEYS if key in sent], ["probe"])
        self.assertEqual((sent["send"], sent["region"], sent["note"]), ("Tab", LIST, PROBE_STEP["note"]))
        self.assertEqual([s["probe"] for s in scenario.probes(loaded)], ["rows"])

    def test_a_click_probe_and_a_caret_mask(self) -> None:
        click = {"probe": "row", "click_at": {"crop": "list", "at": [40, 196]}, "release_at": [300, 80],
                 "region": "list", "quiet": 0.2, "note": "press a partly visible row; release on the title"}
        ring = dict(PROBE_ANALYSES[0], probe="row", masks=[[120, 150, 122, 170]])
        loaded = scenario.validate(probe_spec(steps=SPEC["steps"] + [click], analyses=[ring]))
        step = loaded["steps"][-1]
        self.assertEqual((step["click_at"], step["release_at"], step["mods"], step["keep_pointer"], step["quiet"]),
                         ([140, 296], [300, 80], [], False, 0.2))
        self.assertNotIn("send", step)
        self.assertEqual(loaded["analyses"][0]["masks"], [(120, 150, 122, 170)])
        sent = play.session_step(step, "base")
        self.assertEqual([key for key in session.STEP_KEYS if key in sent], ["probe"])
        self.assertEqual((sent["click_at"], sent["release_at"]), ([140, 296], [300, 80]))
        plain = scenario.validate(spec(steps=[{"probe": "field", "click_at": [500, 300]}], analyses=[]))
        self.assertNotIn("release_at", plain["steps"][0])

    def test_bad_probe_specs(self) -> None:
        def with_probe(**changes):
            return probe_spec(steps=SPEC["steps"] + [dict(PROBE_STEP, **changes)])

        ring = PROBE_ANALYSES[0]
        cases = [
            (spec(steps=[{"probe": "rows"}], analyses=[]),
             r"\$\.steps\[0\]: a probe sends a key \(\"send\"\) or presses a point \(\"click_at\"\), exactly one"),
            (with_probe(click_at=[5, 5]), r"\$\.steps\[6\]: a probe sends a key .* exactly one of them"),
            (spec(steps=[{"probe": "rows", "send": "Tab", "release_at": [1, 1]}], analyses=[]),
             r"\$\.steps\[0\]: \"release_at\" .* needs \"click_at\""),
            (spec(steps=[{"probe": "row", "click_at": [5, 5], "mods": ["Shift_L"]}], analyses=[]),
             r"\$\.steps\[0\]\.mods: a click probe presses the pointer's first button"),
            (spec(steps=[{"probe": "row", "click_at": [5, 5], "keep_pointer": True}], analyses=[]),
             r"\$\.steps\[0\]\.keep_pointer: a click probe"),
            (spec(steps=[{"probe": "row", "click_at": [1000, 5]}], analyses=[]),
             r"\$\.steps\[0\]\.click_at: \(1000,5\) is outside the 1000x680 window"),
            (spec(steps=[{"probe": "row", "click_at": {"crop": "panel", "at": [100, 5]}}], analyses=[]),
             r"\$\.steps\[0\]\.click_at\.at: \(100,5\) is outside the 100x50 crop 'panel'"),
            (spec(steps=[{"probe": "row", "click_at": {"crop": "dialog", "at": [1, 5]}}], analyses=[]),
             r"\$\.steps\[0\]\.click_at\.crop: no crop box named 'dialog'"),
            (spec(steps=[{"probe": "row", "click_at": {"crop": "panel", "x": 1}}], analyses=[]),
             r"\$\.steps\[0\]\.click_at: unknown key\(s\) x"),
            (probe_spec(analyses=[dict(ring, masks=["status-timing"])]), r"\$\.analyses\[0\]\.masks\[0\]: expected a list"),
            (with_probe(send="Ta b"), r"\$\.steps\[6\]\.send: 'Ta b' does not match"),
            (with_probe(region="dialog"), r"\$\.steps\[6\]\.region: no crop box named 'dialog'"),
            (with_probe(region=[0, 0, 1001, 10]), r"\$\.steps\[6\]\.region: .* reaches outside the 1000x680 window"),
            (with_probe(quiet=1, timeout=1), r"\$\.steps\[6\]\.quiet: 1\.0 s is not shorter than the timeout"),
            (with_probe(repeat=0), r"\$\.steps\[6\]\.repeat: 0 is outside 1\.\.200"),
            (with_probe(stable_within=0.05), r"\$\.steps\[6\]\.stable_within: 0\.05 s is shorter than quiet, 0\.051 s, "
                                             r"so the region could never count as settled before a press"),
            (with_probe(quiet=0.4), r"\$\.steps\[6\]\.stable_within: 0\.2 s is shorter than quiet, 0\.4 s"),
            (with_probe(stable_within=0), r"\$\.steps\[6\]\.stable_within: 0 is outside 0\.05\.\.120"),
            (with_probe(await_change=1), r"\$\.steps\[6\]: unknown key\(s\) await_change"),
            (with_probe(key="Tab"), r"\$\.steps\[6\]: needs exactly one action"),
            (probe_spec(steps=SPEC["steps"] + [PROBE_STEP, PROBE_STEP]),
             r"\$\.steps\[7\]: probe 'rows' runs twice in variant midnight"),
            (probe_spec(analyses=[dict(ring, probe="other")]),
             r"\$\.analyses\[0\]: reads probe 'other', which variant midnight never runs"),
            (with_probe(region="panel"),
             r"\$\.analyses\[2\]\.clip: \[100, 100, 400, 300\] is not inside probe 'rows'"),
            (probe_spec(analyses=[dict(ring, expect={"base": "maybe"})]),
             r"\$\.analyses\[0\]\.expect\.base: expected true, false or \"record\", got 'maybe'"),
            (probe_spec(analyses=[dict(ring, surface={"at": [5, 5]})]),
             r"\$\.analyses\[0\]\.surface\.at: \[5, 5\] is outside the clip"),
            (probe_spec(analyses=[dict(PROBE_ANALYSES[1], masks=["status-timing"])]),
             r"\$\.analyses\[0\]\.masks\[0\]: expected a list"),
            (probe_spec(analyses=[dict(ring, frame="rows")]), r"\$\.analyses\[0\]: unknown key\(s\) frame"),
            (probe_spec(analyses=[{k: v for k, v in ring.items() if k != "clip"}]),
             r"\$\.analyses\[0\]: missing required key\(s\) clip"),
            (probe_spec(analyses=[dict(ring, probe="head:rows")]), "'head' is not one of the roles"),
            (probe_spec(steps=SPEC["steps"][:2] + [{"guard": {"kind": "probe_ring", "probe": "rows", "clip": "list"}}]),
             r"\$\.steps\[2\]\.guard\.kind: a guard reads one frame as the steps run"),
        ]
        for data, match in cases:
            with self.subTest(match=match), self.assertRaisesRegex(scenario.SpecError, match):
                scenario.validate(data)

    def test_cli_check_lists_probes(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "probe.json"
            path.write_text(json.dumps(probe_spec(steps=SPEC["steps"] + [
                dict(PROBE_STEP, mods=["Shift_L"]),
                {"probe": "row", "click_at": [140, 296], "release_at": [300, 80], "region": "list"}])))
            result = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(path)],
                                    capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("6 committed crops", result.stdout)
        self.assertIn("2 probe(s), kept in the bundle and never committed:\n"
                      "    rows  (Shift+Tab x2, region [100, 100, 400, 300])\n"
                      "    row  (click at [140, 296] released at [300, 80] x1, region [100, 100, 400, 300])",
                      result.stdout)

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

    def test_cli_check_names_the_home_files_and_the_accessibility_switch(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "spec.json"
            path.write_text(json.dumps(spec(atspi=True, home={"b.txt": "", "a/c.txt": "x"},
                                            steps=SPEC["steps"] + [{"atspi_focus": "switch"}])))
            result = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(path)],
                                    capture_output=True, text=True)
            over = Path(scratch) / "over.json"
            over.write_text(json.dumps(spec(home={f"spec-{i}": "" for i in range(40)}, variants=[
                {"palette": "midnight", "id": "midnight-more", "home": {f"own-{i}": "" for i in range(30)}}])))
            refused = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(over)],
                                     capture_output=True, text=True)
        self.assertEqual(refused.returncode, 2, refused.stdout)
        self.assertIn("$.variants[0].home: with the spec's own HOME files, 70 files; at most 64", refused.stderr)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("  HOME files seeded before each launch: a/c.txt, b.txt\n", result.stdout)
        self.assertIn("  org.a11y.Status IsEnabled set true for each launch, then restored and read back\n",
                      result.stdout)
        plain = subprocess.run([sys.executable, "-B", str(QA), "scenario", "check", str(EXAMPLE)],
                               capture_output=True, text=True)
        self.assertNotIn("HOME files", plain.stdout)
        self.assertNotIn("IsEnabled", plain.stdout)


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

    def test_reading_analyses_read_the_flow_log_of_each_launch(self) -> None:
        from native_qa.test_analysis import SWITCH, focus_reading, snapshot

        steps = SPEC["steps"] + [{"atspi_focus": "switch"}, {"store_snapshot": "store-a"}, {"key": "space"},
                                 {"store_snapshot": "store-b"}]
        loaded = scenario.validate(spec(atspi=True, steps=steps, analyses=[
            {"name": "switch", "kind": "atspi_focus", "focus": "switch", "node": "Follow system appearance",
             "states": ["focused"], "expect": {"base": False, "cand": True}},
            {"name": "keys-keep-store", "kind": "store_compare", "a": "store-a", "b": "store-b"},
            {"name": "builds-agree", "kind": "store_compare", "a": "base:store-b", "b": "cand:store-b",
             "keys": ["settings.theme"]}]), "f" * 64)
        window = dict(name="GitTurtle", role="frame", states=["focused"], depth=1)
        store = b'{"version": 6, "settings": {"theme": "omarchy"}}'
        for role in ("base", "cand"):
            for variant in loaded["variants"]:
                run_dir = self.root / role / variant.id
                run_dir.mkdir(parents=True)
                readings = [focus_reading("switch", SWITCH if role == "cand" else window),
                            snapshot("store-a", store), snapshot("store-b", store)]
                (run_dir / "flow-log.json").write_text(json.dumps(dict(header={}, readings=readings)))
        report = evidence.run_analyses(loaded, self.root)
        self.assertEqual((report["total"], report["as_expected"], report["unexpected"]), (10, 10, []))
        base = next(r for r in report["results"] if r["name"] == "switch" and r["role"] == "base")
        self.assertEqual(base["result"]["focused"], window)
        self.assertEqual(base["frames"], [])
        # A launch whose flow log lost its readings fails every analysis that reads them.
        (self.root / "cand" / "porcelain" / "flow-log.json").write_text(json.dumps(dict(header={})))
        report = evidence.run_analyses(loaded, self.root)
        self.assertEqual(sorted(report["unexpected"]), [
            "builds-agree (both roles, porcelain): failed, expected pass",
            "keys-keep-store (cand, porcelain): failed, expected pass",
            "switch (cand, porcelain): failed, expected pass"])
        # "record" holds the reading kinds to no verdict, as it does the frame and probe kinds.
        recorded = scenario.validate(spec(atspi=True, steps=steps, analyses=[
            {"name": "switch", "kind": "atspi_focus", "focus": "switch", "node": "Follow system appearance",
             "expect": {"base": "record", "cand": True}},
            {"name": "builds-agree", "kind": "store_compare", "a": "base:store-b", "b": "cand:store-b",
             "expect": "record"}]), "f" * 64)
        self.assertEqual((recorded["analyses"][0]["expect"], recorded["analyses"][1]["expect"]),
                         ({"base": "record", "cand": True}, "record"))
        report = evidence.run_analyses(recorded, self.root)
        self.assertEqual((report["total"], report["recorded"], report["unexpected"]),
                         (6, 4, ["switch (cand, porcelain): failed, expected pass"]))
        self.assertEqual([(r["role"], r["variant"], r["passed"]) for r in report["results"]
                          if r["expected"] == "record" and not r["passed"]],
                         [("base", "midnight", False), ("base", "porcelain", False), (None, "porcelain", False)])

    def probes(self, root: Path, loaded: dict, cut_roles=("base",), roles=None) -> None:
        """Each launch's probe through `Session.probe` on a scripted screen: Tab draws the newly focused row with
        its ring cut by the list's lower edge for one frame (`cut_roles`) or reveals it at once, then Shift+Tab
        moves the ring up inside the list."""
        from types import SimpleNamespace

        def window(ring_top, shift=0):
            return list_frame(ring_top, size=(1000, 680), clip=tuple(LIST), ring_size=(240, 40), left=130,
                              shift=shift)

        revealed = window(250, shift=-20)
        for role in roles or loaded["roles"]:
            first = [(0, window(220)), (9, window(270) if role in cut_roles else revealed), (15, revealed)]
            for variant in loaded["variants"]:
                clock = FakeClock()
                run = SimpleNamespace(dirs=SimpleNamespace(root=root / role / variant.id), log={"probes": []},
                                      driver=ScriptedScreen([first, [(0, revealed), (5, window(200, shift=-20))]],
                                                            clock, size=(1000, 680)),
                                      require_unlocked=lambda why: None, clock=clock, pause=clock.advance)
                run.park = lambda why, run=run: run.driver.park()
                step = play.session_step(loaded["steps"][-1], role)
                with contextlib.redirect_stdout(io.StringIO()):
                    session.Session.probe(run, step["probe"], step["send"], step["mods"], None, step["region"],
                                          step["repeat"], step["quiet"], step["timeout"], step["stable_within"],
                                          step["keep_pointer"])

    def test_probe_analyses_record_the_bases_cut_frames_and_hold_the_candidate(self) -> None:
        loaded = scenario.validate(probe_spec(), "f" * 64)
        self.captures(self.root)
        self.probes(self.root, loaded)
        report = evidence.run_analyses(loaded, self.root)
        self.assertEqual((report["total"], report["as_expected"], report["recorded"], report["unexpected"]),
                         (14, 14, 4, []))
        results = {(r["name"], r["role"], r["variant"]): r for r in report["results"]}
        base = results[("rows-ring", "base", "midnight")]
        self.assertEqual((base["expected"], base["passed"], base["as_expected"]), ("record", False, True))
        [cut] = base["result"]["failing"]
        self.assertEqual((cut["press"], cut["index"], cut["ms"], cut["previous_ms"], cut["last_ms"], cut["grabs"]),
                         (1, 1, 8.0, 6.0, 12.0, 3))
        self.assertEqual((cut["file"], cut["box"]), ("base/midnight/probes/rows/001-01.png", [130, 270, 370, 300]))
        self.assertEqual(cut["reasons"], ["bottom: no ring at 220 of 220 positions"])
        self.assertEqual(base["result"]["resolution_ms"], 3.0)
        ends = results[("rows-endpoints", "base", "porcelain")]["result"]
        self.assertEqual([(f["press"], f["ms"]) for f in ends["failing"]], [(1, 8.0)])
        cand = results[("rows-ring", "cand", "midnight")]
        self.assertEqual((cand["expected"], cand["passed"]), (True, True))
        self.assertEqual(cand["result"]["frames"], 2)  # one frame after each press: the reveal, then the move
        self.assertIn("no frame failed at 3.0 ms resolution", cand["result"]["summary"])
        self.assertTrue(results[("rows-endpoints", "cand", "porcelain")]["passed"])
        # A truncated press is inconclusive: it meets neither a pass nor a fail expectation.
        record_path = self.root / "cand" / "midnight" / "probes" / "rows" / "probe.json"
        original = record_path.read_text()
        truncated = json.loads(original)
        truncated["presses"][0].update(ended="byte-budget", truncated=True, settled=False)
        record_path.write_text(json.dumps(truncated))
        report = evidence.run_analyses(loaded, self.root)
        self.assertEqual(report["unexpected"], ["rows-ring (cand, midnight): inconclusive, expected pass",
                                                "rows-endpoints (cand, midnight): inconclusive, expected pass"])
        record_path.write_text(original)
        # The same cut in the candidate is a finding; only the base's is recorded.
        shutil.rmtree(self.root / "cand")
        self.captures(self.root, roles=("cand",))
        self.probes(self.root, loaded, cut_roles=("base", "cand"), roles=("cand",))
        report = evidence.run_analyses(loaded, self.root)
        self.assertEqual(report["unexpected"], [
            "rows-ring (cand, midnight): failed, expected pass", "rows-ring (cand, porcelain): failed, expected pass",
            "rows-endpoints (cand, midnight): failed, expected pass",
            "rows-endpoints (cand, porcelain): failed, expected pass"])
        shutil.rmtree(self.root / "base" / "midnight" / "probes")
        missing = evidence.run_analyses(loaded, self.root)["results"]
        self.assertEqual([r["result"]["reasons"] for r in missing if r["role"] == "base" and r["variant"] == "midnight"
                          and r["kind"] == "probe_ring"], [["probe rows is missing"]])

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
        self.assertEqual(payload["analyses"], dict(total=6, as_expected=6, recorded=0))
        original = (bundle / "analysis.json").read_text()
        evidence.write_json(bundle / "analysis.json", dict(json.loads(original), recorded=2))
        self.assertTrue(evidence.attestation(bundle, "demo-task", CAND_SHA, "e" * 40)["sessions"][0]["what"].endswith(
            "6 of 6 analyses as the scenario expects (2 recorded without a verdict)"))
        (bundle / "analysis.json").write_text(original)
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
    def launch(self, loaded: dict, variant: scenario.Variant, restore_failure: str | None = None, during=None):
        """`play.launch` through a session that keeps its store and runs nothing; the record and that session.
        `during` is called as each step runs, and may raise as a failing step would."""
        sessions = []

        class FakeSession:
            """Runs the steps and, on close, reports `restore_failure` as a read_only path it could not restore."""

            def __init__(self, binary, fixture, run_dir, store, **kwargs) -> None:
                self.store, self.kwargs = store, kwargs
                self.log = dict(header=dict(started_utc="2026-10-02T10:00:00Z", ended_utc="2026-10-02T10:01:00Z"),
                                captures=[], fixture_unchanged=True)
                self.restore_failures, self.steps = [], []
                sessions.append(self)

            def launch(self) -> None:
                pass

            def run(self, steps) -> None:
                if during is not None:
                    during()
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

    def test_a_guard_reads_the_sessions_readings(self) -> None:
        from types import SimpleNamespace

        from native_qa.test_analysis import focus_reading

        loaded = scenario.validate(spec(atspi=True, analyses=[], steps=[
            {"atspi_focus": "switch"},
            {"guard": {"kind": "atspi_focus", "focus": "switch", "node": "Follow system appearance"},
             "on_fail": [{"key": "Escape"}], "note": "the Switch holds focus"}]))
        step = loaded["steps"][1]
        sent = []
        run = SimpleNamespace(readings={"switch": focus_reading("switch")}, frames={}, run=sent.extend,
                              log={"scenario": {"guards": []}})
        with contextlib.redirect_stdout(io.StringIO()):
            play.guard(run, step)
            self.assertEqual(sent, [])
            run.readings["switch"] = focus_reading("switch", dict(name="GitTurtle", role="frame", states=["focused"]))
            with self.assertRaisesRegex(SystemExit, "inconclusive: guard at step 1 .* is 'GitTurtle', not"):
                play.guard(run, step)
        self.assertEqual([key["key"] for key in sent], ["Escape"])
        self.assertEqual([g["passed"] for g in run.log["scenario"]["guards"]], [True, False])

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

    def test_a_launch_seeds_its_variants_home_files_and_records_their_digests(self) -> None:
        loaded = scenario.validate(spec(home={"a/b.txt": "spec", "c.txt": "spec"}, variants=[
            {"palette": "midnight"}, {"palette": "midnight", "id": "midnight-own", "home": {"c.txt": "own"}}],
            steps=[{"wait": 0}], analyses=[]), "f" * 64)
        plain, own = loaded["variants"]
        record, session = self.launch(loaded, own)
        self.assertEqual(session.kwargs["home_files"], {"a/b.txt": b"spec", "c.txt": b"own"})
        sha = evidence.identity.sha256_bytes
        self.assertEqual(record["home_sha256"], {"a/b.txt": sha(b"spec"), "c.txt": sha(b"own")})
        self.assertEqual(session.log["scenario"]["variant"]["home"], ["c.txt"])
        record, session = self.launch(scenario.validate(spec(steps=[{"wait": 0}], analyses=[]), "f" * 64), plain)
        self.assertEqual((record["home_sha256"], session.kwargs["home_files"]), ({}, {}))

    def test_isenabled_is_set_for_each_launch_and_restored_however_it_ends(self) -> None:
        from native_qa import a11y
        from native_qa.test_a11y import TYPES, FakeProxy

        proxy = FakeProxy(enabled=False)
        seen = []
        loaded = scenario.validate(spec(atspi=True, steps=[{"wait": 0}], analyses=[]), "f" * 64)
        variant = loaded["variants"][0]
        self.addCleanup(signal.signal, signal.SIGTERM, signal.getsignal(signal.SIGTERM))
        with mock.patch.object(a11y.Status, "connect", lambda: a11y.Status(proxy, TYPES)), \
                contextlib.redirect_stderr(io.StringIO()):
            record, _ = self.launch(loaded, variant, during=lambda: seen.append(proxy.enabled))
            self.assertEqual((seen, proxy.enabled, record["error"]), ([True], False, None))
            self.assertEqual(record["atspi"], dict(original=False, set=True, restored=False))

            def crash():
                raise RuntimeError("the app went away")

            record, _ = self.launch(loaded, variant, during=crash)
            self.assertEqual((record["error"], record["refusal"], proxy.enabled), ("RuntimeError('the app went away')",
                                                                                   False, False))
            self.assertEqual(record["atspi"]["restored"], False)

            def locked_down():
                proxy.ignore_set = True  # the restore's Set will not take

            record, _ = self.launch(loaded, variant, during=locked_down)
            self.assertTrue(record["error"].startswith("org.a11y.Status IsEnabled read back True after restoring False; restore it by hand"))
            self.assertTrue(record["refusal"])  # a host problem: the run is inconclusive, never a finding
            proxy.ignore_set, proxy.enabled = False, False

            def terminated():
                os.kill(os.getpid(), signal.SIGTERM)
                time.sleep(0.01)  # the tool's handler only marks the launch; the step finishes

            # A SIGTERM during the first of two steps stops the launch before the second, after the restore.
            two = scenario.validate(spec(atspi=True, steps=[{"wait": 0}, {"wait": 0}], analyses=[]), "f" * 64)
            record, stopped = self.launch(two, variant, during=terminated)
            self.assertEqual((record["error"], record["refusal"], len(stopped.steps), proxy.enabled),
                             ("stopped: SIGTERM received", True, 1, False))
            self.assertEqual((record["atspi"]["sigterm"], record["atspi"]["restored"]), (True, False))
            # One during the last step still marks the launch stopped once IsEnabled is back.
            record, stopped = self.launch(loaded, variant, during=terminated)
            self.assertEqual((record["error"], record["refusal"], len(stopped.steps), proxy.enabled),
                             ("stopped: SIGTERM received", True, 1, False))
            proxy.fail_get = True
            record, _ = self.launch(loaded, variant, during=lambda: self.fail("never launched"))
            self.assertIn("refusing: org.a11y.Status IsEnabled cannot be read", record["error"])
            self.assertTrue(record["refusal"])
        # A spec without "atspi" never touches the bus.
        proxy.calls.clear()
        self.launch(scenario.validate(spec(steps=[{"wait": 0}], analyses=[]), "f" * 64), variant)
        self.assertEqual(proxy.calls, [])


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

    def test_missing_accessibility_modules_refuse_before_isenabled_or_a_launch(self) -> None:
        from native_qa import a11y, portal

        self.spec_path.write_text(json.dumps(spec(atspi=True, steps=SPEC["steps"] + [{"atspi_focus": "switch"}])))
        (self.root / "committed").mkdir()
        launched, connect = [], mock.Mock()
        with self.patched(), mock.patch.object(play, "launch", lambda *args: launched.append(args)), \
                mock.patch.object(portal, "_atspi", side_effect=ImportError("No module named 'gi'")), \
                mock.patch.object(a11y.Status, "connect", connect):
            with self.assertRaisesRegex(SystemExit, r"refusing: \"atspi\": true needs gi with Atspi 2\.0 .*No module "
                                                    r"named 'gi'"):
                play.run(self.spec_path, {"base": Path("/x/base"), "cand": Path("/x/cand")}, self.root / "out",
                         self.fixture, templates=self.templates)
            with self.assertRaisesRegex(SystemExit, "needs gi with Atspi 2.0"):
                play.recheck(self.spec_path, Path("/x/cand"), self.root / "committed", self.root / "re", self.fixture)
        self.assertEqual((launched, connect.call_count), ([], 0))
        self.assertFalse((self.root / "out").exists() or (self.root / "re").exists())
        # As a command, that refusal exits 2: a tool this host lacks never grades the build.
        from native_qa import qa

        with self.patched(), mock.patch.object(portal, "_atspi", side_effect=ImportError("No module named 'gi'")), \
                contextlib.redirect_stderr(io.StringIO()) as printed:
            code = qa.main(["recheck", str(self.spec_path), "--exe", "/x/cand", "--committed",
                            str(self.root / "committed"), "--out", str(self.root / "re")])
        self.assertEqual(code, 2)
        self.assertIn("needs gi with Atspi 2.0", printed.getvalue())

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
