//! Pure side-by-side split-diff row alignment and per-side presentation.

use crate::gutter::LineNumbers;
use crate::patch::{PatchPresentation, WordBudget, word_changes};
use std::{mem::size_of, ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub changed: bool,
}

pub struct SplitSide {
    pub text: String,
    pub numbers: Arc<[LineNumbers]>,
    pub changes: Vec<Range<usize>>,
    pub words: Vec<Range<usize>>,
    // Only real source bytes participate in copying; alignment blanks do not.
    pub source_spans: Vec<Range<usize>>,
}

impl SplitSide {
    pub fn source_selection(&self, selection: Range<usize>) -> String {
        let mut text = String::new();
        for span in &self.source_spans {
            let mut start = selection.start.max(span.start);
            let mut end = selection.end.min(span.end);
            while start < end && !self.text.is_char_boundary(start) {
                start += 1;
            }
            while end > start && !self.text.is_char_boundary(end) {
                end -= 1;
            }
            if start < end {
                text.push_str(&self.text[start..end]);
            }
            if span.start >= selection.end {
                break;
            }
        }
        text
    }
}

pub struct SplitPresentation {
    pub sides: [SplitSide; 2],
    pub first_change: usize,
    pub change_rows: Arc<[usize]>,
}

impl SplitPresentation {
    pub fn prepare(old: &str, new: &str, patch: &PatchPresentation) -> Self {
        let sources = [
            old.split_inclusive('\n').collect::<Vec<_>>(),
            new.split_inclusive('\n').collect::<Vec<_>>(),
        ];
        let rows = align_rows(&sources, patch);
        Self::from_rows(&sources, &rows)
    }

    pub fn from_rows(sources: &[Vec<&str>; 2], rows: &[Row]) -> Self {
        let mut budget = WordBudget::default();
        let word_rows = rows
            .iter()
            .map(|row| match (row.old, row.new) {
                (Some(old), Some(new)) if row.changed => word_changes(
                    sources[0].get(old).unwrap_or(&""),
                    sources[1].get(new).unwrap_or(&""),
                    &mut budget,
                ),
                _ => Default::default(),
            })
            .collect::<Vec<_>>();
        let change_rows = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| (row.changed && (i == 0 || !rows[i - 1].changed)).then_some(i))
            .collect::<Vec<_>>()
            .into();
        Self {
            change_rows,
            first_change: rows.iter().position(|row| row.changed).unwrap_or(0),
            sides: std::array::from_fn(|side| {
                let mut text = String::new();
                let mut numbers = Vec::new();
                let mut changes = Vec::new();
                let mut words = Vec::new();
                let mut source_spans = Vec::new();
                for (row_index, row) in rows.iter().enumerate() {
                    let number = if side == 0 { row.old } else { row.new };
                    let start = text.len();
                    if let Some(number) = number {
                        if let Some(line) = sources[side].get(number) {
                            text.push_str(line);
                            source_spans.push(start..text.len());
                            if !line.ends_with('\n') {
                                text.push('\n');
                            }
                        }
                    } else {
                        text.push('\n');
                    }
                    if row.changed {
                        changes.push(start..text.len());
                        words.extend(
                            word_rows[row_index][side]
                                .iter()
                                .map(|range| (start + range.start)..(start + range.end)),
                        );
                    }
                    numbers.push(LineNumbers {
                        old: number.map(|n| n as u64 + 1),
                        new: None,
                    });
                }
                numbers.push(LineNumbers::default());
                SplitSide {
                    text,
                    numbers: numbers.into(),
                    changes,
                    words,
                    source_spans,
                }
            }),
        }
    }

    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + std::mem::size_of_val(self.change_rows.as_ref())
            + 2 * size_of::<usize>()
            + self
                .sides
                .iter()
                .map(|side| {
                    side.text.capacity()
                        + std::mem::size_of_val(side.numbers.as_ref())
                        + 2 * size_of::<usize>()
                        + (side.changes.capacity()
                            + side.words.capacity()
                            + side.source_spans.capacity())
                            * size_of::<Range<usize>>()
                })
                .sum::<usize>()
    }
}

pub fn align_rows(sources: &[Vec<&str>; 2], patch: &PatchPresentation) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut cursor = [0, 0];
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let flush = |rows: &mut Vec<Row>, removed: &mut Vec<usize>, added: &mut Vec<usize>| {
        for i in 0..removed.len().max(added.len()) {
            rows.push(Row {
                old: removed.get(i).copied(),
                new: added.get(i).copied(),
                changed: true,
            });
        }
        removed.clear();
        added.clear();
    };
    for numbers in patch.rows.iter() {
        let numbers = [
            numbers.old.map(|n| n as usize - 1),
            numbers.new.map(|n| n as usize - 1),
        ];
        if numbers == [None, None] {
            continue;
        }
        if numbers.iter().all(Option::is_some)
            || numbers
                .iter()
                .enumerate()
                .any(|(side, number)| number.is_some_and(|n| n > cursor[side]))
        {
            flush(&mut rows, &mut removed, &mut added);
        }
        if removed.is_empty() && added.is_empty() {
            let gap = numbers
                .iter()
                .enumerate()
                .filter_map(|(side, number)| number.map(|n| n.saturating_sub(cursor[side])))
                .min()
                .unwrap_or(0);
            for _ in 0..gap {
                rows.push(Row {
                    old: (cursor[0] < sources[0].len()).then_some(cursor[0]),
                    new: (cursor[1] < sources[1].len()).then_some(cursor[1]),
                    changed: false,
                });
                cursor[0] += 1;
                cursor[1] += 1;
            }
        }
        match numbers {
            [Some(old), Some(new)] => rows.push(Row {
                old: Some(old),
                new: Some(new),
                changed: false,
            }),
            [Some(old), None] => removed.push(old),
            [None, Some(new)] => added.push(new),
            _ => unreachable!(),
        }
        for side in 0..2 {
            if let Some(number) = numbers[side] {
                cursor[side] = number + 1;
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    while cursor[0] < sources[0].len() || cursor[1] < sources[1].len() {
        rows.push(Row {
            old: (cursor[0] < sources[0].len()).then_some(cursor[0]),
            new: (cursor[1] < sources[1].len()).then_some(cursor[1]),
            changed: false,
        });
        cursor[0] += 1;
        cursor[1] += 1;
    }
    rows
}
