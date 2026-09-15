#!/usr/bin/env python3
"""CodeQL experiment policy, bounded cache identity and offline Rust diagnostics."""

import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import signal
import stat
import subprocess
import sys
import time

from metrics import ANSI, sanitize

MAX_INPUT = 16 * 1024 * 1024
MAX_CACHE_BYTES = 4 * 1024 * 1024 * 1024
MAX_CACHE_ENTRIES = 100_000
LANGUAGES = ('actions', 'javascript-typescript', 'python', 'rust')
PHASES = ('LoadManifest', 'LoadSource', 'Extract', 'ExtractLibrary')


class EvidenceError(ValueError):
    pass


def read_bounded(path, limit=MAX_INPUT):
    with Path(path).open('rb') as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise EvidenceError('input exceeds byte limit')
    return data


def command(argv, limit=MAX_INPUT, timeout=30):
    # Bound live output as well as retained evidence. A timed-out CodeQL JVM and
    # its ordinary child processes belong to this command's process group.
    child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, start_new_session=True)
    chunks, size, deadline = [], 0, time.monotonic() + timeout
    completed = False
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(argv, timeout)
                for key, _ in selector.select(min(remaining, 0.5)):
                    data = os.read(key.fileobj.fileno(), min(65536, limit - size + 1))
                    if not data:
                        selector.unregister(key.fileobj)
                        continue
                    size += len(data)
                    if size > limit:
                        raise EvidenceError('context command exceeded output limit')
                    chunks.append(data)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise subprocess.TimeoutExpired(argv, timeout)
        if child.wait(timeout=remaining) != 0:
            raise EvidenceError('context command failed')
        value = b''.join(chunks).decode('utf-8').strip()
        completed = True
        return value
    finally:
        if not completed:
            # The group can outlive its initial process while a child holds stdout.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        if child.poll() is None:
            child.wait()
        child.stdout.close()


def digest(value):
    return hashlib.sha256(value).hexdigest()


def policy(env):
    event, ref = env.get('GITHUB_EVENT_NAME'), env.get('GITHUB_REF')
    supported = event in ('push', 'pull_request', 'schedule', 'workflow_dispatch')
    publish = supported and (
        event != 'workflow_dispatch' or env.get('CODEQL_PUBLISH_REQUESTED') == 'true'
    )
    # PR cache lookup can see PR-local entries. Main-only lookup avoids consuming
    # an untrusted PR cache with a copied key. No privileged workflow_run bridge.
    trusted = ref == 'refs/heads/main' and event in ('push', 'schedule', 'workflow_dispatch')
    return {
        'upload': 'always' if publish else 'never',
        'restore': str(trusted and env.get('CODEQL_COLD_REQUESTED') != 'true').lower(),
        'save': str(trusted and env.get('CODEQL_DIAGNOSTICS_REQUESTED') != 'true').lower(),
    }


def write_outputs(path, values):
    with Path(path).open('a', encoding='utf-8') as out:
        for key, value in values.items():
            if not re.fullmatch(r'[a-z][a-z0-9-]*', key) or not re.fullmatch(r'[A-Za-z0-9._-]+', value):
                raise EvidenceError('invalid workflow output')
            out.write(f'{key}={value}\n')


def write_record(directory, name, value):
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / name).open('x', encoding='utf-8') as out:
        json.dump(value, out, indent=2, sort_keys=True)
        out.write('\n')


def cache_key(identity):
    return 'gitturtle-codeql-rust-v1-' + digest(json.dumps(identity, sort_keys=True).encode())


