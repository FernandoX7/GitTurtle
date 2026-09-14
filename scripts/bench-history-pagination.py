#!/usr/bin/env python3
"""Generate disposable 120k-commit fixtures and record paired release paging samples.

No app launch or user-repository writes. Results/fixtures must be separate dirs.
A --reuse run uses the existing generated fixtures without mutating them.
"""
import argparse
import csv
import hashlib
import json
import os
import platform
import statistics
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENV = {**os.environ, 'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null'}
for key in ('GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR', 'GIT_INDEX_FILE', 'GIT_CONFIG_COUNT', 'GIT_CONFIG_PARAMETERS', 'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES'):
    ENV.pop(key, None)

def command(*args, cwd=ROOT):
    return subprocess.check_output(args, cwd=cwd, env=ENV, text=True).strip()

def generate(path, kind):
    path.mkdir()
    command('git', 'init', '-q', '-b', 'main', str(path))
    proc = subprocess.Popen(['git', '-C', str(path), 'fast-import', '--quiet'], stdin=subprocess.PIPE, env=ENV)
    out = proc.stdin
    index = 0
    def commit(branch, parent=None, merge=None):
        nonlocal index
        index += 1
        message = f'Update module {index % 71}: change {index}\n\nBounded-history benchmark with representative author and message metadata.\n'
        text = f'commit refs/heads/{branch}\nmark :{index}\nauthor Developer {index % 17} <developer{index % 17}@example.invalid> {1700000000 + index} +0000\ncommitter Fixture <fixture@example.invalid> {1700000000 + index} +0000\ndata {len(message)}\n{message}\n'
        if parent is not None: text += f'from :{parent}\n'
        if merge is not None: text += f'merge :{merge}\n'
        value = f'version {index}\n'
        text += f'M 100644 inline module-{index % 71}.txt\ndata {len(value)}\n{value}\n'
        out.write(text.encode())
        return index
    if kind == 'linear':
        for _ in range(120000): commit('main')
    elif kind == 'many-refs':
        for _ in range(100000): commit('main')
        for branch in range(1000):
            parent = 99000 - branch * 17
            for _ in range(20): parent = commit(f'topic-{branch}', parent)
        for ref in range(8000): out.write(f'reset refs/heads/archive-{ref}\nfrom :{100000-ref*11}\n\n'.encode())
    elif kind == 'merge-heavy':
        tip = commit('main')
        for merge in range(40000):
            left = commit('main', tip)
            right = commit(f'side-{merge % 20}', tip)
            tip = commit('main', left, right)
    out.write(b'done\n'); out.close()
    if proc.wait(): raise RuntimeError('fast-import failed')
    return index

def stats(values):
    ordered = sorted(values)
    return {'n': len(values), 'p50_ms': statistics.median(values), 'p95_ms': ordered[(len(values)*95+99)//100-1], 'max_ms': max(values)}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--executable', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=3)
    parser.add_argument('--count', type=int, default=100000)
    parser.add_argument('--reuse', action='store_true')
    args = parser.parse_args()
    try:
        args.executable = args.executable.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        parser.error(f'Cannot resolve --executable: {error}')
    args.fixtures.mkdir(parents=True, exist_ok=True); args.output.mkdir(parents=True, exist_ok=True)
    if args.output.resolve().is_relative_to(args.fixtures.resolve()): parser.error('Results must be outside fixtures')
    metadata = {'started_utc': datetime.now(timezone.utc).isoformat(), 'hardware': command('sysctl','-n','machdep.cpu.brand_string'), 'memory_bytes': command('sysctl','-n','hw.memsize'), 'os': platform.platform(), 'git': command('git','--version'), 'rust': command('rustc','--version'), 'source_base': command('git','rev-parse','HEAD'), 'source_sha256': {str(p): hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in ['crates/git-core/src/history.rs','crates/git-core/src/lib.rs','crates/app/src/graph.rs','crates/git-core/examples/history_pagination_bench.rs']}, 'executable_sha256': hashlib.sha256(args.executable.read_bytes()).hexdigest(), 'cache': 'One full-prefix OS-cache warmup per fixture; new app-side repository handle per pass. No disk-cache flush. Alternating prefix/incremental order by sample.', 'other_load_start': command('uptime'), 'cases': []}
    for kind in ['linear', 'many-refs', 'merge-heavy']:
        path = args.fixtures/kind
        if not args.reuse: generate(path, kind)
        case = {'fixture': kind, 'commits': int(command('git','-C',str(path),'rev-list','--all','--count')), 'refs': len(command('git','-C',str(path),'for-each-ref','--format=%(refname)').splitlines()), 'head': command('git','-C',str(path),'rev-parse','refs/heads/main'), 'load_before': command('uptime')}
        print(f'Measuring {kind}: {case["commits"]} commits / {case["refs"]} refs', flush=True)
        raw = args.output/f'{kind}.csv'
        with raw.open('w') as stream:
            subprocess.run([str(args.executable), str(path), str(args.count), str(args.samples)], stdout=stream, env=ENV, check=True)
        rows = list(csv.DictReader(raw.open()))
        case['summary'] = {}
        for mode in ['prefix','incremental']:
            selected = [r for r in rows if r['mode']==mode]
            case['summary'][mode] = {'initial': stats([float(r['elapsed_ms']) for r in selected if r['offset']=='0']), 'subsequent': stats([float(r['elapsed_ms']) for r in selected if r['offset']!='0']), 'deep_99500': stats([float(r['elapsed_ms']) for r in selected if r['offset']=='99500']), 'total_per_pass': stats([sum(float(r['elapsed_ms']) for r in selected if int(r['sample'])==s) for s in range(args.samples)]), 'peak_metadata_bytes': max(int(r['retained_metadata_bytes']) for r in selected)}
        for mode in ['cancel','selection']:
            selected = [r for r in rows if r['mode']==mode]
            case['summary'][mode] = stats([float(r['elapsed_ms']) for r in selected])
        case['cancel_outcomes'] = [r['fingerprint'] for r in rows if r['mode']=='cancel']
        case['load_after'] = command('uptime')
        metadata['cases'].append(case)
        (args.output/'summary.json').write_text(json.dumps(metadata,indent=2)+'\n')
    metadata['finished_utc'] = datetime.now(timezone.utc).isoformat()
    (args.output/'summary.json').write_text(json.dumps(metadata,indent=2)+'\n')

if __name__ == '__main__': main()
