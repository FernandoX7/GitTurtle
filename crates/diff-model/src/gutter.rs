//! Pure unified-patch line-number gutter parsing and post-write row landing.

use crate::partial_view::{PartialActions, PartialRow};
use std::{collections::BTreeSet, sync::Arc};

pub const FONT_SIZE: f32 = 12.;
/// The space beside a line number inside its gutter column, at the default code size; a
/// number ends this far inside its column's right edge.
pub const CELL_PADDING: f32 = 9.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineNumbers {
    pub old: Option<u64>,
    pub new: Option<u64>,
}

/// A row's place in the file ([`line_places`]): the number of its line on
/// the side a partial write leaves alone, or of the next such line, whether
/// the row is that line, and else its position in its run of such rows.
pub type Place = (u64, bool, usize);

/// Each row's place in the file, read from its number on the side a partial
/// write leaves alone (`staged`: HEAD's, else the worktree's), increasing
/// down the patch: a numbered line is `(number, true, 0)`, and any other row,
/// an added line in the staged area, a removed line in the unstaged one or a
/// header, `(next number, false, k)`, just before the line that follows, `k`
/// its position among the rows of its run that the write leaves, so that
/// each keeps its own place. A row of a run that the write sends
/// (`written`, as [`written_rows`] reads them), which it takes away, has the
/// place of the next row it leaves in its run. Placed alike, every row of a run landed on its first: a caret on
/// the fourth of five staged added lines went to the first as a line of
/// another hunk was unstaged, and the first visible line with it; counted
/// with the rows the write took, a caret on the fourth went to the fifth as
/// the second was unstaged.
pub fn line_places(rows: &[LineNumbers], staged: bool, written: &[bool]) -> Vec<Place> {
    let mut run = 0;
    let mut places: Vec<Place> = rows
        .iter()
        .enumerate()
        .map(
            |(row, numbers)| match if staged { numbers.old } else { numbers.new } {
                Some(number) => {
                    run = 0;
                    (number, true, 0)
                }
                None if written.get(row).copied().unwrap_or(false) => (u64::MAX, false, run),
                None => {
                    run += 1;
                    (u64::MAX, false, run - 1)
                }
            },
        )
        .collect();
    let mut next = u64::MAX;
    for place in places.iter_mut().rev() {
        if place.1 {
            next = place.0;
        } else {
            place.0 = next;
        }
    }
    places
}

/// The rows before the patch's first numbered line, its file headers and
/// first hunk's header.
pub fn header_rows(rows: &[LineNumbers]) -> usize {
    rows.iter()
        .position(|numbers| numbers.old.is_some() || numbers.new.is_some())
        .unwrap_or(rows.len())
}

/// Which rows of the patch `partial` was drawn over a partial write of the
/// changed lines `ids` sends: those lines, and the header of a hunk whose
/// every line it sends, as the hunk goes with them. Those numbered on the
/// side the write leaves alone stay, as context, and the others go.
pub fn written_rows(partial: Option<&PartialActions>, ids: &BTreeSet<usize>) -> Vec<bool> {
    let Some(partial) = partial.filter(|_| !ids.is_empty()) else {
        return Vec::new();
    };
    partial
        .rows
        .iter()
        .map(|row| match row {
            PartialRow::Change { id, .. } => ids.contains(id),
            PartialRow::Hunk(hunk) => hunk.iter().all(|id| ids.contains(id)),
            PartialRow::None => false,
        })
        .collect()
}

/// Where the rows of the patch `old` land in the patch `new`, read again
/// after a partial write ([`DiffView::land`]), each by its place
/// ([`line_places`]), `taken` the rows of `old` the write took away.
pub struct Landing<'a> {
    new: &'a [LineNumbers],
    old_places: Vec<Place>,
    new_places: Vec<Place>,
    taken: Vec<bool>,
    old_headers: usize,
    new_headers: usize,
}

impl<'a> Landing<'a> {
    /// `written` the rows of `old` the write sent ([`written_rows`]).
    pub fn new(
        old: &[LineNumbers],
        new: &'a [LineNumbers],
        staged: bool,
        written: &[bool],
    ) -> Self {
        let old_places = line_places(old, staged, written);
        // Sent, a row numbered on the side the write leaves alone stays,
        // as context: the caret on a staged added line stays on it.
        let taken = written
            .iter()
            .zip(&old_places)
            .map(|(written, place)| *written && !place.1)
            .collect();
        Self {
            new,
            old_places,
            new_places: line_places(new, staged, &[]),
            taken,
            old_headers: header_rows(old),
            new_headers: header_rows(new),
        }
    }

    /// The first row of `new` at or after the place of `row`, else the last.
    pub fn at(&self, row: usize) -> usize {
        let last = self.new.len().saturating_sub(1);
        let Some(place) = self.old_places.get(row) else {
            return row.min(last);
        };
        self.new_places
            .partition_point(|other| other < place)
            .min(last)
    }

