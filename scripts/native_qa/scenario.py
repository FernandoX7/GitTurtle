"""Declarative native-QA scenarios: the versioned JSON spec, its strict validation and the committed names.

A scenario names its fixture (a deterministic recipe that `recipe.py` builds)
and, for a route that writes, the changes each launch makes to its own copy of
it (`writes.py`), the window size, the variants (palette x interface text size,
each optionally with its own store settings and HOME files), one ordered list
of steps that every build role runs identically, named crop boxes, the
captures to commit, the probes (every frame drawn after a key or a click,
analysed but never committed) and the analyses to run on them and on the
steps' readings (the focused AT-SPI node, a settled store). `qa.py scenario run`
drives it (`play.py`), and `evidence.py` turns its captures into the committed
crops, manifest, re-check and attestation.

Validation is strict and happens before anything is built or launched: an
unknown key, a wrong type, a box outside the window, a reference to an
undefined crop, capture, mark or probe, a probe analysis whose clip lies
outside its probe's region, a filter that matches no variant, or two crops
with one committed name is a `SpecError` naming its JSON path.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import re
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

from . import a11y, analysis, frames, runenv, session, stores

VERSION = 1
ROLES = ("base", "cand")
COMMITTED_PREFIX = {"base": "base", "cand": "candidate"}
DEFAULT_WINDOW = (1000, 680)
# INTERFACE_TEXT_RANGE in crates/app/src/appearance.rs; test_scenario checks that they agree.
TEXT_SIZES = range(11, 19)
IDENTIFIER = re.compile(r"[a-z0-9][a-z0-9.-]{0,79}")
PALETTE = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}")
# A variant id keeps a theme key's underscores (`solarized_dark-18pt`), as its default id and committed names do.
VARIANT_ID = re.compile(r"[a-z0-9][a-z0-9_.-]{0,79}")
SIZE_SEGMENT = re.compile(r"[0-9]+pt(-|$)")  # an interface size at the start of a settings variant's own part
KEYSYM = re.compile(r"[A-Za-z0-9_]{1,40}")
NOW = "@now"
CAPTURE_STABLE = 8.0  # seconds a capture waits, after parking, for two identical grabs
CAPTURE_QUIET = 1.15  # seconds between those grabs: longer than a caret blink's half period
RECORD = "record"     # an expectation that measures and writes an analysis without holding it to a verdict

TOP_LEVEL = ({"version", "task", "variants", "steps"},
             {"summary", "limitations", "window", "roles", "fixture", "settings", "env", "crops", "analyses",
              "home", "atspi", "writes"})
# Each step has exactly one action key; these are the options each action takes beside `note` and `when`.
# No option shares an action's name, so a normalised step still names exactly one action.
STEPS = {
    "key": {"mods", "repeat", "wait_after", "await_change"},
    "type": set(),
    "palette": set(),
    "move": set(),
    "glide": {"settle"},
    "click": set(),
    "press": set(),
    "release": set(),
    "wheel": {"steps"},
    "park": set(),
    "wait": set(),
    "stable": {"quiet"},
    "resize": set(),
    "read_only": set(),
    # Readings: the app's state under a label in flow-log.json, for the reading analyses; they send no input.
    "atspi_focus": {"within"},
    "store_snapshot": {"path", "quiet", "within"},
    "mark": {"park_first", "stable_within", "quiet"},
    "guard": {"on_fail"},
    "capture": {"shows", "crop", "roles", "commit", "keep_pointer", "settle", "stable_within", "quiet"},
    "probe": {"send", "mods", "click_at", "release_at", "repeat", "region", "quiet", "timeout", "stable_within",
              "keep_pointer"},
}
WHEN = {"palette", "text_size", "variant"}
ANALYSIS_COMMON = {"name", "kind", "when", "roles", "expect", "note"}
ANALYSES = {
    "ring": ({"frame", "rect"}, {"colour", "sides", "reach", "corner", "tolerance", "min_width", "uniform_width",
                                 "min_contrast", "max_outlines"}),
    "clearance": ({"frame", "rect", "side", "surface"}, {"ring", "at", "reach", "tolerance", "limit", "min_px",
                                                         "max_px"}),
    "fill": ({"frame", "region", "reference"}, {"min_contrast", "max_contrast"}),
    "compare": ({"a", "b"}, {"region", "crop", "masks", "max_pixels", "min_pixels", "bands", "band_min"}),
    "atspi_focus": ({"focus"}, {"node", "role", "states", "not_states", "same_as"}),
    "store_compare": ({"a", "b"}, {"keys"}),
    "probe_ring": ({"probe", "clip"}, {"colour", "surface", "tolerance", "corner", "min_width", "uniform_width",
                                       "min_contrast", "max_outlines", "masks"}),
    "probe_endpoints": ({"probe", "clip"}, {"masks", "max_pixels"}),
}
READING_STEPS = tuple(analysis.READING_KINDS.values())
JSON_KEY = re.compile(r"[A-Za-z0-9_-]{1,64}(\.[A-Za-z0-9_-]{1,64}){0,15}")
# Recipe operations: the action key and the options it takes beside `date`.
OPERATIONS = {
    "commit": {"files", "allow_empty"},
    "branch": {"at"},
    "tag": {"at", "message"},
    "checkout": {"create", "at", "detach"},
    "merge": {"message"},
    "reset": {"mode"},
    "remote": {"bare"},
    "push": {"refs", "set_upstream"},
    "worktree": {"new_branch", "at"},
}
REF_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._/-]{0,99}")
REMOTE = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,39}")
REVISION = re.compile(r"@[0-9]{1,4}|[A-Za-z0-9][A-Za-z0-9._/@{}~^-]{0,99}")
OBJECT_ID = re.compile(r"[0-9a-f]{40}|[0-9a-f]{64}")
REFSPEC = re.compile(r"\+?[A-Za-z0-9][A-Za-z0-9._/-]{0,99}(:[A-Za-z0-9][A-Za-z0-9._/-]{0,99})?")


class SpecError(ValueError):
    """A scenario spec that cannot run; the message starts with the JSON path at fault."""


@dataclass(frozen=True)
class Variant:
    id: str
    palette: str
    text_size: int | None
    settings: dict = field(default_factory=dict, hash=False)  # store settings over the spec's, for this variant
    home: dict = field(default_factory=dict, hash=False)  # HOME files (path: bytes) over the spec's, for this variant

    @property
    def label(self) -> str:
        """The variant's part of a committed name: `midnight`, or `midnight-13pt` with a text size; a variant with
        its own settings or HOME files uses its id, which extends that with what they change
        (`midnight-13pt-code-18pt`)."""
        if self.settings or self.home:
            return self.id
        return palette_label(self.palette, self.text_size)

    def describe(self) -> dict:
        described = dict(id=self.id, palette=self.palette, text_size=self.text_size)
        if self.settings:
            described["settings"] = self.settings
        if self.home:
            described["home"] = sorted(self.home)
        return described


@dataclass(frozen=True)
class Crop:
    """One committed crop: which launch's capture it is cut from, with what box, under which name."""
    name: str
    role: str
    variant: Variant
    capture: str
    crop: str | None
    box: tuple[int, int, int, int] | None
    shows: str


# ---------- primitives ----------
def fail(path: str, message: str):
    raise SpecError(f"{path}: {message}")


def obj(value, path: str, required=(), optional=()) -> dict:
    if not isinstance(value, dict):
        fail(path, f"expected an object, got {type(value).__name__}")
    unknown = sorted(set(value) - set(required) - set(optional))
    if unknown:
        fail(path, f"unknown key(s) {', '.join(unknown)}; allowed: {', '.join(sorted({*required, *optional}))}")
    missing = sorted(set(required) - set(value))
    if missing:
        fail(path, f"missing required key(s) {', '.join(missing)}")
    return value


