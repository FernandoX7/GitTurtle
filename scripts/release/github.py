#!/usr/bin/env python3
"""Bounded GitHub release transport and fail-closed publication state machine.

Only the CLI's explicitly activated publication operation writes. Reads and
writes have no automatic retries; uncertain writes leave an audit record.
"""
from __future__ import annotations

import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request

REPOSITORY = "FernandoX7/GitTurtle"
MAX_JSON = 8 * 1024 * 1024
MAX_ASSET = 1024 * 1024 * 1024
API_VERSION = "2026-03-10"


class Error(ValueError):
    pass


class Absent(Error):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise Error("Duplicate JSON key")
        result[key] = value
    return result


def read_document(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_JSON:
        raise Error("Release document is not a bounded regular file")
    value = json.loads(path.read_bytes(), object_pairs_hook=unique_object)
    if not isinstance(value, dict):
        raise Error("Expected a release JSON object")
    return value


def oid(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise Error("Expected a full lowercase commit or tag object SHA")
    return value


def positive(value):
    if isinstance(value, bool) or not re.fullmatch(r"[1-9][0-9]{0,19}", str(value)):
        raise Error("Expected a positive GitHub identifier")
    return int(value)


class GitHub:
    def __init__(self, token):
        if not token or "\n" in token or "\r" in token:
            raise Error("A narrowly scoped GitHub token is required")
        self.token = token
        self.opener = urllib.request.build_opener(NoRedirect())

    def request(self, method, route, value=None):
        if not route.startswith("/") or ".." in route or "\n" in route:
            raise Error("Invalid GitHub API route")
        data = None if value is None else canonical(value)
        request = urllib.request.Request("https://api.github.com" + route, data=data,
            headers={"Authorization": "Bearer " + self.token,
                     "Accept": "application/vnd.github+json", "Content-Type": "application/json",
                     "X-GitHub-Api-Version": API_VERSION, "User-Agent": "GitTurtle-release"}, method=method)
        try:
            with self.opener.open(request, timeout=30) as response:
                raw = response.read(MAX_JSON + 1)
            if len(raw) > MAX_JSON:
                raise Error("GitHub response exceeds the limit")
            return json.loads(raw, object_pairs_hook=unique_object)
        except urllib.error.HTTPError as error:
            if method == "GET" and error.code == 404:
                raise Absent("GitHub object does not exist") from None
            raise Error(f"GitHub {method} returned HTTP {error.code}; inspect before retry") from None
        except (OSError, ValueError) as error:
            if isinstance(error, Error):
                raise
            raise Error(f"GitHub {method} did not produce a verified response; inspect before retry") from None

    def pages(self, route, key=None):
        all_items = []
        separator = "&" if "?" in route else "?"
        for page in range(1, 11):
            value = self.request("GET", f"{route}{separator}per_page=100&page={page}")
            items = value.get(key) if key and isinstance(value, dict) else value
            if not isinstance(items, list):
                raise Error("Malformed GitHub collection")
            all_items.extend(items)
            if len(items) < 100:
                return all_items
        raise Error("GitHub collection exceeds the pagination limit")

    def download_digest(self, asset):
        size = asset.get("size")
        if type(size) is not int or not 0 <= size <= MAX_ASSET:
            raise Error("Invalid release asset size")
        route = f"https://api.github.com/repos/{REPOSITORY}/releases/assets/{positive(asset.get('id'))}"
        headers = {"Authorization": "Bearer " + self.token, "Accept": "application/octet-stream",
                   "User-Agent": "GitTurtle-release", "X-GitHub-Api-Version": API_VERSION}
        request = urllib.request.Request(route, headers=headers)
        try:
            try:
                response = self.opener.open(request, timeout=30)
            except urllib.error.HTTPError as error:
                if error.code not in (301, 302, 303, 307, 308):
                    raise
                location = error.headers.get("Location", "")
                parsed = urllib.parse.urlsplit(location)
                if (parsed.scheme != "https" or parsed.username or parsed.password or parsed.port not in (None, 443)
                        or not parsed.hostname or not parsed.hostname.endswith(".githubusercontent.com")):
                    raise Error("Untrusted release download redirect")
                # Signed object storage URLs must never receive the API token.
                response = self.opener.open(urllib.request.Request(location,
                    headers={"User-Agent": "GitTurtle-release"}), timeout=30)
            digest = hashlib.sha256()
            received = 0
            deadline = time.monotonic() + 300
            with response:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    received += len(chunk)
                    if received > size or time.monotonic() > deadline:
                        raise Error("Release download exceeds its size/time bound")
                    digest.update(chunk)
            if received != size:
                raise Error("Release download is truncated")
            return digest.hexdigest()
        except (OSError, urllib.error.URLError):
            raise Error("Release asset download could not be verified") from None

    def upload(self, release_id, path):
        """Stream once, with explicit Content-Length; never retry an upload."""
        length = path.stat().st_size
        if length > MAX_ASSET:
            raise Error("Release asset exceeds upload limit")
        route = f"/repos/{REPOSITORY}/releases/{positive(release_id)}/assets?name=" + urllib.parse.quote(path.name, safe="")
        connection = http.client.HTTPSConnection("uploads.github.com", timeout=300, context=ssl.create_default_context())
        try:
            connection.putrequest("POST", route)
            for key, value in {"Authorization": "Bearer " + self.token,
                    "Content-Type": "application/octet-stream", "Content-Length": str(length),
                    "User-Agent": "GitTurtle-release", "Accept": "application/vnd.github+json",
                    "X-GitHub-Api-Version": API_VERSION}.items():
                connection.putheader(key, value)
            connection.endheaders()
            deadline = time.monotonic() + 900
            with path.open("rb") as stream:
                while chunk := stream.read(1024 * 1024):
                    if time.monotonic() > deadline:
                        raise Error("Release asset upload exceeded its time bound; inspect before retry")
                    connection.send(chunk)
            response = connection.getresponse()
            raw = response.read(MAX_JSON + 1)
            if response.status != 201 or len(raw) > MAX_JSON:
                raise Error("Release asset upload was not confirmed; inspect before retry")
            return json.loads(raw, object_pairs_hook=unique_object)
        except (OSError, http.client.HTTPException):
            raise Error("Release asset upload is uncertain; inspect before retry") from None
        finally:
            connection.close()


def remote_identity(api, identity):
    prefix = f"/repos/{REPOSITORY}"
    main = api.request("GET", prefix + "/git/ref/heads/main")
    if main.get("object", {}).get("sha") != identity["commit"]:
        raise Error("Release source is no longer the captured main revision")
    tag = api.request("GET", prefix + "/git/ref/tags/" + urllib.parse.quote(identity["tag"], safe=""))
    obj = tag.get("object", {})
    if obj.get("sha") != identity["tag_object"]:
        raise Error("Remote tag moved or differs from the approved tag object")
    for _ in range(10):
        if obj.get("type") == "commit":
            if obj.get("sha") != identity["commit"]:
                raise Error("Remote tag targets another source")
            return
        if obj.get("type") != "tag":
            break
        annotated = api.request("GET", prefix + "/git/tags/" + oid(obj.get("sha")))
        if annotated.get("sha") != obj["sha"]:
            raise Error("Annotated tag identity disagrees with its response")
        obj = annotated.get("object", {})
    raise Error("Remote tag cannot be resolved within the supported bound")


def quality_evidence(api, commit, run_id):
    prefix = f"/repos/{REPOSITORY}/actions/runs/{positive(run_id)}"
    run = api.request("GET", prefix)
    if (run.get("head_sha") != commit or run.get("head_branch") != "main"
            or run.get("event") not in ("push", "workflow_dispatch")
            or run.get("path") != ".github/workflows/quality.yml"
            or run.get("repository", {}).get("full_name") != REPOSITORY
            or run.get("head_repository", {}).get("full_name") != REPOSITORY
            or run.get("status") != "completed" or run.get("conclusion") != "success"):
        raise Error("Quality run is not a successful trusted main run for the exact source")
    attempt = positive(run.get("run_attempt"))
    jobs = api.pages(prefix + f"/attempts/{attempt}/jobs", "jobs")
    required = {"Quality gate", "Rust formatting", "Rust tests and Clippy · macos-15",
                "Rust tests and Clippy · ubuntu-24.04", "Rust release · macos-26", "Rust release · ubuntu-24.04"}
    for name in required:
        matches = [job for job in jobs if job.get("name") == name]
        if (len(matches) != 1 or matches[0].get("head_sha") != commit
                or matches[0].get("status") != "completed" or matches[0].get("conclusion") != "success"):
            raise Error(f"Exact-source required validation is missing or unsuccessful: {name}")
    return {"run_id": positive(run_id), "run_attempt": attempt,
            "url": f"https://github.com/{REPOSITORY}/actions/runs/{positive(run_id)}",
            "required_jobs": sorted(required)}


def verify_checks(api, context):
    if quality_evidence(api, context["identity"]["commit"], context["quality"]["run_id"]) != context["quality"]:
        raise Error("Quality evidence changed after the approved release context")


def validate_context(context, environ):
    source = oid(context["identity"]["commit"])
    wanted = {"GITHUB_REPOSITORY": REPOSITORY, "GITHUB_EVENT_NAME": "workflow_dispatch",
              "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": source,
              "GITHUB_WORKFLOW_SHA": source,
              "GITHUB_WORKFLOW_REF": REPOSITORY + "/.github/workflows/release.yml@refs/heads/main"}
    if any(environ.get(key) != value for key, value in wanted.items()):
        raise Error("Release execution is not the captured trusted main workflow")
    if (positive(environ.get("GITHUB_RUN_ID")) != context["run_id"]
            or positive(environ.get("GITHUB_RUN_ATTEMPT")) != context["run_attempt"]):
        raise Error("Release context comes from a different workflow run or attempt")


def require_activation(context, environ, variable):
    if environ.get(variable) != context["approval_sha256"]:
        raise Error(f"Protected environment {variable} does not authorize this concrete release context")


def file_record(path):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_ASSET:
        raise Error("Release asset is not a bounded regular file")
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"name": path.name, "sha256": digest, "bytes": path.stat().st_size}


def asset_equal(api, actual, expected):
    return (actual.get("name") == expected["name"] and actual.get("size") == expected["bytes"]
            and actual.get("state") == "uploaded" and api.download_digest(actual) == expected["sha256"])


def publish(api, context, directory, report_path):
    """Create draft, verify all uploaded bytes, then publish once. Existing partial
    drafts are an inspection boundary; they are never repaired automatically.
    """
    manifest = directory / "release-manifest.json"
    release = read_document(manifest)
    identity = context["identity"]
    if release.get("context") != context:
        raise Error("Assembly and publication contexts differ")
    records = release["assets"] + [file_record(manifest)]
    if len(records) > 30 or len({item["name"] for item in records}) != len(records):
        raise Error("Release asset set is not bounded and unique")
    for item in records:
        if (not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,199}", item["name"])
                or file_record(directory / item["name"]) != item):
            raise Error("Release asset changed after assembly")
    if set(p.name for p in directory.iterdir()) != {item["name"] for item in records}:
        raise Error("Unexpected release assets are present")
    notes = directory / "RELEASE_NOTES.md"
    if notes.stat().st_size > MAX_JSON:
        raise Error("Release notes exceed the document bound")
    body = notes.read_text()
    metadata = {"tag_name": identity["tag"], "target_commitish": identity["commit"],
                "name": "GitTurtle " + identity["version"], "body": body,
                "prerelease": "-" in identity["version"]}
    report = {"format": 1, "status": "preflight", "tag": identity["tag"],
              "source": identity["commit"], "manifest_sha256": file_record(manifest)["sha256"],
              "operations": []}

    def record(stage, **fields):
        report["status"] = stage
        report.update(fields)
        # A report belongs to one invocation. Replacement updates that same
        # private record only; no published objects are rewritten here.
        temp = report_path.with_suffix(".partial")
        with temp.open("x") as stream:
            json.dump(report, stream, indent=2, sort_keys=True)
            stream.write("\n")
        temp.replace(report_path)

    if report_path.exists() or report_path.is_symlink():
        raise Error("Publication report already exists; inspect the earlier attempt")
    record("preflight")
    try:
        remote_identity(api, identity)
        verify_checks(api, context)
        prefix = f"/repos/{REPOSITORY}/releases"
        # The list includes draft releases for this write-capable token. Do not
        # rely on the tag lookup alone to detect a previously partial draft.
        matches = [item for item in api.pages(prefix) if item.get("tag_name") == identity["tag"]]
        if len(matches) > 1:
            raise Error("Multiple releases use the selected tag; inspect before retry")
        if matches:
            found = matches[0]
            release_id = positive(found.get("id"))
            if any(found.get(key) != value for key, value in metadata.items()):
                raise Error("Existing release has a different identity or description")
            if found.get("draft") is not False:
                raise Error("An existing draft/partial release requires inspection; no automatic resume")
            actual = api.pages(prefix + f"/{release_id}/assets")
            by_name = {item.get("name"): item for item in actual}
            if len(by_name) != len(actual) or set(by_name) != {item["name"] for item in records}:
                raise Error("Existing release has an incomplete or different asset set")
            if not all(asset_equal(api, by_name[item["name"]], item) for item in records):
                raise Error("Existing release asset bytes differ; nothing will be overwritten")
            record("already-published-identical", release_id=release_id)
            return report
        remote_identity(api, identity)
        record("creating-draft-uncertain")
        created = api.request("POST", prefix, {**metadata, "draft": True, "make_latest": "false"})
        release_id = positive(created.get("id"))
        if created.get("draft") is not True or any(created.get(key) != value for key, value in metadata.items()):
            raise Error("Draft creation returned inconsistent metadata; inspect before retry")
        record("draft-created", release_id=release_id)
        remote_identity(api, identity)
        for item in records:
            report["operations"].append({"asset": item["name"], "state": "upload-uncertain"})
            record("upload-uncertain")
            uploaded = api.upload(release_id, directory / item["name"])
            if not asset_equal(api, uploaded, item):
                raise Error("Uploaded asset could not be verified; draft left for inspection")
            report["operations"][-1]["state"] = "verified"
            record("asset-verified")
        remote_identity(api, identity)
        current = api.request("GET", prefix + f"/{release_id}")
        if current.get("draft") is not True or any(current.get(key) != value for key, value in metadata.items()):
            raise Error("Draft changed during upload; inspect before publication")
        actual = api.pages(prefix + f"/{release_id}/assets")
        by_name = {item.get("name"): item for item in actual}
        if (len(by_name) != len(actual) or set(by_name) != {item["name"] for item in records}
                or not all(asset_equal(api, by_name[item["name"]], item) for item in records)):
            raise Error("Final remote asset set does not match the approved assembly")
        verify_checks(api, context)
        remote_identity(api, identity)
        record("publishing-uncertain")
        published = api.request("PATCH", prefix + f"/{release_id}", {"draft": False, "make_latest": "false"})
        if published.get("draft") is not False or any(published.get(key) != value for key, value in metadata.items()):
            raise Error("Release publication was not confirmed; inspect before retry")
        record("published-awaiting-tag-verification")
        remote_identity(api, identity)
        record("published", url=f"https://github.com/{REPOSITORY}/releases/tag/{urllib.parse.quote(identity['tag'], safe='')}")
        return report
    except BaseException:
        # Keep the last possibly completed operation. Never delete a release,
        # delete an asset, move a tag or attempt a second write on uncertainty.
        report["inspection_required"] = True
        record(report["status"])
        raise
