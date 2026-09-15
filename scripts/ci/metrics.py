#!/usr/bin/env python3
"""Bounded Actions timing reports and explicit local command diagnostics (stdlib only)."""

import argparse
from collections import defaultdict, deque
from datetime import datetime
import html
import http.client
import json
import math
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request

MAX_INPUT = 16 * 1024 * 1024
MAX_RESPONSE = 2 * 1024 * 1024
MAX_RUNS = 10
MAX_JOBS = 500
MAX_STEPS = 100
MAX_LOG = 1024 * 1024
MAX_LINE = 16384
REPOSITORY = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,99}/[A-Za-z0-9_][A-Za-z0-9_.-]{0,99}\Z")
NAME = re.compile(r"[a-z][a-z0-9-]{0,47}\Z")
# Saved terminal exports can render ESC as the literal two-character caret form ^[.
# Normalize the same CSI sequences before both parsing and credential redaction.
ANSI = re.compile(r"(?:\x1b|\^\[)\[[0-?]*[ -/]*[@-~]")
CONTEXT_ENV = (("Repository", "GITHUB_REPOSITORY"), ("Run", "GITHUB_RUN_ID"),
               ("Attempt", "GITHUB_RUN_ATTEMPT"), ("Commit", "GITHUB_SHA"),
               ("Event", "GITHUB_EVENT_NAME"), ("OS", "RUNNER_OS"),
               ("Architecture", "RUNNER_ARCH"), ("Runner image", "ImageOS"),
               ("Image version", "ImageVersion"))


class MetricsError(ValueError):
    """An invalid or incomplete input that cannot produce a trustworthy report."""


def sanitize(value):
    """Remove common credential forms, URLs, absolute paths and control sequences.

    Diagnostics are still untrusted text; never emit them as workflow commands.
    No environment dump, command arguments, raw API bodies or runner names are kept.
    """
    # JSON can contain escaped lone surrogates, which cannot be printed as UTF-8.
    value = ANSI.sub("", str(value).encode("utf-8", errors="replace").decode("utf-8"))
    value = re.sub(r"(?:https?|ssh)://[^\s<>\"']+", "[url]", value)
    value = re.sub(r"\b(?:github_pat_|gh[pousr]_)[A-Za-z0-9_]+", "[credential]", value)
    for key, secret in os.environ.items():
        if re.search(r"(?i)(token|password|secret|api_?key)", key) and len(secret) >= 8:
            value = value.replace(secret, "[credential]")
    # Authorization values contain a scheme and may include spaces, quoted
    # parameters and commas (e.g. Basic or Digest). Drop the rest of that line
    # instead of retaining a credential after redacting only its scheme.
    value = re.sub(
        r'''(?i)\b((?:proxy-)?authorization)(?:\\?["']|&quot;|&#(?:34|39);)?[ \t]*[:=][ \t]*[^\r\n]*''',
        r"\1=[redacted]", value,
    )
    # Credential keys also occur in JSON and compound names such as access_token,
    # refreshToken and client-secret. As with authorization, discard the rest of
    # the line: quoted values can contain whitespace, commas and escaped quotes.
    value = re.sub(
        r'''(?i)\b([\w.-]*(?:token|password|passwd|secret|api[_-]?key|credential|private[_-]?key)[\w.-]*)(?:\\?["']|&quot;|&#(?:34|39);)?[ \t]*[:=][^\r\n]*''',
        r"\1=[redacted]", value,
    )
    # Spaces can be part of private path components. Conservatively redact to
    # the next quote, markup delimiter or newline rather than leaking a suffix.
    value = re.sub(r"(?<![\w<])(?:[A-Za-z]:[\\/]|/)[^\r\n<>\"'`]+", "[path]", value)
    # Use known environment roots too, including escaped paths in Cargo HTML.
    for key in ("GITHUB_WORKSPACE", "RUNNER_TEMP", "HOME", "CARGO_HOME", "RUSTUP_HOME"):
        root = os.environ.get(key)
        if root and len(root) > 1:
            value = value.replace(root, "[path]").replace(root.replace("\\", "\\\\"), "[path]")
    return "".join(c for c in value if c in "\n\t" or ord(c) >= 32 and ord(c) != 127)


