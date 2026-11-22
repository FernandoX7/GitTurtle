//! Pure bounded text-review patch transformation and change navigation.

use crate::patch::PatchPresentation;
use crate::split_diff::{self, Row, SplitPresentation};
use anyhow::{Result, ensure};
use std::{ops::Range, sync::Arc};

pub const CONTEXT_STEPS: [usize; 4] = [3, 12, 48, 192];
pub const MAX_REVIEW_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REVIEW_ROWS: usize = 100_000;
pub const MAX_REVIEW_HUNKS: usize = 4096;
/// The collapsed Options button's tooltip: the explanations of the options

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub hide_whitespace: bool,
    pub context: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            hide_whitespace: false,
            context: 3,
        }
    }
}

/// Prepared text review presentation produced from a captured text snapshot.
pub struct ReviewedText {
    pub patch: String,
    pub presentation: Arc<PatchPresentation>,
    pub split: Arc<SplitPresentation>,
}

/// Transform a captured text comparison using review `options` (hide whitespace,
/// expanded context) without any Git, filesystem, or UI access.
pub fn prepare_text_review(
    patch: &str,
    old: &str,
    new: &str,
    presentation: &PatchPresentation,
    options: Options,
    checkpoint: impl Fn() -> Result<()>,
) -> Result<ReviewedText> {
    checkpoint()?;
    ensure!(
        CONTEXT_STEPS.contains(&options.context),
        "Choose a supported context size."
    );
    ensure!(
        patch.len().max(old.len()).max(new.len()) <= MAX_REVIEW_BYTES,
        "This comparison exceeds the text review limit. Use the original diff or source tabs."
    );
    let sources = [
        old.split_inclusive('\n').collect::<Vec<_>>(),
        new.split_inclusive('\n').collect::<Vec<_>>(),
    ];
    ensure!(
        sources[0].len() + sources[1].len() <= MAX_REVIEW_ROWS,
        "This file exceeds the 100,000-line review limit. Use the original diff or source tabs."
    );
    checkpoint()?;
    let mut rows = split_diff::align_rows(&sources, presentation);
    if options.hide_whitespace {
        for (i, row) in rows.iter_mut().enumerate() {
            if i % 256 == 0 {
                checkpoint()?;
            }
            if row.changed && whitespace_only(row, &sources) {
                row.changed = false;
            }
        }
    }
    let patch = review_patch(patch, &sources, &rows, options.context, &checkpoint)?;
    checkpoint()?;
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    checkpoint()?;
    let split = Arc::new(SplitPresentation::from_rows(&sources, &rows));
    checkpoint()?;
    Ok(ReviewedText {
        patch,
        presentation,
        split,
    })
}

fn whitespace_only(row: &Row, sources: &[Vec<&str>; 2]) -> bool {
    let old = row
        .old
        .and_then(|i| sources[0].get(i))
        .copied()
        .unwrap_or("");
    let new = row
        .new
        .and_then(|i| sources[1].get(i))
        .copied()
        .unwrap_or("");
    old.chars()
        .filter(|c| !c.is_whitespace())
        .eq(new.chars().filter(|c| !c.is_whitespace()))
}

fn review_patch(
    original: &str,
    sources: &[Vec<&str>; 2],
    rows: &[Row],
    context: usize,
    checkpoint: &impl Fn() -> Result<()>,
) -> Result<String> {
    let mut groups: Vec<Range<usize>> = Vec::new();
    // A hidden insertion/deletion cannot be represented as a two-sided context
    // line. It divides review hunks instead, keeping both gutters honest.
    let mut segment = 0;
    while segment < rows.len() {
        let end = rows[segment..]
            .iter()
            .position(|row| !row.changed && (row.old.is_none() || row.new.is_none()))
            .map_or(rows.len(), |i| segment + i);
        let group_start = groups.len();
        for (i, row) in rows.iter().enumerate().take(end).skip(segment) {
            if i % 256 == 0 {
                checkpoint()?;
            }
            if !row.changed {
                continue;
            }
            let span = i.saturating_sub(context).max(segment)..(i + context + 1).min(end);
            if groups.len() > group_start
                && let Some(last) = groups.last_mut()
                && last.end >= span.start
            {
                last.end = span.end;
            } else {
                groups.push(span);
            }
            ensure!(
                groups.len() <= MAX_REVIEW_HUNKS,
                "Too many separate review hunks. Reset the review options to inspect the original diff."
            );
        }
        segment = end + 1;
    }
    if groups.is_empty() {
        return Ok(String::new());
    }
    let mut cursors = Vec::with_capacity(rows.len() + 1);
    let mut cursor = [0, 0];
    for row in rows {
        cursors.push(cursor);
        if let Some(old) = row.old {
            cursor[0] = old + 1;
        }
        if let Some(new) = row.new {
            cursor[1] = new + 1;
        }
    }
    cursors.push(cursor);
    let header_length = original
        .split_inclusive('\n')
        .take_while(|line| !line.starts_with("@@ "))
        .map(str::len)
        .sum();
    let mut result = original[..header_length].to_owned();
    for group in groups {
        checkpoint()?;
        let counts = [
            rows[group.clone()]
                .iter()
                .filter(|r| r.old.is_some())
                .count(),
            rows[group.clone()]
                .iter()
                .filter(|r| r.new.is_some())
                .count(),
        ];
        let starts = [
            cursors[group.start][0] + usize::from(counts[0] > 0),
            cursors[group.start][1] + usize::from(counts[1] > 0),
        ];
        result.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            starts[0], counts[0], starts[1], counts[1]
        ));
        let mut i = group.start;
        while i < group.end {
            if i % 256 == 0 {
                checkpoint()?;
            }
            if rows[i].changed {
                let end = (i..group.end)
                    .find(|&j| !rows[j].changed)
                    .unwrap_or(group.end);
                for (side, prefix) in [(0, '-'), (1, '+')] {
                    for row in &rows[i..end] {
                        if let Some(number) = if side == 0 { row.old } else { row.new } {
                            push_line(&mut result, prefix, sources[side][number])?;
                        }
                    }
                }
                i = end;
            } else {
                if let Some(number) = rows[i].new {
                    push_line(&mut result, ' ', sources[1][number])?;
                }
                i += 1;
            }
        }
    }
    Ok(result)
}

fn push_line(output: &mut String, prefix: char, line: &str) -> Result<()> {
    ensure!(
        output.len() + line.len() + 35 <= MAX_REVIEW_BYTES,
        "Expanded context exceeds the 4 MiB review limit. Choose less context or use the source tabs."
    );
    output.push(prefix);
    output.push_str(line);
    if !line.ends_with('\n') {
        output.push_str("\n\\ No newline at end of file\n");
    }
    Ok(())
}

pub fn next_change(
    rows: &[usize],
    current: Option<usize>,
    visible: usize,
    forward: bool,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    if let Some(current) = current {
        Some(if forward {
            (current + 1) % rows.len()
        } else {
            (current + rows.len() - 1) % rows.len()
        })
    } else if forward {
        Some(rows.iter().position(|&row| row >= visible).unwrap_or(0))
    } else {
        Some(
            rows.iter()
                .rposition(|&row| row < visible)
                .unwrap_or(rows.len() - 1),
        )
    }
}
