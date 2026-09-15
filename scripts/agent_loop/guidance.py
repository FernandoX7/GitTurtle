"""Check guidance links and discovery metadata using only the standard library.

This is a structural check, not an evaluation of an agent's instructions. It
does not infer paths from prose or execute commands found in documentation.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import html
import os
from pathlib import Path
import re
import sys
import tomllib
import unicodedata
from urllib.parse import unquote, urlsplit


@dataclass(frozen=True)
class Issue:
    path: Path
    line: int
    message: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: {self.message}"


def guidance_files(root: Path) -> list[Path]:
    """Select maintained sources; prune generated trees when finding guides."""
    paths = {root / name for name in (
        "AGENTS.md", "CONTRIBUTING.md", "docs/agent-guidance.md"
    )}
    for directory, children, files in os.walk(root, followlinks=False):
        children[:] = [name for name in children if name not in {
            ".git", ".local", "target", "node_modules", ".agent", "__pycache__"
        } and not (Path(directory) / name).is_symlink()]
        if "AGENTS.md" in files:
            paths.add(Path(directory) / "AGENTS.md")
    paths.update((root / "docs/development").glob("*.md"))
    paths.update((root / ".agents/skills").glob("**/*.md"))
    paths.update((root / ".codex/agents").glob("*.toml"))
    return sorted(paths)


def _blank(value: str) -> str:
    return "".join("\n" if char == "\n" else " " for char in value)


def _without_fences(text: str) -> str:
    lines = []
    fence = ""
    for line in text.splitlines(keepends=True):
        marker = re.match(r" {0,3}(`{3,}|~{3,})", line)
        if fence:
            lines.append(_blank(line))
            if re.match(r" {0,3}" + re.escape(fence[0]) +
                        "{" + str(len(fence)) + r",}\s*$", line):
                fence = ""
        elif marker:
            fence = marker.group(1)
            lines.append(_blank(line))
        else:
            lines.append(line)
    return re.sub(r"<!--.*?-->", lambda match: _blank(match[0]),
                  "".join(lines), flags=re.DOTALL)


def _without_inline_code(text: str) -> str:
    return re.sub(r"(`+)(?!`)(.+?)(?<!`)\1(?!`)",
                  lambda match: _blank(match[0]), text, flags=re.DOTALL)


def _unescape(value: str) -> str:
    return re.sub(r"\\([!\"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])", r"\1", value)


def _destination(text: str) -> str | None:
    """Read a Markdown destination, excluding an optional link title."""
    text = text.lstrip()
    if text.startswith("<"):
        end = text.find(">")
        return _unescape(text[1:end]) if end >= 0 else None
    depth = 0
    end = 0
    while end < len(text):
        char = text[end]
        if char == "\\" and end + 1 < len(text):
            end += 2
            continue
        if char.isspace() and depth == 0:
            break
        if char == "(":
            depth += 1
        elif char == ")":
            if depth == 0:
                break
            depth -= 1
        end += 1
    return _unescape(text[:end])


def markdown_links(text: str) -> list[tuple[int, str]]:
    """Extract inline and defined reference links, excluding code examples."""
    visible = _without_inline_code(_without_fences(text))
    links = []
    # Check definitions once. Shortcut/reference uses need no duplicate report.
    for match in re.finditer(r"(?m)^ {0,3}\[[^\]\n]+\]:\s*(.*)$", visible):
        destination = _destination(match[1])
        if destination is not None:
            links.append((visible.count("\n", 0, match.start()) + 1, destination))
    # Labels may contain nested brackets (for example, an image in a link).
    for match in re.finditer(r"(?<!\\)\]\(", visible):
        destination = _destination(visible[match.end():])
        if destination is not None:
            links.append((visible.count("\n", 0, match.start()) + 1, destination))
    return links


def markdown_anchors(text: str) -> set[str]:
    """GitHub-style heading slugs, duplicate suffixes, and explicit HTML IDs."""
    visible = _without_fences(text)
    anchors = set(re.findall(r"\b(?:id|name)=[\"']([^\"']+)[\"']",
                             _without_inline_code(visible)))
    counts: dict[str, int] = {}
    lines = visible.splitlines()
    for index, line in enumerate(lines):
        match = re.match(r" {0,3}#{1,6}\s+(.+?)\s*#*\s*$", line)
        heading = match[1] if match else None
        if (heading is None and index + 1 < len(lines) and line.strip()
                and re.fullmatch(r" {0,3}(?:=+|-+)\s*", lines[index + 1])):
            heading = line.strip()
        if heading is None:
            continue
        heading = re.sub(r"<[^>]*>", "", heading)
        heading = re.sub(r"!?\[([^\]]+)\]\([^)]*\)", r"\1", heading)
        heading = html.unescape(_unescape(heading)).lower()
        slug = "".join(char for char in heading if char in "-_ " or
                       not unicodedata.category(char).startswith(("P", "S", "C")))
        slug = slug.replace(" ", "-")
        suffix = counts.get(slug, 0)
        candidate = f"{slug}-{suffix}" if suffix else slug
        while candidate in anchors:
            suffix += 1
            candidate = f"{slug}-{suffix}"
        counts[slug] = suffix + 1
        anchors.add(candidate)
    return anchors


def _link_issues(root: Path, source: Path, text: str,
                 anchors: dict[Path, set[str]]) -> list[Issue]:
    issues = []
    for line, destination in markdown_links(text):
        try:
            url = urlsplit(html.unescape(destination))
        except ValueError as error:
            issues.append(Issue(source, line, f"invalid link {destination!r}: {error}"))
            continue
        if url.scheme or url.netloc:
            continue
        path = unquote(url.path)
        target = ((root / path.lstrip("/")) if path.startswith("/") else
                  (source.parent / path) if path else source).resolve()
        if not target.is_relative_to(root):
            issues.append(Issue(source, line, f"local link escapes repository: {destination}"))
        elif not target.exists():
            issues.append(Issue(source, line, f"missing local link target: {destination}"))
        elif url.fragment and target.suffix.lower() in {".md", ".markdown"}:
            if target not in anchors:
                try:
                    anchors[target] = markdown_anchors(target.read_text(encoding="utf-8"))
                except (OSError, UnicodeError) as error:
                    issues.append(Issue(source, line, f"cannot read link target: {error}"))
                    continue
            if unquote(url.fragment) not in anchors[target]:
                issues.append(Issue(source, line, f"missing Markdown anchor: {destination}"))
    return issues


def _frontmatter_strings(text: str) -> tuple[dict[str, str], list[str]]:
    """Parse the scalar metadata subset used by skills, without a YAML package.

    Unrelated metadata is ignored. Required fields support plain/quoted strings
    and indented literal/folded block strings; structured values are rejected.
    """
    lines = text.splitlines()
    if not lines or lines[0] != "---":
        return {}, ["skill must begin with --- frontmatter"]
    try:
        end = lines.index("---", 1)
    except ValueError:
        return {}, ["skill frontmatter has no closing ---"]
    values = {}
    errors = []
    index = 1
    while index < end:
        match = re.match(r"^(name|description):\s*(.*)$", lines[index])
        index += 1
        if not match:
            continue
        key, value = match.groups()
        if key in values:
            errors.append(f"duplicate skill field: {key}")
        if value in {"|", "|-", "|+", ">", ">-", ">+"}:
            parts = []
            while index < end and (not lines[index] or lines[index][0].isspace()):
                parts.append(lines[index].strip())
                index += 1
            value = (" " if value.startswith(">") else "\n").join(parts)
        elif value.startswith("\""):
            try:
                value = tomllib.loads("value = " + value)["value"]
            except tomllib.TOMLDecodeError:
                errors.append(f"invalid quoted skill field: {key}")
                value = ""
        elif value.startswith("'"):
            match_quote = re.fullmatch(r"'((?:[^']|'')*)'\s*(?:#.*)?", value)
            if match_quote:
                value = match_quote[1].replace("''", "'")
            else:
                errors.append(f"invalid quoted skill field: {key}")
                value = ""
        else:
            value = re.split(r"\s+#", value, maxsplit=1)[0].strip()
            if (value.startswith(("[", "{", "&", "*", "!")) or
                    value.lower() in {"true", "false", "null", "~"} or
                    re.fullmatch(r"[-+]?\d+(?:\.\d+)?", value)):
                errors.append(f"skill field must be a string: {key}")
                value = ""
        values[key] = value
    return values, errors


def _skill_issues(path: Path, text: str) -> list[Issue]:
    fields, errors = _frontmatter_strings(text)
    issues = [Issue(path, 1, error) for error in errors]
    for key in ("name", "description"):
        if not fields.get(key, "").strip():
            issues.append(Issue(path, 1, f"skill requires a nonempty {key}"))
    if fields.get("name") and fields["name"] != path.parent.name:
        issues.append(Issue(path, 1, "skill name must match its directory"))
    if fields.get("name") and not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", fields["name"]):
        issues.append(Issue(path, 1, "skill name must use lowercase letters, digits, and hyphens"))
    return issues


def _agent_issues(path: Path, text: str, names: dict[str, Path]
                  ) -> tuple[list[Issue], str]:
    try:
        fields = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        return [Issue(path, 1, f"invalid agent TOML: {error}")], ""
    issues = []
    for key in ("name", "description", "developer_instructions"):
        if not isinstance(fields.get(key), str) or not fields[key].strip():
            issues.append(Issue(path, 1, f"agent requires a nonempty string {key}"))
    for key in ("model", "model_reasoning_effort", "reasoning_effort"):
        if key in fields:
            issues.append(Issue(path, 1, f"agent {key} must be omitted to inherit session settings"))
    name = fields.get("name")
    if isinstance(name, str) and name.strip():
        if name in names:
            issues.append(Issue(path, 1, f"duplicate agent name {name!r}; also in {names[name]}"))
        names[name] = path
    instructions = fields.get("developer_instructions", "")
    return issues, instructions if isinstance(instructions, str) else ""


def validate(root: Path) -> list[Issue]:
    """Return actionable structural problems in the selected repository."""
    root = root.resolve()
    issues = []
    anchors: dict[Path, set[str]] = {}
    names: dict[str, Path] = {}
    for path in guidance_files(root):
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            issues.append(Issue(path, 1, f"cannot read guidance: {error}"))
            continue
        if path.suffix == ".toml":
            metadata_issues, instructions = _agent_issues(path, text, names)
            issues.extend(metadata_issues)
            start = text.find(instructions) if instructions else -1
            declaration = re.search(r"(?m)^developer_instructions\s*=", text)
            for issue in _link_issues(root, path, instructions, anchors):
                line = 1
                if start >= 0:
                    line = issue.line + text.count("\n", 0, start)
                elif declaration:
                    line = text.count("\n", 0, declaration.start()) + 1
                issues.append(Issue(path, line, issue.message))
        else:
            if path.name == "SKILL.md":
                issues.extend(_skill_issues(path, text))
            issues.extend(_link_issues(root, path, text, anchors))
    return issues


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2],
                        help="repository root (defaults to this script's repository)")
    args = parser.parse_args(argv)
    issues = validate(args.root)
    if issues:
        for issue in issues:
            print(issue, file=sys.stderr)
        print(f"Guidance validation failed: {len(issues)} issue(s).", file=sys.stderr)
        return 1
    print(f"Guidance validation passed: {len(guidance_files(args.root.resolve()))} files; "
          "local links, Markdown anchors, skill metadata, and agent TOML.")
    return 0
