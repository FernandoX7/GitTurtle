"""The strict result validator and the recovery of a result object from session text."""

from __future__ import annotations

import json
import time
import unittest

from agent_loop.codex import BUILD_SCHEMA, REVIEW_SCHEMA
from agent_loop.process import LoopError
from agent_loop.result_schema import MAX_DECODE_ATTEMPTS, MAX_RESULT_TEXT, check_schema, schema_error, text_result
from agent_loop.security_review import SECURITY_REVIEW_SCHEMA
from agent_loop.test_codex_process import passing_review
from agent_loop.test_security_review import finding, passing_security


COUNTED = {
    "type": "object", "additionalProperties": False, "required": ["count"],
    "properties": {"count": {"type": "integer"}, "ratio": {"type": "number"}, "done": {"type": "boolean"},
                   "absent": {"type": "null"}},
}


class SchemaTests(unittest.TestCase):
    def test_every_role_schema_is_fully_understood(self):
        for schema in (BUILD_SCHEMA, REVIEW_SCHEMA, SECURITY_REVIEW_SCHEMA):
            check_schema(schema)
        self.assertIsNone(schema_error(passing_review(), REVIEW_SCHEMA))
        failing = dict(passing_security(), verdict="fail", findings=[finding()])
        self.assertIsNone(schema_error(failing, SECURITY_REVIEW_SCHEMA))
        self.assertIsNone(schema_error({"task_id": "one", "status": "blocked", "summary": "x"}, BUILD_SCHEMA))

    def test_a_schema_the_validator_cannot_enforce_is_refused(self):
        string = {"type": "string"}
        for schema, message in (
            (dict(COUNTED, properties={"count": {"type": "integer", "minimum": 0}}), "unsupported keywords: minimum"),
            ({**COUNTED, "$schema": "https://json-schema.org/draft/2020-12/schema"}, r"unsupported keywords: \$schema"),
            (dict(COUNTED, properties={"count": {"type": ["integer", "null"]}}), "one supported type"),
            (dict(COUNTED, properties={"count": {"description": "untyped"}}), "one supported type"),
            ({key: value for key, value in COUNTED.items() if key != "additionalProperties"}, "forbid additional"),
            (dict(COUNTED, additionalProperties=string), "forbid additional"),
            (dict(COUNTED, required=["count", "count"]), "unknown or repeated"),
            (dict(COUNTED, required=["total"]), "unknown or repeated"),
            ({"type": "array"}, r"\$\[\] is not an object"),
            ({"type": "string", "items": string}, "uses items on type string"),
            ({"type": "array", "items": string, "properties": {}}, "uses properties on type array"),
            ({"type": "string", "enum": []}, "enum that does not match"),
            ({"type": "string", "enum": ["pass", 1]}, "enum that does not match"),
            ({"type": "integer", "enum": [True]}, "enum that does not match"),
            ({"type": "string", "description": 3}, "non-string description"),
        ):
            with self.subTest(message=message), self.assertRaisesRegex(LoopError, message):
                schema_error("value", schema)

    def test_violations_name_the_failing_location(self):
        review = passing_review()
        for change, expected in (
            ({"verdict": "approved"}, "$.verdict is not one of pass, fail, blocked"),
            ({"recommendation": "merge"}, "$ has unexpected properties: recommendation"),
            ({"findings": "none"}, "$.findings is not of type array"),
            ({"notes": [None]}, "$.notes[0] is not of type string"),
            ({"criteria": [{"id": "selection", "status": "pass"}]}, "$.criteria[0] is missing evidence"),
            ({"criteria": [{"id": "selection", "status": "pass", "evidence": "x", "extra": 1}]},
             "$.criteria[0] has unexpected properties: extra"),
            ({"criteria": [{"id": "selection", "status": "skipped", "evidence": "x"}]},
             "$.criteria[0].status is not one of pass, fail, unverified"),
        ):
            with self.subTest(change=change):
                self.assertEqual(schema_error(review | change, REVIEW_SCHEMA), expected)
        missing = {key: value for key, value in review.items() if key != "candidate"}
        self.assertEqual(schema_error(missing, REVIEW_SCHEMA), "$ is missing candidate")
        self.assertEqual(schema_error([review], REVIEW_SCHEMA), "$ is not of type object")
        security = passing_security()
        security["findings"] = [dict(finding(), severity="severe")]
        self.assertEqual(schema_error(security, SECURITY_REVIEW_SCHEMA),
                         "$.findings[0].severity is not one of critical, high, medium, low")
        security["findings"] = [{key: value for key, value in finding().items() if key != "sink"}]
        self.assertEqual(schema_error(security, SECURITY_REVIEW_SCHEMA), "$.findings[0] is missing sink")

    def test_scalar_types_are_exact(self):
        self.assertIsNone(schema_error({"count": 3, "ratio": 0.5, "done": False, "absent": None}, COUNTED))
        self.assertIsNone(schema_error({"count": 3, "ratio": 2}, COUNTED))
        # Python's bool is an int; JSON's is not a number.
        for value, expected in (
            ({"count": True}, "$.count is not of type integer"),
            ({"count": 3.0}, "$.count is not of type integer"),
            ({"count": "3"}, "$.count is not of type integer"),
            ({"count": 3, "ratio": True}, "$.ratio is not of type number"),
            ({"count": 3, "ratio": float("nan")}, "$.ratio is not of type number"),
            ({"count": 3, "done": 0}, "$.done is not of type boolean"),
            ({"count": 3, "absent": 0}, "$.absent is not of type null"),
        ):
            with self.subTest(value=value):
                self.assertEqual(schema_error(value, COUNTED), expected)


BUILD = {"task_id": "one", "status": "ready", "summary": "Done."}
NOT_JSON = (None, "the last result object in the final message is not valid JSON")


