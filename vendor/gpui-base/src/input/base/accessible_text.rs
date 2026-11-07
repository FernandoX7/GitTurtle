//! GitTurtle patch: a field's text as assistive technology reads it.
//!
//! AccessKit's adapters read a text field's text only through text runs,
//! child nodes that hold its characters: without one, the AT-SPI adapter
//! gives the field no Text interface, so Orca read a field holding "QA
//! Fixture" by its placeholder, "Your name", and never heard its value. The
//! node's own value is what macOS reports as its `AXValue`, and it is kept.
//!
//! A field writes one run for each line, its line break included, and an
//! empty run for the line after a final break (or for an empty field), so
//! every caret position lies in a run. A long text writes its lines one run
//! each only near the caret ([`LINE_BLOCK`]); the lines before and after
//! those go in one run each, so what a field writes each frame is bounded
//! by its text, not by its line count.

use std::ops::Range;
use std::rc::Rc;

use gpui::SharedString;
use gpui::accesskit::{Node, NodeId, Role, TextPosition, TextSelection};
use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation as _;

/// A field writes the lines in the block of this many that holds the caret,
/// and in the block on each side of it, as runs of their own: at most
/// `3 * LINE_BLOCK + 2` runs, however long its text. A screen reader reads
/// the line the caret is on, so the caret's line is always one of them.
const LINE_BLOCK: usize = 256;

/// The key of the run holding the lines before the caret's blocks.
const BEFORE: usize = usize::MAX - 1;
/// The key of the run holding the lines after the caret's blocks.
const AFTER: usize = usize::MAX;

/// A text segmented into the characters and lines assistive technology
/// reads, kept by the field while a client is active and rebuilt only
/// when its text changes.
pub(crate) struct Segmented {
    /// The text segmented: a clone shares the field's chunks.
    text: Rope,
    /// The text whole, the field node's value.
    value: SharedString,
    /// Each character's length in bytes: a grapheme cluster, the smallest
    /// unit the field's caret moves over, or each of its code points where
    /// a cluster is longer than a length can say (255 bytes).
    lengths: Vec<u8>,
    /// Each line's first byte and first character, in order: the first
    /// line's, then one after each line break.
    lines: Vec<(usize, usize)>,
}

impl Segmented {
    pub(crate) fn of(text: &Rope) -> Self {
        let value: SharedString = text.to_string().into();
        let mut lengths = Vec::with_capacity(value.len());
        let mut lines = vec![(0, 0)];
        for (start, grapheme) in value.grapheme_indices(true) {
            match u8::try_from(grapheme.len()) {
                Ok(length) => lengths.push(length),
                Err(_) => lengths.extend(grapheme.chars().map(|c| c.len_utf8() as u8)),
            }
            // A line break ends its cluster ("\n" or "\r\n"), and a run.
            if grapheme.ends_with('\n') {
                lines.push((start + grapheme.len(), lengths.len()));
            }
        }
        Self {
            text: text.clone(),
            value,
            lengths,
            lines,
        }
    }

    /// Whether this segments `text`.
    pub(crate) fn is_of(&self, text: &Rope) -> bool {
        self.text == *text
    }

    /// The first byte and character of line `line`, or the text's end.
    fn line_start(&self, line: usize) -> (usize, usize) {
        self.lines
            .get(line)
            .copied()
            .unwrap_or((self.value.len(), self.lengths.len()))
    }

    /// The line holding byte `byte`: the one after a break for the byte
    /// just past it.
    fn line_of(&self, byte: usize) -> usize {
        self.lines.partition_point(|(start, _)| *start <= byte) - 1
    }

    /// The run of the lines `lines`, with the starts of the words among its
    /// first 256 characters where `words` (AccessKit counts them in `u8`s).
    fn run(&self, lines: Range<usize>, words: bool) -> Node {
        let (first_byte, first_char) = self.line_start(lines.start);
        let (end_byte, end_char) = self.line_start(lines.end);
        let lengths = &self.lengths[first_char..end_char];
        let text = &self.value[first_byte..end_byte];
        let mut run = Node::new(Role::TextRun);
        run.set_value(text);
        run.set_character_lengths(lengths);
        if words {
            run.set_word_starts(word_starts(text, lengths));
        }
        run
    }

    /// Characters from the start of line `line` to byte `byte` in it.
    fn characters_to(&self, line: usize, byte: usize) -> usize {
        let (mut at, first) = self.line_start(line);
        let (_, end) = self.line_start(line + 1);
        let mut count = 0;
        for length in &self.lengths[first..end] {
            if at >= byte {
                break;
            }
            at += *length as usize;
            count += 1;
        }
        count
    }
}

/// The starts of the words among a run's first 256 characters: a
/// character that is not white space after one that is. Trailing white
/// space belongs to the word before it and a line's leading white space is
/// a word of its own, as AccessKit asks; the first character starts a word
/// implicitly.
fn word_starts(text: &str, lengths: &[u8]) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut at = 0;
    let mut after_space = false;
    for (index, length) in lengths.iter().enumerate().take(u8::MAX as usize + 1) {
        let space = text[at..].chars().next().is_some_and(char::is_whitespace);
        if index > 0 && after_space && !space {
            starts.push(index as u8);
        }
        after_space = space;
        at += *length as usize;
    }
    starts
}