def label(value):
    return sanitize(value)[:160] if value is not None else None


def runner_context():
    return {title: label(os.environ.get(key)) for title, key in CONTEXT_ENV}


def positive_id(value, field):
    if type(value) is not int or value < 1:
        raise MetricsError(f"{field} must be a positive integer")
    return value


def number(value):
    if value is None:
        return None
    try:
        valid = type(value) in (int, float) and math.isfinite(value) and value >= 0
    except OverflowError:
        valid = False
    if not valid:
        raise MetricsError("measurement must be finite and nonnegative")
    return value


def timestamp(value):
    if not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            raise ValueError
        # Actions sometimes represents an unstarted step with a year-one sentinel.
        return parsed if parsed.year > 1970 else None
    except (ValueError, TypeError, AttributeError):
        raise MetricsError("invalid timestamp (expected ISO 8601 with timezone)") from None


def seconds(start, end):
    if start is None or end is None:
        return None
    duration = (end - start).total_seconds()
    if duration < 0:
        raise MetricsError("completion precedes start")
    return round(duration, 6)


def object_value(value, field):
    if not isinstance(value, dict):
        raise MetricsError(f"{field} must be an object")
    return value


def list_value(value, field, limit):
    if not isinstance(value, list) or len(value) > limit:
        raise MetricsError(f"{field} must be an array of at most {limit} entries")
    return value


def read_json(stream, limit=MAX_INPUT):
    data = stream.read(limit + 1)
    if len(data) > limit:
        raise MetricsError("JSON input exceeds byte limit")
    try:
        return json.loads(data)
    except (ValueError, UnicodeError, RecursionError):
        raise MetricsError("invalid JSON input") from None


def cargo_timings(log):
    if not isinstance(log, str):
        raise MetricsError("step log must be text of at most 1 MiB")
    try:
        size = len(log.encode("utf-8"))
    except UnicodeError:
        raise MetricsError("step log must contain valid Unicode text") from None
    if size > MAX_LOG:
        raise MetricsError("step log must be text of at most 1 MiB")
    log = ANSI.sub("", log)
    compilation = []
    for match in re.finditer(r"Finished `[^`]+`[^\n]*? in (?:(\d+)m )?(\d+(?:\.\d+)?)s", log):
        compilation.append(60 * number(float(match[1] or 0)) + number(float(match[2])))
    harness = [float(m) for m in re.findall(r"test result: [^\n]*?finished in (\d+(?:\.\d+)?)s", log)]
    return {
        "cargo_compilation_seconds": round(number(sum(compilation)), 6) if compilation else None,
        "test_harness_seconds": round(number(sum(harness)), 6) if harness else None,
    }


def cache_measurement(value=None):
    value = object_value({} if value is None else value, "cache")
    hit = value.get("hit")
    if hit is not None and type(hit) is not bool:
        raise MetricsError("cache hit must be boolean or null")
    size = value.get("size_bytes")
    if size is not None and (type(size) is not int or size < 0):
        raise MetricsError("cache size must be nonnegative integer bytes or null")
    return {"restore_seconds": number(value.get("restore_seconds")),
            "save_seconds": number(value.get("save_seconds")), "hit": hit, "size_bytes": size}


def platform_identity(job):
    labels = list_value(job.get("labels", []), "runner labels", 100)
    os_name = architecture = None
    for item in labels:
        if not isinstance(item, str):
            raise MetricsError("runner label must be text")
        lower = item.lower()
        if lower == "linux" or lower.startswith("ubuntu-"):
            os_name = "Linux"
        elif lower == "macos" or lower.startswith("macos-"):
            os_name = "macOS"
        elif lower == "windows" or lower.startswith("windows-"):
            os_name = "Windows"
        if lower in ("x64", "arm64", "arm", "x86"):
            architecture = lower.upper()
    # Explicit runner context is an optional export enrichment, not a guess from OS.
    runner = object_value(job.get("runner", {}), "runner")
    if runner.get("os") in ("Linux", "macOS", "Windows"):
        os_name = runner["os"]
    if runner.get("architecture") in ("X64", "ARM64", "ARM", "X86"):
        architecture = runner["architecture"]
    return os_name, architecture