def mapping(value, path: str) -> dict:
    """An object whose keys are names the spec chooses (crops, settings, variables)."""
    if not isinstance(value, dict):
        fail(path, f"expected an object, got {type(value).__name__}")
    return value


def integer(value, path: str, minimum: int | None = None, maximum: int | None = None) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        fail(path, f"expected an integer, got {value!r}")
    if (minimum is not None and value < minimum) or (maximum is not None and value > maximum):
        fail(path, f"{value} is outside {minimum}..{maximum}")
    return value


def number(value, path: str, minimum: float = 0.0, maximum: float = 600.0) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        fail(path, f"expected a number, got {value!r}")
    if not minimum <= value <= maximum:
        fail(path, f"{value} is outside {minimum}..{maximum}")
    return float(value)


def boolean(value, path: str) -> bool:
    if not isinstance(value, bool):
        fail(path, f"expected true or false, got {value!r}")
    return value


def text(value, path: str, pattern: re.Pattern | None = None, limit: int = 2000) -> str:
    if not isinstance(value, str) or not value or len(value) > limit or "\0" in value:
        fail(path, f"expected a non-empty string of at most {limit} characters, got {value!r}")
    if pattern is not None and not pattern.fullmatch(value):
        fail(path, f"{value!r} does not match {pattern.pattern}")
    return value


def array(value, path: str, minimum: int = 1, maximum: int = 10_000) -> list:
    if not isinstance(value, list) or not minimum <= len(value) <= maximum:
        fail(path, f"expected a list of {minimum} to {maximum} items, got {value!r}")
    return value


def colour(value, path: str) -> tuple[int, int, int]:
    array(value, path, 3, 3)
    return tuple(integer(channel, f"{path}[{index}]", 0, 255) for index, channel in enumerate(value))


def point(value, path: str, window) -> tuple[int, int]:
    array(value, path, 2, 2)
    x, y = (integer(v, f"{path}[{i}]", 0) for i, v in enumerate(value))
    if x >= window[0] or y >= window[1]:
        fail(path, f"({x},{y}) is outside the {window[0]}x{window[1]} window")
    return x, y


def box(value, path: str, window) -> tuple[int, int, int, int]:
    """`[x0, y0, x1, y1]` with exclusive ends, non-empty and inside the window."""
    array(value, path, 4, 4)
    x0, y0, x1, y1 = (integer(v, f"{path}[{i}]", 0) for i, v in enumerate(value))
    if x0 >= x1 or y0 >= y1:
        fail(path, f"{value} is empty: a box is [x0, y0, x1, y1] with x0 < x1 and y0 < y1")
    if x1 > window[0] or y1 > window[1]:
        fail(path, f"{value} reaches outside the {window[0]}x{window[1]} window")
    return x0, y0, x1, y1


def area(value, path: str, spec: dict) -> tuple[int, int, int, int]:
    """A box, or the name of one of the spec's crop boxes."""
    if isinstance(value, str):
        if value not in spec["crops"]:
            fail(path, f"no crop box named {value!r}; defined: {', '.join(spec['crops']) or 'none'}")
        return spec["crops"][value]
    return box(value, path, spec["window"])


def spot(value, path: str, spec: dict) -> list[int]:
    """A window point `[x, y]`, or `{"crop": name, "at": [x, y]}` relative to one of the spec's crop boxes."""
    if not isinstance(value, dict):
        return list(point(value, path, spec["window"]))
    obj(value, path, {"crop", "at"})
    x0, y0, x1, y1 = area(text(value["crop"], f"{path}.crop", IDENTIFIER), f"{path}.crop", spec)
    array(value["at"], f"{path}.at", 2, 2)
    x, y = (integer(v, f"{path}.at[{i}]", 0) for i, v in enumerate(value["at"]))
    if x0 + x >= x1 or y0 + y >= y1:
        fail(f"{path}.at", f"({x},{y}) is outside the {x1 - x0}x{y1 - y0} crop {value['crop']!r}")
    return [x0 + x, y0 + y]


def inside(inner, outer) -> bool:
    return outer[0] <= inner[0] and outer[1] <= inner[1] and inner[2] <= outer[2] and inner[3] <= outer[3]


def expectation(value, path: str):
    """true, false or "record" (measured and written, never a finding)."""
    if value == RECORD:
        return RECORD
    if not isinstance(value, bool):
        fail(path, f"expected true, false or \"{RECORD}\", got {value!r}")
    return value


def git_date(value, path: str) -> tuple[int, str]:
    """An ISO 8601 time with an explicit offset as (unix seconds, +HHMM); a fixture never uses "now"."""
    try:
        moment = datetime.fromisoformat(text(value, path, limit=40))
    except ValueError:
        fail(path, f"{value!r} is not an ISO 8601 time such as 2026-09-01T09:21:00Z")
    if moment.tzinfo is None:
        fail(path, f"{value!r} has no offset; add Z or +HH:MM so the date cannot depend on the host")
    offset = int(moment.utcoffset().total_seconds()) // 60
    sign = "-" if offset < 0 else "+"
    return int(moment.timestamp()), f"{sign}{abs(offset) // 60:02d}{abs(offset) % 60:02d}"


def relative_path(value, path: str) -> str:
    """A POSIX path inside the fixture: relative, no `.`/`..` parts and no `.git` part (in any case)."""
    text(value, path, limit=300)
    parts = value.split("/")
    if (value.startswith("/") or "\\" in value or any(part in ("", ".", "..") for part in parts)
            or any(part.lower() == ".git" for part in parts)):
        fail(path, f"{value!r} must be a relative POSIX path without empty, . or .. parts, outside .git")
    return value