/// What a field writes to assistive technology in one frame: its value,
/// the text runs that hold it and its selection on them
/// ([`crate::input::InputBaseState::accessible_text`]).
#[derive(Clone)]
pub struct AccessibleText {
    segmented: Rc<Segmented>,
    /// Where the selection starts and where its caret is, in bytes.
    anchor: usize,
    focus: usize,
}

impl AccessibleText {
    pub(crate) fn new(segmented: Rc<Segmented>, anchor: usize, focus: usize) -> Self {
        let end = segmented.value.len();
        Self {
            segmented,
            anchor: anchor.min(end),
            focus: focus.min(end),
        }
    }

    /// The text whole, the field node's value.
    pub fn value(&self) -> SharedString {
        self.segmented.value.clone()
    }

    /// The text runs, each with the node ID `id` gives its key (the same
    /// key always gives the same ID), in order, and the selection on them,
    /// which goes on the field's own node.
    pub fn runs(&self, id: impl Fn(usize) -> NodeId) -> (Vec<(NodeId, Node)>, TextSelection) {
        let text = &*self.segmented;
        let lines = text.lines.len();
        let block = text.line_of(self.focus) / LINE_BLOCK;
        let first = block.saturating_sub(1) * LINE_BLOCK;
        let end = lines.min((block + 2) * LINE_BLOCK);
        let mut runs = Vec::with_capacity(end - first + 2);
        if first > 0 {
            runs.push((id(BEFORE), text.run(0..first, false)));
        }
        for line in first..end {
            runs.push((id(line), text.run(line..line + 1, true)));
        }
        if end < lines {
            runs.push((id(AFTER), text.run(end..lines, false)));
        }
        let position = |byte: usize| {
            let line = text.line_of(byte);
            let index = text.characters_to(line, byte);
            let (key, index) = if line < first {
                (BEFORE, text.line_start(line).1 + index)
            } else if line >= end {
                (
                    AFTER,
                    text.line_start(line).1 - text.line_start(end).1 + index,
                )
            } else {
                (line, index)
            };
            TextPosition {
                node: id(key),
                character_index: index,
            }
        };
        let selection = TextSelection {
            anchor: position(self.anchor),
            focus: position(self.focus),
        };
        (runs, selection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str, anchor: usize, focus: usize) -> (Vec<(NodeId, Node)>, TextSelection) {
        let segmented = Rc::new(Segmented::of(&Rope::from(text)));
        AccessibleText::new(segmented, anchor, focus).runs(|key| NodeId(key as u64))
    }

    fn values(runs: &[(NodeId, Node)]) -> Vec<&str> {
        runs.iter().map(|(_, run)| run.value().unwrap()).collect()
    }

    #[test]
    fn each_line_is_a_run_with_its_break_and_the_caret_on_it() {
        let (found, selection) = runs("one two\nthree\n", 4, 9);
        assert_eq!(values(&found), ["one two\n", "three\n", ""]);
        assert_eq!(found[0].1.character_lengths(), [1; 8]);
        assert_eq!(found[0].1.word_starts(), [4]);
        assert_eq!(selection.anchor.node, NodeId(0));
        assert_eq!(selection.anchor.character_index, 4);
        assert_eq!(selection.focus.node, NodeId(1));
        assert_eq!(selection.focus.character_index, 1);
        // The caret after the final break is on the empty line after it.
        let (_, selection) = runs("one\n", 4, 4);
        assert_eq!(selection.focus.node, NodeId(1));
        assert_eq!(selection.focus.character_index, 0);
    }

    #[test]
    fn a_character_is_a_grapheme_cluster() {
        let (found, selection) = runs("e\u{301}🙂a", 7, 7);
        assert_eq!(found[0].1.character_lengths(), [3, 4, 1]);
        assert_eq!(selection.focus.character_index, 2);
        let (found, _) = runs("", 0, 0);
        assert_eq!(values(&found), [""]);
        assert!(found[0].1.character_lengths().is_empty());
    }

    #[test]
    fn a_long_text_writes_its_lines_near_the_caret_and_the_rest_in_two_runs() {
        let text = "line\n".repeat(2000);
        // Line 1000 is in block 3: blocks 2 to 4 are lines 512 to 1279.
        let caret = 1000 * 5 + 2;
        let (found, selection) = runs(&text, caret, caret);
        assert_eq!(found.len(), 3 * LINE_BLOCK + 2);
        assert_eq!(found[0].0, NodeId(BEFORE as u64));
        assert_eq!(found[0].1.value().unwrap().len(), 512 * 5);
        assert_eq!(found[1].0, NodeId(512));
        assert_eq!(found.last().unwrap().0, NodeId(AFTER as u64));
        assert_eq!(selection.focus.node, NodeId(1000));
        assert_eq!(selection.focus.character_index, 2);
        let joined: String = values(&found).concat();
        assert_eq!(joined, text);
        // A selection reaching past the caret's blocks ends in a run that
        // holds them, at the character its byte is.
        let (_, selection) = runs(&text, 3, caret);
        assert_eq!(selection.anchor.node, NodeId(BEFORE as u64));
        assert_eq!(selection.anchor.character_index, 3);
        let (_, selection) = runs(&text, 1500 * 5 + 1, caret);
        assert_eq!(selection.anchor.node, NodeId(AFTER as u64));
        assert_eq!(selection.anchor.character_index, (1500 - 1280) * 5 + 1);
    }
}
