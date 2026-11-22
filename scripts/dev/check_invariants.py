#!/usr/bin/env python3
"""Fast build-free architectural fitness linter for GitTurtle invariants.

Checks crate-level boundaries documented in AGENTS.md in <0.2 s without Cargo:
1. crates/git-core declares `#![forbid(unsafe_code)]`,
   `#![cfg_attr(not(test), deny(clippy::unwrap_used))]`, and zero gpui/app deps.
2. Production `Command::new("git")` calls live only in crates/git-core's
   centralized passive (`lib.rs`) and write (`work.rs`) command builders.
3. Production code in crates/preview/src/ never touches `std::fs`, `std::net`,
   `gpui`, `File::open`/`create`, `OpenOptions`, or `Command::new`, and
   crates/preview/Cargo.toml has zero `gpui*` dependencies.
4. Production GPUI `fn render(` bodies in crates/app/src/ never execute
   blocking filesystem I/O, process spawns, or `std::thread::sleep`.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import sys
import tomllib

ALLOWED_GIT_COMMAND_FILES = frozenset({
    "crates/git-core/src/lib.rs",
    "crates/git-core/src/work.rs",
})

GIT_COMMAND_RE = re.compile(r'\b(?:std::process::)?Command::new\(\s*"git"\s*\)')
PREVIEW_FORBIDDEN_RE = re.compile(
    r"\b(?:std::fs\b|std::net\b|gpui(?:_kit|_base|_component|_pre)?::"
    r"|use\s+gpui(?:_kit|_base|_component|_pre)?\b"
    r"|File::(?:open|create|create_new)\s*\("
    r"|OpenOptions::new\s*\("
    r"|(?:std::process::)?Command::new\s*\()"
)
RENDER_FN_RE = re.compile(r"\bfn\s+render\s*(?:<[^>]*>)?\s*\(")
RENDER_BLOCKING_RE = re.compile(
    r"\b(?:std::fs::(?:write|read|read_to_string|create_dir|create_dir_all"
    r"|remove_file|remove_dir|remove_dir_all|rename|copy|metadata|symlink_metadata"
    r"|read_dir|OpenOptions|File)\b"
    r"|(?:std::process::)?Command::new\s*\("
    r"|std::thread::sleep\s*\()"
)
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\(([^]]*)\)\s*\]")
PATH_ATTR_RE = re.compile(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]')
MOD_DECL_RE = re.compile(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")

# C-speed Rust lexer for comments, raw strings, string literals, and char/byte literals.
RUST_TOKEN_RE = re.compile(
    r"(?P<line_comment>//[^\n]*)"
    r"|(?P<block_comment>/\*)"
    r'|(?P<raw_string>\bb?r(?P<hashes>#*)".*?"(?P=hashes))'
    r'|(?P<string>"(?:\\.|[^"\\])*")'
    r"|(?P<char>'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\}|.)|[^'\\\n])')",
    re.DOTALL,
)
BRACE_OR_SEMI_RE = re.compile(r"[{};]")
NEWLINE_PRESERVE_RE = re.compile(r"[^\n]")


@dataclass(frozen=True)
class Violation:
    path: str
    line: int
    rule: str
    message: str

    def format(self) -> str:
        return f"{self.path}:{self.line}: [{self.rule}] {self.message}"


CFG_TOKEN_RE = re.compile(r'\s*(?:(?P<lparen>\()|(?P<rparen>\))|(?P<comma>,)|(?P<eq>=)|(?P<str>"(?:\\.|[^"\\])*")|(?P<ident>[A-Za-z_][A-Za-z0-9_]*))')


def _eval_cfg_without_test(expr: str) -> str:
    """Evaluate a Rust cfg(...) expression in 3-valued logic (T/F/U) assuming
    `test` and `feature = "test-support"` are both false and all other
    predicates are unknown.
    """
    expr = expr.strip()
    tokens: list[tuple[str, str]] = []
    pos = 0
    while pos < len(expr):
        match = CFG_TOKEN_RE.match(expr, pos)
        if not match or match.end() == pos:
            return "U"
        kind = match.lastgroup
        if kind is not None:
            tokens.append((kind, match.group(kind)))
        pos = match.end()
    idx = 0

    def parse_expr() -> str:
        nonlocal idx
        if idx >= len(tokens):
            return "U"
        kind, val = tokens[idx]
        if kind != "ident":
            idx += 1
            return "U"
        idx += 1
        if idx < len(tokens) and tokens[idx][0] == "lparen":
            idx += 1
            args: list[str] = []
            while idx < len(tokens) and tokens[idx][0] != "rparen":
                args.append(parse_expr())
                if idx < len(tokens) and tokens[idx][0] == "comma":
                    idx += 1
            if idx < len(tokens) and tokens[idx][0] == "rparen":
                idx += 1
            if val == "not":
                inner = args[0] if args else "U"
                return {"T": "F", "F": "T", "U": "U"}[inner]
            if val == "all":
                if any(a == "F" for a in args):
                    return "F"
                return "T" if all(a == "T" for a in args) else "U"
            if val == "any":
                if any(a == "T" for a in args):
                    return "T"
                return "F" if args and all(a == "F" for a in args) else "U"
            return "U"
        if idx < len(tokens) and tokens[idx][0] == "eq":
            idx += 1
            rhs = ""
            if idx < len(tokens) and tokens[idx][0] in ("str", "ident"):
                rhs = tokens[idx][1].strip('"')
                idx += 1
            if val == "feature" and rhs == "test-support":
                return "F"
            return "U"
        return "F" if val == "test" else "U"

    return parse_expr()


def _is_test_cfg(expr: str) -> bool:
    """Return True only when a #[cfg(...)] expression cannot evaluate to true
    in production builds (where `test` and `feature = "test-support"` are false).
    """
    return _eval_cfg_without_test(expr) == "F"


def _blank_preserving_newlines(segment: str) -> str:
    if "\n" not in segment:
        return " " * len(segment)
    return NEWLINE_PRESERVE_RE.sub(" ", segment)


def sanitize_rust(text: str, *, need_with_strings: bool = True) -> tuple[str, str]:
    """Return (no_comments_with_strings, no_comments_no_strings) preserving exact offsets/newlines."""
    if "/*" not in text:
        no_strings = RUST_TOKEN_RE.sub(lambda m: _blank_preserving_newlines(m.group(0)), text)
        if not need_with_strings:
            return no_strings, no_strings
        with_strings = RUST_TOKEN_RE.sub(
            lambda m: _blank_preserving_newlines(m.group(0)) if m.lastgroup == "line_comment" else m.group(0),
            text,
        )
        return with_strings, no_strings
    with_strings_parts: list[str] = []
    no_strings_parts: list[str] = []
    n = len(text)
    pos = 0
    while pos < n:
        match = RUST_TOKEN_RE.search(text, pos)
        if not match:
            tail = text[pos:]
            with_strings_parts.append(tail)
            no_strings_parts.append(tail)
            break
        start, end = match.span()
        if start > pos:
            chunk = text[pos:start]
            with_strings_parts.append(chunk)
            no_strings_parts.append(chunk)
        kind = match.lastgroup
        if kind == "line_comment":
            blank = " " * (end - start)
            with_strings_parts.append(blank)
            no_strings_parts.append(blank)
            pos = end
        elif kind == "block_comment":
            depth = 1
            cursor = end
            while cursor < n and depth > 0:
                nxt_open = text.find("/*", cursor)
                nxt_close = text.find("*/", cursor)
                if nxt_close == -1:
                    cursor = n
                    break
                if nxt_open != -1 and nxt_open < nxt_close:
                    depth += 1
                    cursor = nxt_open + 2
                else:
                    depth -= 1
                    cursor = nxt_close + 2
            blank = _blank_preserving_newlines(text[start:cursor])
            with_strings_parts.append(blank)
            no_strings_parts.append(blank)
            pos = cursor
        else:
            token = match.group(0)
            with_strings_parts.append(token)
            no_strings_parts.append(_blank_preserving_newlines(token))
            pos = end
    return "".join(with_strings_parts), "".join(no_strings_parts)


def _cfg_test_spans(no_strings: str) -> list[tuple[int, int]]:
    """Compute (start, end) byte spans for every #[cfg(...test...)] annotated item."""
    spans: list[tuple[int, int]] = []
    n = len(no_strings)
    pos = 0
    while pos < n:
        match = CFG_TEST_RE.search(no_strings, pos)
        if not match:
            break
        if not _is_test_cfg(match.group(1)):
            pos = match.end()
            continue
        cursor = match.end()
        while cursor < n:
            while cursor < n and no_strings[cursor].isspace():
                cursor += 1
            if cursor < n and no_strings[cursor] == "#":
                bracket = no_strings.find("[", cursor)
                if bracket == -1:
                    break
                depth = 1
                cursor = bracket + 1
                while cursor < n and depth > 0:
                    c = no_strings[cursor]
                    if c == "[":
                        depth += 1
                    elif c == "]":
                        depth -= 1
                    cursor += 1
                continue
            break
        end = cursor
        brace_depth = 0
        saw_brace = False
        for sym in BRACE_OR_SEMI_RE.finditer(no_strings, cursor):
            ch = sym.group(0)
            if ch == "{":
                brace_depth += 1
                saw_brace = True
            elif ch == "}":
                if brace_depth > 0:
                    brace_depth -= 1
                    if saw_brace and brace_depth == 0:
                        end = sym.end()
                        break
            elif ch == ";" and not saw_brace:
                end = sym.end()
                break
        else:
            end = n
        spans.append((match.start(), end))
        pos = end
    return spans