class TextResultTests(unittest.TestCase):
    def test_a_valid_object_after_prose_is_the_result(self):
        self.assertEqual(text_result("The patch is ready. " + json.dumps(BUILD) + " Thanks.", BUILD_SCHEMA), (BUILD, ""))

    def test_only_objects_opening_with_a_result_field_are_candidates(self):
        fenced = "```json\n" + json.dumps({"summary": "Closes }{ and {\"task_id\": 1}", "task_id": "one", "status": "ready"},
                                         indent=2) + "\n```"
        text = 'Prose with {} and {braces}, {"example": {"status": 1}} then ' + fenced + ' and {"other": 1}'
        self.assertEqual(text_result(text, BUILD_SCHEMA)[0]["summary"], 'Closes }{ and {"task_id": 1}')
        # Another object around a candidate is not decoded, so the candidate is still found.
        self.assertEqual(text_result('{"previous": ' + json.dumps(BUILD) + "}", BUILD_SCHEMA), (BUILD, ""))
        # Deeper than the decoder's recursion limit, and never closed, around the result.
        self.assertEqual(text_result('{"other": [' * 5000 + json.dumps(BUILD), BUILD_SCHEMA), (BUILD, ""))

    def test_a_corrected_verdict_that_is_not_valid_json_is_never_replaced_by_the_draft(self):
        draft = json.dumps(passing_review())
        corrected = json.dumps(dict(passing_review(), verdict="fail", findings=["docs/one.md:1 is wrong"]))
        for fault, text in (
            ("trailing comma", corrected[:-1] + ", }"),
            ("unescaped quote", corrected.replace('"docs/one.md:1 is wrong"', '"docs/one.md:1 is "wrong""')),
            ("cut off", corrected[:-30]),
        ):
            with self.subTest(fault=fault):
                self.assertEqual(text_result(f"Draft: {draft}\nCorrected: {text}", REVIEW_SCHEMA), NOT_JSON)
        # A broken one alone is reported the same way.
        self.assertEqual(text_result(draft[:-1], REVIEW_SCHEMA), NOT_JSON)

    def test_a_broken_draft_before_a_valid_result_does_not_hide_it(self):
        broken = json.dumps(BUILD)[:-1] + ", }"
        self.assertEqual(text_result(f"Draft: {broken}\nFinal: {json.dumps(BUILD)}", BUILD_SCHEMA), (BUILD, ""))

    def test_a_valid_object_inside_a_broken_one_does_not_decide(self):
        text = '{"task_id": "one", "summary": ' + json.dumps(BUILD) + ', "status": '
        self.assertEqual(text_result(text, BUILD_SCHEMA), NOT_JSON)

    def test_duplicate_keys_and_non_json_constants_are_not_valid_json(self):
        for text in ('{"task_id": "one", "status": "blocked", "status": "ready", "summary": "s"}',
                     '{"task_id": NaN, "status": "ready", "summary": "s"}',
                     json.dumps(BUILD) + ' {"task_id": "one", "status": "ready", "summary": "s", "summary": "t"}'):
            with self.subTest(text=text[-30:]):
                self.assertEqual(text_result(text, BUILD_SCHEMA), NOT_JSON)

    def test_the_search_is_bounded_however_the_text_is_built(self):
        for text, expected in (
            # Many nested, never-closed candidates: each would rescan the rest.
            (('{"verdict": [' * 5000).ljust(MAX_RESULT_TEXT), f"final message opens more than {MAX_DECODE_ATTEMPTS} result objects"),
            (('{"verdict": [' * MAX_DECODE_ATTEMPTS).ljust(MAX_RESULT_TEXT), NOT_JSON[1]),
            ('{"verdict": ' + "[" * 100_000, NOT_JSON[1]),
            ("{" * MAX_RESULT_TEXT, "final message holds no JSON object with the result's fields"),
        ):
            with self.subTest(expected=expected, size=len(text)):
                started = time.monotonic()
                self.assertEqual(text_result(text, REVIEW_SCHEMA), (None, expected))
                self.assertLess(time.monotonic() - started, 2.0)

    def test_the_last_object_carrying_result_fields_decides(self):
        stale = dict(passing_review(), candidate="c" * 40)
        final = passing_review()
        text = f"Earlier: {json.dumps(stale)}\nNow: {json.dumps(final)}\nUnrelated: {{\"example\": true}}"
        self.assertEqual(text_result(text, REVIEW_SCHEMA), (final, ""))
        # A malformed last verdict is not replaced by the valid one before it.
        withdrawn = dict(passing_review(), verdict="failed")
        value, problem = text_result(f"{json.dumps(final)}\n{json.dumps(withdrawn)}", REVIEW_SCHEMA)
        self.assertIsNone(value)
        self.assertEqual(problem, "$.verdict is not one of pass, fail, blocked")
        # So is a partial restatement that carries one of its fields.
        value, problem = text_result(f'{json.dumps(final)}\nIn short: {{"verdict": "pass"}}', REVIEW_SCHEMA)
        self.assertIsNone(value)
        self.assertIn("is missing task_id", problem)

    def test_text_without_a_result_object_says_so(self):
        for text in ("", "I could not finish the review.", '{"example": 1}', 'The verdict is {pass}.'):
            with self.subTest(text=text[:20]):
                self.assertEqual(text_result(text, REVIEW_SCHEMA),
                                 (None, "final message holds no JSON object with the result's fields"))

    def test_an_oversized_message_is_not_searched(self):
        text = json.dumps(passing_review()).ljust(MAX_RESULT_TEXT + 1)
        value, problem = text_result(text, REVIEW_SCHEMA)
        self.assertIsNone(value)
        self.assertIn("longer than", problem)


if __name__ == "__main__":
    unittest.main()
