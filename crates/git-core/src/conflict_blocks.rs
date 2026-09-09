//! Bounded, literal conflict-marker inspection and per-block draft decisions.
//! Parsing belongs on a read worker. These helpers never access a repository.
use anyhow::{Result, bail, ensure};
use std::ops::Range;

const MAX_BLOCKS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextConflictBlock {
    /// Includes every marker line and both versions, in UTF-8 byte offsets.
    pub range: Range<usize>,
    pub current: Range<usize>,
    pub base: Option<Range<usize>>,
    pub incoming: Range<usize>,
    pub first_line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictBlockChoice {
    Current,
    Incoming,
    Both,
}

/// Handles merge, diff3 and zdiff3 marker output, including custom marker widths,
/// CRLF, empty sides and Unicode. A malformed/nested block refuses decisions.
pub fn text_conflict_blocks(source: &str) -> Result<Vec<TextConflictBlock>> {
    ensure!(
        source.len() <= crate::MAX_DIFF_BYTES && !source.contains('\0'),
        "Block review requires UTF-8 text of at most 2 MiB without NUL bytes"
    );
    let mut blocks = Vec::new();
    let mut offset = 0;
    // start, marker width, opening line, current start/end, base start/end,
    // incoming start. Ranges are filled only after a complete validated block.
    let mut open: Option<OpenBlock> = None;
    for (line_index, line) in source.split_inclusive('\n').enumerate() {
        ensure!(
            line_index < crate::MAX_DIFF_LINES,
            "Block review exceeds the 100,000-line limit"
        );
        let content = line.trim_end_matches(['\r', '\n']);
        if let Some(width) = marker(content, b'<') {
            ensure!(
                open.is_none(),
                "Nested conflict markers cannot be resolved safely by block; edit the result manually"
            );
            open = Some(OpenBlock {
                start: offset,
                width,
                first_line: line_index + 1,
                current_start: offset + line.len(),
                current_end: None,
                base_start: None,
                base_end: None,
                incoming_start: None,
            });
        } else if let Some(block) = &mut open {
            if marker(content, b'|') == Some(block.width) {
                ensure!(
                    block.base_start.is_none() && block.incoming_start.is_none(),
                    "Unexpected base marker in conflict block"
                );
                block.current_end = Some(offset);
                block.base_start = Some(offset + line.len());
            } else if separator(content, block.width) {
                ensure!(
                    block.incoming_start.is_none(),
                    "Duplicate separator in conflict block"
                );
                if block.base_start.is_some() {
                    block.base_end = Some(offset);
                } else {
                    block.current_end = Some(offset);
                }
                block.incoming_start = Some(offset + line.len());
            } else if marker(content, b'>') == Some(block.width) {
                let incoming_start = block.incoming_start.context_marker()?;
                let current_end = block.current_end.context_marker()?;
                let base = match (block.base_start, block.base_end) {
                    (Some(start), Some(end)) => Some(start..end),
                    (None, None) => None,
                    _ => bail!("Incomplete base section in conflict block"),
                };
                blocks.push(TextConflictBlock {
                    range: block.start..offset + line.len(),
                    current: block.current_start..current_end,
                    base,
                    incoming: incoming_start..offset,
                    first_line: block.first_line,
                });
                ensure!(
                    blocks.len() <= MAX_BLOCKS,
                    "Block review exceeds the 4,096-conflict limit; use whole-file or external resolution"
                );
                open = None;
            } else if marker(content, b'>').is_some() || marker(content, b'|').is_some() {
                bail!(
                    "Conflict marker widths differ; repair the result manually before making block decisions"
                );
            }
        }
        offset += line.len();
    }
    ensure!(
        open.is_none(),
        "An opening conflict marker has no complete matching separator and closing marker; edit the result manually"
    );
    Ok(blocks)
}

struct OpenBlock {
    start: usize,
    width: usize,
    first_line: usize,
    current_start: usize,
    current_end: Option<usize>,
    base_start: Option<usize>,
    base_end: Option<usize>,
    incoming_start: Option<usize>,
}

trait MarkerOffset {
    fn context_marker(self) -> Result<usize>;
}
impl MarkerOffset for Option<usize> {
    fn context_marker(self) -> Result<usize> {
        self.ok_or_else(|| {
            anyhow::anyhow!("Conflict block is missing its separator; edit the result manually")
        })
    }
}

fn marker(line: &str, byte: u8) -> Option<usize> {
    let width = line
        .as_bytes()
        .iter()
        .take_while(|value| **value == byte)
        .count();
    (width >= 7
        && line
            .as_bytes()
            .get(width)
            .is_none_or(|value| value.is_ascii_whitespace()))
    .then_some(width)
}
fn separator(line: &str, width: usize) -> bool {
    line.len() == width && line.as_bytes().iter().all(|byte| *byte == b'=')
}

/// Produces a draft only. The source must still be the exact parsed snapshot;
/// saving/staging subsequently uses the conflict operation's original guards.
pub fn choose_conflict_block(
    source: &str,
    block: &TextConflictBlock,
    choice: ConflictBlockChoice,
) -> Result<String> {
    ensure!(
        block.range.start <= block.current.start
            && block.current.end <= block.incoming.start
            && block.incoming.end <= block.range.end
            && block.range.end <= source.len(),
        "The conflict block no longer matches the draft"
    );
    let before = source
        .get(..block.range.start)
        .ok_or_else(|| anyhow::anyhow!("Invalid conflict text boundary"))?;
    let after = source
        .get(block.range.end..)
        .ok_or_else(|| anyhow::anyhow!("Invalid conflict text boundary"))?;
    let current = source
        .get(block.current.clone())
        .ok_or_else(|| anyhow::anyhow!("Invalid current-side boundary"))?;
    let incoming = source
        .get(block.incoming.clone())
        .ok_or_else(|| anyhow::anyhow!("Invalid incoming-side boundary"))?;
    let mut result = String::with_capacity(source.len());
    result.push_str(before);
    if matches!(
        choice,
        ConflictBlockChoice::Current | ConflictBlockChoice::Both
    ) {
        result.push_str(current);
    }
    if matches!(
        choice,
        ConflictBlockChoice::Incoming | ConflictBlockChoice::Both
    ) {
        result.push_str(incoming);
    }
    result.push_str(after);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_preserve_unicode_crlf_other_blocks_and_missing_final_newline() {
        let text = "prefix é\r\n<<<<<<< ours\r\ncurrent 🐢\r\n=======\r\nincoming Ω\r\n>>>>>>> theirs\r\nmiddle\r\n<<<<<<< ours\r\n=======\r\nadded\r\n>>>>>>> theirs\r\nsuffix";
        let blocks = text_conflict_blocks(text).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].first_line, 2);
        assert!(blocks[1].current.is_empty());
        let chosen = choose_conflict_block(text, &blocks[0], ConflictBlockChoice::Both).unwrap();
        assert!(chosen.starts_with("prefix é\r\ncurrent 🐢\r\nincoming Ω\r\nmiddle\r\n"));
        assert!(chosen.ends_with("suffix"));
        let blocks = text_conflict_blocks(&chosen).unwrap();
        assert_eq!(blocks.len(), 1);
        let chosen =
            choose_conflict_block(&chosen, &blocks[0], ConflictBlockChoice::Current).unwrap();
        assert_eq!(
            chosen,
            "prefix é\r\ncurrent 🐢\r\nincoming Ω\r\nmiddle\r\nsuffix"
        );
    }

    #[test]
    fn diff3_base_and_custom_width_are_preserved_and_literal_marker_like_text_is_not_a_block() {
        let text =
            "<<<<<<<<<< current\na\n|||||||||| base\nb\n==========\nc\n>>>>>>>>>> incoming\n";
        let block = text_conflict_blocks(text).unwrap().remove(0);
        assert_eq!(&text[block.base.clone().unwrap()], "b\n");
        assert_eq!(
            choose_conflict_block(text, &block, ConflictBlockChoice::Incoming).unwrap(),
            "c\n"
        );
        assert!(
            text_conflict_blocks("less <<<<<<< x\n<<<<<<<literal\n=======\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn malformed_nested_and_excessive_inputs_do_not_offer_partial_decisions() {
        for text in [
            "<<<<<<< ours\na\n",
            "<<<<<<< ours\n>>>>>>> theirs\n",
            "<<<<<<< ours\n<<<<<<< nested\n=======\na\n>>>>>>> theirs\n",
            "<<<<<<< ours\n=======\na\n>>>>>>>> theirs\n",
        ] {
            assert!(text_conflict_blocks(text).is_err(), "{text}");
        }
        assert!(text_conflict_blocks(&"x".repeat(crate::MAX_DIFF_BYTES + 1)).is_err());
        assert!(
            text_conflict_blocks(
                &"<<<<<<< ours\na\n=======\nb\n>>>>>>> theirs\n".repeat(MAX_BLOCKS + 1)
            )
            .is_err()
        );
    }
}
