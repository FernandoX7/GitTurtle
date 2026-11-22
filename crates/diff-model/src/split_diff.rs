//! Pure side-by-side split-diff row alignment and per-side presentation.

use crate::gutter::LineNumbers;
use crate::partial_view::PartialRow;
use crate::patch::{PatchPresentation, WordBudget, word_changes};
use anyhow::{Result, ensure};
use gitturtle_core::{PartialDiff, PartialHunk, PartialLineKind};
use std::{collections::HashMap, mem::size_of, ops::Range, sync::Arc};

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
    pub row_changed: Arc<[bool]>,
}

/// One contiguous changed block in an aligned split view, anchored at its
/// first aligned row (`start_row`) and holding the exact core change IDs
/// across both Before and After sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitChangeBlock {
    pub start_row: usize,
    pub row_count: usize,
    pub ids: Arc<[usize]>,
}

/// Worker-prepared partial-staging and partial-discard actions aligned to an
/// [`SplitPresentation`]. Selection IDs come from the core snapshot, never
/// from copied editor text.
pub struct SplitPartialActions {
    pub diff: Arc<PartialDiff>,
    pub sides: [Arc<[PartialRow]>; 2],
    pub blocks: Arc<[SplitChangeBlock]>,
}

impl SplitPartialActions {
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + size_of::<PartialDiff>()
            + self.diff.bytes()
            + self
                .sides
                .iter()
                .map(|rows| std::mem::size_of_val(rows.as_ref()) + 2 * size_of::<usize>())
                .sum::<usize>()
            + std::mem::size_of_val(self.blocks.as_ref())
            + 2 * size_of::<usize>()
            + self
                .blocks
                .iter()
                .map(|block| std::mem::size_of_val(block.ids.as_ref()) + 2 * size_of::<usize>())
                .sum::<usize>()
    }

    pub fn block_at_row(&self, row: usize) -> Option<&SplitChangeBlock> {
        self.blocks
            .binary_search_by_key(&row, |block| block.start_row)
            .ok()
            .map(|index| &self.blocks[index])
    }
}

/// Map `diff`'s captured change IDs onto every aligned row and contiguous
/// change block of `split`. Fails closed if any displayed changed line
/// disagrees with the core snapshot.
pub fn prepare_split_partial(
    split: &SplitPresentation,
    diff: Arc<PartialDiff>,
) -> Result<SplitPartialActions> {
    let (sides, blocks) = prepare_split_rows(split, &diff.hunks)?;
    Ok(SplitPartialActions {
        diff,
        sides,
        blocks,
    })
}

type SplitPartialMapping = ([Arc<[PartialRow]>; 2], Arc<[SplitChangeBlock]>);

