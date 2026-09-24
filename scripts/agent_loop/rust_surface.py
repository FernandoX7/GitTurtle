"""Decide whether a change to app Rust can alter what the app draws.

Native evidence exists so that no rendering change ships unseen, so the question
this module answers is deliberately lopsided: it returns "this may render" for
anything it does not positively recognize as inert. A change is inert only when,
after comments are blanked and `#[cfg(test)]` items removed, it removes no
non-blank line and every line it adds is a declaration that cannot alter the code
compiled into the application: a module, a non-glob import, a constant, a
lint-control attribute or a brace.

The additive requirement carries most of the safety. Editing `const
DEFAULT_INTERFACE_TEXT_SIZE` from 13 to 14 changes every screen and looks exactly
like an inert constant, but it also removes the old line, so it keeps the native
profile. Only a constant nothing reads yet can be added without a removal.
"""

from __future__ import annotations

from difflib import SequenceMatcher
import re


TEST_ATTRIBUTE = "#[cfg(test)]"

_RAW_STRING = re.compile(r"(?:b|c)?r(#*)\"")
_STRING = re.compile(r"(?:b|c)?\"")
_CHAR = re.compile(r"'(?:\\.|[^\\'])'")
_IDENT_TAIL = re.compile(r"[A-Za-z0-9_]")

# Attributes that only steer diagnostics; none of them change generated code.
_LINT_ATTRIBUTE = re.compile(
    r"^#!?\[\s*(?:allow|expect|warn|deny|forbid|doc)\b.*\]$|"
    r"^#!?\[\s*cfg_attr\s*\(\s*not\s*\(\s*test\s*\)\s*,\s*(?:allow|expect|warn|deny|forbid|doc)\b.*\]$"
)
_VISIBILITY = r"(?:pub(?:\s*\([^)]*\))?\s+)?"
_INERT_LINE = (
    re.compile(r"^[{}]$"),
    re.compile(r"^\};$"),
    _LINT_ATTRIBUTE,
    # A glob import can shadow a name the surrounding code already resolved.
    re.compile(rf"^{_VISIBILITY}use\s+[^;*()]+;$"),
    re.compile(rf"^{_VISIBILITY}mod\s+[A-Za-z_][A-Za-z0-9_]*\s*[;{{]$"),
    # A call on the right-hand side could reach anything, so only paths and
    # literals qualify.
    re.compile(rf"^{_VISIBILITY}const\s+[A-Za-z_][A-Za-z0-9_]*\s*:[^=;]+=[^;(){{}}!]*;$"),
)


def scrub(source: str, *, literals: bool = True) -> str:
    """Blank comments, and optionally literal contents, character for character.

    Brace matching needs text in which a `}` inside a string cannot be mistaken
    for code; comparison needs the opposite, because retitling a button is a
    rendering change. Both renditions keep every offset, so spans found in one
    apply to the other.
    """
    out: list[str] = []
    index, end = 0, len(source)
    while index < end:
        character = source[index]
        if source.startswith("//", index):
            stop = source.find("\n", index)
            stop = end if stop < 0 else stop
            out.append(" " * (stop - index))
            index = stop
        elif source.startswith("/*", index):
            depth, stop = 1, index + 2
            while stop < end and depth:
                if source.startswith("/*", stop):
                    depth, stop = depth + 1, stop + 2
                elif source.startswith("*/", stop):
                    depth, stop = depth - 1, stop + 2
                else:
                    stop += 1
            out.append("".join("\n" if item == "\n" else " " for item in source[index:stop]))
            index = stop
        elif (raw := _RAW_STRING.match(source, index)) and not _preceded_by_identifier(source, index):
            closer = "\"" + raw.group(1)
            stop = source.find(closer, raw.end())
            stop = end if stop < 0 else stop + len(closer)
            out.append(_blank_literal(source[index:stop], raw.end() - index, stop - index - len(closer)) if literals else source[index:stop])
            index = stop
        elif (plain := _STRING.match(source, index)) and not _preceded_by_identifier(source, index):
            stop, body = plain.end(), plain.end()
            while stop < end and source[stop] != "\"":
                stop += 2 if source[stop] == "\\" else 1
            stop = min(stop + 1, end)
            out.append(_blank_literal(source[index:stop], body - index, stop - index - 1) if literals else source[index:stop])
            index = stop
        elif character == "'" and (literal := _CHAR.match(source, index)):
            out.append(_blank_literal(literal.group(0), 1, literal.end() - index - 1) if literals else literal.group(0))
            index = literal.end()
        else:
            out.append(character)
            index += 1
    return "".join(out)


def _preceded_by_identifier(source: str, index: int) -> bool:
    return index > 0 and bool(_IDENT_TAIL.match(source[index - 1]))


def _blank_literal(text: str, start: int, stop: int) -> str:
    body = "".join("\n" if item == "\n" else " " for item in text[start:stop])
    return text[:start] + body + text[stop:]


def strip_test_items(scrubbed: str) -> str:
    """Remove `#[cfg(test)]` items; test code cannot reach the shipped binary."""
    return _without(scrubbed, cfg_test_spans(scrubbed))


def cfg_test_spans(neutral: str) -> list[tuple[int, int]]:
    """Offsets of every `#[cfg(test)]` item in literal-blanked text.

    An item whose extent cannot be determined is left alone, which keeps its
    lines in the comparison and so keeps the native profile.
    """
    spans: list[tuple[int, int]] = []
    searched = 0
    while (start := neutral.find(TEST_ATTRIBUTE, searched)) >= 0:
        stop = _item_end(neutral, start + len(TEST_ATTRIBUTE))
        searched = start + len(TEST_ATTRIBUTE)
        if stop is not None:
            spans.append((start, stop))
            searched = stop
    return spans


def _without(text: str, spans: list[tuple[int, int]]) -> str:
    kept, previous = [], 0
    for start, stop in spans:
        kept.append(text[previous:start])
        previous = stop
    kept.append(text[previous:])
    return "".join(kept)


def _item_end(text: str, index: int) -> int | None:
    end = len(text)
    while index < end:
        character = text[index]
        if character.isspace():
            index += 1
        elif character == "#":
            index = _balanced(text, text.find("[", index), "[", "]")
            if index is None:
                return None
        elif character == ";":
            return index + 1
        elif character == "{":
            return _balanced(text, index, "{", "}")
        else:
            index += 1
    return None


def _balanced(text: str, index: int, opener: str, closer: str) -> int | None:
    if index < 0:
        return None
    depth = 0
    for position in range(index, len(text)):
        if text[position] == opener:
            depth += 1
        elif text[position] == closer:
            depth -= 1
            if not depth:
                return position + 1
    return None


def surface(source: str) -> list[str]:
    """The lines of `source` that can affect what the application compiles."""
    return _without(scrub(source, literals=False), cfg_test_spans(scrub(source))).splitlines()


def inert_line(line: str) -> bool:
    stripped = line.strip()
    return not stripped or any(pattern.match(stripped) for pattern in _INERT_LINE)


def renders(before: str, after: str) -> bool:
    """Whether the change from `before` to `after` may alter rendering."""
    previous, current = surface(before), surface(after)
    matcher = SequenceMatcher(a=previous, b=current, autojunk=False)
    for tag, start, stop, added_start, added_stop in matcher.get_opcodes():
        if tag == "equal":
            continue
        if any(line.strip() for line in previous[start:stop]):
            return True
        if not all(inert_line(line) for line in current[added_start:added_stop]):
            return True
    return False