# ---------- the fixture recipe ----------
def fixture_recipe(value, path: str) -> dict:
    obj(value, path, {"name", "operations"}, {"description", "repository", "branch", "clock", "expect"})
    recipe = dict(name=text(value["name"], f"{path}.name", IDENTIFIER),
                  description=text(value["description"], f"{path}.description") if "description" in value else "",
                  repository=text(value.get("repository", "repo"), f"{path}.repository", IDENTIFIER),
                  branch=text(value.get("branch", "main"), f"{path}.branch", REF_NAME))
    clock = obj(value.get("clock", {}), f"{path}.clock", (), {"start", "step"})
    start, offset = git_date(clock.get("start", "2026-01-01T00:00:00Z"), f"{path}.clock.start")
    step = integer(clock.get("step", 60), f"{path}.clock.step", 1, 86_400)
    # The build's own files (recipe.MANIFEST, its partial and writes.MARKER) are never a recipe path.
    places = {recipe["repository"], "fixture-manifest.json", ".fixture-manifest.json.partial",
              ".gitturtle-fixture-copy.json"}
    operations, commits = [], 0
    remotes: set[str] = set()
    for index, op in enumerate(array(value["operations"], f"{path}.operations", 1, 2000)):
        where = f"{path}.operations[{index}]"
        actions = [key for key in OPERATIONS if isinstance(op, dict) and key in op]
        if len(actions) != 1:
            fail(where, f"needs exactly one of {', '.join(OPERATIONS)}")
        action = actions[0]
        obj(op, where, {action}, OPERATIONS[action] | {"date"})
        moment = git_date(op["date"], f"{where}.date") if "date" in op else (start + index * step, offset)

        def revision(key, default="HEAD"):
            rev = text(op.get(key, default), f"{where}.{key}", REVISION, 100)
            if rev.startswith("@") and int(rev[1:]) >= commits:
                fail(f"{where}.{key}", f"{rev} names commit {rev[1:]}, but only {commits} precede this operation")
            return rev

        entry = dict(action=action, date=f"{moment[0]} {moment[1]}")
        if action == "commit":
            files = op.get("files", {})
            if not isinstance(files, dict):
                fail(f"{where}.files", "expected an object of path: content")
            contents = {}
            for name, content in files.items():
                relative_path(name, f"{where}.files")
                if content is None or isinstance(content, str):
                    contents[name] = content
                else:
                    encoded = obj(content, f"{where}.files[{name!r}]", {"base64"})["base64"]
                    contents[name] = {"base64": text(encoded, f"{where}.files[{name!r}].base64", limit=8_000_000)}
            entry.update(message=text(op["commit"], f"{where}.commit"), files=contents,
                         allow_empty=boolean(op.get("allow_empty", False), f"{where}.allow_empty"))
            if not contents and not entry["allow_empty"]:
                fail(where, "a commit without files needs \"allow_empty\": true")
            commits += 1
        elif action in ("branch", "tag"):
            entry.update(name=text(op[action], f"{where}.{action}", REF_NAME), at=revision("at"))
            if action == "tag" and "message" in op:
                entry["message"] = text(op["message"], f"{where}.message")
        elif action == "checkout":
            create = boolean(op.get("create", False), f"{where}.create")
            detach = boolean(op.get("detach", False), f"{where}.detach")
            if create and detach:
                fail(where, "create and detach exclude each other")
            if "at" in op and not create:
                fail(f"{where}.at", "only a checkout that creates a branch takes a start point")
            entry.update(target=text(op["checkout"], f"{where}.checkout", REF_NAME) if create else revision("checkout"),
                         create=create, detach=detach, at=revision("at") if "at" in op else None)
        elif action == "merge":
            if "message" not in op:
                fail(where, "a merge needs a message")
            entry.update(target=revision("merge"), message=text(op["message"], f"{where}.message"))
            commits += 1
        elif action == "reset":
            mode = op.get("mode", "hard")
            if mode not in ("soft", "mixed", "hard"):
                fail(f"{where}.mode", f"expected soft, mixed or hard, got {mode!r}")
            entry.update(target=revision("reset"), mode=mode)
        elif action == "remote":
            if "bare" not in op:
                fail(where, "a remote needs \"bare\": the path of its local bare repository in the fixture")
            entry.update(name=text(op["remote"], f"{where}.remote", REMOTE),
                         bare=relative_path(op["bare"], f"{where}.bare"))
            if entry["name"] in remotes:
                fail(f"{where}.remote", f"remote {entry['name']!r} is already defined")
            remotes.add(entry["name"])
        elif action == "push":
            refs = array(op.get("refs"), f"{where}.refs", 1, 100)
            if op["push"] not in remotes:
                fail(f"{where}.push", f"{op['push']!r} is not a remote this recipe defined earlier; a push only "
                                      "reaches a local bare repository inside the fixture")
            entry.update(remote=op["push"],
                         refs=[text(ref, f"{where}.refs[{i}]", REFSPEC) for i, ref in enumerate(refs)])
            # Only when set, so the normalised entry, and with it the digest of every earlier recipe, stays as it was.
            if boolean(op.get("set_upstream", False), f"{where}.set_upstream"):
                entry["set_upstream"] = True
        elif action == "worktree":
            entry.update(path=relative_path(op["worktree"], f"{where}.worktree"), at=revision("at"),
                         branch=text(op["new_branch"], f"{where}.new_branch", REF_NAME) if "new_branch" in op else None)
        for key in ("bare", "path"):
            if key in entry:
                place = entry[key]
                clash = next((used for used in places if place == used or place.startswith(used + "/")
                              or used.startswith(place + "/")), None)
                if clash is not None:
                    fail(where, f"{place!r} overlaps {clash!r}, already used in the fixture")
                places.add(place)
        operations.append(entry)
    recipe.update(clock=dict(start=start, offset=offset, step=step), operations=operations, commits=commits)
    expect = mapping(value.get("expect", {}), f"{path}.expect")
    recipe["expect"] = {}
    for rev, oid in expect.items():
        text(rev, f"{path}.expect", REVISION, 100)
        if rev.startswith("@") and int(rev[1:]) >= commits:
            fail(f"{path}.expect", f"{rev} names a commit the recipe never makes")
        recipe["expect"][rev] = text(oid, f"{path}.expect[{rev!r}]", OBJECT_ID)
    return recipe


# ---------- the writes a route makes to its own copy of the fixture ----------
FULL_REF = re.compile(r"refs/[A-Za-z0-9][A-Za-z0-9._/-]{0,99}")
# What a declared ref holds after the launch: a recipe commit, an object ID, or the repository's ref of that name.
WRITE_TARGET = re.compile(rf"@[0-9]{{1,4}}|{OBJECT_ID.pattern}|{FULL_REF.pattern}")
# A configuration key: section, an optional subsection and a variable, as `git config` names them.
CONFIG_KEY = re.compile(r"[A-Za-z][A-Za-z0-9-]{0,63}(\.[^\s\0]{1,200})?\.[A-Za-z][A-Za-z0-9-]{0,63}")
DELETED = "deleted"


def fixture_writes(value, path: str, spec: dict) -> dict:
    """`writes`: the changes every launch makes to its own copy of the recipe build, as {role: changes}.

    One declaration for every role, or `{"base": ..., "cand": ...}` naming each
    of the scenario's roles. `writes.py` copies the build for each launch and
    compares the copy before and after it with these changes.
    """
    if spec["fixture"] is None:
        fail(path, "a scenario that writes needs a fixture recipe: every launch writes to its own copy of its build")
    if isinstance(value, dict) and value and set(value) <= set(ROLES):
        if set(value) != set(spec["roles"]):
            fail(path, f"a declaration per role names exactly the scenario's roles: {', '.join(spec['roles'])}")
        return {role: write_changes(value[role], f"{path}.{role}", spec["fixture"]) for role in spec["roles"]}
    changes = write_changes(value, path, spec["fixture"])
    return {role: changes for role in spec["roles"]}


def config_key(name: str) -> str:
    """Git's form of a configuration key: section and variable in lower case, a subsection as written."""
    section, _, rest = name.partition(".")
    middle, _, variable = rest.rpartition(".")
    return ".".join(part for part in (section.lower(), middle, variable.lower()) if part)