fn prepare_split_rows(
    split: &SplitPresentation,
    hunks: &[PartialHunk],
) -> Result<SplitPartialMapping> {
    let mut changes = HashMap::new();
    for line in hunks.iter().flat_map(|hunk| &hunk.lines) {
        let (Some(id), Some(number)) = (line.change_id, line.old_line.or(line.new_line)) else {
            continue;
        };
        changes.insert(
            (line.kind == PartialLineKind::Addition, number),
            (id, line.text.as_str()),
        );
    }
    let expected = changes.len();
    let row_count = split.row_changed.len();
    let rendered = [
        split.sides[0]
            .text
            .split_inclusive('\n')
            .collect::<Vec<_>>(),
        split.sides[1]
            .text
            .split_inclusive('\n')
            .collect::<Vec<_>>(),
    ];
    let mut side_rows = [
        vec![PartialRow::None; split.sides[0].numbers.len()],
        vec![PartialRow::None; split.sides[1].numbers.len()],
    ];
    let mut blocks = Vec::new();
    let mut selected = 0usize;
    let mut row = 0usize;
    while row < row_count {
        if !split.row_changed[row] {
            row += 1;
            continue;
        }
        let start_row = row;
        let mut block_ids = Vec::new();
        while row < row_count && split.row_changed[row] {
            for side in 0..2 {
                let Some(number) = split.sides[side]
                    .numbers
                    .get(row)
                    .and_then(|numbers| numbers.old)
                else {
                    continue;
                };
                let added = side == 1;
                let line_number = number as usize;
                let Some((id, source)) = changes.remove(&(added, line_number)) else {
                    anyhow::bail!(
                        "Changed lines no longer match the working snapshot. Refresh to select lines."
                    );
                };
                let actual = rendered[side].get(row).copied().unwrap_or_default();
                ensure!(
                    actual == source
                        || (!source.ends_with('\n') && actual.strip_suffix('\n') == Some(source)),
                    "Changed text no longer matches the working snapshot. Refresh to select lines."
                );
                side_rows[side][row] = PartialRow::Change {
                    id,
                    added,
                    line: line_number,
                };
                block_ids.push(id);
                selected += 1;
            }
            row += 1;
        }
        if !block_ids.is_empty() {
            blocks.push(SplitChangeBlock {
                start_row,
                row_count: row - start_row,
                ids: block_ids.into(),
            });
        }
    }
    ensure!(
        selected > 0 && selected == expected && changes.is_empty(),
        "The complete set of changed lines is unavailable. Use the whole-file action."
    );
    let [before_rows, after_rows] = side_rows;
    Ok(([before_rows.into(), after_rows.into()], blocks.into()))
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
        let row_changed: Arc<[bool]> = rows
            .iter()
            .map(|row| row.changed)
            .collect::<Vec<_>>()
            .into();
        Self {
            change_rows,
            row_changed,
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
            + std::mem::size_of_val(self.row_changed.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use gitturtle_core::PartialLine;

    fn change(id: usize, added: bool, number: usize, text: &str) -> PartialLine {
        PartialLine {
            kind: if added {
                PartialLineKind::Addition
            } else {
                PartialLineKind::Deletion
            },
            old_line: (!added).then_some(number),
            new_line: added.then_some(number),
            text: text.into(),
            change_id: Some(id),
        }
    }

    #[test]
    fn maps_replacements_additions_and_blocks_across_both_split_sides() {
        let old = "ctx\nold1\nkeep\nold2";
        let new = "ctx\nnew1\nnew2\nkeep\nnew3";
        let patch = "--- a/f\n+++ b/f\n@@ -1,4 +1,5 @@\n ctx\n-old1\n+new1\n+new2\n keep\n-old2\n+new3\n\\ No newline at end of file\n";
        let presentation = PatchPresentation::prepare(patch);
        let split = SplitPresentation::prepare(old, new, &presentation);
        let hunks = [PartialHunk {
            header: "@@ -1,4 +1,5 @@".into(),
            lines: vec![
                change(0, false, 2, "old1\n"),
                change(1, true, 2, "new1\n"),
                change(2, true, 3, "new2\n"),
                change(3, false, 4, "old2"),
                change(4, true, 5, "new3"),
            ],
        }];
        let (sides, blocks) = prepare_split_rows(&split, &hunks).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].start_row, 1);
        assert_eq!(blocks[0].row_count, 2);
        assert_eq!(blocks[0].ids.as_ref(), &[0, 1, 2]);
        assert_eq!(blocks[1].start_row, 4);
        assert_eq!(blocks[1].row_count, 1);
        assert_eq!(blocks[1].ids.as_ref(), &[3, 4]);
        assert_eq!(
            sides[0][1],
            PartialRow::Change {
                id: 0,
                added: false,
                line: 2
            }
        );
        assert_eq!(sides[0][2], PartialRow::None);
        assert_eq!(
            sides[1][1],
            PartialRow::Change {
                id: 1,
                added: true,
                line: 2
            }
        );
        assert_eq!(
            sides[1][2],
            PartialRow::Change {
                id: 2,
                added: true,
                line: 3
            }
        );
        assert_eq!(
            sides[0][4],
            PartialRow::Change {
                id: 3,
                added: false,
                line: 4
            }
        );
        assert_eq!(
            sides[1][4],
            PartialRow::Change {
                id: 4,
                added: true,
                line: 5
            }
        );
    }

    #[test]
    fn rejects_split_partial_when_text_or_ids_mismatch() {
        let old = "a\n";
        let new = "b\n";
        let patch = "@@ -1 +1 @@\n-a\n+b\n";
        let split = SplitPresentation::prepare(old, new, &PatchPresentation::prepare(patch));
        let wrong_text = [PartialHunk {
            header: String::new(),
            lines: vec![change(0, false, 1, "x\n"), change(1, true, 1, "b\n")],
        }];
        assert!(prepare_split_rows(&split, &wrong_text).is_err());

        let missing_id = [PartialHunk {
            header: String::new(),
            lines: vec![change(0, false, 1, "a\n")],
        }];
        assert!(prepare_split_rows(&split, &missing_id).is_err());
    }
}
