//! Worker-prepared actions for the literal unified patch. Selection IDs come
//! from the core's captured index/worktree snapshot, never from copied text.

use crate::text::PatchPresentation;
use anyhow::{Result, ensure};
use gitturtle_core::{PartialDiff, PartialHunk, PartialLineKind};
use std::{collections::HashMap, sync::Arc};

pub struct PartialActions {
    pub diff: Arc<PartialDiff>,
    pub rows: Arc<[PartialRow]>,
}

impl PartialActions {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of::<PartialDiff>()
            + self.diff.bytes()
            + std::mem::size_of_val(self.rows.as_ref())
            + 3 * 2 * std::mem::size_of::<usize>()
            + self
                .rows
                .iter()
                .map(|row| match row {
                    PartialRow::Hunk(ids) => {
                        std::mem::size_of_val(ids.as_ref()) + 2 * std::mem::size_of::<usize>()
                    }
                    _ => 0,
                })
                .sum::<usize>()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PartialRow {
    #[default]
    None,
    Hunk(Arc<[usize]>),
    Change {
        id: usize,
        added: bool,
        line: usize,
    },
}

/// Preserve the actual patch and its displayed hunk boundaries. The core's
/// bounded diff grouping may differ, so each visible hunk acts on the exact
/// change IDs shown beneath its header rather than assuming matching indices.
pub fn prepare(
    patch: &str,
    presentation: &PatchPresentation,
    diff: Arc<PartialDiff>,
) -> Result<PartialActions> {
    let rows = prepare_rows(patch, presentation, &diff.hunks)?;
    Ok(PartialActions { diff, rows })
}

fn prepare_rows(
    patch: &str,
    presentation: &PatchPresentation,
    hunks: &[PartialHunk],
) -> Result<Arc<[PartialRow]>> {
    let mut changes = HashMap::new();
    for line in hunks.iter().flat_map(|hunk| &hunk.lines) {
        let (Some(id), Some(number)) = (line.change_id, line.old_line.or(line.new_line)) else {
            continue;
        };
        changes.insert(
            (line.kind == PartialLineKind::Addition, number),
            (id, &line.text),
        );
    }
    let expected = changes.len();
    let mut selected = 0;
    let mut rows = vec![PartialRow::None; presentation.rows.len()];
    let mut headers: Vec<(usize, Vec<usize>)> = Vec::new();
    for (row, line) in patch.split_inclusive('\n').enumerate() {
        if line.starts_with("@@ ") {
            headers.push((row, Vec::new()));
            continue;
        }
        let Some(numbers) = presentation.rows.get(row) else {
            anyhow::bail!("Patch rows no longer match the prepared comparison.");
        };
        let (added, number) = match (numbers.old, numbers.new, line.as_bytes().first()) {
            (None, Some(number), Some(b'+')) => (true, number as usize),
            (Some(number), None, Some(b'-')) => (false, number as usize),
            _ => continue,
        };
        let Some((id, source)) = changes.remove(&(added, number)) else {
            anyhow::bail!(
                "Changed lines no longer match the working snapshot. Refresh to select lines."
            );
        };
        let actual = &line[1..];
        ensure!(
            actual == source
                || (!source.ends_with('\n') && actual.strip_suffix('\n') == Some(source.as_str())),
            "Changed text no longer matches the working snapshot. Refresh to select lines."
        );
        let Some((_, ids)) = headers.last_mut() else {
            anyhow::bail!("A changed line has no matching diff hunk.");
        };
        ids.push(id);
        rows[row] = PartialRow::Change {
            id,
            added,
            line: number,
        };
        selected += 1;
    }
    ensure!(
        selected > 0 && selected == expected && changes.is_empty(),
        "The complete set of changed lines is unavailable. Use the whole-file action."
    );
    for (row, ids) in headers {
        if !ids.is_empty() {
            rows[row] = PartialRow::Hunk(ids.into());
        }
    }
    Ok(rows.into())
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
    fn literal_hunk_actions_keep_exact_ids_across_separate_display_groups() {
        let patch =
            "--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new\n@@ -10 +10 @@\n-last\n+next\n";
        let hunks = [PartialHunk {
            header: "one differently grouped core hunk".into(),
            lines: vec![
                change(0, false, 1, "old\n"),
                change(1, true, 1, "new\n"),
                change(2, false, 10, "last\n"),
                change(3, true, 10, "next\n"),
            ],
        }];
        let rows = prepare_rows(patch, &PatchPresentation::prepare(patch), &hunks).unwrap();
        assert_eq!(rows[2], PartialRow::Hunk(Arc::from([0, 1])));
        assert_eq!(rows[5], PartialRow::Hunk(Arc::from([2, 3])));
        assert_eq!(
            rows[3],
            PartialRow::Change {
                id: 0,
                added: false,
                line: 1
            }
        );
        assert_eq!(
            rows[7],
            PartialRow::Change {
                id: 3,
                added: true,
                line: 10
            }
        );
        assert_eq!(rows[8], PartialRow::None);
    }

    #[test]
    fn crlf_and_missing_final_newline_do_not_shift_actions_or_modify_text() {
        let patch =
            "--- a/file\n+++ b/file\n@@ -1 +1 @@\n-你好\r\n+🐢\n\\ No newline at end of file\n";
        let hunks = [PartialHunk {
            header: String::new(),
            lines: vec![change(0, false, 1, "你好\r\n"), change(1, true, 1, "🐢")],
        }];
        let rows = prepare_rows(patch, &PatchPresentation::prepare(patch), &hunks).unwrap();
        assert_eq!(rows[2], PartialRow::Hunk(Arc::from([0, 1])));
        assert_eq!(
            rows[4],
            PartialRow::Change {
                id: 1,
                added: true,
                line: 1
            }
        );
        assert_eq!(rows[5], PartialRow::None);
        assert_eq!(hunks[0].lines[1].text, "🐢");
    }

    #[test]
    fn mismatched_text_or_missing_changes_never_produce_actionable_rows() {
        let patch = "@@ -1 +1 @@\n-old\n+new\n";
        for lines in [
            vec![
                change(0, false, 1, "different\n"),
                change(1, true, 1, "new\n"),
            ],
            vec![change(0, false, 1, "old\n")],
            vec![
                change(0, false, 1, "old\n"),
                change(1, true, 1, "new\n"),
                change(2, true, 8, "extra\n"),
            ],
        ] {
            let hunks = [PartialHunk {
                header: String::new(),
                lines,
            }];
            assert!(prepare_rows(patch, &PatchPresentation::prepare(patch), &hunks).is_err());
        }
    }
}