def write_changes(value, path: str, recipe: dict) -> dict:
    """One role's declared changes to the repository (`refs`, `config`) and, by recipe remote, to its bare copy
    (`remotes`: `{"origin": {"refs": ..., "config": ...}}`).

    `refs` maps a full ref name to "deleted" or to what it holds after the
    launch: `@N`, an object ID, or a ref name, which means that ref of the
    repository (never of the remote) after the launch. `config` maps a key of
    the repository's (or the remote's) own configuration file to "deleted", to
    its one value after the launch, or to the list of its values. Anything
    undeclared stays as it was.
    """
    obj(value, path, (), {"refs", "config", "remotes"})
    defined = {op["name"] for op in recipe["operations"] if op["action"] == "remote"}

    def ref_changes(items, where: str) -> dict:
        result = {}
        for name, target in mapping(items, where).items():
            text(name, where, FULL_REF, 120)
            at = f"{where}[{name!r}]"
            if target != DELETED:
                text(target, at, WRITE_TARGET, 120)
                if target.startswith("@") and int(target[1:]) >= recipe["commits"]:
                    fail(at, f"{target} names a commit the recipe never makes")
            result[name] = target
        return result

    def config_changes(items, where: str) -> dict:
        result = {}
        for name, item in mapping(items, where).items():
            key = config_key(text(name, where, CONFIG_KEY, 300))
            at = f"{where}[{name!r}]"
            if key in result:
                fail(at, f"{key} is declared twice")
            if item == DELETED:
                result[key] = DELETED
            elif isinstance(item, list):  # every value, in order; ["deleted"] is the literal value
                result[key] = [text(v, f"{at}[{i}]", limit=500) for i, v in enumerate(array(item, at, 1, 16))]
            else:
                result[key] = [text(item, at, limit=500)]
        return result

    declared = dict(refs=ref_changes(value.get("refs", {}), f"{path}.refs"),
                    config=config_changes(value.get("config", {}), f"{path}.config"), remotes={})
    for remote, items in mapping(value.get("remotes", {}), f"{path}.remotes").items():
        where = f"{path}.remotes.{remote}"
        if remote not in defined:
            fail(f"{path}.remotes", f"{remote!r} is not a remote the recipe defines; only a recipe remote's local bare "
                                    "repository is copied with the fixture")
        obj(items, where, (), {"refs", "config"})
        declared["remotes"][remote] = dict(refs=ref_changes(items.get("refs", {}), f"{where}.refs"),
                                           config=config_changes(items.get("config", {}), f"{where}.config"))
    sections = [(f"{path}.refs", declared["refs"], True),
                *((f"{path}.remotes.{name}.refs", items["refs"], False)
                  for name, items in declared["remotes"].items())]
    for where, items, local in sections:
        for name, target in items.items():
            if not target.startswith("refs/"):
                continue
            if declared["refs"].get(target) == DELETED:
                fail(f"{where}[{name!r}]", f"{target} is declared deleted, so nothing can equal it after the launch")
            if local and target == name:
                fail(f"{where}[{name!r}]", "a ref declared equal to itself declares no change")
    return declared


# ---------- variants, filters and references ----------
def palette_label(palette: str, text_size: int | None) -> str:
    """`midnight` or `midnight-13pt`: a plain variant's default id and its part of a committed name."""
    return palette if text_size is None else f"{palette}-{text_size}pt"


def store_settings_entry(value, path: str) -> dict:
    """Extra preference-store settings, the spec's or a variant's; neither sets what the variants or the run decide."""
    settings = mapping(value, path)
    if "theme" in settings or "interface_text_size" in settings:
        fail(path, "the palette and text size come from the variants")
    if "follow_system" in settings:
        fail(f"{path}.follow_system", "the generated store turns Follow system off so frames do not depend on the "
                                      "host's appearance")
    return dict(settings)


def home_entry(value, path: str) -> dict[str, bytes]:
    """Files seeded under each launch's HOME, the spec's or a variant's: a relative path to its text, or to
    `{"base64": ...}`; never the run's Git identity, a path leaving HOME or a file another needs as a directory."""
    files = mapping(value, path)
    if not files:
        fail(path, "no files; leave \"home\" out instead")
    if len(files) > runenv.HOME_FILES:
        fail(path, f"{len(files)} files; at most {runenv.HOME_FILES}")
    seeded = {}
    for name, content in files.items():
        problem = runenv.home_file_problem(name)
        if problem is not None:
            fail(path, problem)
        where = f"{path}[{name!r}]"
        if isinstance(content, str):  # an empty file, such as Omarchy's light.mode marker, is allowed
            try:
                data = content.encode()
            except UnicodeEncodeError:
                fail(where, "the text is not valid UTF-8; give {\"base64\": ...} instead")
        else:
            encoded = obj(content, where, {"base64"})["base64"]
            if not isinstance(encoded, str):
                fail(f"{where}.base64", f"expected a base64 string, got {encoded!r}")
            try:
                data = base64.b64decode(encoded, validate=True)
            except binascii.Error as error:
                fail(f"{where}.base64", f"not base64 ({error})")
        if len(data) > runenv.HOME_FILE_BYTES:
            fail(where, f"{len(data)} bytes; at most {runenv.HOME_FILE_BYTES}")
        seeded[name] = data
    problem = runenv.home_overlap(seeded)
    if problem is not None:
        fail(path, problem)
    return seeded


def variant_list(value, path: str) -> list[Variant]:
    if isinstance(value, dict):
        obj(value, path, {"palettes"}, {"text_sizes"})
        palettes = [text(p, f"{path}.palettes[{i}]", PALETTE) for i, p in enumerate(array(value["palettes"],
                                                                                            f"{path}.palettes", 1, 32))]
        sizes = [None] if "text_sizes" not in value else [
            integer(s, f"{path}.text_sizes[{i}]", TEXT_SIZES.start, TEXT_SIZES.stop - 1)
            for i, s in enumerate(array(value["text_sizes"], f"{path}.text_sizes", 1, 8))]
        items = [dict(palette=p, text_size=s) for p in palettes for s in sizes]
    else:
        items = array(value, path, 1, 64)
    variants = []
    for index, item in enumerate(items):
        where = f"{path}[{index}]"
        obj(item, where, {"palette"}, {"text_size", "id", "settings", "home"})
        size = item.get("text_size")
        if size is not None:
            integer(size, f"{where}.text_size", TEXT_SIZES.start, TEXT_SIZES.stop - 1)
        palette = text(item["palette"], f"{where}.palette", PALETTE)
        default = palette_label(palette, size)
        settings = {}
        if "settings" in item:
            settings = store_settings_entry(item["settings"], f"{where}.settings")
            if not settings:
                fail(f"{where}.settings", "no settings; leave \"settings\" out instead")
        home = home_entry(item["home"], f"{where}.home") if "home" in item else {}
        if settings or home:
            what = "settings" if settings else "HOME files"
            ident = item.get("id")
            own = ident[len(default) + 1:] if isinstance(ident, str) and ident.startswith(f"{default}-") else ""
            if not own:
                fail(f"{where}.id", f"a variant with {what} needs an \"id\" that extends '{default}-' with what "
                                    f"they change, such as '{default}-code-18pt'; its committed names carry that id "
                                    f"in place of '{default}'")
            if SIZE_SEGMENT.match(own):
                fail(f"{where}.id", f"{ident!r} names an interface size after '{default}-' that only \"text_size\" "
                                    f"sets; extend '{default}-' with what the {what} change instead")
        variants.append(Variant(text(item.get("id", default), f"{where}.id", VARIANT_ID), palette, size, settings,
                                home))
    ids = [variant.id for variant in variants]
    duplicate = sorted({i for i in ids if ids.count(i) > 1})
    if duplicate:
        fail(path, f"variant id(s) {', '.join(duplicate)} repeat; give each variant a distinct \"id\"")
    return variants


def when_filter(value, path: str, variants: list[Variant]) -> dict | None:
    if value is None:
        return None
    obj(value, path, (), WHEN)
    if not value:
        fail(path, "an empty filter; leave \"when\" out instead")
    result = {}
    for key, items in value.items():
        array(items, f"{path}.{key}", 1, 64)
        if key == "text_size":
            result[key] = {None if s is None else integer(s, f"{path}.{key}", TEXT_SIZES.start, TEXT_SIZES.stop - 1)
                           for s in items}
        else:
            result[key] = {text(item, f"{path}.{key}", PALETTE if key == "palette" else VARIANT_ID) for item in items}
    if not any(applies(result, variant) for variant in variants):
        fail(path, "matches no variant, so the entry would never run")
    return result


def applies(when: dict | None, variant: Variant) -> bool:
    if not when:
        return True
    return (variant.palette in when.get("palette", {variant.palette})
            and variant.text_size in when.get("text_size", {variant.text_size})
            and variant.id in when.get("variant", {variant.id}))


