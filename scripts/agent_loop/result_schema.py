"""Strict validation of a session's result object, and its recovery from text.

Every controller session is given a JSON schema for its result. Claude Code
usually returns the result as structured output, but a session can also leave
it in its final message as text, after a sentence, fenced or not. Both forms
are held to one bar here: the whole schema is enforced, and a schema keyword
this module does not understand is refused rather than ignored, so a later
schema change cannot silently weaken the check. Semantic rules (task id,
candidate sha, verdict consistency) stay with the callers.
"""

from __future__ import annotations

import json
import math
import re

from .records import LoopError


# `description` is an annotation; every other keyword is enforced.
KEYWORDS = frozenset({"type", "properties", "required", "additionalProperties", "enum", "items", "description"})
TYPES = frozenset({"object", "array", "string", "integer", "number", "boolean", "null"})
# A final message is far smaller. With the attempt cap this bounds the text
# search, which runs after the session and outside its watchdog: each decode
# attempt scans at most the whole text once.
MAX_RESULT_TEXT = 1 << 20
MAX_DECODE_ATTEMPTS = 64


def check_schema(schema: object, path: str = "$") -> None:
    """Refuse a schema that uses anything `schema_error` does not enforce.

    Every node names one type, objects are closed and list their properties,
    and arrays say what they hold: a node without those would accept
    arbitrary data, which is the weakening this check exists to prevent.
    """
    if not isinstance(schema, dict):
        raise LoopError(f"result schema {path} is not an object")
    unknown = sorted(set(schema) - KEYWORDS)
    if unknown:
        raise LoopError(f"result schema {path} uses unsupported keywords: {', '.join(unknown)}")
    kind = schema.get("type")
    if not isinstance(kind, str) or kind not in TYPES:
        raise LoopError(f"result schema {path} must name one supported type")
    if "description" in schema and not isinstance(schema["description"], str):
        raise LoopError(f"result schema {path} has a non-string description")
    misplaced = {"properties", "required", "additionalProperties"} & set(schema) if kind != "object" else set()
    misplaced |= {"items"} & set(schema) if kind != "array" else set()
    if misplaced:
        raise LoopError(f"result schema {path} uses {', '.join(sorted(misplaced))} on type {kind}")
    if kind == "object":
        properties = schema.get("properties")
        if not isinstance(properties, dict) or not all(isinstance(key, str) for key in properties):
            raise LoopError(f"result schema {path} needs named properties")
        if schema.get("additionalProperties") is not False:
            raise LoopError(f"result schema {path} must forbid additional properties")
        required = schema.get("required", [])
        if (not isinstance(required, list) or len(set(required)) != len(required)
                or not all(isinstance(key, str) and key in properties for key in required)):
            raise LoopError(f"result schema {path} requires an unknown or repeated property")
        for key, child in properties.items():
            check_schema(child, f"{path}.{key}")
    if kind == "array":
        check_schema(schema.get("items"), f"{path}[]")
    if "enum" in schema:
        members = schema["enum"]
        if not isinstance(members, list) or not members or not all(
                _has_type(member, kind) and not isinstance(member, (dict, list)) for member in members):
            raise LoopError(f"result schema {path} has an enum that does not match its type")


def schema_error(value: object, schema: dict) -> str | None:
    """Why `value` does not satisfy `schema`, or None when it does."""
    check_schema(schema)
    return _error(value, schema, "$")


def _has_type(value: object, kind: str) -> bool:
    if kind == "object":
        return isinstance(value, dict)
    if kind == "array":
        return isinstance(value, list)
    if kind == "string":
        return isinstance(value, str)
    if kind == "boolean":
        return isinstance(value, bool)
    if kind == "null":
        return value is None
    # JSON has one number type; Python's bool is an int, and neither counts here.
    if isinstance(value, bool):
        return False
    if kind == "integer":
        return isinstance(value, int)
    return isinstance(value, int) or (isinstance(value, float) and math.isfinite(value))


def _error(value: object, schema: dict, path: str) -> str | None:
    kind = schema["type"]
    if not _has_type(value, kind):
        return f"{path} is not of type {kind}"
    if "enum" in schema and value not in schema["enum"]:
        return f"{path} is not one of {', '.join(map(str, schema['enum']))}"
    if kind == "object":
        properties = schema["properties"]
        missing = [key for key in schema.get("required", ()) if key not in value]
        if missing:
            return f"{path} is missing {', '.join(missing)}"
        unexpected = sorted(set(value) - set(properties))
        if unexpected:
            # Names come from the session; keep the diagnosis short.
            return f"{path} has unexpected properties: {', '.join(unexpected)[:200]}"
        for key, child in properties.items():
            if key in value and (problem := _error(value[key], child, f"{path}.{key}")):
                return problem
    if kind == "array":
        for index, item in enumerate(value):
            if problem := _error(item, schema["items"], f"{path}[{index}]"):
                return problem
    return None


def _unique_object(pairs: list[tuple[str, object]]) -> dict:
    # A repeated key would let the last of two verdicts win unseen.
    result: dict = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate key {key!r}")
        result[key] = value
    return result


def _refuse_constant(name: str) -> object:
    raise ValueError(f"{name} is not JSON")


DECODER = json.JSONDecoder(object_pairs_hook=_unique_object, parse_constant=_refuse_constant)


def text_result(text: str, schema: dict) -> tuple[dict | None, str]:
    """The result object a session left in its final message, or why there is none.

    Candidates are the objects that open like a result: a brace, then one of the
    result's own field names as the first key. Prose, Markdown fences and other
    objects around them are passed over, so an unrelated object quoted after the
    result does not hide it, and only candidates are ever decoded. A candidate
    decoded whole is skipped past; inside one that is not valid JSON the search
    goes on, so its own candidates are still found.

    The last candidate decides and must validate. An earlier valid one never
    stands in for it, whether the later one breaks the schema or is not even
    valid JSON (a trailing comma, a stray quote, a cut-off end): a session that
    writes a verdict and then a corrected one has withdrawn the first, and a
    retry asks it for the second. A candidate that is not valid JSON decides
    when it starts after the last decoded one or extends past its start.
    """
    check_schema(schema)
    if len(text) > MAX_RESULT_TEXT:
        return None, f"final message is longer than {MAX_RESULT_TEXT} characters"
    opener = re.compile(r'\{\s*"(?:' + "|".join(map(re.escape, sorted(schema["properties"]))) + r')"\s*:')
    last, last_start = None, -1
    # How far into the text a candidate that is not valid JSON may reach.
    reach = -1
    index = 0
    attempts = 0
    while match := opener.search(text, index):
        start = match.start()
        attempts += 1
        if attempts > MAX_DECODE_ATTEMPTS:
            return None, f"final message opens more than {MAX_DECODE_ATTEMPTS} result objects"
        try:
            value, end = DECODER.raw_decode(text, start)
        except json.JSONDecodeError as error:
            reach = max(reach, error.pos)
        except (ValueError, RecursionError):
            # A repeated key, NaN or too deep: where it ends is unknown.
            reach = len(text)
        else:
            last, last_start, index = value, start, end
            continue
        index = start + 1
    if reach > last_start:
        return None, "the last result object in the final message is not valid JSON"
    if last is None:
        return None, "final message holds no JSON object with the result's fields"
    problem = _error(last, schema, "$")
    return (None, problem) if problem else (last, "")