def _apply_spans(text: str, spans: list[tuple[int, int]]) -> str:
    if not spans:
        return text
    parts: list[str] = []
    cursor = 0
    for start, end in spans:
        if start > cursor:
            parts.append(text[cursor:start])
        parts.append(_blank_preserving_newlines(text[start:end]))
        cursor = end
    if cursor < len(text):
        parts.append(text[cursor:])
    return "".join(parts)


def discover_test_only_modules(src_root: Path, contents: dict[Path, str] | None = None) -> set[Path]:
    """Find external .rs files included solely via #[cfg(...test...)] mod ...;."""
    test_files: set[Path] = set()
    prod_files: set[Path] = set()
    files = sorted(contents) if contents is not None else sorted(src_root.rglob("*.rs"))
    for rs in files:
        text = contents[rs] if contents is not None else rs.read_text(encoding="utf-8", errors="replace")
        if "mod " not in text:
            continue
        for mod_match in MOD_DECL_RE.finditer(text):
            mod_name = mod_match.group(1)
            prefix = text[max(0, mod_match.start() - 600) : mod_match.start()]
            # Walk backward across contiguous outer attributes (including multi-line
            # #[cfg(all(\n    test,\n    ...\n))] forms), blank lines, and comments.
            lines = prefix.splitlines()
            attr_lines: list[str] = []
            bracket_depth = 0
            for line in reversed(lines):
                stripped = line.strip()
                if not stripped or stripped.startswith("//"):
                    if bracket_depth > 0:
                        attr_lines.append(stripped)
                    continue
                if bracket_depth > 0:
                    attr_lines.append(stripped)
                    bracket_depth += stripped.count("]") - stripped.count("[")
                    if bracket_depth < 0:
                        bracket_depth = 0
                    continue
                if stripped.startswith("#[") or stripped.endswith("]"):
                    attr_lines.append(stripped)
                    bracket_depth += stripped.count("]") - stripped.count("[")
                    if bracket_depth < 0:
                        bracket_depth = 0
                else:
                    break
            attrs = "\n".join(reversed(attr_lines))
            path_override = PATH_ATTR_RE.search(attrs)
            base_dir = rs.parent if rs.stem in ("lib", "main", "mod") else rs.parent / rs.stem
            candidates: list[Path] = []
            if path_override:
                candidates.append((rs.parent / path_override.group(1)).resolve())
                candidates.append((base_dir / path_override.group(1)).resolve())
            else:
                candidates.append((base_dir / f"{mod_name}.rs").resolve())
                candidates.append((base_dir / mod_name / "mod.rs").resolve())
            resolved = next((c for c in candidates if c.is_file()), None)
            if resolved is None:
                continue
            is_test = any(_is_test_cfg(m.group(1)) for m in CFG_TEST_RE.finditer(attrs))
            if is_test:
                test_files.add(resolved)
                if resolved.name == "mod.rs":
                    test_files.update(p.resolve() for p in resolved.parent.rglob("*.rs"))
            else:
                prod_files.add(resolved)
    return test_files - prod_files