def split_ref(ref: str, role: str | None) -> tuple[str | None, str]:
    """`capture` (the analysis's own role) or `role:capture`."""
    named, sep, capture = ref.partition(":")
    return (named, capture) if sep else (role, ref)


# ---------- steps ----------
def step_entry(value, path: str, spec: dict) -> dict:
    window, crops, roles = spec["window"], spec["crops"], spec["roles"]
    actions = [key for key in STEPS if isinstance(value, dict) and key in value]
    if len(actions) != 1:
        fail(path, f"needs exactly one action of {', '.join(STEPS)}")
    action = actions[0]
    obj(value, path, {action}, STEPS[action] | {"note", "when"})
    step = dict(value)
    if "note" in step:
        text(step["note"], f"{path}.note", limit=300)
    step["when"] = when_filter(value.get("when"), f"{path}.when", spec["variants"])
    where = f"{path}.{action}"
    if action in READING_STEPS:
        return reading_step(step, path, spec)
    if action == "key":
        text(value["key"], where, KEYSYM)
        step["mods"] = [text(m, f"{path}.mods[{i}]", KEYSYM)
                        for i, m in enumerate(array(value.get("mods", []), f"{path}.mods", 0, 4))]
        step["repeat"] = integer(value.get("repeat", 1), f"{path}.repeat", 1, 200)
        step["wait_after"] = number(value.get("wait_after", 0.4), f"{path}.wait_after", 0, 30)
        if "await_change" in value:
            step["await_change"] = number(value["await_change"], f"{path}.await_change", 0.05, 30)
    elif action in ("type", "palette"):
        text(value[action], where, limit=200)
    elif action in ("move", "glide", "click"):
        step[action] = list(point(value[action], where, window))
        if "settle" in value:
            number(value["settle"], f"{path}.settle", 0, 30)
    elif action in ("press", "release"):
        integer(value[action], where, 1, 3)
    elif action == "wheel":
        step["wheel"] = list(point(value["wheel"], where, window))
        if "steps" not in value:
            fail(path, "a wheel needs \"steps\"")
        if integer(value["steps"], f"{path}.steps", -100, 100) == 0:
            fail(f"{path}.steps", "0 steps sends nothing")
    elif action == "park":
        if value["park"] is not True:
            fail(where, "expected true")
    elif action == "wait":
        number(value["wait"], where, 0, 120)
    elif action == "stable":
        number(value["stable"], where, 0.1, 120)
        if "quiet" in value:
            number(value["quiet"], f"{path}.quiet", 0.05, 10)
    elif action == "resize":
        array(value["resize"], where, 2, 2)
        step["resize"] = [integer(v, f"{where}[{i}]", 200, 8192) for i, v in enumerate(value["resize"])]
    elif action == "read_only":
        problem = runenv.read_only_problem(value["read_only"])
        if problem is not None:
            fail(where, problem)
    elif action == "mark":
        text(value["mark"], where, IDENTIFIER)
        step["park_first"] = boolean(value.get("park_first", False), f"{path}.park_first")
        if "stable_within" in value:
            number(value["stable_within"], f"{path}.stable_within", 0.1, 120)
        step["quiet"] = number(value.get("quiet", 0.5), f"{path}.quiet", 0.05, 10)
    elif action == "guard":
        step["guard"] = analysis_entry(value["guard"], where, spec, guard=True)
        on_fail = []
        for i, item in enumerate(array(value.get("on_fail", []), f"{path}.on_fail", 0, 8)):
            obj(item, f"{path}.on_fail[{i}]", {"key"}, {"mods"})
            mods = array(item.get("mods", []), f"{path}.on_fail[{i}].mods", 0, 4)
            on_fail.append(dict(key=text(item["key"], f"{path}.on_fail[{i}].key", KEYSYM),
                                mods=[text(m, f"{path}.on_fail[{i}].mods[{j}]", KEYSYM) for j, m in enumerate(mods)],
                                note="guard failed"))
        step["on_fail"] = on_fail
    elif action == "capture":
        text(value["capture"], where, IDENTIFIER)
        step["commit"] = boolean(value.get("commit", True), f"{path}.commit")
        step["roles"] = [text(r, f"{path}.roles", re.compile("|".join(roles))) for r in
                         array(value.get("roles", list(roles)), f"{path}.roles", 1, len(roles))]
        crop = value.get("crop")
        if crop is not None and crop not in crops:
            fail(f"{path}.crop", f"no crop box named {crop!r}; defined: {', '.join(crops) or 'none'}")
        step["crop"] = crop
        shows = value.get("shows")
        if isinstance(shows, dict):
            obj(shows, f"{path}.shows", (), roles)
            step["shows"] = {role: text(line, f"{path}.shows.{role}", limit=300) for role, line in shows.items()}
        elif shows is not None:
            step["shows"] = text(shows, f"{path}.shows", limit=300)
        if step["commit"]:
            if shows is None:
                fail(path, "a committed capture needs \"shows\": one line on what the frame shows")
            if isinstance(step["shows"], dict) and set(step["shows"]) != set(step["roles"]):
                fail(f"{path}.shows", f"give one line for each committed role: {', '.join(step['roles'])}")
        step["keep_pointer"] = boolean(value.get("keep_pointer", False), f"{path}.keep_pointer")
        step["settle"] = number(value.get("settle", 0.0), f"{path}.settle", 0, 30)
        step["stable_within"] = number(value.get("stable_within", CAPTURE_STABLE), f"{path}.stable_within", 0, 120)
        step["quiet"] = number(value.get("quiet", CAPTURE_QUIET), f"{path}.quiet", 0.05, 10)
    elif action == "probe":
        text(value["probe"], where, IDENTIFIER)
        problem = session.probe_input_problem(value.get("send"), value.get("click_at"), value.get("release_at"))
        if problem is not None:
            fail(path, problem)
        if "send" in value:
            step["send"] = text(value["send"], f"{path}.send", KEYSYM)
            step["mods"] = [text(m, f"{path}.mods[{i}]", KEYSYM)
                            for i, m in enumerate(array(value.get("mods", []), f"{path}.mods", 0, 4))]
        else:
            for key in ("mods", "keep_pointer"):
                if key in value:
                    fail(f"{path}.{key}", "a click probe presses the pointer's first button at \"click_at\", "
                                          "so it takes no modifier and always moves the pointer")
            step["mods"] = []
            step["click_at"] = spot(value["click_at"], f"{path}.click_at", spec)
            if "release_at" in value:
                step["release_at"] = spot(value["release_at"], f"{path}.release_at", spec)
        step["repeat"] = integer(value.get("repeat", 1), f"{path}.repeat", 1, 200)
        step["region"] = list(area(value["region"], f"{path}.region", spec) if "region" in value
                              else (0, 0, *window))
        step["quiet"] = number(value.get("quiet", session.PROBE_QUIET), f"{path}.quiet", 0.05, 10)
        step["timeout"] = number(value.get("timeout", session.PROBE_TIMEOUT), f"{path}.timeout", 0.1, 30)
        if step["quiet"] >= step["timeout"]:
            fail(f"{path}.quiet", f"{step['quiet']} s is not shorter than the timeout, {step['timeout']} s, so no "
                                  "press could settle")
        step["stable_within"] = number(value.get("stable_within", session.PROBE_STABLE), f"{path}.stable_within",
                                       0.05, 120)
        if step["stable_within"] < step["quiet"]:
            fail(f"{path}.stable_within", f"{step['stable_within']} s is shorter than quiet, {step['quiet']} s, so the "
                                          "region could never count as settled before a press")
        step["keep_pointer"] = boolean(value.get("keep_pointer", False), f"{path}.keep_pointer")
    return step