def context(directory, output):
    binary = os.environ['CODEQL_BINARY']
    version = json.loads(command([binary, 'version', '--format=json']))
    if version.get('version') != '2.27.0':
        raise EvidenceError('review CodeQL compatibility before changing the pinned bundle')
    extractor = Path(json.loads(command([binary, 'resolve', 'extractor', '--language=rust', '--format=json'])))
    schema = read_bounded(extractor / 'codeql-extractor.yml')
    if not re.search(rb'^  cargo_target_dir:', schema, re.MULTILINE):
        raise EvidenceError('installed Rust extractor does not expose cargo_target_dir')
    command(['git', 'diff', '--quiet', 'HEAD', '--'])
    tree = command(['git', 'rev-parse', '--verify', 'HEAD^{tree}'])
    if not re.fullmatch(r'[0-9a-f]{40,64}', tree):
        raise EvidenceError('invalid source tree identity')
    # Exact-tree keys deliberately invalidate every tracked input, including all
    # vendor contents, build scripts and their resources. No restore prefixes.
    rustc = command(['rustc', '--version', '--verbose'])
    native = command(['dpkg-query', '-W', '-f=${binary:Package}=${Version}\n'])
    target = Path(os.environ['CODEQL_EXTRACTOR_RUST_OPTION_CARGO_TARGET_DIR'])
    expected = Path(os.environ['RUNNER_TEMP']) / 'gitturtle-codeql-target'
    if target != expected or target.is_symlink():
        raise EvidenceError('unexpected extractor target directory')
    identity = {
        'tree': tree,
        'os': os.environ.get('RUNNER_OS'), 'arch': os.environ.get('RUNNER_ARCH'),
        'image': os.environ.get('ImageOS'), 'image_version': os.environ.get('ImageVersion'),
        'codeql_version': version['version'], 'extractor_schema_sha256': digest(schema),
        'rustc_sha256': digest(rustc.encode()), 'native_packages_sha256': digest(native.encode()),
        'rust_environment_sha256': digest(json.dumps({
            key: os.environ.get(key) for key in (
                'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTDOCFLAGS',
                'CARGO_ENCODED_RUSTDOCFLAGS', 'CC', 'CXX', 'CFLAGS', 'CXXFLAGS',
                'CODEQL_EXTRACTOR_RUST_OPTION_CARGO_TARGET',
                'CODEQL_EXTRACTOR_RUST_OPTION_CARGO_FEATURES',
                'CODEQL_EXTRACTOR_RUST_OPTION_CARGO_CFG_OVERRIDES',
            )
        }, sort_keys=True).encode()),
        'target_policy': 'extractor host target, all features, unchanged cfg defaults',
    }
    if any(not identity[key] for key in ('os', 'arch', 'image', 'image_version')):
        raise EvidenceError('runner identity is incomplete')
    key = cache_key(identity)
    write_record(directory, 'context.json', {
        'schema_version': 1, 'identity': identity, 'cache_key': key,
        'rustc': sanitize(rustc), 'logical_cpu_count': os.cpu_count(),
        'extractor_target': '$RUNNER_TEMP/gitturtle-codeql-target',
        'policy': policy(os.environ), 'max_cache_uncompressed_bytes': MAX_CACHE_BYTES,
        'run_id': sanitize(os.environ.get('GITHUB_RUN_ID', '')),
        'run_attempt': sanitize(os.environ.get('GITHUB_RUN_ATTEMPT', '')),
    })
    write_outputs(output, {'cache-key': key})


def footprint(root, max_bytes=MAX_CACHE_BYTES, max_entries=MAX_CACHE_ENTRIES):
    result = {'uncompressed_bytes': 0, 'entries': 0, 'complete': False, 'reason': None}
    if not root.exists() or root.is_symlink() or not root.is_dir():
        result['reason'] = 'target absent or not a real directory'
        return result
    start = time.monotonic()
    pending = [root]
    while pending:
        with os.scandir(pending.pop()) as entries:
            for entry in entries:
                result['entries'] += 1
                if result['entries'] > max_entries or time.monotonic() - start > 30:
                    result['reason'] = 'entry or time limit exceeded'
                    return result
                info = entry.stat(follow_symlinks=False)
                if stat.S_ISDIR(info.st_mode):
                    pending.append(Path(entry.path))
                elif stat.S_ISREG(info.st_mode):
                    result['uncompressed_bytes'] += info.st_size
                else:
                    result['reason'] = 'symlink or special entry refused'
                    return result
                if result['uncompressed_bytes'] > max_bytes:
                    result['reason'] = 'uncompressed byte limit exceeded'
                    return result
    result.update(complete=True, reason='within bounds')
    return result


