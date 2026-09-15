"""Outcome tests using synthetic exports and retained log formats; no network required."""

import contextlib
import copy
import importlib.util
import io
import http.client
import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import urllib.error


METRICS = Path(__file__).resolve().parents[1] / "metrics.py"
FIXTURE = Path(__file__).parent / "fixtures" / "representative-runs.json"
SPEC = importlib.util.spec_from_file_location("ci_metrics", METRICS)
metrics = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(metrics)


def fixture():
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


def credential_fixture():
    return json.loads((FIXTURE.parent / "credentials.json").read_text(encoding="utf-8"))


def invoke(*arguments, input_text=None, env=None):
    return subprocess.run(
        [sys.executable, "-B", str(METRICS), *arguments],
        input=input_text,
        text=True,
        capture_output=True,
        check=False,
        cwd=METRICS.parents[2],
        env=env,
    )


class ReportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = invoke("report", "--input", str(FIXTURE), "--format", "json")
        if result.returncode:
            raise AssertionError(result.stderr)
        cls.report = json.loads(result.stdout)
        cls.runs = {run["run_id"]: run for run in cls.report["runs"]}

    def report_for(self, data):
        result = invoke(
            "report", "--input", "-", "--format", "json", input_text=json.dumps(data)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_run_and_runner_identity_are_preserved(self):
        completed = self.runs[101]
        self.assertEqual(completed["repository"], "example/gitturtle")
        self.assertEqual(completed["workflow_id"], 500)
        self.assertEqual(completed["workflow_name"], "Quality")
        self.assertEqual(completed["workflow_path"], ".github/workflows/quality.yml")
        self.assertEqual(completed["attempt"], 1)
        self.assertEqual(completed["commit"], "a" * 40)
        self.assertEqual(completed["event"], "push")
        self.assertEqual(completed["status"], "completed")
        self.assertEqual(completed["conclusion"], "success")
        linux, macos = completed["jobs"]
        self.assertEqual(linux["job_id"], 1001)
        self.assertEqual(linux["os"], "Linux")
        self.assertIsNone(linux["architecture"], "An OS label does not establish CPU architecture")
        self.assertEqual(macos["os"], "macOS")
        self.assertEqual(macos["architecture"], "ARM64")
        self.assertEqual(self.runs[102]["jobs"][0]["architecture"], "X64")

    def test_overlapping_jobs_do_not_inflate_elapsed_time(self):
        completed = self.runs[101]
        self.assertEqual(completed["queue_delay_seconds"], 20)
        self.assertEqual(completed["elapsed_seconds"], 250)
        self.assertEqual(completed["critical_path_seconds"], 230)
        self.assertEqual(completed["runner_busy_seconds"], 230)
        self.assertEqual(completed["total_runner_seconds"], 340)
        self.assertEqual(completed["known_runner_seconds"], 340)
        self.assertEqual([job["duration_seconds"] for job in completed["jobs"]], [120, 220])
        self.assertGreater(completed["total_runner_seconds"], completed["elapsed_seconds"])

    def test_skipped_job_timestamps_do_not_change_execution_measurements(self):
        data = json.loads((FIXTURE.parent / "skipped-jobs.json").read_text())
        completed = self.report_for(data)["runs"][0]
        for field in (
            "queue_delay_seconds", "elapsed_seconds", "critical_path_seconds",
            "runner_busy_seconds", "total_runner_seconds", "known_runner_seconds",
        ):
            with self.subTest(field=field):
                self.assertEqual(completed[field], self.runs[101][field])
        self.assertIsNone(completed["jobs"][0]["duration_seconds"])
        self.assertTrue(completed["jobs_complete"])
        self.assertEqual([job["duration_seconds"] for job in completed["jobs"][1:]], [120, 220])

    def test_all_skipped_jobs_do_not_establish_execution_or_zero_cost(self):
        data = json.loads((FIXTURE.parent / "skipped-jobs.json").read_text())
        data["runs"][0]["jobs"] = data["runs"][0]["jobs"][:1]
        skipped = self.report_for(data)["runs"][0]
        for field in (
            "queue_delay_seconds", "elapsed_seconds", "critical_path_seconds",
            "runner_busy_seconds", "total_runner_seconds", "known_runner_seconds",
        ):
            with self.subTest(field=field):
                self.assertIsNone(skipped[field])

    def test_unstarted_job_timestamp_does_not_establish_execution(self):
        data = json.loads((FIXTURE.parent / "skipped-jobs.json").read_text())
        run = data["runs"][0]
        run["run"].update(status="in_progress", conclusion=None)
        run["jobs"][0].update(status="queued", conclusion=None, completed_at=None)
        running = self.report_for(data)["runs"][0]
        self.assertEqual(running["queue_delay_seconds"], 20)
        self.assertIsNone(running["critical_path_seconds"])
        self.assertIsNone(running["total_runner_seconds"])
        self.assertEqual(running["known_runner_seconds"], 340)

    def test_inter_job_gap_is_in_critical_span_but_not_busy_time(self):
        data = fixture()
        data["runs"] = [data["runs"][0]]
        second = data["runs"][0]["jobs"][1]
        second["started_at"] = "2026-09-15T10:03:20Z"
        second["completed_at"] = "2026-09-15T10:04:20Z"
        second["steps"] = []
        completed = self.report_for(data)["runs"][0]
        self.assertEqual(completed["critical_path_seconds"], 240)
        self.assertEqual(completed["runner_busy_seconds"], 180)
        self.assertEqual(completed["total_runner_seconds"], 180)

    def test_cargo_logs_establish_compilation_and_harness_time(self):
        step = self.runs[101]["jobs"][0]["steps"][1]
        self.assertEqual(step["duration_seconds"], 80)
        self.assertEqual(step["cargo_compilation_seconds"], 60)
        self.assertEqual(step["test_harness_seconds"], 18)
        unavailable = self.runs[101]["jobs"][1]["steps"][1]
        self.assertEqual(unavailable["duration_seconds"], 205)
        self.assertIsNone(unavailable["cargo_compilation_seconds"])
        self.assertIsNone(unavailable["test_harness_seconds"])

    def test_failure_reports_known_cost_without_inventing_harness_finish(self):
        failed = self.runs[103]
        self.assertEqual(failed["conclusion"], "failure")
        self.assertEqual(failed["total_runner_seconds"], 90)
        test, clippy = failed["jobs"][0]["steps"]
        self.assertEqual(test["conclusion"], "failure")
        self.assertEqual(test["cargo_compilation_seconds"], 60)
        self.assertIsNone(test["test_harness_seconds"])
        self.assertIsNone(clippy["duration_seconds"])

    def test_retained_cargo_export_lines_establish_compilation_time(self):
        # These exact lines were retained from the September 15 Quality PR run
        # 34992350894. Replaying their formatting is not a fresh hosted CI sample.
        observations = (
            ("2026-09-15T16:14:24.4330433Z ^[[1m^[[92m    Finished^[[0m `test` profile [unoptimized + debuginfo] target(s) in 10m 49s", 649),
            ("2026-09-15T16:16:41.1931680Z ^[[1m^[[92m    Finished^[[0m `test` profile [unoptimized + debuginfo] target(s) in 13m 18s", 798),
        )
        for log, expected in observations:
            for representation in (log, log.replace("^[", "\x1b")):
                with self.subTest(expected=expected, caret="^[" in representation):
                    data = fixture()
                    data["runs"][0]["jobs"][0]["steps"][1]["log"] = representation
                    step = self.report_for(data)["runs"][0]["jobs"][0]["steps"][1]
                    self.assertEqual(step["cargo_compilation_seconds"], expected)
                    self.assertIsNone(step["test_harness_seconds"])

    def test_mixed_terminal_controls_do_not_disrupt_completed_marker_totals(self):
        log = (
            "\x1b[1mFinished^[[0m `test` profile target(s) in 1m 02s\n"
            "^[[92mFinished\x1b[0m `dev` profile target(s) in 3.25s\n"
            "test result: ok. 1 passed; finished in ^[[1m0.50s^[[0m\n"
            "Compiling interrupted-command v0.1.0\n"
        )
        self.assertEqual(metrics.cargo_timings(log), {
            "cargo_compilation_seconds": 65.25, "test_harness_seconds": 0.5,
        })

    def test_cancelled_step_without_end_is_unavailable(self):
        cancelled = self.runs[104]
        self.assertEqual(cancelled["conclusion"], "cancelled")
        self.assertEqual(cancelled["total_runner_seconds"], 40)
        step = cancelled["jobs"][0]["steps"][0]
        self.assertIsNone(step["duration_seconds"])
        self.assertIsNone(step["cargo_compilation_seconds"])
        self.assertIsNone(step["test_harness_seconds"])

    def test_queued_run_has_no_measured_zero_durations(self):
        queued = self.runs[105]
        self.assertEqual(queued["status"], "queued")
        self.assertEqual(queued["jobs"], [])
        for field in (
            "queue_delay_seconds", "elapsed_seconds", "critical_path_seconds",
            "runner_busy_seconds", "total_runner_seconds",
        ):
            with self.subTest(field=field):
                self.assertIsNone(queued[field])

    def test_missing_steps_and_runner_identity_remain_unavailable(self):
        missing = self.runs[106]["jobs"][0]
        self.assertEqual(missing["duration_seconds"], 40)
        self.assertEqual(missing["steps"], [])
        self.assertIsNone(missing["os"])
        self.assertIsNone(missing["architecture"])

    def test_rerun_does_not_use_original_creation_for_queue_or_elapsed(self):
        retried = self.runs[106]
        self.assertEqual(retried["attempt"], 2)
        self.assertIsNone(retried["queue_delay_seconds"])
        self.assertIsNone(retried["elapsed_seconds"])
        self.assertEqual(retried["critical_path_seconds"], 40)

    def test_run_update_timestamp_does_not_change_measured_completion(self):
        data = fixture()
        data["runs"][0]["run"]["updated_at"] = "2026-09-17T00:00:00Z"
        completed = self.report_for(data)["runs"][0]
        self.assertEqual(completed["elapsed_seconds"], 250)
        self.assertEqual(completed["critical_path_seconds"], 230)

    def test_incomplete_job_list_keeps_only_known_cost(self):
        data = fixture()
        data["runs"][0]["jobs_complete"] = False
        completed = self.report_for(data)["runs"][0]
        self.assertIsNone(completed["queue_delay_seconds"])
        self.assertIsNone(completed["total_runner_seconds"])
        self.assertEqual(completed["known_runner_seconds"], 340)

    def test_job_with_missing_end_prevents_a_complete_total(self):
        data = fixture()
        data["runs"][0]["jobs"][1]["completed_at"] = None
        completed = self.report_for(data)["runs"][0]
        self.assertIsNone(completed["total_runner_seconds"])
        self.assertEqual(completed["known_runner_seconds"], 120)
        self.assertIsNone(completed["jobs"][1]["duration_seconds"])

    def test_job_with_missing_start_prevents_an_exact_queue_delay(self):
        data = fixture()
        data["runs"][0]["jobs"][0]["started_at"] = None
        completed = self.report_for(data)["runs"][0]
        self.assertIsNone(completed["queue_delay_seconds"])
        self.assertIsNone(completed["total_runner_seconds"])
        self.assertEqual(completed["known_runner_seconds"], 220)

    def test_cache_observations_distinguish_missing_from_miss_and_zero(self):
        linux, macos = self.runs[101]["jobs"]
        restore = linux["steps"][0]["cache"]
        self.assertEqual(restore["restore_seconds"], 10)
        self.assertTrue(restore["hit"])
        self.assertEqual(restore["size_bytes"], 131072000)
        self.assertEqual(linux["steps"][2]["cache"]["save_seconds"], 10)
        self.assertFalse(macos["steps"][0]["cache"]["hit"])
        self.assertIsNone(macos["steps"][0]["cache"]["size_bytes"])
        self.assertIsNone(macos["steps"][1]["cache"]["hit"])
        data = fixture()
        data["runs"][0]["jobs"][0]["steps"][0]["cache"] = {
            "restore_seconds": 0, "hit": False, "size_bytes": 0,
        }
        zero = self.report_for(data)["runs"][0]["jobs"][0]["steps"][0]["cache"]
        self.assertEqual(zero["restore_seconds"], 0)
        self.assertEqual(zero["size_bytes"], 0)
        self.assertFalse(zero["hit"])

    def test_duplicate_push_pr_observations_remain_distinct_billed_work(self):
        duplicate = self.report["duplicate_push_pr"]
        self.assertEqual(len(duplicate), 1)
        self.assertEqual(duplicate[0]["repository"], "example/gitturtle")
        self.assertEqual(duplicate[0]["workflow_id"], 500)
        self.assertEqual(duplicate[0]["commit"], "a" * 40)
        self.assertEqual(set(duplicate[0]["run_ids"]), {101, 102})
        self.assertEqual(self.runs[101]["total_runner_seconds"], 340)
        self.assertEqual(self.runs[102]["total_runner_seconds"], 30)

    def test_same_commit_in_other_workflows_or_events_is_not_duplicate_work(self):
        data = fixture()
        # Match the observed Quality push/PR plus dynamically triggered CodeQL
        # shape, and ensure other events of the Quality workflow stay distinct.
        for run_id, workflow_id, event, name, path in (
            (201, 501, "dynamic", "PR #7", "dynamic/github-code-scanning/codeql"),
            (202, 501, "push", "CodeQL", ".github/workflows/codeql.yml"),
            (203, 500, "workflow_dispatch", "Quality", ".github/workflows/quality.yml"),
        ):
            entry = copy.deepcopy(data["runs"][0])
            entry["run"].update(id=run_id, workflow_id=workflow_id, event=event, name=name, path=path)
            for job in entry["jobs"]:
                job["run_id"] = run_id
            data["runs"].append(entry)
        report = self.report_for(data)
        self.assertEqual(report["duplicate_push_pr"], [{
            "repository": "example/gitturtle", "workflow_id": 500,
            "commit": "a" * 40, "run_ids": [101, 102],
        }])
        for run in report["runs"][-3:]:
            self.assertEqual(run["total_runner_seconds"], 340)
        self.assertEqual(report["runs"][-3]["workflow_path"], "dynamic/github-code-scanning/codeql")

    def test_shared_names_or_paths_do_not_override_distinct_workflow_ids(self):
        data = fixture()
        data["runs"][1]["run"]["workflow_id"] = 501
        self.assertEqual(self.report_for(data)["duplicate_push_pr"], [])

    def test_workflow_label_changes_do_not_hide_matching_identity(self):
        data = fixture()
        data["runs"][1]["run"].update(name="Renamed Quality", path=".github/workflows/renamed.yml")
        report = self.report_for(data)
        self.assertEqual(report["duplicate_push_pr"][0]["run_ids"], [101, 102])
        self.assertEqual(report["runs"][1]["workflow_name"], "Renamed Quality")

    def test_missing_workflow_id_does_not_claim_duplicate_work(self):
        for missing in ((0,), (1,), (0, 1)):
            for explicit_null in (False, True):
                with self.subTest(missing=missing, explicit_null=explicit_null):
                    data = fixture()
                    for index in missing:
                        run = data["runs"][index]["run"]
                        if explicit_null:
                            run["workflow_id"] = None
                        else:
                            del run["workflow_id"]
                    report = self.report_for(data)
                    self.assertEqual(report["duplicate_push_pr"], [])
                    for index in missing:
                        self.assertIsNone(report["runs"][index]["workflow_id"])
                    self.assertEqual(report["runs"][0]["total_runner_seconds"], 340)
                    self.assertEqual(report["runs"][1]["total_runner_seconds"], 30)

    def test_identical_observation_does_not_double_count_a_run(self):
        data = fixture()
        data["runs"].append(copy.deepcopy(data["runs"][0]))
        report = self.report_for(data)
        self.assertEqual(len(report["runs"]), 6)
        self.assertEqual(report["duplicate_observations"], 1)
        self.assertEqual(report["runs"][0]["total_runner_seconds"], 340)

    def test_markdown_explains_unavailable_measurements(self):
        result = invoke("report", "--input", str(FIXTURE), "--format", "markdown")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("unavailable", result.stdout.lower())
        self.assertIn("101", result.stdout)
        self.assertIn("340", result.stdout)
        self.assertIn("230", result.stdout)
        self.assertIn("Workflow: Quality; ID: 500; path: .github/workflows/quality.yml", result.stdout)


class RejectionTests(unittest.TestCase):
    def assert_rejected(self, data, expected=None):
        result = invoke(
            "report", "--input", "-", "--format", "json", input_text=json.dumps(data)
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertNotIn("Traceback", result.stderr)
        if expected is not None:
            self.assertIn(expected, result.stderr.lower())
        return result

    def test_conflicting_duplicate_cannot_silently_choose_a_snapshot(self):
        data = fixture()
        conflicting = copy.deepcopy(data["runs"][0])
        conflicting["run"]["conclusion"] = "failure"
        data["runs"].append(conflicting)
        self.assert_rejected(data, "duplicate")

    def test_invalid_workflow_identity_is_rejected(self):
        for workflow_id in (False, 0, -1, "500", 500.5, {}):
            with self.subTest(workflow_id=workflow_id):
                data = fixture()
                data["runs"][0]["run"]["workflow_id"] = workflow_id
                self.assert_rejected(data, "workflow id")

    def test_conflicting_workflow_identity_in_duplicate_snapshot_is_rejected(self):
        data = fixture()
        conflicting = copy.deepcopy(data["runs"][0])
        conflicting["run"]["workflow_id"] = 501
        data["runs"].append(conflicting)
        self.assert_rejected(data, "duplicate")

    def test_unsupported_export_version_is_rejected(self):
        data = fixture()
        data["schema_version"] = 99
        self.assert_rejected(data, "schema")

    def test_invalid_json_reports_a_concise_error(self):
        result = invoke("report", "--input", "-", input_text="{broken-json")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertNotIn("Traceback", result.stderr)

    def test_invalid_unicode_log_is_rejected_without_traceback(self):
        data = fixture()
        data["runs"][0]["jobs"][0]["steps"][0]["log"] = "\ud800"
        self.assert_rejected(data, "valid unicode")

    def test_invalid_unicode_display_field_is_safe_in_both_formats(self):
        data = fixture()
        data["runs"][0]["jobs"][0]["name"] = "job\ud800name"
        for output_format in ("json", "markdown"):
            with self.subTest(output_format=output_format):
                result = invoke("report", "--input", "-", "--format", output_format,
                                input_text=json.dumps(data))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("job?name", result.stdout)

    def test_invalid_cache_numbers_are_not_reported_as_measurements(self):
        for value in (-1, "unknown", True):
            with self.subTest(value=value):
                data = fixture()
                data["runs"][0]["jobs"][0]["steps"][0]["cache"]["size_bytes"] = value
                self.assert_rejected(data)

    def test_numeric_overflow_is_rejected_without_traceback_or_infinity(self):
        data = fixture()
        data["runs"][0]["jobs"][0]["steps"][0]["cache"]["restore_seconds"] = 10 ** 500
        with self.assertRaises(metrics.MetricsError):
            metrics.make_report(data)
        result = self.assert_rejected(data, "finite")
        self.assertNotIn("Infinity", result.stdout + result.stderr)

    def test_extreme_cargo_markers_are_rejected_without_nonstandard_json(self):
        huge = "9" * 500
        for log in (
            f"Finished `test` profile target(s) in {huge}s\n",
            f"Finished `test` profile target(s) in {huge}m 01s\n",
            f"test result: ok. 1 passed; finished in {huge}s\n",
        ):
            with self.subTest(marker=log[:30]):
                with self.assertRaises(metrics.MetricsError):
                    metrics.cargo_timings(log)
                data = fixture()
                data["runs"][0]["jobs"][0]["steps"][0]["log"] = log
                result = self.assert_rejected(data, "finite")
                self.assertNotIn("Infinity", result.stdout + result.stderr)

    def test_log_payload_and_private_metadata_are_not_echoed(self):
        data = fixture()
        job = data["runs"][0]["jobs"][0]
        job["runner_name"] = "private-runner-secret"
        job["steps"][1]["log"] += (
            "Authorization: Bearer SECRET_TOKEN_MARKER\n"
            "/home/private-person/work/internal-project\n"
        )
        result = invoke(
            "report", "--input", "-", "--format", "json", input_text=json.dumps(data)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        for private in ("SECRET_TOKEN_MARKER", "private-person", "private-runner-secret"):
            self.assertNotIn(private, result.stdout + result.stderr)

    def test_help_runs_without_repository_or_credentials(self):
        for arguments in (("--help",), ("report", "--help"), ("measure", "--help"), ("summary", "--help")):
            with self.subTest(arguments=arguments):
                result = invoke(*arguments)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("usage:", result.stdout.lower())


class BoundedCollectionTests(unittest.TestCase):
    def test_collector_requires_explicit_repository_and_run(self):
        for arguments in (
            ("report",), ("report", "--repo", "example/gitturtle"),
            ("report", "--repo", "../private", "--run-id", "101"),
            ("report", "--repo", "example/gitturtle", "--run-id", "0"),
        ):
            with self.subTest(arguments=arguments):
                result = invoke(*arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertNotIn("Traceback", result.stderr)

    def test_export_size_is_bounded_before_json_parsing(self):
        with self.assertRaisesRegex(metrics.MetricsError, "byte limit"):
            metrics.read_json(io.BytesIO(b" " * 33), limit=32)

    def test_run_job_step_and_log_limits_refuse_partial_results(self):
        base = fixture()
        too_many_runs = {"schema_version": 1, "runs": [base["runs"][0]] * (metrics.MAX_RUNS + 1)}
        with self.assertRaises(metrics.MetricsError):
            metrics.make_report(too_many_runs)
        for field, count in (("jobs", metrics.MAX_JOBS + 1), ("steps", metrics.MAX_STEPS + 1)):
            data = fixture()
            entry = data["runs"][0] if field == "jobs" else data["runs"][0]["jobs"][0]
            entry[field] = [entry[field][0]] * count
            with self.subTest(field=field), self.assertRaises(metrics.MetricsError):
                metrics.make_report(data)
        data = fixture()
        data["runs"][0]["jobs"][0]["steps"][0]["log"] = "x" * (metrics.MAX_LOG + 1)
        with self.assertRaisesRegex(metrics.MetricsError, "step log"):
            metrics.make_report(data)

    def test_collection_requests_only_named_attempt_with_bounded_pages(self):
        entry = fixture()["runs"][5]
        jobs = [dict(entry["jobs"][0], id=2000 + index) for index in range(101)]
        with mock.patch.object(metrics, "GitHub") as api:
            api.return_value.get.side_effect = [
                entry["run"], {"total_count": 101, "jobs": jobs[:100]},
                {"total_count": 101, "jobs": jobs[100:]},
            ]
            export = metrics.collect("example/gitturtle", [106, 106])
        self.assertEqual(api.return_value.get.call_args_list, [
            mock.call("/repos/example/gitturtle/actions/runs/106"),
            mock.call("/repos/example/gitturtle/actions/runs/106/attempts/2/jobs?per_page=100&page=1"),
            mock.call("/repos/example/gitturtle/actions/runs/106/attempts/2/jobs?per_page=100&page=2"),
        ])
        self.assertEqual(len(export["runs"]), 1)
        self.assertTrue(export["runs"][0]["jobs_complete"])
        self.assertEqual(len(metrics.make_report(export)["runs"][0]["jobs"]), 101)

    def test_job_pagination_budget_and_changing_pages_fail_clearly(self):
        entry = fixture()["runs"][0]
        for page in (
            {"total_count": 501, "jobs": []},
            {"total_count": 2, "jobs": []},
            {"total_count": 0, "jobs": [entry["jobs"][0]]},
        ):
            with self.subTest(page=page["total_count"]), mock.patch.object(metrics, "GitHub") as api:
                api.return_value.get.side_effect = [entry["run"], page]
                with self.assertRaises(metrics.MetricsError):
                    metrics.collect("example/gitturtle", [101])
                self.assertEqual(api.return_value.get.call_count, 2)

    def test_pagination_never_exceeds_five_job_pages(self):
        entry = fixture()["runs"][0]
        with mock.patch.object(metrics, "GitHub") as api:
            api.return_value.get.side_effect = [entry["run"]] + [
                {"total_count": 500, "jobs": [entry["jobs"][0]]}
            ] * 5
            with self.assertRaisesRegex(metrics.MetricsError, "pagination limit"):
                metrics.collect("example/gitturtle", [101])
            self.assertEqual(api.return_value.get.call_count, 6)

    def test_api_request_is_get_to_fixed_host_with_timeout(self):
        api = metrics.GitHub()
        api.opener = mock.Mock()
        api.opener.open.return_value = io.BytesIO(b'{"id":101}')
        with mock.patch.dict(os.environ, {"GH_TOKEN": "private-token-marker"}):
            self.assertEqual(api.get("/repos/example/gitturtle/actions/runs/101"), {"id": 101})
        request = api.opener.open.call_args.args[0]
        self.assertEqual(request.get_method(), "GET")
        self.assertEqual(request.full_url, "https://api.github.com/repos/example/gitturtle/actions/runs/101")
        self.assertEqual(api.opener.open.call_args.kwargs, {"timeout": 20})

    def test_http_failure_does_not_expose_response_body_or_token(self):
        api = metrics.GitHub()
        api.opener = mock.Mock()
        api.opener.open.side_effect = urllib.error.HTTPError(
            "https://api.github.com/private-token-marker", 403, "private-response-marker", {},
            io.BytesIO(b"private-response-marker"),
        )
        with self.assertRaises(metrics.MetricsError) as caught:
            api.get("/repos/example/gitturtle/actions/runs/101")
        message = str(caught.exception)
        self.assertIn("HTTP 403", message)
        self.assertNotIn("private-", message)
        with mock.patch.object(metrics, "collect", side_effect=caught.exception):
            stdout, stderr = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                code = metrics.main(["report", "--repo", "example/gitturtle", "--run-id", "101"])
        self.assertEqual(code, 2)
        self.assertEqual(stdout.getvalue(), "")
        self.assertIn("HTTP 403", stderr.getvalue())
        self.assertNotIn("Traceback", stderr.getvalue())

    def test_network_failure_and_oversize_response_have_specific_errors(self):
        api = metrics.GitHub()
        api.opener = mock.Mock()
        api.opener.open.side_effect = TimeoutError("private-network-marker")
        with self.assertRaisesRegex(metrics.MetricsError, "network or timeout"):
            api.get("/repos/example/gitturtle/actions/runs/101")
        api.opener.open.side_effect = None
        api.remaining = 10
        api.opener.open.return_value = io.BytesIO(b" " * 11)
        with self.assertRaisesRegex(metrics.MetricsError, "byte budget"):
            api.get("/repos/example/gitturtle/actions/runs/101")

    def test_http_transport_failures_emit_no_report_or_private_exception(self):
        for failure in (
            http.client.IncompleteRead(b"PRIVATE_PARTIAL_RESPONSE", 100),
            http.client.BadStatusLine("PRIVATE_STATUS_LINE"),
            http.client.RemoteDisconnected("PRIVATE_DISCONNECT_DETAIL"),
            http.client.HTTPException("PRIVATE_TRANSPORT_DETAIL"),
        ):
            # Exercise both opening the response and reading a truncated body
            # through the real collector/CLI error boundary.
            for phase in ("open", "read"):
                with self.subTest(failure=type(failure).__name__, phase=phase):
                    opener = mock.Mock()
                    if phase == "open":
                        opener.open.side_effect = failure
                    else:
                        response = mock.MagicMock()
                        response.__enter__.return_value.read.side_effect = failure
                        opener.open.return_value = response
                    stdout, stderr = io.StringIO(), io.StringIO()
                    with mock.patch.object(metrics.urllib.request, "build_opener", return_value=opener):
                        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                            code = metrics.main(["report", "--repo", "example/gitturtle", "--run-id", "101"])
                    self.assertEqual(code, 2)
                    self.assertEqual(stdout.getvalue(), "")
                    self.assertIn("network or timeout", stderr.getvalue())
                    self.assertNotIn("PRIVATE_", stderr.getvalue())
                    self.assertNotIn("Traceback", stderr.getvalue())
                    self.assertEqual(opener.open.call_count, 1, "A failed query is not retried")

    def test_api_redirect_cannot_forward_authentication_to_another_host(self):
        handler = metrics.NoRedirect()
        request = urllib.request.Request("https://api.github.com/repos/example/gitturtle")
        with self.assertRaisesRegex(metrics.MetricsError, "redirect refused"):
            handler.redirect_request(request, None, 302, "Found", {}, "https://other.example/private")


class CommandDiagnosticsTests(unittest.TestCase):
    def test_output_failure_terminates_and_reaps_owned_command(self):
        owned = []
        real_popen = subprocess.Popen

        def launch(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            owned.append(process)
            return process

        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            args = metrics.argparse.Namespace(
                name="broken-output", directory=directory,
                command=[sys.executable, "-c", "import time; print('READY', flush=True); time.sleep(60)"],
            )
            try:
                with mock.patch.object(metrics.subprocess, "Popen", side_effect=launch):
                    with mock.patch("builtins.print", side_effect=BrokenPipeError("closed output")):
                        with self.assertRaises(BrokenPipeError):
                            metrics.measure(args)
                self.assertEqual(len(owned), 1)
                self.assertIsNotNone(owned[0].returncode, "The wrapper must reap its child before returning")
                self.assertFalse((Path(directory) / "broken-output.json").exists())
            finally:
                for process in owned:
                    if process.poll() is None:
                        process.kill()
                        process.wait()

    def test_quoted_and_compound_credentials_are_completely_redacted(self):
        for case in credential_fixture():
            with self.subTest(input=case["input"]):
                clean = metrics.sanitize(case["input"] + "\nNEXT_DIAGNOSTIC")
                self.assertIn("=[redacted]", clean)
                self.assertTrue(clean.endswith("\nNEXT_DIAGNOSTIC"))
                for private in case["private"]:
                    self.assertNotIn(private, clean)
        self.assertEqual(metrics.sanitize('password="private password value"'), "password=[redacted]")

    def test_credential_fixtures_are_redacted_from_console_and_retained_log(self):
        cases = credential_fixture()
        payload = "\n".join(case["input"] for case in cases) + "\nUSEFUL_FAILURE_DIAGNOSTIC"
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            result = invoke(
                "measure", "--name", "credentials", "--directory", directory, "--",
                sys.executable, "-c", f"import sys; print({payload!r}); sys.exit(7)",
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            log = (Path(directory) / "credentials.log").read_text()
            for output in (result.stdout, log):
                self.assertIn("USEFUL_FAILURE_DIAGNOSTIC", output)
                for case in cases:
                    for private in case["private"]:
                        self.assertNotIn(private, output)

    def test_absolute_paths_with_spaces_do_not_leave_private_suffixes(self):
        for path in (
            "/home/Private Person/project/src/a.rs",
            "/Users/Private Person/project/src/a.rs",
            r"C:\Users\Private Person\project\src\a.rs",
            r"C:\\Users\\Private Person\\project\\src\\a.rs",
        ):
            for quote in ("", '"', "'", "`"):
                with self.subTest(path=path, quote=quote):
                    self.assertEqual(
                        metrics.sanitize(f"file: {quote}{path}{quote}\nNEXT_DIAGNOSTIC"),
                        f"file: {quote}[path]{quote}\nNEXT_DIAGNOSTIC",
                    )

    def test_complete_authorization_values_are_redacted(self):
        for header in ("Authorization", "Proxy-Authorization", "pRoXy-AuThOrIzAtIoN"):
            for value in (
                "Basic Zml4dHVyZTpmaXh0dXJl",
                "Bearer fixture-access-value",
                'Digest username="fixture-user", response="fixture-response", nonce="fixture-nonce"',
                "CustomScheme fixture-opaque-value",
            ):
                for separator in (": ", "=", '\": "'):
                    with self.subTest(header=header, value=value, separator=separator):
                        clean = metrics.sanitize(f"{header}{separator}{value}\nNEXT_DIAGNOSTIC")
                        self.assertEqual(clean, f"{header}=[redacted]\nNEXT_DIAGNOSTIC")

    def test_caret_control_normalization_keeps_diagnostics_redacted(self):
        text = (
            "^[[1mFinished^[[0m `test` profile target(s) in 10m 49s\n"
            "Authori^[[92mzation: Bearer SECRET_MARKER\n"
            "ghp_fixture^[[0mCredentialMarker1234\n"
            "opaque-token-^[[1mfixture-value\n"
            "https:^[[0m//private.example/project\n"
            "/home/^[[0mprivate-person/work/project\n"
            "^[[0m::error::untrusted workflow command\n"
        )
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            result = invoke(
                "measure", "--name", "exported", "--directory", directory, "--",
                sys.executable, "-c", "import sys; sys.stdout.write(" + repr(text) + ")",
                env=dict(os.environ, GH_TOKEN="opaque-token-fixture-value"),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            data = json.loads((Path(directory) / "exported.json").read_text())
            self.assertEqual(data["cargo_compilation_seconds"], 649)
            log = (Path(directory) / "exported.log").read_text()
            for private in ("SECRET_MARKER", "CredentialMarker", "opaque-token", "private.example", "private-person", "^[["):
                self.assertNotIn(private, result.stdout + result.stderr + log)
            for redaction in ("[redacted]", "[credential]", "[url]", "[path]"):
                self.assertIn(redaction, log)
            self.assertIn("[ci] ::error::", result.stdout)
            self.assertFalse(any(line.startswith("::") for line in result.stdout.splitlines()))

    def test_explicit_bash_heredoc_inherits_stdin_and_preserves_exit(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            result = invoke(
                "measure", "--name", "package", "--directory", directory, "--", "bash", "-e",
                input_text="cat <<'PACKAGE_INPUT'\nHEREDOC_MARKER\nPACKAGE_INPUT\nexit 9\n",
            )
            self.assertEqual(result.returncode, 9, result.stderr)
            self.assertIn("HEREDOC_MARKER", result.stdout)
            self.assertIn("HEREDOC_MARKER", (Path(directory) / "package.log").read_text())
            data = json.loads((Path(directory) / "package.json").read_text())
            self.assertEqual(data["exit_code"], 9)

    def test_same_name_retry_cannot_retain_old_success_if_command_cannot_start(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            root = Path(directory)
            result = invoke(
                "measure", "--name", "tests", "--directory", directory, "--",
                sys.executable, "-c", "print('PREVIOUS_SUCCESS')",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads((root / "tests.json").read_text())["exit_code"], 0)
            (root / "tests-timing.txt").write_text("previous build timing fixture")
            (root / "other.log").write_text("unrelated diagnostic")
            failed = invoke(
                "measure", "--name", "tests", "--directory", directory, "--",
                str(root / "missing-command"),
            )
            self.assertEqual(failed.returncode, 2)
            self.assertIn("could not start", failed.stderr)
            self.assertNotIn(str(root), failed.stderr)
            for name in ("tests.json", "tests.log", "tests-timing.txt"):
                self.assertFalse((root / name).exists(), name)
            self.assertEqual((root / "other.log").read_text(), "unrelated diagnostic")

    def test_invalid_cargo_marker_does_not_abandon_running_command(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            result = invoke(
                "measure", "--name", "invalid-marker", "--directory", directory, "--",
                sys.executable, "-c",
                "import sys; print('Finished `test` profile target(s) in ' + '9' * 500 + 's'); "
                "print('CHILD_COMPLETED'); sys.exit(7)",
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertIn("CHILD_COMPLETED", result.stdout)
            raw = (Path(directory) / "invalid-marker.json").read_text()
            data = json.loads(raw)
            self.assertEqual(data["exit_code"], 7)
            self.assertIsNone(data["cargo_compilation_seconds"])
            self.assertIsNone(data["test_harness_seconds"])
            self.assertNotIn("Infinity", raw)

    def test_failing_command_preserves_exit_and_sanitized_bounded_diagnostics(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            command = (
                "import sys; "
                "print('::error::untrusted workflow command'); "
                "print('Authorization: Bearer SECRET_MARKER'); "
                "print('Authorization: Basic Zml4dHVyZTpmaXh0dXJl'); "
                "print('Proxy-Authorization: Basic cHJveHktZml4dHVyZTpzZWNyZXQ='); "
                "print('{\"token\": \"JSON_SECRET_MARKER\"}'); "
                "print('opaque-token-fixture-value'); "
                "print('/home/private-person/work/project/file.rs'); "
                "print('/home/Private Person/project/src/a.rs'); "
                "print('ghp_fixtureCredentialMarker1234'); "
                "print('Finished `test` profile target(s) in 2m 03.4s'); "
                "print('test result: ok. 2 passed; finished in 0.25s'); "
                "print('test result: FAILED. 1 failed; finished in 0.50s'); "
                "sys.exit(7)"
            )
            result = invoke(
                "measure", "--name", "tests", "--directory", directory, "--",
                sys.executable, "-c", command,
                env=dict(os.environ, GH_TOKEN="opaque-token-fixture-value"),
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            data = json.loads((Path(directory) / "tests.json").read_text())
            log = (Path(directory) / "tests.log").read_text()
            self.assertEqual(data["exit_code"], 7)
            self.assertGreaterEqual(data["elapsed_seconds"], 0)
            self.assertEqual(data["cargo_compilation_seconds"], 123.4)
            self.assertEqual(data["test_harness_seconds"], 0.75)
            self.assertIsNone(data["build_timing_artifact"])
            self.assertFalse(data["log_truncated"])
            self.assertIn("[ci] ::error::", result.stdout)
            self.assertFalse(any(line.startswith("::") for line in result.stdout.splitlines()))
            for private in (
                "SECRET_MARKER", "JSON_SECRET_MARKER", "opaque-token-fixture-value",
                "private-person", "ghp_fixtureCredentialMarker1234",
                "Private Person", "Person/project",
                "Zml4dHVyZTpmaXh0dXJl", "cHJveHktZml4dHVyZTpzZWNyZXQ=",
            ):
                self.assertNotIn(private, result.stdout + result.stderr + log)

    def test_long_lines_are_dropped_and_retained_log_has_a_tail_bound(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            result = invoke(
                "measure", "--name", "verbose", "--directory", directory, "--",
                sys.executable, "-c",
                "print('X' * 20000 + 'ghp_secretTailMarker'); "
                "[print('diagnostic-line-' + 'x' * 1000) for _ in range(1200)]; print('FINAL_MARKER')",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            log_path = Path(directory) / "verbose.log"
            log = log_path.read_text()
            data = json.loads((Path(directory) / "verbose.json").read_text())
            self.assertTrue(data["log_truncated"])
            self.assertLessEqual(log_path.stat().st_size, metrics.MAX_LOG + 64)
            self.assertIn("FINAL_MARKER", log)
            self.assertIn("overlong diagnostic line omitted", result.stdout)
            self.assertNotIn("secretTailMarker", result.stdout + log)

    def test_summary_reports_runner_identity_and_unavailable_cache(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            env = dict(os.environ, GITHUB_REPOSITORY="example/gitturtle", GITHUB_RUN_ID="101",
                       GITHUB_RUN_ATTEMPT="1", GITHUB_SHA="a" * 40, GITHUB_EVENT_NAME="push",
                       RUNNER_OS="Linux", RUNNER_ARCH="X64", ImageOS="ubuntu24", ImageVersion="fixture-image")
            result = invoke("summary", "--directory", directory, env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        for value in ("example/gitturtle", "101", "Linux", "X64", "fixture-image"):
            self.assertIn(value, result.stdout)
        self.assertIn("Command measurements unavailable", result.stdout)
        self.assertIn("Cache restore duration: unavailable", result.stdout)
        self.assertIn("save duration: unavailable", result.stdout)
        self.assertIn("3-day retention", result.stdout)

    def test_only_fresh_cargo_timing_is_retained_as_sanitized_text(self):
        with tempfile.TemporaryDirectory(dir=METRICS.parent / "tests") as directory:
            root = Path(directory)
            cargo = root / "cargo"
            timing_payload = (
                '<html>BUILD_MARKER /home/private-builder/work/crate token=SECRET_HTML_MARKER\n'
                'Authorization: Basic Zml4dHVyZTpmaXh0dXJl\n'
                'Proxy-Authorization: Basic cHJveHktZml4dHVyZTpzZWNyZXQ=\n'
                + "\n".join(case["input"] for case in credential_fixture()) + '\n</html>'
            )
            cargo.write_text(
                "#!/usr/bin/env python3\n"
                "import os, pathlib, sys\n"
                "if '--fresh' in sys.argv:\n"
                "    p = pathlib.Path(os.environ['CARGO_TARGET_DIR']) / 'cargo-timings/cargo-timing.html'\n"
                "    p.parent.mkdir(parents=True, exist_ok=True)\n"
                f"    p.write_text({timing_payload!r})\n"
                "print('Finished `test` profile target(s) in 0.02s')\n"
            )
            cargo.chmod(0o755)
            env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"))
            result = invoke(
                "measure", "--name", "fresh", "--directory", str(root / "diagnostics"), "--",
                str(cargo), "test", "--timings", "--fresh", env=env,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            data = json.loads((root / "diagnostics/fresh.json").read_text())
            self.assertTrue(data["build_timing_artifact"].endswith(".txt"))
            timing = (root / "diagnostics" / data["build_timing_artifact"]).read_text()
            self.assertIn("BUILD_MARKER", timing)
            self.assertNotIn("private-builder", timing)
            self.assertNotIn("SECRET_HTML_MARKER", timing)
            self.assertNotIn("Zml4dHVyZTpmaXh0dXJl", timing)
            self.assertNotIn("cHJveHktZml4dHVyZTpzZWNyZXQ=", timing)
            for case in credential_fixture():
                for private in case["private"]:
                    self.assertNotIn(private, timing)
            os.utime(root / "target/cargo-timings/cargo-timing.html", (1, 1))
            stale = invoke(
                "measure", "--name", "stale", "--directory", str(root / "diagnostics"), "--",
                str(cargo), "test", "--timings", env=env,
            )
            self.assertEqual(stale.returncode, 0, stale.stderr)
            data = json.loads((root / "diagnostics/stale.json").read_text())
            self.assertIsNone(data["build_timing_artifact"])
            self.assertFalse((root / "diagnostics/stale-timing.txt").exists())

    def test_markdown_escapes_untrusted_job_and_step_names(self):
        data = fixture()
        data["runs"][0]["run"]["name"] = "<script>workflow</script>| injected"
        data["runs"][0]["run"]["path"] = "^[[1m/home/private-workflow/file.yml^[[0m"
        data["runs"][0]["jobs"][0]["name"] = "<script>alert(1)</script>| injected"
        data["runs"][0]["jobs"][0]["steps"][0]["name"] = "`injected` | column"
        report = metrics.markdown(metrics.make_report(data))
        self.assertNotIn("<script>", report)
        self.assertIn("&lt;script&gt;", report)
        self.assertIn("&#124;", report)
        self.assertIn("&#96;injected&#96;", report)
        self.assertNotIn("private-workflow", report)
        self.assertNotIn("^[[", report)


if __name__ == "__main__":
    unittest.main()