def interval_union(intervals):
    if not intervals:
        return None
    total = 0.0
    start, end = sorted(intervals)[0]
    for next_start, next_end in sorted(intervals)[1:]:
        if next_start <= end:
            end = max(end, next_end)
        else:
            total += seconds(start, end)
            start, end = next_start, next_end
    return round(total + seconds(start, end), 6)


def report_run(entry):
    entry = object_value(entry, "run entry")
    repository = entry.get("repository")
    if not isinstance(repository, str) or not REPOSITORY.fullmatch(repository):
        raise MetricsError("repository must be OWNER/REPO")
    run = object_value(entry.get("run"), "run")
    run_id = positive_id(run.get("id"), "run id")
    workflow_id = run.get("workflow_id")
    if workflow_id is not None:
        workflow_id = positive_id(workflow_id, "workflow id")
    attempt = positive_id(run.get("run_attempt", 1), "run attempt")
    sha = run.get("head_sha")
    if sha is not None and (not isinstance(sha, str) or not re.fullmatch(r"[0-9a-fA-F]{40,64}", sha)):
        raise MetricsError("head_sha must be a full hexadecimal commit identity")
    created = timestamp(run.get("created_at"))
    jobs = list_value(entry.get("jobs", []), "jobs", MAX_JOBS)
    coverage = entry.get("jobs_complete") is True
    complete = coverage
    starts_complete = coverage
    output_jobs, intervals, starts = [], [], []
    seen_jobs = set()
    for job in jobs:
        job = object_value(job, "job")
        job_id = positive_id(job.get("id"), "job id")
        if job_id in seen_jobs:
            raise MetricsError("duplicate job id in run attempt")
        seen_jobs.add(job_id)
        if job.get("run_id", run_id) != run_id or job.get("run_attempt", attempt) != attempt:
            raise MetricsError("job identity differs from run attempt")
        start, end = timestamp(job.get("started_at")), timestamp(job.get("completed_at"))
        executed = job.get("status") in ("in_progress", "completed") and job.get("conclusion") != "skipped"
        duration = seconds(start, end) if executed and job.get("status") == "completed" else None
        # Skipped and queued jobs can have synthetic timestamps without ever
        # occupying a runner. They cannot establish the first execution start.
        if executed and start:
            starts.append(start)
        elif job.get("conclusion") != "skipped" and job.get("status") not in ("queued", "waiting", "pending", "requested"):
            starts_complete = False
        if duration is not None:
            intervals.append((start, end))
        elif job.get("conclusion") != "skipped":
            complete = False
        os_name, architecture = platform_identity(job)
        steps = job.get("steps")
        output_steps = []
        for step in list_value([] if steps is None else steps, "steps", MAX_STEPS):
            step = object_value(step, "step")
            output_steps.append({
                "number": positive_id(step.get("number"), "step number"),
                "name": label(step.get("name")), "status": label(step.get("status")),
                "conclusion": label(step.get("conclusion")),
                "duration_seconds": seconds(timestamp(step.get("started_at")), timestamp(step.get("completed_at")))
                if step.get("status") == "completed" and step.get("conclusion") != "skipped" else None,
                **cargo_timings(step.get("log", "")), "cache": cache_measurement(step.get("cache")),
            })
        output_jobs.append({"job_id": job_id, "name": label(job.get("name")),
                            "status": label(job.get("status")), "conclusion": label(job.get("conclusion")),
                            "os": os_name, "architecture": architecture,
                            "duration_seconds": duration, "steps_available": isinstance(steps, list) and bool(steps),
                            "steps": output_steps})
    finished = complete and run.get("status") == "completed" and bool(intervals)
    first = min(starts) if starts else None
    last = max(end for _, end in intervals) if intervals else None
    known = round(sum(seconds(start, end) for start, end in intervals), 6) if intervals else None
    return {"repository": repository, "run_id": run_id, "attempt": attempt,
            "workflow_id": workflow_id, "workflow_name": label(run.get("name")),
            "workflow_path": label(run.get("path")),
            "commit": sha, "event": label(run.get("event")), "status": label(run.get("status")),
            "conclusion": label(run.get("conclusion")),
            "queue_delay_seconds": seconds(created, first) if attempt == 1 and starts_complete else None,
            "elapsed_seconds": seconds(created, last) if finished and attempt == 1 else None,
            "critical_path_seconds": seconds(first, last) if finished else None,
            "runner_busy_seconds": interval_union(intervals) if finished else None,
            "total_runner_seconds": known if finished else None, "known_runner_seconds": known,
            "jobs_complete": complete, "jobs": output_jobs}