def record_footprint(directory, output):
    record = footprint(Path(os.environ['CODEQL_EXTRACTOR_RUST_OPTION_CARGO_TARGET_DIR']))
    record['analysis_outcome'] = os.environ.get('CODEQL_ANALYSIS_OUTCOME', 'unknown')
    # An absent output is unavailable, not a cache miss (e.g. a skipped restore).
    record['exact_cache_hit'] = {'true': True, 'false': False}.get(os.environ.get('CODEQL_CACHE_MATCH'))
    record['compressed_bytes'] = None
    record['restore_seconds'] = None
    record['save_seconds'] = None
    write_record(directory, 'cache-candidate.json', record)
    eligible = record['complete'] and record['uncompressed_bytes'] > 0 and record['analysis_outcome'] == 'success'
    write_outputs(output, {'save-eligible': str(eligible).lower()})


def clean_result(value, workspace, depth=0):
    if depth > 64:
        raise EvidenceError('diagnostic result nesting exceeds limit')
    if isinstance(value, str):
        # Preserve checkout-relative file identities before removing private roots.
        value = value.replace('file://' + workspace + '/', '').replace(workspace + '/', '')
        return sanitize(value)
    if isinstance(value, list):
        return [clean_result(item, workspace, depth + 1) for item in value]
    if isinstance(value, dict):
        return {sanitize(key): clean_result(item, workspace, depth + 1) for key, item in value.items()}
    return value


def capture_results(directory):
    binary = os.environ['CODEQL_BINARY']
    database = Path(os.environ['RUNNER_TEMP']) / 'gitturtle-codeql-databases' / 'rust'
    results = database / 'results' / 'codeql' / 'rust-queries' / 'queries'
    names = (
        'diagnostics/ExtractedFiles', 'diagnostics/ExtractionErrors', 'diagnostics/ExtractionWarnings',
        'summary/NumberOfSuccessfullyExtractedFiles', 'summary/NumberOfFilesExtractedWithErrors',
        'telemetry/ExtractorInformation',
    )
    captured = {}
    for name in names:
        source = results / (name + '.bqrs')
        if not source.is_file() or source.is_symlink():
            captured[name] = {'status': 'unavailable'}
            continue
        try:
            raw = command([binary, 'bqrs', 'decode', '--format=json', '--entities=all', '--', str(source)])
            value = clean_result(json.loads(raw), os.environ['GITHUB_WORKSPACE'])
            write_record(directory, 'rust-' + source.stem + '.json', value)
            captured[name] = {'status': 'captured', 'decoded_sha256': digest(raw.encode())}
        except (EvidenceError, ValueError, OSError, subprocess.TimeoutExpired, RecursionError):
            captured[name] = {'status': 'decode failed or bounded output unavailable'}
    write_record(directory, 'rust-results-index.json', {
        'schema_version': 1, 'results': captured,
        'coverage_equivalence': 'requires comparison; missing results are not clean coverage',
    })


def duration(text):
    if len(text) > 32:
        return None
    match = re.fullmatch(r'(?:(\d+)min)?(\d+(?:\.\d+)?)s', text)
    if not match:
        return None
    return round(int(match[1] or 0) * 60 + float(match[2]), 6)


