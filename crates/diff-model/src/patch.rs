//! Pure unified patch presentation, hunk range scanning, and bounded word-level diffing.

use crate::gutter::{LineNumbers, prepare_gutter};
use std::{mem::size_of, ops::Range, sync::Arc};

/// Immutable patch metadata prepared by the repository worker and shared with
/// the editor/gutter. Keeping the literal patch separate preserves copy/search.
pub struct PatchPresentation {
    pub rows: Arc<[LineNumbers]>,
    pub column_width: f32,
    pub change_rows: Arc<[usize]>,
    pub ranges: Arc<[DiffRange]>,
}

impl PatchPresentation {
    pub fn prepare(patch: &str) -> Self {
        let (rows, column_width) = prepare_gutter(patch);
        let ranges = review_ranges(patch, &rows);
        let mut previous_changed = false;
        let change_rows = rows
            .iter()
            .zip(patch.split('\n'))
            .enumerate()
            .filter_map(|(row, (numbers, line))| {
                let changed = numbers.old.is_some() != numbers.new.is_some();
                let start = changed && !previous_changed;
                // A no-newline marker must not divide a replacement.
                if !line.starts_with('\\') {
                    previous_changed = changed;
                }
                start.then_some(row)
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            rows,
            column_width,
            change_rows,
            ranges: ranges.into(),
        }
    }

    /// Retained metadata allocations, including the outer Arc payload and the
    /// three Arc reference-count headers; allocator bookkeeping is excluded.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + std::mem::size_of_val(self.rows.as_ref())
            + std::mem::size_of_val(self.ranges.as_ref())
            + std::mem::size_of_val(self.change_rows.as_ref())
            + 4 * 2 * size_of::<usize>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Added,
    Removed,
    Hunk,
    AddedWord,
    RemovedWord,
    /// The `---` and `+++` file-header lines before the first hunk.
    Header,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DiffRange {
    pub range: Range<usize>,
    pub kind: Kind,
}

/// One shared work allowance per file, never quadratic in unbounded line size.
/// Beyond the allowance we retain ordinary whole-line highlighting.
pub struct WordBudget {
    pub remaining: usize,
}

impl Default for WordBudget {
    fn default() -> Self {
        Self {
            remaining: 1_000_000,
        }
    }
}

pub fn word_changes(old: &str, new: &str, budget: &mut WordBudget) -> [Vec<Range<usize>>; 2] {
    if old.len().max(new.len()) > 16 * 1024 || budget.remaining == 0 {
        return Default::default();
    }
    fn tokens(text: &str) -> Option<Vec<Range<usize>>> {
        let mut result: Vec<Range<usize>> = Vec::new();
        let mut previous = None;
        for (offset, ch) in text.char_indices() {
            let kind = if ch.is_alphanumeric() || ch == '_' {
                0
            } else if ch.is_whitespace() {
                1
            } else {
                2
            };
            if previous == Some(kind)
                && kind != 2
                && let Some(last) = result.last_mut()
            {
                last.end = offset + ch.len_utf8();
            } else {
                if result.len() == 256 {
                    return None;
                }
                result.push(offset..offset + ch.len_utf8());
            }
            previous = Some(kind);
        }
        Some(result)
    }
    let (Some(a), Some(b)) = (tokens(old), tokens(new)) else {
        return Default::default();
    };
    let cells = (a.len() + 1) * (b.len() + 1);
    if cells > budget.remaining {
        budget.remaining = 0;
        return Default::default();
    }
    budget.remaining -= cells;
    let width = b.len() + 1;
    let mut lcs = vec![0u16; cells];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * width + j] = if old[a[i].clone()] == new[b[j].clone()] {
                1 + lcs[(i + 1) * width + j + 1]
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let mut result: [Vec<Range<usize>>; 2] = Default::default();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && old[a[i].clone()] == new[b[j].clone()] {
            i += 1;
            j += 1;
            continue;
        }
        let side = usize::from(
            i == a.len() || (j < b.len() && lcs[i * width + j + 1] > lcs[(i + 1) * width + j]),
        );
        let range = if side == 0 {
            let r = a[i].clone();
            i += 1;
            r
        } else {
            let r = b[j].clone();
            j += 1;
            r
        };
        if let Some(last) = result[side].last_mut()
            && last.end == range.start
        {
            last.end = range.end;
        } else {
            result[side].push(range);
        }
    }
    result
}

/// Styles are disjoint: the pinned editor does not give overlapping decoration
/// collections a stable precedence. Find can therefore mask these backgrounds.
pub fn review_ranges(patch: &str, rows: &[LineNumbers]) -> Vec<DiffRange> {
    let lines = patch.split_inclusive('\n').collect::<Vec<_>>();
    let mut offsets = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        offsets.push(offset);
        offset += line.len();
    }
    let mut words: Vec<DiffRange> = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut budget = WordBudget::default();
    let flush = |removed: &mut Vec<usize>,
                 added: &mut Vec<usize>,
                 words: &mut Vec<DiffRange>,
                 budget: &mut WordBudget| {
        for (&old, &new) in removed.iter().zip(added.iter()) {
            let ranges = word_changes(&lines[old][1..], &lines[new][1..], budget);
            for (side, ranges) in ranges.into_iter().enumerate() {
                let row = [old, new][side];
                for range in ranges {
                    words.push(DiffRange {
                        range: (offsets[row] + 1 + range.start)..(offsets[row] + 1 + range.end),
                        kind: if side == 0 {
                            Kind::RemovedWord
                        } else {
                            Kind::AddedWord
                        },
                    });
                }
            }
        }
        removed.clear();
        added.clear();
    };
    for (i, line) in lines.iter().enumerate() {
        let numbers = rows[i];
        match (numbers.old, numbers.new) {
            (Some(_), None) => removed.push(i),
            (None, Some(_)) => added.push(i),
            _ if !line.starts_with('\\') => {
                flush(&mut removed, &mut added, &mut words, &mut budget)
            }
            _ => {}
        }
    }
    flush(&mut removed, &mut added, &mut words, &mut budget);
    words.sort_unstable_by_key(|range| range.range.start);
    let mut result = Vec::new();
    let mut words = words.into_iter().peekable();
    for base in diff_ranges(patch) {
        let mut start = base.range.start;
        while let Some(word) = words.next_if(|word| word.range.start < base.range.end) {
            if word.range.start < start {
                continue;
            }
            if start < word.range.start {
                result.push(DiffRange {
                    range: start..word.range.start,
                    kind: base.kind,
                });
            }
            start = word.range.end;
            result.push(word);
        }
        if start < base.range.end {
            result.push(DiffRange {
                range: start..base.range.end,
                kind: base.kind,
            });
        }
    }
    result
}