def make_report(data):
    data = object_value(data, "export")
    if data.get("schema_version") != 1:
        raise MetricsError("export schema_version must be 1")
    seen, runs, duplicates = {}, [], 0
    for entry in list_value(data.get("runs"), "runs", MAX_RUNS):
        result = report_run(entry)
        key = (result["repository"].lower(), result["run_id"], result["attempt"])
        if key in seen:
            if seen[key] != result:
                raise MetricsError("conflicting duplicate observation; keep one snapshot per run attempt")
            duplicates += 1
        else:
            seen[key] = result
            runs.append(result)
    groups = defaultdict(list)
    for run in runs:
        # Names and sanitized paths are descriptive labels, not stable identities.
        # Keep incomplete exports usable without inferring duplicated work from SHA alone.
        if run["commit"] and run["workflow_id"] is not None and run["event"] in ("push", "pull_request"):
            groups[(run["repository"].lower(), run["workflow_id"], run["commit"].lower())].append(run)
    push_pr = []
    for (repository, workflow_id, commit), group in groups.items():
        if {"push", "pull_request"} <= {r["event"] for r in group}:
            push_pr.append({"repository": repository, "workflow_id": workflow_id, "commit": commit,
                            "run_ids": sorted({r["run_id"] for r in group})})
    return {"schema_version": 1, "runs": runs, "duplicate_observations": duplicates,
            "duplicate_push_pr": push_pr}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise MetricsError("GitHub API redirect refused; use the repository's current name")


class GitHub:
    def __init__(self):
        self.opener = urllib.request.build_opener(NoRedirect)
        self.remaining = MAX_INPUT

    def get(self, path):
        headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2026-03-10",
                   "User-Agent": "gitturtle-ci-metrics"}
        token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
        if token:
            headers["Authorization"] = f"Bearer {token}"
        request = urllib.request.Request("https://api.github.com" + path, headers=headers, method="GET")
        try:
            with self.opener.open(request, timeout=20) as response:
                data = response.read(min(MAX_RESPONSE, self.remaining) + 1)
            if len(data) > min(MAX_RESPONSE, self.remaining):
                raise MetricsError("GitHub API response exceeds byte budget")
            self.remaining -= len(data)
            return json.loads(data)
        except urllib.error.HTTPError as error:
            raise MetricsError(f"GitHub API HTTP {error.code}; check repository/run, read access or rate limit") from None
        except (urllib.error.URLError, TimeoutError, OSError, http.client.HTTPException):
            raise MetricsError("GitHub API request failed (network or timeout); no report produced") from None
        except MetricsError:
            raise
        except (ValueError, UnicodeError, RecursionError):
            raise MetricsError("GitHub API returned invalid JSON") from None