def _line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _find_render_bodies(no_strings: str, with_strings: str) -> list[tuple[int, str]]:
    """Return (start_line, body_with_strings) for every `fn render(...) { ... }`."""
    bodies: list[tuple[int, str]] = []
    n = len(no_strings)
    for match in RENDER_FN_RE.finditer(no_strings):
        i = match.end() - 1
        paren = 0
        while i < n:
            c = no_strings[i]
            if c == "(":
                paren += 1
            elif c == ")":
                paren -= 1
                if paren == 0:
                    i += 1
                    break
            i += 1
        while i < n and no_strings[i] not in "{;":
            i += 1
        if i >= n or no_strings[i] != "{":
            continue
        body_start = i
        depth = 0
        body_end = n
        for sym in BRACE_OR_SEMI_RE.finditer(no_strings, body_start):
            ch = sym.group(0)
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    body_end = sym.end()
                    break
        bodies.append((_line_number(no_strings, body_start), with_strings[body_start:body_end]))
    return bodies


def _check_cargo_toml(
    root: Path,
    crate_rel: str,
    forbidden_exact: tuple[str, ...],
    forbidden_prefixes: tuple[str, ...] = ("gpui",),
) -> list[Violation]:
    path = root / crate_rel / "Cargo.toml"
    if not path.is_file():
        return []
    rel = path.relative_to(root).as_posix()
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        return [Violation(rel, 1, "cargo-toml", f"cannot parse Cargo.toml: {error}")]
    violations: list[Violation] = []
    dep_tables: list[dict] = []
    if isinstance(data.get("dependencies"), dict):
        dep_tables.append(data["dependencies"])
    for target in (data.get("target") or {}).values():
        if isinstance(target, dict) and isinstance(target.get("dependencies"), dict):
            dep_tables.append(target["dependencies"])
    for table in dep_tables:
        for dep, spec in sorted(table.items()):
            if dep in forbidden_exact or any(
                dep == prefix or dep.startswith(prefix + "-") or dep.startswith(prefix + "_")
                for prefix in forbidden_prefixes
            ):
                violations.append(
                    Violation(rel, 1, "crate-boundary", f"forbidden production dependency {dep!r} in {crate_rel}")
                )
            if isinstance(spec, dict):
                features = spec.get("features") or []
                if isinstance(features, list) and "test-support" in features:
                    violations.append(
                        Violation(
                            rel,
                            1,
                            "test-support-isolation",
                            f"production dependency {dep!r} enables `test-support` feature in {crate_rel}",
                        )
                    )
    return violations