/// Track unified hunk counts so `---` / `+++` file headers are not marked as
/// changes, while real changed source beginning with those characters is.
/// The headers before the first hunk are marked as headers; other lines outside
/// hunks stay neutral. Byte offsets include existing line endings and are valid
/// UTF-8 boundaries.
pub fn diff_ranges(value: &str) -> Vec<DiffRange> {
    let mut result: Vec<DiffRange> = Vec::new();
    let mut remaining: Option<(usize, usize)> = None;
    let mut offset = 0;
    let first_hunk = value
        .split_inclusive('\n')
        .try_fold(0, |start, line| match hunk_counts(line) {
            Some(_) => Err(start),
            None => Ok(start + line.len()),
        })
        .err();
    for line in value.split_inclusive('\n') {
        let kind = if let Some(counts) = hunk_counts(line) {
            remaining = Some(counts);
            Some(Kind::Hunk)
        } else if let Some((old, new)) = remaining.as_mut() {
            match line.as_bytes().first() {
                Some(b'+') if *new > 0 => {
                    *new -= 1;
                    Some(Kind::Added)
                }
                Some(b'-') if *old > 0 => {
                    *old -= 1;
                    Some(Kind::Removed)
                }
                Some(b' ') if *old > 0 && *new > 0 => {
                    *old -= 1;
                    *new -= 1;
                    None
                }
                Some(b'\\') => None, // "No newline at end of file" marker.
                _ => {
                    remaining = None;
                    None
                }
            }
        } else if first_hunk.is_some_and(|first| offset < first)
            && (line.starts_with("--- ") || line.starts_with("+++ "))
        {
            Some(Kind::Header)
        } else {
            None
        };
        if remaining == Some((0, 0)) {
            remaining = None;
        }
        let end = offset + line.len();
        if let Some(kind) = kind {
            if let Some(previous) = result.last_mut()
                && previous.kind == kind
                && previous.range.end == offset
            {
                previous.range.end = end;
            } else {
                result.push(DiffRange {
                    range: offset..end,
                    kind,
                });
            }
        }
        offset = end;
    }
    result
}

fn hunk_counts(line: &str) -> Option<(usize, usize)> {
    if !line.starts_with("@@ ") {
        return None;
    }
    let mut fields = line.split_ascii_whitespace();
    if fields.next()? != "@@" {
        return None;
    }
    let old = range_count(fields.next()?.strip_prefix('-')?)?;
    let new = range_count(fields.next()?.strip_prefix('+')?)?;
    if fields.next()? != "@@" {
        return None;
    }
    Some((old, new))
}

fn range_count(range: &str) -> Option<usize> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    start.parse::<usize>().ok()?;
    count.parse().ok()
}
