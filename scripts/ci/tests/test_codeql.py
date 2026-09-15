"""Behavioral fixtures for CodeQL measurements; no hosted scan is simulated."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

CI = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(CI))
spec = importlib.util.spec_from_file_location('codeql_evidence', CI / 'codeql.py')
codeql = importlib.util.module_from_spec(spec)
spec.loader.exec_module(codeql)


class CommandTests(unittest.TestCase):
    def test_command_output_limit_is_enforced_during_capture(self):
        with self.assertRaises(codeql.EvidenceError):
            codeql.command([sys.executable, '-c', 'print("x" * 8192)'], limit=1024)

    def test_failed_command_is_not_a_successful_empty_context(self):
        with self.assertRaises(codeql.EvidenceError):
            codeql.command([sys.executable, '-c', 'raise SystemExit(7)'])

    def test_command_timeout_terminates_owned_process(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            codeql.command([sys.executable, '-c', 'import time; time.sleep(30)'], timeout=0.05)

    def test_successful_command_preserves_actual_output(self):
        self.assertEqual(codeql.command([sys.executable, '-c', 'print("actual")']), 'actual')


class PolicyTests(unittest.TestCase):
    def check_policy(self, event, ref, **values):
        return codeql.policy({'GITHUB_EVENT_NAME': event, 'GITHUB_REF': ref, **values})

    def test_pr_uploads_keep_restricted_token_path_and_never_use_cache(self):
        for ref in ('refs/pull/9/merge', 'refs/heads/main'):
            self.assertEqual(self.check_policy('pull_request', ref),
                             {'upload': 'always', 'restore': 'false', 'save': 'false'})

    def test_main_push_and_schedule_publish_and_cache(self):
        for event in ('push', 'schedule'):
            self.assertEqual(self.check_policy(event, 'refs/heads/main'),
                             {'upload': 'always', 'restore': 'true', 'save': 'true'})

    def test_manual_default_is_rehearsal(self):
        self.assertEqual(self.check_policy('workflow_dispatch', 'refs/heads/topic'),
                         {'upload': 'never', 'restore': 'false', 'save': 'false'})

    def test_manual_main_cold_scan_can_seed_cache(self):
        self.assertEqual(self.check_policy('workflow_dispatch', 'refs/heads/main',
                                          CODEQL_COLD_REQUESTED='true'),
                         {'upload': 'never', 'restore': 'false', 'save': 'true'})

    def test_debug_measurements_do_not_seed_cache(self):
        result = self.check_policy('workflow_dispatch', 'refs/heads/main',
                                   CODEQL_DIAGNOSTICS_REQUESTED='true', CODEQL_PUBLISH_REQUESTED='true')
        self.assertEqual(result, {'upload': 'always', 'restore': 'true', 'save': 'false'})

    def test_unknown_events_fail_closed(self):
        for event in ('pull_request_target', 'workflow_run', '', None):
            self.assertEqual(self.check_policy(event, 'refs/heads/main'),
                             {'upload': 'never', 'restore': 'false', 'save': 'false'})

    def test_workflow_output_rejects_command_injection(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'out'
            with self.assertRaises(codeql.EvidenceError):
                codeql.write_outputs(out, {'upload': 'always\nsecret=bad'})
            self.assertEqual(out.read_text(), '')

    def test_actual_policy_cli_defaults_do_not_upload_or_cache(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / 'output'
            result = subprocess.run([sys.executable, str(CI / 'codeql.py'), 'policy', '--output', str(out)],
                                    env={'PATH': os.environ['PATH']}, capture_output=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(out.read_text(), 'upload=never\nrestore=false\nsave=false\n')


class FootprintTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'target'
        self.root.mkdir()

    def test_measures_payload_and_counts_hardlinks_conservatively(self):
        (self.root / 'deps').mkdir()
        (self.root / 'deps' / 'lib.rlib').write_bytes(b'abc')
        os.link(self.root / 'deps' / 'lib.rlib', self.root / 'lib.rlib')
        report = codeql.footprint(self.root)
        self.assertTrue(report['complete'])
        self.assertEqual(report['uncompressed_bytes'], 6)
        self.assertEqual(report['entries'], 3)

    def test_byte_cap_aborts_without_removing_build_outputs(self):
        file = self.root / 'object'
        file.write_bytes(b'1234')
        self.assertFalse(codeql.footprint(self.root, max_bytes=3)['complete'])
        self.assertEqual(file.read_bytes(), b'1234')

    def test_entry_cap_bounds_walk(self):
        for name in ('a', 'b', 'c'):
            (self.root / name).touch()
        result = codeql.footprint(self.root, max_entries=2)
        self.assertFalse(result['complete'])
        self.assertEqual(result['reason'], 'entry or time limit exceeded')

    def test_symlink_payload_is_never_followed(self):
        outside = self.root.parent / 'private'
        outside.write_bytes(b'secret')
        (self.root / 'link').symlink_to(outside)
        result = codeql.footprint(self.root)
        self.assertFalse(result['complete'])
        self.assertEqual(result['uncompressed_bytes'], 0)
        self.assertEqual(outside.read_bytes(), b'secret')

    def test_missing_and_symlink_roots_are_unavailable(self):
        self.assertFalse(codeql.footprint(self.root / 'absent')['complete'])
        alias = self.root.parent / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        self.assertFalse(codeql.footprint(alias)['complete'])

    def test_failed_analysis_never_saves_and_skipped_restore_is_unknown(self):
        (self.root / 'object').write_bytes(b'1234')
        out = self.root.parent / 'outputs'
        directory = self.root.parent / 'evidence'
        with patch.dict(os.environ, {
            'CODEQL_EXTRACTOR_RUST_OPTION_CARGO_TARGET_DIR': str(self.root),
            'CODEQL_ANALYSIS_OUTCOME': 'failure', 'CODEQL_CACHE_MATCH': ''
        }):
            codeql.record_footprint(directory, out)
        self.assertEqual(out.read_text(), 'save-eligible=false\n')
        record = json.loads((directory / 'cache-candidate.json').read_text())
        self.assertIsNone(record['exact_cache_hit'])
        self.assertIsNone(record['save_seconds'])
        self.assertIsNone(record['compressed_bytes'])


class ContextTests(unittest.TestCase):
    def test_compatibility_changes_cannot_restore_an_old_key(self):
        initial = {'tree': 'a' * 40, 'codeql_version': '2.27.0', 'arch': 'X64',
                   'rustc_sha256': 'rustc', 'native_packages_sha256': 'sdk', 'image_version': 'image'}
        first = codeql.cache_key(initial)
        for field in initial:
            changed = {**initial, field: initial[field] + '-changed'}
            self.assertNotEqual(first, codeql.cache_key(changed), field)
        self.assertEqual(first, codeql.cache_key(dict(reversed(list(initial.items())))))

    def test_context_records_actual_schema_toolchain_and_no_raw_environment(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            extractor = root / 'rust'
            extractor.mkdir()
            (extractor / 'codeql-extractor.yml').write_text('name: rust\noptions:\n  cargo_target_dir:\n    type: string\n')
            replies = [json.dumps({'version': '2.27.0'}), json.dumps(str(extractor)), '',
                       'a' * 40, 'rustc 1.98.0\nhost: x86_64-unknown-linux-gnu', 'libc6=2.39']
            env = {'CODEQL_BINARY': '/tool/codeql', 'RUNNER_TEMP': tmp,
                   'CODEQL_EXTRACTOR_RUST_OPTION_CARGO_TARGET_DIR': str(root / 'gitturtle-codeql-target'),
                   'RUNNER_OS': 'Linux', 'RUNNER_ARCH': 'X64', 'ImageOS': 'ubuntu24', 'ImageVersion': '20260914',
                   'RUSTFLAGS': '--cfg secret_value', 'GITHUB_EVENT_NAME': 'pull_request'}
            with patch.dict(os.environ, env), patch.object(codeql, 'command', side_effect=replies):
                codeql.context(root / 'evidence', root / 'output')
            text = (root / 'evidence' / 'context.json').read_text()
            self.assertNotIn('secret_value', text)
            self.assertNotIn(str(extractor), text)
            record = json.loads(text)
            self.assertEqual(record['identity']['tree'], 'a' * 40)
            self.assertEqual(record['policy']['restore'], 'false')
            self.assertIn(record['cache_key'], (root / 'output').read_text())

    def test_unsupported_bundle_fails_before_cache_use(self):
        with patch.dict(os.environ, {'CODEQL_BINARY': '/tool/codeql'}), \
                patch.object(codeql, 'command', return_value='{"version":"2.28.0"}'):
            with self.assertRaises(codeql.EvidenceError):
                codeql.context(Path('unused'), Path('unused'))


class CaptureTests(unittest.TestCase):
    def test_result_paths_remain_useful_without_private_roots_or_credentials(self):
        value = {'rows': [['file:///checkout/crates/app/x.rs', '/checkout/vendor/kit/x.rs',
                           'Authorization: Bearer test-credential', '/private/user/notes']]}
        cleaned = codeql.clean_result(value, '/checkout')
        self.assertEqual(cleaned['rows'][0][:2], ['crates/app/x.rs', 'vendor/kit/x.rs'])
        self.assertNotIn('test-credential', json.dumps(cleaned))
        self.assertNotIn('/private/user', json.dumps(cleaned))

    def test_missing_database_is_recorded_as_unavailable(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.dict(os.environ, {'RUNNER_TEMP': tmp, 'CODEQL_BINARY': '/no/tool'}):
                codeql.capture_results(Path(tmp) / 'evidence')
            data = json.loads((Path(tmp) / 'evidence/rust-results-index.json').read_text())
            self.assertEqual(len(data['results']), 6)
            self.assertTrue(all(value['status'] == 'unavailable' for value in data['results'].values()))

    def test_capture_decodes_available_results_and_preserves_missing_ones(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            query = root / 'gitturtle-codeql-databases/rust/results/codeql/rust-queries/queries/diagnostics/ExtractedFiles.bqrs'
            query.parent.mkdir(parents=True)
            query.write_bytes(b'fixture only')
            with patch.dict(os.environ, {'RUNNER_TEMP': tmp, 'CODEQL_BINARY': '/tool', 'GITHUB_WORKSPACE': '/checkout'}), \
                    patch.object(codeql, 'command', return_value='{"rows":["/checkout/crates/app/x.rs"]}') as call:
                codeql.capture_results(root / 'evidence')
            self.assertEqual(call.call_count, 1)
            self.assertIn('bqrs', call.call_args.args[0])
            self.assertEqual(json.loads((root / 'evidence/rust-ExtractedFiles.json').read_text()),
                             {'rows': ['crates/app/x.rs']})
            index = json.loads((root / 'evidence/rust-results-index.json').read_text())
            self.assertEqual(index['results']['diagnostics/ExtractionErrors']['status'], 'unavailable')


class ReportTests(unittest.TestCase):
    def baseline(self):
        return (CI / 'tests' / 'fixtures' / 'codeql-rust-baseline.txt').read_text()

    def test_historical_baseline_reconciles_all_four_warning_files(self):
        value = codeql.rust_report(self.baseline())
        self.assertEqual(value['counts'], {'successful_files': 600, 'files_with_errors_or_warnings': 4})
        self.assertEqual(value['phase_seconds'], {
            'LoadManifest': 369.705, 'LoadSource': 116.108, 'Extract': 33.007, 'ExtractLibrary': 87.603})
        self.assertEqual(value['extraction_seconds'], 665.829869)
        self.assertEqual(value['finalization_seconds'], 75.233546)
        self.assertEqual(value['warning_or_error_files'], [
            'crates/app/src/shortcuts.rs', 'docs/benchmarks/large-pr-probe.rs',
            'docs/experiments/native-glass-prototype.rs', 'vendor/gpui-pre-macos/src/display_link.rs'])
        self.assertTrue(any(d['severity'] == 'INFO' and 'macos' in d['file'] for d in value['diagnostics']))
        self.assertIn('not established', value['coverage_equivalence'])

    def test_missing_or_duplicate_observations_do_not_become_zero_or_pass(self):
        empty = codeql.rust_report('analysis interrupted')
        self.assertIsNone(empty['counts']['successful_files'])
        self.assertIsNone(empty['phase_seconds']['LoadManifest'])
        duplicate = codeql.rust_report(self.baseline() * 2)
        self.assertIsNone(duplicate['counts']['successful_files'])
        self.assertIsNone(duplicate['phase_seconds']['LoadManifest'])
        self.assertEqual(len(duplicate['warning_or_error_files']), 4)

    def test_other_language_job_does_not_pollute_rust_counts(self):
        line = 'Analyze (python)\tStep\t| Total number of Rust files that were extracted without error | 42 |'
        self.assertIsNone(codeql.rust_report(line)['counts']['successful_files'])

    def test_caret_ansi_and_auth_diagnostics_are_sanitized(self):
        text = '^[[33m WARN /checkout/crates/app/x.rs:1:2: Authorization: Basic dGVzdDp0ZXN0'
        value = codeql.rust_report(text)
        self.assertEqual(value['diagnostics'][0]['file'], 'crates/app/x.rs')
        self.assertNotIn('dGVzdDp0ZXN0', json.dumps(value))

    def test_bounded_report_input_and_existing_evidence_preservation(self):
        with tempfile.TemporaryDirectory() as tmp:
            source, output = Path(tmp) / 'source', Path(tmp) / 'report'
            source.write_text(self.baseline())
            output.write_text('keep existing evidence')
            result = subprocess.run([sys.executable, str(CI / 'codeql.py'), 'report', str(source),
                                     '--output', str(output)], capture_output=True, check=False)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(output.read_text(), 'keep existing evidence')
            with self.assertRaises(codeql.EvidenceError):
                codeql.read_bounded(source, 1)


if __name__ == '__main__':
    unittest.main()
