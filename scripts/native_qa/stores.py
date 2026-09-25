"""Preference stores seeded into a launch's XDG_CONFIG_HOME before the app starts.

A generated store selects a built-in theme with Follow system off, so the
desktop's colour scheme cannot change the frame, and can list projects.
Stores with custom themes are supplied as files and copied byte for byte:
their tokens are resolved from the exercised source, which this module does
not mirror.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

# `STORE_VERSION` in crates/app/src/preferences.rs; test_stores checks it.
STORE_VERSION = 6


def parse_project(spec: str) -> tuple[str, str | None]:
    """`PATH` or `PATH=NAME`; the path must be absolute so the store never depends on the cwd."""
    path, _, name = spec.partition("=")
    if not Path(path).is_absolute():
        raise SystemExit(f"refusing: project path {path!r} must be absolute")
    return path, (name or None)


def store_text(theme: str = "midnight", projects: list[str] = (), follow_system: bool = False,
               settings: dict | None = None) -> bytes:
    store: dict = {"version": STORE_VERSION, "settings": {"theme": theme, "follow_system": follow_system}}
    store["settings"].update(settings or {})
    parsed = [parse_project(spec) for spec in projects]
    if parsed:
        store["recent_repositories"] = [path for path, _ in parsed]
        store["project_library"] = [{"project": {"path": path}} for path, _ in parsed]
        names = [{"path": path, "name": name} for path, name in parsed if name]
        if names:
            store["project_names"] = names
    return json.dumps(store).encode()


def load_file(path: Path) -> bytes:
    """A supplied store, validated as JSON with a version but otherwise unchanged."""
    data = Path(path).read_bytes()
    try:
        version = json.loads(data).get("version")
    except (ValueError, AttributeError) as error:
        raise SystemExit(f"refusing: {path} is not a JSON preference store ({error})")
    if not isinstance(version, int):
        raise SystemExit(f"refusing: {path} has no integer version")
    return data


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()