    /// Where `row` lands as the first visible line: on its own place where
    /// `new` shows it, a header row by its place among the headers, else on
    /// the first row after where it stood.
    pub fn line(&self, row: usize) -> usize {
        if row < self.old_headers {
            return row
                .min(self.new_headers.saturating_sub(1))
                .min(self.new.len().saturating_sub(1));
        }
        self.at(row)
    }

    /// Where the caret on `row` lands: on its own place where `new` shows
    /// it, else on the nearest changed line after it, else before, else
    /// where [`Self::line`] puts it. A row the write took away shows
    /// nowhere, though the next row of its run has its place.
    pub fn caret(&self, row: usize) -> usize {
        let at = self.at(row);
        let place = self.old_places.get(row);
        let taken = self.taken.get(row).copied().unwrap_or(false);
        if !taken && place.is_some() && place == self.new_places.get(at) {
            return at;
        }
        let new = self.new;
        let changed = |row: &usize| new[*row].old.is_some() != new[*row].new.is_some();
        (at..new.len())
            .find(changed)
            .or_else(|| (0..at).rev().find(changed))
            .unwrap_or_else(|| self.line(row))
    }
}

/// Pure worker-side preparation; the UI constructor only shares these results.
pub fn prepare_gutter(patch: &str) -> (Arc<[LineNumbers]>, f32) {
    let rows: Arc<[LineNumbers]> = parse_rows(patch).into();
    let widest = rows
        .iter()
        .flat_map(|row| [row.old, row.new])
        .flatten()
        .max()
        .unwrap_or(0);
    (rows, line_number_column_width(widest))
}

/// The width at the default code size of a line-number column that holds `widest`, at least
/// three digits, with [`CELL_PADDING`] on either side. Compare's gutter and the GitHub
/// review's Changes rows share it, so a patch's numbers stand at one place in both.
pub fn line_number_column_width(widest: u64) -> f32 {
    let digits = (widest.checked_ilog10().unwrap_or(0) + 1).max(3);
    digits as f32 * FONT_SIZE * 0.65 + CELL_PADDING * 2.
}

#[derive(Clone, Copy)]
struct HunkSide {
    next: u64,
    remaining: u64,
}

impl HunkSide {
    fn consume(&mut self) -> Option<u64> {
        if self.remaining == 0 {
            return None;
        }
        let number = self.next;
        self.remaining -= 1;
        self.next = self.next.saturating_add(1);
        Some(number)
    }
}

/// One entry per LF-delimited editor row, including the empty final row. CRLF
/// stays untouched and is treated as one line, matching GPUI's LF line metric.
pub fn parse_rows(patch: &str) -> Vec<LineNumbers> {
    let mut hunk: Option<(HunkSide, HunkSide)> = None;
    patch
        .split('\n')
        .map(|line| {
            if let Some(sides) = parse_hunk(line) {
                hunk = Some(sides);
                return LineNumbers::default();
            }
            let Some((old, new)) = hunk.as_mut() else {
                return LineNumbers::default();
            };
            let numbers = match line.as_bytes().first() {
                Some(b'-') if old.remaining > 0 => LineNumbers {
                    old: old.consume(),
                    new: None,
                },
                Some(b'+') if new.remaining > 0 => LineNumbers {
                    old: None,
                    new: new.consume(),
                },
                Some(b' ') if old.remaining > 0 && new.remaining > 0 => LineNumbers {
                    old: old.consume(),
                    new: new.consume(),
                },
                Some(b'\\') if line.trim_end_matches('\r') == "\\ No newline at end of file" => {
                    LineNumbers::default()
                }
                _ => {
                    hunk = None;
                    return LineNumbers::default();
                }
            };
            if hunk
                .as_ref()
                .is_some_and(|(old, new)| old.remaining == 0 && new.remaining == 0)
            {
                hunk = None;
            }
            numbers
        })
        .collect()
}

fn parse_hunk(line: &str) -> Option<(HunkSide, HunkSide)> {
    // A context line can itself contain header-looking source after its leading
    // space. Only a marker in column zero starts a new unified hunk.
    if !line.starts_with("@@ ") {
        return None;
    }
    let mut fields = line.split_ascii_whitespace();
    if fields.next()? != "@@" {
        return None;
    }
    let old = parse_side(fields.next()?.strip_prefix('-')?)?;
    let new = parse_side(fields.next()?.strip_prefix('+')?)?;
    if fields.next()? != "@@" {
        return None;
    }
    Some((old, new))
}

fn parse_side(range: &str) -> Option<HunkSide> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    let next: u64 = start.parse().ok()?;
    let remaining: u64 = count.parse().ok()?;
    if remaining > 0 {
        if next == 0 {
            return None;
        }
        next.checked_add(remaining - 1)?;
    }
    Some(HunkSide { next, remaining })
}