def collect(repository, run_ids):
    if not REPOSITORY.fullmatch(repository):
        raise MetricsError("repository must be OWNER/REPO")
    if not run_ids or len(run_ids) > MAX_RUNS:
        raise MetricsError(f"name between 1 and {MAX_RUNS} runs explicitly")
    api = GitHub()
    entries = []
    for run_id in dict.fromkeys(run_ids):
        positive_id(run_id, "run id")
        prefix = f"/repos/{repository}/actions/runs/{run_id}"
        run = object_value(api.get(prefix), "API run")
        if run.get("id") != run_id:
            raise MetricsError("GitHub API returned a different run identity")
        attempt = positive_id(run.get("run_attempt"), "run attempt")
        jobs = []
        for page in range(1, 6):
            response = object_value(api.get(f"{prefix}/attempts/{attempt}/jobs?per_page=100&page={page}"), "API jobs")
            batch = list_value(response.get("jobs"), "API jobs", 100)
            total = response.get("total_count")
            if type(total) is not int or not 0 <= total <= MAX_JOBS:
                raise MetricsError("GitHub job count missing or exceeds 500-job limit")
            jobs.extend(batch)
            if len(jobs) == total:
                break
            if not batch or len(jobs) > total:
                raise MetricsError("GitHub job pagination changed; retry a completed run")
        else:
            raise MetricsError("GitHub job pagination limit reached; no partial report produced")
        entries.append({"repository": repository, "run": run, "jobs": jobs, "jobs_complete": True})
    return {"schema_version": 1, "runs": entries}


def display(value):
    if value is None:
        return "unavailable"
    return html.escape(str(value)).replace("|", "&#124;").replace("\n", " ").replace("`", "&#96;")


def markdown(report):
    lines = ["# CI timing report", "", "Seconds; unavailable values are not zero. Elapsed ends at the last job; critical path is the observed first-start to last-end span, not a dependency-DAG calculation.", ""]
    for run in report["runs"]:
        lines += ["", f"## {display(run['repository'])} run {run['run_id']} attempt {run['attempt']}", "",
                  f"Workflow: {display(run['workflow_name'])}; ID: {display(run['workflow_id'])}; path: {display(run['workflow_path'])}.", "",
                  f"Commit: {display(run['commit'])}; event: {display(run['event'])}; status: {display(run['status'])}/{display(run['conclusion'])}.", "",
                  "| Measurement | Seconds |", "| --- | ---: |"]
        for field in ("queue_delay_seconds", "elapsed_seconds", "critical_path_seconds", "runner_busy_seconds", "total_runner_seconds", "known_runner_seconds"):
            lines.append(f"| {field} | {display(run[field])} |")
        for job in run["jobs"]:
            lines += ["", f"### Job {job['job_id']}: {display(job['name'])}", "",
                      f"OS/architecture: {display(job['os'])}/{display(job['architecture'])}; duration: {display(job['duration_seconds'])} s; result: {display(job['status'])}/{display(job['conclusion'])}.", "",
                      "| Step | Result | Duration s | Cargo compilation s | Test harness sum s | Cache restore/save s; hit; bytes |",
                      "| --- | --- | ---: | ---: | ---: | --- |"]
            for step in job["steps"]:
                cache = step["cache"]
                lines.append(f"| {step['number']}. {display(step['name'])} | {display(step['conclusion'] or step['status'])} | {display(step['duration_seconds'])} | {display(step['cargo_compilation_seconds'])} | {display(step['test_harness_seconds'])} | " + "; ".join(display(cache[k]) for k in ("restore_seconds", "save_seconds", "hit", "size_bytes")) + " |")
            if not job["steps_available"]:
                lines += ["", "Step timing unavailable."]
    lines += ["", f"Duplicate observations removed: {report['duplicate_observations']}."]
    for group in report["duplicate_push_pr"]:
        lines += [f"Potential duplicate push/PR work: {display(group['repository'])} workflow {group['workflow_id']} {group['commit']}: runs {', '.join(map(str, group['run_ids']))}. Each actual run retains its runner cost."]
    lines += ["", "Cargo compilation and test harness totals require matching log markers; test harness totals exclude startup and doctest compilation. No cross-run duration sum represents elapsed time.", ""]
    return "\n".join(lines)