def reading_step(step: dict, path: str, spec: dict) -> dict:
    """An `atspi_focus` or `store_snapshot` step, normalised: a label and its waits (and a snapshot's store)."""
    action = next(key for key in READING_STEPS if key in step)
    text(step[action], f"{path}.{action}", IDENTIFIER)
    if action == "atspi_focus":
        if not spec["atspi"]:
            fail(f"{path}.atspi_focus", "needs \"atspi\": true, which sets org.a11y.Status IsEnabled for each launch "
                                        "so the app registers on the accessibility bus")
        step["within"] = number(step.get("within", a11y.WITHIN), f"{path}.within", 0.1, 60)
        return step
    problem = runenv.read_only_problem(step.get("path", stores.PREFERENCES))  # a run-directory path in its XDG homes
    if problem is not None:
        fail(f"{path}.path", problem)
    step["path"] = step.get("path", stores.PREFERENCES)
    step["quiet"] = number(step.get("quiet", stores.SNAPSHOT_QUIET), f"{path}.quiet", 0.1, 30)
    step["within"] = number(step.get("within", stores.SNAPSHOT_WITHIN), f"{path}.within", 0.1, 120)
    if step["within"] < step["quiet"]:
        fail(f"{path}.within", f"{step['within']} s is shorter than quiet ({step['quiet']} s), so the store could "
                               "never settle")
    return step


def reading_analysis(entry: dict, value, path: str, ref) -> None:
    """The parameters of an `atspi_focus` or `store_compare` analysis; `ref` validates a reading's label."""
    def reading(key):
        label = ref(key)
        if label == NOW:
            fail(f"{path}.{key}", f"{NOW} is a frame; give a reading's label")
        return label

    if entry["kind"] == "atspi_focus":
        entry["focus"] = reading("focus")
        if "same_as" in value:
            entry["same_as"] = reading("same_as")
        if "node" in value:
            entry["node"] = text(value["node"], f"{path}.node", limit=300)
        if "role" in value:
            entry["role"] = text(value["role"], f"{path}.role", limit=80)
        for key in ("states", "not_states"):
            states = array(value.get(key, []), f"{path}.{key}", 0, len(a11y.STATES))
            for i, state in enumerate(states):
                if state not in a11y.STATES:
                    fail(f"{path}.{key}[{i}]", f"{state!r} is not a state a reading reports: "
                                               f"{', '.join(a11y.STATES)}")
            entry[key] = list(states)
        if set(entry["states"]) & set(entry["not_states"]):
            fail(path, "a state cannot be both required and refused")
        if not ({"node", "role", "same_as"} & set(entry) or entry["states"] or entry["not_states"]):
            fail(path, "an atspi_focus analysis needs node, role, states, not_states or same_as to pass or fail")
        return
    entry.update(a=reading("a"), b=reading("b"))
    if entry["a"] == entry["b"]:
        fail(path, "a and b name the same snapshot")
    if "keys" in value:
        entry["keys"] = [text(key, f"{path}.keys[{i}]", JSON_KEY)
                         for i, key in enumerate(array(value["keys"], f"{path}.keys", 1, 64))]