def rust_report(text):
    """Parse observations, never convert missing diagnostics into a coverage pass."""
    phases = {name: [] for name in PHASES}
    counts = {'successful_files': [], 'files_with_errors_or_warnings': []}
    diagnostics = set()
    markers = {key: [] for key in ('extraction_start', 'finalization_start', 'queries_start', 'queries_end')}
    for line in ANSI.sub('', text).splitlines():
        # A full `gh run view --log` export can contain four concurrent jobs.
        # Accept plain extractor logs, but ignore other tab-prefixed job exports.
        if '\t' in line and not re.match(r'[^\t]*\brust\b[^\t]*\t', line, re.I):
            continue
        phase = re.search(r'total duration \((LoadManifest|LoadSource|Extract|ExtractLibrary)\): ([0-9.min]+s)', line)
        if phase:
            phases[phase[1]].append(duration(phase[2]))
        for phrase, key in (
            ('without error', 'successful_files'), ('with errors', 'files_with_errors_or_warnings')
        ):
            match = re.search(r'\| Total number of Rust files that were extracted ' + phrase + r'\s*\|\s*(\d+)\s*\|', line)
            if match:
                counts[key].append(int(match[1]))
        issue = re.search(r'\b(WARN|ERROR|INFO)\s+.*?((?:crates|vendor|docs)/[^:\r\n]+\.rs):(\d+):(\d+):\s*(.*)', line)
        if issue:
            severity, path, row, column, message = issue.groups()
            if '..' not in path.split('/'):
                diagnostics.add((severity, sanitize(path)[:512], int(row), int(column), sanitize(message)[:1024]))
        stamp = re.search(r'\b(20\d\d-\d\d-\d\dT[0-9:.]+Z) ', line)
        if stamp:
            for phrase, key in (
                ('##[group]Extracting rust', 'extraction_start'),
                ('##[group]Finalizing rust', 'finalization_start'),
                ('##[group]Running queries for rust', 'queries_start'),
                ('Interpreted diagnostic query "Extraction errors"', 'queries_end'),
            ):
                if phrase in line:
                    markers[key].append(stamp[1])
    def only(values):
        return values[0] if len(values) == 1 else None
    def interval(start, end):
        a, b = only(markers[start]), only(markers[end])
        if not a or not b:
            return None
        value = (datetime.fromisoformat(b.replace('Z', '+00:00')) - datetime.fromisoformat(a.replace('Z', '+00:00'))).total_seconds()
        return round(value, 6) if value >= 0 else None
    return {
        'schema_version': 1,
        'phase_seconds': {key: only(values) for key, values in phases.items()},
        'phase_observations': phases,
        'counts': {key: only(values) for key, values in counts.items()},
        'count_observations': counts,
        'diagnostics': [dict(zip(('severity', 'file', 'line', 'column', 'message'), value)) for value in sorted(diagnostics)],
        'warning_or_error_files': sorted({value[1] for value in diagnostics if value[0] != 'INFO'}),
        'timing_markers': markers,
        'extraction_seconds': interval('extraction_start', 'finalization_start'),
        'finalization_seconds': interval('finalization_start', 'queries_start'),
        'queries_until_interpretation_seconds': interval('queries_start', 'queries_end'),
        'coverage_equivalence': 'not established; compare analyzed file identities, diagnostics, configuration and source changes',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('policy').add_argument('--output', type=Path, required=True)
    for name in ('context', 'footprint'):
        p = sub.add_parser(name)
        p.add_argument('--directory', type=Path, required=True)
        p.add_argument('--output', type=Path, required=True)
    capture = sub.add_parser('capture', help='Decode existing Rust diagnostic results without rerunning queries')
    capture.add_argument('--directory', type=Path, required=True)
    report = sub.add_parser('report', help='Parse an existing full run export or Rust extractor log')
    report.add_argument('log', type=Path)
    report.add_argument('--output', type=Path, required=True)
    sub.add_parser('summary')
    args = parser.parse_args()
    try:
        if args.command == 'policy':
            write_outputs(args.output, policy(os.environ))
        elif args.command == 'context':
            context(args.directory, args.output)
        elif args.command == 'footprint':
            record_footprint(args.directory, args.output)
        elif args.command == 'capture':
            capture_results(args.directory)
        elif args.command == 'report':
            value = rust_report(read_bounded(args.log).decode('utf-8', errors='replace'))
            with args.output.open('x', encoding='utf-8') as out:
                json.dump(value, out, indent=2, sort_keys=True)
                out.write('\n')
        else:
            print('## CodeQL analysis\n')
            for key in ('CODEQL_LANGUAGE', 'RESULT_UPLOAD', 'ANALYSIS_OUTCOME', 'CODEQL_DIAGNOSTICS_REQUESTED'):
                print(f'- {key}: `{sanitize(os.environ.get(key, "unavailable"))[:160]}`')
            print('\nUpload `never` is a rehearsal and does not satisfy code-scanning protection.')
            print('Full job, extraction, finalization and query timings come from the completed Actions logs.')
        return 0
    except EvidenceError as error:
        print('CodeQL evidence: ' + sanitize(str(error)), file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, subprocess.TimeoutExpired, RecursionError):
        print('CodeQL evidence: invalid, unavailable or oversized input; no success inferred', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