def measure(args):
    if not NAME.fullmatch(args.name):
        raise MetricsError("measurement name must be lowercase letters/digits/hyphens, at most 48 characters")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        raise MetricsError("measure requires an explicit command after --")
    directory = Path(args.directory)
    directory.mkdir(parents=True, exist_ok=True)
    # A retry in the same explicit output directory must not retain this command's old success.
    for suffix in (".log", ".json", "-timing.txt"):
        (directory / f"{args.name}{suffix}").unlink(missing_ok=True)
    tail, size, truncated = deque(), 0, False
    compilation = harness = None
    started_at = time.time()
    started = time.monotonic()
    try:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   start_new_session=os.name == "posix")
    except OSError:
        raise MetricsError("measurement command could not start") from None
    previous = {}

    def forward(signum, _frame):
        try:
            if os.name == "posix":
                os.killpg(process.pid, signum)
            else:
                process.send_signal(signum)
        except ProcessLookupError:
            pass

    for signum in (signal.SIGTERM, signal.SIGINT):
        previous[signum] = signal.signal(signum, forward)
    try:
        with process.stdout:
            while raw := process.stdout.readline(MAX_LINE + 1):
                oversized = len(raw) > MAX_LINE
                # Discard long lines in entirety: never expose a split credential/path.
                if oversized:
                    while raw and not raw.endswith(b"\n"):
                        raw = process.stdout.readline(MAX_LINE + 1)
                    clean = "[overlong diagnostic line omitted]\n"
                else:
                    text = raw.decode("utf-8", errors="replace")
                    try:
                        timings = cargo_timings(text)
                    except MetricsError:
                        # Malformed untrusted diagnostics must not abandon a running command.
                        timings = {"cargo_compilation_seconds": None, "test_harness_seconds": None}
                    if timings["cargo_compilation_seconds"] is not None:
                        compilation = (compilation or 0) + timings["cargo_compilation_seconds"]
                    if timings["test_harness_seconds"] is not None:
                        harness = (harness or 0) + timings["test_harness_seconds"]
                    clean = sanitize(text)
                # Prefixing prevents untrusted stdout from issuing Actions commands.
                print("[ci] " + clean.rstrip("\n"), flush=True)
                tail.append(clean)
                size += len(clean.encode("utf-8"))
                while size > MAX_LOG:
                    size -= len(tail.popleft().encode("utf-8"))
                    truncated = True
        returncode = process.wait()
    except BaseException:
        # A broken output pipe or read failure must not leave this explicit
        # command running after the wrapper reports failure. Reap the child and
        # kill remaining members of its owned process group on POSIX.
        forward(signal.SIGTERM, None)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        finally:
            if os.name == "posix":
                forward(signal.SIGKILL, None)
        raise
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    elapsed = round(time.monotonic() - started, 6)
    (directory / f"{args.name}.log").write_text(("[earlier output truncated]\n" if truncated else "") + "".join(tail), encoding="utf-8")
    timing_artifact = None
    timing_path = Path(os.environ.get("CARGO_TARGET_DIR", "target")) / "cargo-timings/cargo-timing.html"
    if Path(command[0]).name == "cargo" and "--timings" in command and timing_path.is_file() and timing_path.stat().st_mtime >= started_at:
        with timing_path.open("rb") as stream:
            timing_bytes = stream.read(MAX_RESPONSE + 1)
        if len(timing_bytes) <= MAX_RESPONSE:
            # Store as plain text; the report still contains Cargo's unit timing data.
            timing_artifact = f"{args.name}-timing.txt"
            (directory / timing_artifact).write_text(sanitize(timing_bytes.decode("utf-8", errors="replace")), encoding="utf-8")
    result = {"schema_version": 1, "name": args.name, "context": runner_context(), "elapsed_seconds": elapsed,
              "exit_code": returncode, "cargo_compilation_seconds": round(compilation, 6) if compilation is not None and math.isfinite(compilation) else None,
              "test_harness_seconds": round(harness, 6) if harness is not None and math.isfinite(harness) else None,
              "log_truncated": truncated, "log": f"{args.name}.log", "build_timing_artifact": timing_artifact,
              "cache": cache_measurement()}
    (directory / f"{args.name}.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return returncode if returncode >= 0 else 128 - returncode


def job_summary(directory):
    lines = ["## CI command diagnostics", ""]
    for title, value in runner_context().items():
        lines.append(f"{title}: {display(value)}  ")
    lines += ["", "Queue, final job/step durations, critical path and total runner time: unavailable until the run finishes; collect the Actions run/jobs export with `report`.", "",
              "| Command | Exit | Wall s | Cargo compilation s | Test harness sum s | Build timing artifact |", "| --- | ---: | ---: | ---: | ---: | --- |"]
    files = sorted(Path(directory).glob("*.json"))
    if len(files) > 100:
        raise MetricsError("too many command measurements")
    cache_rows = []
    for path in files:
        with path.open("rb") as stream:
            item = object_value(read_json(stream, MAX_RESPONSE), "command measurement")
        if item.get("schema_version") != 1 or not isinstance(item.get("name"), str) or not NAME.fullmatch(item["name"]):
            raise MetricsError("invalid command measurement")
        lines.append("| " + " | ".join(display(label(item.get(key))) for key in ("name", "exit_code", "elapsed_seconds", "cargo_compilation_seconds", "test_harness_seconds", "build_timing_artifact")) + " |")
        cache = cache_measurement(item.get("cache"))
        if any(value is not None for value in cache.values()):
            cache_rows.append("| " + display(label(item["name"])) + " | " + " | ".join(display(cache[key]) for key in ("restore_seconds", "save_seconds", "hit", "size_bytes")) + " |")
    if not files:
        lines += ["", "Command measurements unavailable (the job may have failed before instrumentation)."]
    if cache_rows:
        lines += ["", "### Explicit cache observations", "",
                  "| Measurement | Restore s | Save s | Exact hit | Size bytes |",
                  "| --- | ---: | ---: | --- | ---: |", *cache_rows]
    else:
        lines += ["", "Cache restore duration: unavailable; save duration: unavailable; hit/miss: unavailable; size bytes: unavailable (no explicit cache observation)."]
    lines += ["", "Unavailable cache fields are not zero. Restore boundaries belong to their measurement record; save/size may require completed post-job evidence.", "",
              "Diagnostics artifact: `ci-diagnostics-<job>-<OS>-<architecture>-<run>-<attempt>` (3-day retention). Logs retain up to 1 MiB per command; build timing text up to 2 MiB per command. Missing Cargo markers/artifacts stay unavailable; harness totals exclude startup and doctest compilation.", "",
              "This summary precedes upload and post-job actions; it does not establish their success or duration. Cancellation or runner loss can prevent final diagnostics.", ""]
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    report = sub.add_parser("report", help="read an export or GET explicitly named GitHub runs; print only",
                            description="Use GH_TOKEN or GITHUB_TOKEN for optional Actions:read authentication. No API writes, logs download, redirects or automatic retries.")
    source = report.add_mutually_exclusive_group(required=True)
    source.add_argument("--input", help="schema-version-1 JSON export, or - for stdin (16 MiB max)")
    source.add_argument("--repo", help="explicit GitHub OWNER/REPO; never inferred from local Git")
    report.add_argument("--run-id", type=int, action="append", help="explicit run ID; repeat up to 10 times")
    report.add_argument("--format", choices=("json", "markdown"), default="markdown")
    measure_parser = sub.add_parser("measure", help="execute an explicit local command and retain bounded sanitized diagnostics")
    measure_parser.add_argument("--name", required=True)
    measure_parser.add_argument("--directory", required=True)
    measure_parser.add_argument("command", nargs=argparse.REMAINDER)
    summary = sub.add_parser("summary", help="print local command summary; redirect to GITHUB_STEP_SUMMARY in CI")
    summary.add_argument("--directory", required=True)
    args = parser.parse_args(argv)
    try:
        if args.mode == "measure":
            return measure(args)
        if args.mode == "summary":
            print(job_summary(args.directory))
            return 0
        if args.input:
            if args.run_id:
                raise MetricsError("--run-id requires --repo")
            if args.input == "-":
                data = read_json(sys.stdin.buffer)
            else:
                with Path(args.input).open("rb") as stream:
                    data = read_json(stream)
        else:
            data = collect(args.repo, args.run_id)
        result = make_report(data)
        print(json.dumps(result, indent=2) if args.format == "json" else markdown(result))
        return 0
    except (MetricsError, OSError) as error:
        # Never echo an OSError filename, API response body, command or token.
        message = str(error) if isinstance(error, MetricsError) else "local input/output failed; check access and paths"
        print(f"ci-metrics: {message}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