# ---------- analyses ----------
def analysis_entry(value, path: str, spec: dict, guard: bool = False) -> dict:
    """A normalised analysis: its kind's parameters with defaults; a guard has no name, filter or expectation."""
    window, crops, roles = spec["window"], spec["crops"], spec["roles"]
    kind = value.get("kind") if isinstance(value, dict) else None
    if kind not in ANALYSES:
        fail(f"{path}.kind", f"expected one of {', '.join(ANALYSES)}, got {kind!r}")
    if guard and kind in analysis.PROBE_KINDS:
        fail(f"{path}.kind", f"a guard reads one frame as the steps run; {kind} reads a probe's frames afterwards")
    required, optional = ANALYSES[kind]
    common = {"kind", "note"} if guard else ANALYSIS_COMMON
    obj(value, path, required | ({"kind"} if guard else {"name", "kind"}), optional | common)
    entry = dict(kind=kind)
    if not guard:
        entry["name"] = text(value["name"], f"{path}.name", IDENTIFIER)
        if "note" in value:
            entry["note"] = text(value["note"], f"{path}.note", limit=300)

    def frame_ref(name, where):
        text(name, where, limit=120)
        if guard:
            if name != NOW and not IDENTIFIER.fullmatch(name):
                fail(where, f"a guard reads {NOW}, a mark or an earlier capture, not {name!r}")
        else:
            named, capture = split_ref(name, None)
            if named is not None and named not in roles:
                fail(where, f"{named!r} is not one of the roles {', '.join(roles)}")
            if not IDENTIFIER.fullmatch(capture):
                what = ("probe" if kind in analysis.PROBE_KINDS else "reading" if kind in analysis.READING_KINDS
                        else "capture")
                fail(where, f"{capture!r} is not a {what} name")
        return name

    def ref(key):
        return frame_ref(value[key], f"{path}.{key}")

    def optional_number(key, minimum=0.0, maximum=1000.0):
        if key in value:
            entry[key] = number(value[key], f"{path}.{key}", minimum, maximum)

    def optional_int(key, minimum=0, maximum=100_000_000):
        if key in value:
            entry[key] = integer(value[key], f"{path}.{key}", minimum, maximum)

    if kind in analysis.READING_KINDS:
        reading_analysis(entry, value, path, ref)
    elif kind == "ring":
        entry.update(frame=ref("frame"), rect=box(value["rect"], f"{path}.rect", window))
        detect = value.get("colour", "detect")
        entry["colour"] = "detect" if detect == "detect" else colour(detect, f"{path}.colour")
        sides = array(value.get("sides", list(analysis.SIDES)), f"{path}.sides", 1, 4)
        if any(side not in analysis.SIDES for side in sides) or len(set(sides)) != len(sides):
            fail(f"{path}.sides", f"expected distinct sides of {', '.join(analysis.SIDES)}")
        entry.update(sides=tuple(sides), reach=integer(value.get("reach", analysis.REACH), f"{path}.reach", 1, 16),
                     corner=integer(value.get("corner", analysis.CORNER), f"{path}.corner", 0, 64),
                     tolerance=integer(value.get("tolerance", analysis.TOLERANCE), f"{path}.tolerance", 0, 64),
                     min_width=integer(value.get("min_width", 1), f"{path}.min_width", 1, 16),
                     uniform_width=boolean(value.get("uniform_width", True), f"{path}.uniform_width"))
        optional_number("min_contrast", 1.0, 21.0)
        optional_int("max_outlines", 1, 8)
    elif kind == "clearance":
        entry.update(frame=ref("frame"), rect=box(value["rect"], f"{path}.rect", window))
        if value["side"] not in analysis.SIDES:
            fail(f"{path}.side", f"expected one of {', '.join(analysis.SIDES)}")
        entry["side"] = value["side"]
        surface = value["surface"]
        if isinstance(surface, dict):
            entry["surface"] = {"at": point(obj(surface, f"{path}.surface", {"at"})["at"], f"{path}.surface.at",
                                            window)}
        else:
            entry["surface"] = colour(surface, f"{path}.surface")
        ring = value.get("ring")
        entry["ring"] = ring if ring in (None, "detect") else colour(ring, f"{path}.ring")
        if "at" in value:
            limit = window[0] if entry["side"] in ("top", "bottom") else window[1]
            entry["at"] = [integer(p, f"{path}.at[{i}]", 0, limit - 1)
                           for i, p in enumerate(array(value["at"], f"{path}.at", 1, 16))]
        entry.update(reach=integer(value.get("reach", analysis.REACH), f"{path}.reach", 1, 16),
                     tolerance=integer(value.get("tolerance", analysis.TOLERANCE), f"{path}.tolerance", 0, 64),
                     limit=integer(value.get("limit", 40), f"{path}.limit", 1, 400))
        optional_int("min_px", 0, 400)
        optional_int("max_px", 0, 400)
        if "min_px" not in entry and "max_px" not in entry:
            fail(path, "a clearance needs min_px or max_px to pass or fail")
    elif kind in analysis.PROBE_KINDS:
        entry.update(probe=ref("probe"), clip=area(value["clip"], f"{path}.clip", spec))
        if kind == "probe_ring":
            detect = value.get("colour", "detect")
            entry["colour"] = "detect" if detect == "detect" else colour(detect, f"{path}.colour")
            surface = value.get("surface")
            if isinstance(surface, dict):
                at = point(obj(surface, f"{path}.surface", {"at"})["at"], f"{path}.surface.at", window)
                if not inside((*at, at[0] + 1, at[1] + 1), entry["clip"]):
                    fail(f"{path}.surface.at", f"{list(at)} is outside the clip {list(entry['clip'])}")
                entry["surface"] = {"at": at}
            else:
                entry["surface"] = None if surface is None else colour(surface, f"{path}.surface")
            entry.update(tolerance=integer(value.get("tolerance", analysis.TOLERANCE), f"{path}.tolerance", 0, 64),
                         corner=integer(value.get("corner", analysis.CORNER), f"{path}.corner", 0, 64),
                         min_width=integer(value.get("min_width", 1), f"{path}.min_width", 1, 16),
                         uniform_width=boolean(value.get("uniform_width", True), f"{path}.uniform_width"))
            optional_number("min_contrast", 1.0, 21.0)
            optional_int("max_outlines", 1, 8)
        else:
            entry["max_pixels"] = integer(value.get("max_pixels", 0), f"{path}.max_pixels", 0, 100_000_000)
        entry["masks"] = [box(mask, f"{path}.masks[{i}]", window)
                          for i, mask in enumerate(array(value.get("masks", []), f"{path}.masks", 0, 32))]
    elif kind == "fill":
        entry.update(frame=ref("frame"), region=box(value["region"], f"{path}.region", window))
        reference = value["reference"]
        if isinstance(reference, dict) and "colour" in reference:
            obj(reference, f"{path}.reference", {"colour"})
            entry["reference"] = {"colour": colour(reference["colour"], f"{path}.reference.colour")}
        else:
            obj(reference, f"{path}.reference", {"region"}, {"frame"})
            entry["reference"] = {"region": box(reference["region"], f"{path}.reference.region", window),
                                  "frame": frame_ref(reference["frame"], f"{path}.reference.frame")
                                  if "frame" in reference else None}
        optional_number("min_contrast", 1.0, 21.0)
        optional_number("max_contrast", 1.0, 21.0)
        if "min_contrast" not in entry and "max_contrast" not in entry:
            fail(path, "a fill needs min_contrast or max_contrast to pass or fail")
    else:
        entry.update(a=ref("a"), b=ref("b"))
        if "crop" in value and "region" in value:
            fail(path, "give a region or a crop, not both")
        if "crop" in value:
            if value["crop"] not in crops:
                fail(f"{path}.crop", f"no crop box named {value['crop']!r}")
            entry["region"] = crops[value["crop"]]
        elif "region" in value:
            entry["region"] = box(value["region"], f"{path}.region", window)
        masks = []
        for i, mask in enumerate(array(value.get("masks", []), f"{path}.masks", 0, 32)):
            if isinstance(mask, str):
                if mask not in frames.NAMED_MASKS:
                    fail(f"{path}.masks[{i}]", f"no named mask {mask!r}; named: {', '.join(frames.NAMED_MASKS)}")
                masks.append(mask)
            else:
                masks.append(box(mask, f"{path}.masks[{i}]", window))
        entry.update(masks=masks, band_min=integer(value.get("band_min", 1), f"{path}.band_min", 1, 8192))
        optional_int("max_pixels")
        optional_int("min_pixels")
        if "bands" in value:
            bands = []
            for i, band in enumerate(array(value["bands"], f"{path}.bands", 0, 256)):
                array(band, f"{path}.bands[{i}]", 2, 2)
                y0, y1 = (integer(v, f"{path}.bands[{i}]", 0, window[1] - 1) for v in band)
                if y0 > y1:
                    fail(f"{path}.bands[{i}]", "a band is [first row, last row], inclusive")
                bands.append([y0, y1])
            entry["bands"] = bands
        if not {"max_pixels", "min_pixels", "bands"} & set(entry):
            entry["max_pixels"] = 0
    if not guard:
        per_role = any(split_ref(r, None)[0] is None for r in analysis.refs(entry))
        entry["when"] = when_filter(value.get("when"), f"{path}.when", spec["variants"])
        if per_role:
            entry["roles"] = [text(r, f"{path}.roles", re.compile("|".join(roles)))
                              for r in array(value.get("roles", list(roles)), f"{path}.roles", 1, len(roles))]
            expect = value.get("expect", True)
            if isinstance(expect, dict):
                obj(expect, f"{path}.expect", (), roles)
                entry["expect"] = {r: expectation(expect.get(r, True), f"{path}.expect.{r}") for r in entry["roles"]}
            else:
                entry["expect"] = {r: expectation(expect, f"{path}.expect") for r in entry["roles"]}
        else:
            if "roles" in value:
                fail(f"{path}.roles", "every frame names its role, so the analysis runs once per variant")
            entry["roles"] = None
            entry["expect"] = expectation(value.get("expect", True), f"{path}.expect")
    return entry


# ---------- the whole spec ----------
def validate(data, sha256: str = "") -> dict:
    """The normalised spec, or a SpecError naming the first problem."""
    obj(data, "$", *TOP_LEVEL)
    if data["version"] != VERSION:
        fail("$.version", f"this tooling reads version {VERSION}, got {data['version']!r}")
    spec: dict = dict(version=VERSION, sha256=sha256, task=text(data["task"], "$.task", IDENTIFIER),
                      summary=text(data["summary"], "$.summary") if "summary" in data else "",
                      limitations=text(data["limitations"], "$.limitations") if "limitations" in data else "")
    window = data.get("window", list(DEFAULT_WINDOW))
    array(window, "$.window", 2, 2)
    spec["window"] = tuple(integer(v, f"$.window[{i}]", 200, 8192) for i, v in enumerate(window))
    roles = array(data.get("roles", list(ROLES)), "$.roles", 1, 2)
    if any(role not in ROLES for role in roles) or len(set(roles)) != len(roles):
        fail("$.roles", f"expected distinct roles of {', '.join(ROLES)}")
    spec["roles"] = [role for role in ROLES if role in roles]
    spec["fixture"] = fixture_recipe(data["fixture"], "$.fixture") if "fixture" in data else None
    spec["writes"] = fixture_writes(data["writes"], "$.writes", spec) if "writes" in data else None
    spec["settings"] = store_settings_entry(data.get("settings", {}), "$.settings")
    env = mapping(data.get("env", {}), "$.env")
    for key, item in env.items():
        text(key, "$.env", re.compile(r"[A-Z][A-Z0-9_]{0,63}"))
        if runenv.reserved(key):
            fail(f"$.env.{key}", "the run sets this itself (display, scale factor, HOME, XDG, Wayland, D-Bus or "
                                 "Git isolation), so a spec cannot change what run.json and the attestation report")
        text(item, f"$.env.{key}", limit=500)
    spec["env"] = env
    spec["home"] = home_entry(data["home"], "$.home") if "home" in data else {}
    spec["atspi"] = boolean(data.get("atspi", False), "$.atspi")
    spec["variants"] = variant_list(data["variants"], "$.variants")
    for index, variant in enumerate(spec["variants"]):  # what each launch would seed, as Session checks it
        problem = runenv.home_files_problem(home_files(spec, variant))
        if problem is not None:
            fail(f"$.variants[{index}].home", f"with the spec's own HOME files, {problem}")
    spec["crops"] = {}
    for name, value in mapping(data.get("crops", {}), "$.crops").items():
        spec["crops"][text(name, "$.crops", IDENTIFIER)] = box(value, f"$.crops.{name}", spec["window"])
    spec["steps"] = []
    for index, value in enumerate(array(data["steps"], "$.steps", 1, 2000)):
        step = step_entry(value, f"$.steps[{index}]", spec)
        step["index"] = index
        spec["steps"].append(step)
    spec["analyses"] = [analysis_entry(value, f"$.analyses[{i}]", spec)
                        for i, value in enumerate(array(data.get("analyses", []), "$.analyses", 0, 500))]
    names = [entry["name"] for entry in spec["analyses"]]
    if len(set(names)) != len(names):
        fail("$.analyses", "analysis names repeat")
    check_references(spec)
    committed(spec)  # refuses two crops with one committed name
    return spec