def check_repo(root: Path) -> list[Violation]:
    """Run all architectural invariant checks against `root`."""
    root = root.resolve()
    violations: list[Violation] = []

    # 1. Headless crate safety attributes and dependency boundaries.
    for crate_rel, forbid_attr in (
        ("crates/git-core", "#![forbid(unsafe_code)]"),
        ("crates/theme", "#![forbid(unsafe_code)]"),
        ("crates/diff-model", "#![forbid(unsafe_code)]"),
        ("crates/github", "#![forbid(unsafe_code)]"),
        ("crates/prefs", '#![cfg_attr(not(any(test, feature = "test-support")), forbid(unsafe_code))]'),
        ("crates/preview", None),
    ):
        lib_path = root / crate_rel / "src/lib.rs"
        if lib_path.is_file():
            rel_lib = f"{crate_rel}/src/lib.rs"
            lib_with_strings, _ = sanitize_rust(lib_path.read_text(encoding="utf-8"))
            if forbid_attr and forbid_attr not in lib_with_strings:
                violations.append(
                    Violation(rel_lib, 1, "crate-safety", f"missing `{forbid_attr}`")
                )
            if "#![cfg_attr(not(test), deny(clippy::unwrap_used))]" not in lib_with_strings:
                violations.append(
                    Violation(
                        rel_lib,
                        1,
                        "crate-safety",
                        "missing `#![cfg_attr(not(test), deny(clippy::unwrap_used))]`",
                    )
                )

    violations.extend(_check_cargo_toml(root, "crates/git-core", ("gitturtle-preview", "gitturtle-theme", "gitturtle-diff-model", "gitturtle-prefs", "gitturtle-github", "gitturtle")))
    violations.extend(_check_cargo_toml(root, "crates/preview", ("gitturtle-core", "gitturtle-theme", "gitturtle-diff-model", "gitturtle-prefs", "gitturtle-github", "gitturtle")))
    violations.extend(_check_cargo_toml(root, "crates/theme", ("gitturtle-core", "gitturtle-preview", "gitturtle-diff-model", "gitturtle-prefs", "gitturtle-github", "gitturtle")))
    violations.extend(_check_cargo_toml(root, "crates/diff-model", ("gitturtle-theme", "gitturtle-prefs", "gitturtle-github", "gitturtle-preview", "gitturtle")))
    violations.extend(_check_cargo_toml(root, "crates/prefs", ("gitturtle-github", "gitturtle-preview", "gitturtle")))
    violations.extend(_check_cargo_toml(root, "crates/github", ("gitturtle-preview", "gitturtle")))
    violations.extend(_check_cargo_toml(root, "crates/app", (), forbidden_prefixes=()))

    # 2–4. Source scans across all workspace crates.
    for crate_name in ("git-core", "preview", "theme", "diff-model", "prefs", "github", "app"):
        src_root = root / "crates" / crate_name / "src"
        if not src_root.is_dir():
            continue
        contents = {
            rs: rs.read_text(encoding="utf-8", errors="replace")
            for rs in sorted(src_root.rglob("*.rs"))
        }
        test_only = discover_test_only_modules(src_root, contents)
        for rs, raw in contents.items():
            resolved = rs.resolve()
            if resolved in test_only:
                continue
            rel = rs.relative_to(root).as_posix()

            # Fast raw pre-filter so clean files skip full tokenization.
            need_git = rel not in ALLOWED_GIT_COMMAND_FILES and bool(GIT_COMMAND_RE.search(raw))
            need_preview = crate_name == "preview" and bool(PREVIEW_FORBIDDEN_RE.search(raw))
            need_render = (
                crate_name == "app"
                and bool(RENDER_FN_RE.search(raw))
                and bool(RENDER_BLOCKING_RE.search(raw))
            )
            if not (need_git or need_preview or need_render):
                continue

            with_strings, no_strings = sanitize_rust(raw, need_with_strings=need_git)
            spans = _cfg_test_spans(no_strings)
            no_strings = _apply_spans(no_strings, spans)
            with_strings = _apply_spans(with_strings, spans) if need_git else no_strings

            if need_git:
                for match in GIT_COMMAND_RE.finditer(with_strings):
                    violations.append(
                        Violation(
                            rel,
                            _line_number(with_strings, match.start()),
                            "git-command-boundary",
                            'Command::new("git") is confined to crates/git-core centralized builders',
                        )
                    )

            if need_preview:
                for match in PREVIEW_FORBIDDEN_RE.finditer(no_strings):
                    violations.append(
                        Violation(
                            rel,
                            _line_number(no_strings, match.start()),
                            "preview-purity",
                            f"forbidden filesystem/network/UI/process call `{match.group(0).strip()}` in crates/preview/src/",
                        )
                    )

            if need_render:
                for body_line, body in _find_render_bodies(no_strings, no_strings):
                    for match in RENDER_BLOCKING_RE.finditer(body):
                        line = body_line + body.count("\n", 0, match.start())
                        violations.append(
                            Violation(
                                rel,
                                line,
                                "ui-render-nonblocking",
                                f"blocking I/O or process spawn `{match.group(0).strip()}` inside fn render()",
                            )
                        )

    return violations


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Architectural fitness linter for GitTurtle.")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="Repository root (default: this script's repository)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    violations = check_repo(args.root)
    if violations:
        for violation in violations:
            print(violation.format(), file=sys.stderr)
        print(f"Architectural invariant check failed: {len(violations)} violation(s).", file=sys.stderr)
        return 1
    print('Architectural invariants passed: git-core safety, Command::new("git") boundary, preview purity, UI render non-blocking.')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