def check_references(spec: dict) -> None:
    """Per variant: captures, probes and readings are unique, guards read only frames and readings taken before
    them, analyses only captures and readings, and probe analyses only probes whose region holds their clip."""
    for variant in spec["variants"]:
        taken: set[str] = set()
        captures: set[str] = set()
        readings: dict[str, str] = {}  # label: the step kind that took it
        probed: dict[str, list[int]] = {}
        for step in steps_for(spec, variant):
            where = f"$.steps[{step['index']}]"
            kind = next((key for key in READING_STEPS if key in step), None)
            if kind is not None:
                if step[kind] in readings:
                    fail(where, f"reading {step[kind]!r} is taken twice in variant {variant.id}")
                readings[step[kind]] = kind
            if "capture" in step:
                if step["capture"] in captures:
                    fail(where, f"capture {step['capture']!r} is taken twice in variant {variant.id}")
                captures.add(step["capture"])
                taken.add(step["capture"])
            elif "probe" in step:
                if step["probe"] in probed:
                    fail(where, f"probe {step['probe']!r} runs twice in variant {variant.id}")
                probed[step["probe"]] = step["region"]
            elif "mark" in step:
                taken.add(step["mark"])
            elif "guard" in step:
                for ref in analysis.frame_refs(step["guard"]):
                    if ref != NOW and ref not in taken:
                        fail(where, f"the guard reads {ref!r}, which is not marked or captured before it "
                                    f"in variant {variant.id}")
                check_readings(step["guard"], readings, where, "is not taken before it", variant)
        for index, entry in enumerate(spec["analyses"]):
            if not applies(entry["when"], variant):
                continue
            for ref in analysis.frame_refs(entry):
                name = split_ref(ref, None)[1]
                if entry["kind"] not in analysis.PROBE_KINDS:
                    if name not in captures:
                        fail(f"$.analyses[{index}]", f"reads {ref!r}, which variant {variant.id} never captures")
                elif name not in probed:
                    fail(f"$.analyses[{index}]", f"reads probe {ref!r}, which variant {variant.id} never runs")
                elif not inside(entry["clip"], probed[name]):
                    fail(f"$.analyses[{index}].clip", f"{list(entry['clip'])} is not inside probe {name!r}'s region "
                                                      f"{probed[name]}, the only pixels it grabs")
            check_readings(entry, readings, f"$.analyses[{index}]", "is never taken", variant)


def check_readings(entry: dict, readings: dict[str, str], where: str, absent: str, variant: Variant) -> None:
    """An analysis or guard reads only readings its variant takes, each of the kind it reads."""
    wanted = analysis.READING_KINDS.get(entry["kind"])
    for ref in analysis.reading_refs(entry):
        label = split_ref(ref, None)[1]
        if label not in readings:
            fail(where, f"reads reading {ref!r}, which {absent} in variant {variant.id}")
        if readings[label] != wanted:
            fail(where, f"reads {ref!r}, which comes from {readings[label]}, but {entry['kind']} reads only "
                        f"{wanted} readings")


def load(path: Path) -> dict:
    data = Path(path).read_bytes()
    try:
        parsed = json.loads(data)
    except ValueError as error:
        raise SpecError(f"$: {path} is not JSON ({error})") from None
    return validate(parsed, hashlib.sha256(data).hexdigest())


# ---------- planning ----------
def steps_for(spec: dict, variant: Variant) -> list[dict]:
    return [step for step in spec["steps"] if applies(step["when"], variant)]


def crop_name(role: str, variant: Variant, window, capture: str) -> str:
    """The committed file name: `{base|candidate}-{palette}[-{size}pt]-{W}x{H}-{capture}.png`, with the variant's
    id in place of `{palette}[-{size}pt]` when it has its own settings."""
    return f"{COMMITTED_PREFIX[role]}-{variant.label}-{window[0]}x{window[1]}-{capture}.png"


def shows_for(step: dict, role: str) -> str:
    shows = step.get("shows", "")
    return shows.get(role, "") if isinstance(shows, dict) else shows


def committed(spec: dict, roles=None) -> list[Crop]:
    """Every crop the scenario commits, variant by variant, in step order, base before candidate."""
    crops, seen = [], {}
    for variant in spec["variants"]:
        for step in steps_for(spec, variant):
            if "capture" not in step or not step["commit"]:
                continue
            for role in spec["roles"]:
                if role not in step["roles"] or (roles is not None and role not in roles):
                    continue
                name = crop_name(role, variant, spec["window"], step["capture"])
                if name in seen:
                    index, other = seen[name]
                    if other != variant.id:
                        fail("$.variants", f"variants {other} and {variant.id} would both commit {name} (step "
                                           f"{index}); a committed name tells variants apart by palette and text "
                                           "size, or by the id of a variant with \"settings\"")
                    fail(f"$.steps[{step['index']}]", f"commits {name}, as step {index} already does")
                seen[name] = (step["index"], variant.id)
                crops.append(Crop(name, role, variant, step["capture"], step["crop"],
                                  spec["crops"].get(step["crop"]) if step["crop"] else None, shows_for(step, role)))
    return crops


def analysis_runs(spec: dict) -> list[tuple[dict, str | None, Variant]]:
    """(entry, role or None for a cross-role analysis, variant) for every evaluation the bundle needs."""
    runs = []
    for entry in spec["analyses"]:
        for variant in spec["variants"]:
            if not applies(entry["when"], variant):
                continue
            for role in (entry["roles"] or [None]):
                runs.append((entry, role, variant))
    return runs


def expected(entry: dict, role: str | None) -> bool | str:
    """True or False, the verdict the analysis must reach, or RECORD when it is only measured."""
    return entry["expect"] if role is None else entry["expect"][role]


def probes(spec: dict) -> list[dict]:
    """Every probe step, once each, in step order (the same name in other variants is listed once)."""
    seen: dict[str, dict] = {}
    for step in spec["steps"]:
        if "probe" in step:
            seen.setdefault(step["probe"], step)
    return list(seen.values())


def store_settings(spec: dict, variant: Variant) -> dict:
    """The settings seeded into a launch's store: the spec's, the variant's own over them, then its text size."""
    settings = {**spec["settings"], **variant.settings}
    if variant.text_size is not None:
        settings["interface_text_size"] = variant.text_size
    return settings


def home_files(spec: dict, variant: Variant) -> dict[str, bytes]:
    """The files seeded under a launch's HOME: the spec's, with the variant's own over them path by path."""
    return {**spec["home"], **variant.home}
