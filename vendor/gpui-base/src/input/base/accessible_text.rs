//! GitTurtle patch: a field's text as assistive technology reads it.
//!
//! AccessKit's adapters read a text field's text only through text runs,
//! child nodes that hold its characters: without one, the AT-SPI adapter
//! gives the field no Text interface, so Orca read a field holding "QA
//! Fixture" by its placeholder, "Your name", and never heard its value. The
//! node's own value is what macOS reports as its `AXValue`, and it is kept.
//!
//! A field writes each line, its line break included, as runs of at most
//! [`RUN_CHARACTERS`] characters linked on their line, and an empty run for
//! the line after a final break (or for an empty field), so every caret
//! position lies in a run. AccessKit's consumer looks a position's word
//! start up as a `u8`, so a longer run read the words past its 256th
//! character as those 256 characters before them. A long text writes its
//! runs one node each only near the caret ([`RUN_BLOCK`]); the text before
//! and after those goes in at most two runs on each side (the lines wholly
//! outside them, and the rest of a line they cut), so what a field writes
//! each frame is bounded by its text, not by its line or run count.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui::SharedString;
use gpui::accesskit::{Node, NodeId, Role, TextPosition, TextSelection};
use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation as _;

/// The most characters a run near the caret holds: every position in it,
/// its end included, is a `u8`, as AccessKit's word starts are.
const RUN_CHARACTERS: usize = u8::MAX as usize;

/// A field writes the runs in the block of this many that holds the caret,
/// and in the block on each side of it, as nodes of their own: at most
/// `3 * RUN_BLOCK + 4` runs, however long its text. A screen reader reads
/// the line the caret is on, which the caret's runs and those linked with
/// them on its line hold whole.
const RUN_BLOCK: usize = 256;

/// Every this many characters, [`Segmented`] records the byte the character
/// starts at, so finding a byte's character walks at most this many.
const CHECKPOINT: usize = 1024;

/// The key of the run holding the lines before the caret's blocks.
const BEFORE: usize = usize::MAX - 3;
/// The key of the run holding the start of the line the caret's blocks
/// begin in, before them.
const HEAD: usize = usize::MAX - 2;
/// The key of the run holding the end of the line the caret's blocks end
/// in, after them.
const TAIL: usize = usize::MAX - 1;
/// The key of the run holding the lines after the caret's blocks.
const AFTER: usize = usize::MAX;

thread_local! {
    /// What this thread's fields segmented and built ([`Builds`]).
    static BUILDS: Cell<Builds> = const { Cell::new(Builds { segmented: 0, runs: 0 }) };
}

/// How many times this thread's fields segmented a text and built the
/// run nodes of a caret's window, since [`AccessibleText::reset_builds`]:
/// for tests and benchmarks of what a frame rebuilds.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Builds {
    pub segmented: usize,
    pub runs: usize,
}

fn count(build: impl FnOnce(&mut Builds)) {
    BUILDS.with(|builds| {
        let mut counted = builds.get();
        build(&mut counted);
        builds.set(counted);
    });
}

/// What a field keeps of its text for assistive technology while a client
/// is active: the text segmented at one revision of it, and the run nodes
/// last built for a caret's window, which a frame clones and links rather
/// than builds again.
pub(crate) struct Cache {
    /// The field's text revision this segments.
    revision: u64,
    segmented: Segmented,
    built: RefCell<Option<Built>>,
}

impl Cache {
    pub(crate) fn new(revision: u64, text: &Rope) -> Self {
        count(|builds| builds.segmented += 1);
        Self {
            revision,
            segmented: Segmented::of(text),
            built: RefCell::new(None),
        }
    }

    /// The field's text revision this segments.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}

/// The run nodes of one caret window (`first..end` of the runs), without
/// their links, which name node IDs only a frame's builder gives.
struct Built {
    first: usize,
    end: usize,
    /// The first character of the line the window begins in, the
    /// character after its last run and the first of the line after the
    /// one it ends in.
    head: usize,
    to: usize,
    tail: usize,
    /// Whether the run of the lines after the window was written: not
    /// where the window ends in the text's last line, whose rest, to the
    /// text's end, the run of its tail holds.
    after: bool,
    runs: Vec<BuiltRun>,
}

struct BuiltRun {
    key: usize,
    node: Node,
    previous: Option<usize>,
    next: Option<usize>,
}

/// A text segmented into the characters, lines and runs assistive
/// technology reads.
struct Segmented {
    /// The text whole, the field node's value.
    value: SharedString,
    /// Each character's length in bytes: a grapheme cluster, the smallest
    /// unit the field's caret moves over, or each of its code points where
    /// a cluster is longer than a length can say (255 bytes).
    lengths: Vec<u8>,
    /// Each line's first byte and first character, in order: the first
    /// line's, then one after each line break.
    lines: Vec<(usize, usize)>,
    /// Each line's first run, counting the runs of every line before it.
    line_runs: Vec<usize>,
    /// The runs of every line.
    runs: usize,
    /// The first byte of every [`CHECKPOINT`]th character.
    checkpoints: Vec<usize>,
}

impl Segmented {
    fn of(text: &Rope) -> Self {
        let value: SharedString = text.to_string().into();
        let mut lengths = Vec::with_capacity(value.len());
        let mut checkpoints = Vec::with_capacity(value.len() / CHECKPOINT + 1);
        let mut lines = vec![(0, 0)];
        let mut push = |length: u8, at: usize, lengths: &mut Vec<u8>| {
            if lengths.len() % CHECKPOINT == 0 {
                checkpoints.push(at);
            }
            lengths.push(length);
        };
        for (start, grapheme) in value.grapheme_indices(true) {
            match u8::try_from(grapheme.len()) {
                Ok(length) => push(length, start, &mut lengths),
                Err(_) => {
                    for (offset, c) in grapheme.char_indices() {
                        push(c.len_utf8() as u8, start + offset, &mut lengths);
                    }
                }
            }
            // A line break ends its cluster ("\n" or "\r\n"), and a line.
            if grapheme.ends_with('\n') {
                lines.push((start + grapheme.len(), lengths.len()));
            }
        }
        let mut line_runs = Vec::with_capacity(lines.len());
        let mut runs = 0;
        for line in 0..lines.len() {
            line_runs.push(runs);
            let characters =
                lines.get(line + 1).map_or(lengths.len(), |next| next.1) - lines[line].1;
            runs += characters.div_ceil(RUN_CHARACTERS).max(1);
        }
        Self {
            value,
            lengths,
            lines,
            line_runs,
            runs,
            checkpoints,
        }
    }

    /// The first character of line `line`, or the text's end.
    fn line_start(&self, line: usize) -> usize {
        self.lines
            .get(line)
            .map_or(self.lengths.len(), |line| line.1)
    }

    /// The line holding character `character`: the one after a break for
    /// the character just past it.
    fn line_of(&self, character: usize) -> usize {
        self.lines.partition_point(|(_, start)| *start <= character) - 1
    }

    /// The line run `run` is on.
    fn line_of_run(&self, run: usize) -> usize {
        self.line_runs.partition_point(|first| *first <= run) - 1
    }

    /// The characters of run `run`, and whether it begins and ends its line.
    fn run_characters(&self, run: usize) -> (Range<usize>, bool, bool) {
        let line = self.line_of_run(run);
        let (start, end) = (self.line_start(line), self.line_start(line + 1));
        let first = start + (run - self.line_runs[line]) * RUN_CHARACTERS;
        let last = end.min(first + RUN_CHARACTERS);
        let ends_line = self.line_runs.get(line + 1).copied().unwrap_or(self.runs) == run + 1;
        (first..last, first == start, ends_line)
    }

    /// The run holding character `character`, and its place in it: the run
    /// it starts, or at a line's end the line's last run.
    fn run_at(&self, character: usize) -> (usize, usize) {
        let line = self.line_of(character);
        let start = self.line_start(line);
        let runs =
            self.line_runs.get(line + 1).map_or(self.runs, |next| *next) - self.line_runs[line];
        let within = ((character - start) / RUN_CHARACTERS).min(runs - 1);
        (
            self.line_runs[line] + within,
            character - start - within * RUN_CHARACTERS,
        )
    }

    /// The first byte of character `character`, or the text's end.
    fn byte_of(&self, character: usize) -> usize {
        if character >= self.lengths.len() {
            return self.value.len();
        }
        let checkpoint = character / CHECKPOINT;
        let at = self.checkpoints[checkpoint];
        let walked: usize = self.lengths[checkpoint * CHECKPOINT..character]
            .iter()
            .map(|length| *length as usize)
            .sum();
        at + walked
    }

    /// The character starting at byte `byte`, or the one it lies inside.
    fn character_at(&self, byte: usize) -> usize {
        let checkpoint = self
            .checkpoints
            .partition_point(|start| *start <= byte)
            .saturating_sub(1);
        let mut at = self.checkpoints.get(checkpoint).copied().unwrap_or(0);
        let mut character = checkpoint * CHECKPOINT;
        while character < self.lengths.len() && at < byte {
            at += self.lengths[character] as usize;
            character += 1;
        }
        character
    }

    /// A run holding the characters `characters`, with the starts of its
    /// words where `words`: what follows its first character, as a
    /// continuation of its line, when `continues`.
    /// The caret's window: the runs in the block holding run `caret` and
    /// the blocks on each side of it.
    fn window(&self, caret: usize) -> (usize, usize) {
        let block = caret / RUN_BLOCK;
        let first = block.saturating_sub(1) * RUN_BLOCK;
        (first, self.runs.min((block + 2) * RUN_BLOCK))
    }

    /// The run nodes of the window `first..end`, with the text before and
    /// after it in at most two runs each.
    fn build(&self, first: usize, end: usize) -> Built {
        count(|builds| builds.runs += 1);
        let (first_line, last_line) = (self.line_of_run(first), self.line_of_run(end - 1));
        let (head, tail) = (self.line_start(first_line), self.line_start(last_line + 1));
        let from = self.run_characters(first).0.start;
        let to = self.run_characters(end - 1).0.end;
        let mut runs = Vec::with_capacity(end - first + 4);
        let mut push = |key, node, previous, next| {
            runs.push(BuiltRun {
                key,
                node,
                previous,
                next,
            })
        };
        if first_line > 0 {
            push(BEFORE, self.run(0..head, false, false), None, None);
        }
        if head < from {
            push(HEAD, self.run(head..from, false, false), None, Some(first));
        }
        for index in first..end {
            let (characters, begins, ends) = self.run_characters(index);
            let previous = (!begins).then_some(if index > first { index - 1 } else { HEAD });
            let next = (!ends).then_some(if index + 1 < end { index + 1 } else { TAIL });
            push(index, self.run(characters, true, !begins), previous, next);
        }
        if to < tail {
            push(TAIL, self.run(to..tail, false, true), Some(end - 1), None);
        }
        let after = last_line + 1 < self.lines.len();
        if after {
            push(
                AFTER,
                self.run(tail..self.lengths.len(), false, false),
                None,
                None,
            );
        }
        Built {
            first,
            end,
            head,
            to,
            tail,
            after,
            runs,
        }
    }

    fn run(&self, characters: Range<usize>, words: bool, continues: bool) -> Node {
        let (first, end) = (self.byte_of(characters.start), self.byte_of(characters.end));
        let lengths = &self.lengths[characters.clone()];
        let text = &self.value[first..end];
        let mut run = Node::new(Role::TextRun);
        run.set_value(text);
        run.set_character_lengths(lengths);
        if words {
            let after_space = continues
                && self.value[..first]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace);
            run.set_word_starts(word_starts(text, lengths, after_space, continues));
        }
        run
    }
}

/// The starts of the words in a run of at most [`RUN_CHARACTERS`]
/// characters: a character that is not white space after one that is.
/// Trailing white space belongs to the word before it and a line's leading
/// white space is a word of its own, as AccessKit asks. A line's first
/// character starts a word implicitly; a run that `continues` its line
/// lists its first if a word starts there, after white space (`after_space`).
fn word_starts(text: &str, lengths: &[u8], mut after_space: bool, continues: bool) -> Vec<u8> {
    let mut starts = Vec::new();
    let mut at = 0;
    for (index, length) in lengths.iter().enumerate() {
        let space = text[at..].chars().next().is_some_and(char::is_whitespace);
        if (index > 0 || continues) && after_space && !space {
            starts.push(index as u8);
        }
        after_space = space;
        at += *length as usize;
    }
    starts
}

/// Where a field writes its text runs ([`AccessibleText::write`]): GPUI's
/// synthetic-child builder for the field's node, or a test's tree.
pub trait AccessibleTextSink {
    /// The node ID of the run with key `key`, always the same for one key.
    fn run_id(&self, key: usize) -> NodeId;
    /// Adds `run` as the field node's next child.
    fn push_run(&mut self, id: NodeId, run: Node);
    /// The field's own node.
    fn field(&mut self) -> &mut Node;
}

impl AccessibleTextSink for gpui::A11ySubtreeBuilder<'_> {
    fn run_id(&self, key: usize) -> NodeId {
        self.synthetic_node_id(key)
    }

    fn push_run(&mut self, id: NodeId, run: Node) {
        self.push_child(id, run);
    }

    fn field(&mut self) -> &mut Node {
        self.parent_node()
    }
}

/// What a field writes to assistive technology in one frame: its value,
/// the text runs that hold it and its selection on them
/// ([`crate::input::InputBaseState::accessible_text`]).
#[derive(Clone)]
pub struct AccessibleText {
    cache: Rc<Cache>,
    /// Where the selection starts and where its caret is, in bytes.
    anchor: usize,
    focus: usize,
}

impl AccessibleText {
    pub(crate) fn new(cache: Rc<Cache>, anchor: usize, focus: usize) -> Self {
        let end = cache.segmented.value.len();
        Self {
            cache,
            anchor: anchor.min(end),
            focus: focus.min(end),
        }
    }

    /// How many times this thread's fields segmented a text and built a
    /// caret window's runs since [`Self::reset_builds`].
    #[doc(hidden)]
    pub fn builds() -> Builds {
        BUILDS.with(Cell::get)
    }

    /// Starts [`Self::builds`] again from none.
    #[doc(hidden)]
    pub fn reset_builds() {
        BUILDS.with(|builds| builds.set(Builds::default()));
    }

    /// The text whole, the field node's value.
    pub fn value(&self) -> SharedString {
        self.cache.segmented.value.clone()
    }

    /// Writes the text runs as children of the field's node, in order, and
    /// their selection on that node: what the field's editing element
    /// writes through GPUI's synthetic-child builder in a frame drawn with
    /// an assistive technology active.
    pub fn write(&self, sink: &mut impl AccessibleTextSink) {
        let (runs, selection) = self.runs(|key| sink.run_id(key));
        for (id, run) in runs {
            sink.push_run(id, run);
        }
        sink.field().set_text_selection(selection);
    }

    /// The text runs, each with the node ID `id` gives its key (the same
    /// key always gives the same ID), in order, and the selection on them,
    /// which goes on the field's own node. The window's nodes are built
    /// only when the text or the caret's window changed.
    fn runs(&self, id: impl Fn(usize) -> NodeId) -> (Vec<(NodeId, Node)>, TextSelection) {
        let text = &self.cache.segmented;
        let (first, end) = text.window(text.run_at(text.character_at(self.focus)).0);
        let mut built = self.cache.built.borrow_mut();
        if !built
            .as_ref()
            .is_some_and(|built| built.first == first && built.end == end)
        {
            *built = Some(text.build(first, end));
        }
        let built = built.as_ref().expect("built for this window");
        let runs = built
            .runs
            .iter()
            .map(|run| {
                // A copy of the run's text each frame: GPUI takes every run as
                // an owned node, keeps none of an earlier frame and drops a
                // child the frame does not write, and a node owns its text
                // (the patch note names the places).
                let mut node = run.node.clone();
                if let Some(previous) = run.previous {
                    node.set_previous_on_line(id(previous));
                }
                if let Some(next) = run.next {
                    node.set_next_on_line(id(next));
                }
                (id(run.key), node)
            })
            .collect();
        // Every position names a run this frame writes: AccessKit's consumer
        // unwraps a selection's runs (`accesskit_consumer` 0.38.0 `text.rs`,
        // `text_selection`), so one naming a run never written panicked on
        // the UI thread as an adapter read it. Past the window, a position
        // lies in the run of the lines after it only where that was written:
        // a window ending in the text's last line leaves the rest to its tail.
        let position = |byte: usize| {
            let character = text.character_at(byte);
            let (run, index) = text.run_at(character);
            let (key, index) = if run < first {
                if character < built.head {
                    (BEFORE, character)
                } else {
                    (HEAD, character - built.head)
                }
            } else if run >= end {
                if built.after && character >= built.tail {
                    (AFTER, character - built.tail)
                } else {
                    (TAIL, character - built.to)
                }
            } else {
                (run, index)
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
        let cache = Rc::new(Cache::new(0, &Rope::from(text)));
        AccessibleText::new(cache, anchor, focus).runs(|key| NodeId(key as u64))
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
    fn a_long_line_is_runs_of_255_characters_linked_on_it() {
        // 600 characters of words "w000 " to "w119 ", then a break.
        let line: String = (0..120).map(|word| format!("w{word:03} ")).collect();
        let text = format!("{line}\n");
        let (found, selection) = runs(&text, 262, 262);
        let lengths: Vec<usize> = found
            .iter()
            .map(|(_, run)| run.character_lengths().len())
            .collect();
        assert_eq!(lengths, [255, 255, 91, 0]);
        assert_eq!(values(&found).concat(), text);
        assert_eq!(found[0].1.next_on_line(), Some(NodeId(1)));
        assert_eq!(found[1].1.previous_on_line(), Some(NodeId(0)));
        assert_eq!(found[1].1.next_on_line(), Some(NodeId(2)));
        assert_eq!(found[2].1.next_on_line(), None);
        assert_eq!(
            found[3].1.previous_on_line(),
            None,
            "the empty line after the break"
        );
        // "w051" starts the second run (character 255), which lists it, as
        // a run that continues its line must; "w100" at 500 is its last.
        assert_eq!(found[1].1.word_starts()[..2], [0, 5]);
        assert_eq!(*found[1].1.word_starts().last().unwrap(), 250);
        assert_eq!(selection.focus.node, NodeId(1));
        assert_eq!(selection.focus.character_index, 7);
        // A run that starts inside a word ("w0042 " is characters 252 to
        // 257) lists the next one, "w0043" at 258, first.
        let words: String = (0..100).map(|word| format!("w{word:04} ")).collect();
        let (found, _) = runs(&words, 0, 0);
        assert_eq!(found[1].1.word_starts()[0], 3);
    }

    #[test]
    fn a_long_text_writes_its_runs_near_the_caret_and_the_rest_in_at_most_four() {
        let text = "line\n".repeat(2000);
        // Line 1000 is in block 3: blocks 2 to 4 are lines 512 to 1279.
        let caret = 1000 * 5 + 2;
        let (found, selection) = runs(&text, caret, caret);
        assert_eq!(found.len(), 3 * RUN_BLOCK + 2);
        assert_eq!(found[0].0, NodeId(BEFORE as u64));
        assert_eq!(found[0].1.value().unwrap().len(), 512 * 5);
        assert_eq!(found[1].0, NodeId(512));
        assert_eq!(found.last().unwrap().0, NodeId(AFTER as u64));
        assert_eq!(selection.focus.node, NodeId(1000));
        assert_eq!(selection.focus.character_index, 2);
        assert_eq!(values(&found).concat(), text);
        // A selection reaching past the caret's blocks ends in a run that
        // holds them, at the character its byte is.
        let (_, selection) = runs(&text, 3, caret);
        assert_eq!(selection.anchor.node, NodeId(BEFORE as u64));
        assert_eq!(selection.anchor.character_index, 3);
        let (_, selection) = runs(&text, 1500 * 5 + 1, caret);
        assert_eq!(selection.anchor.node, NodeId(AFTER as u64));
        assert_eq!(selection.anchor.character_index, (1500 - 1280) * 5 + 1);
    }

    #[test]
    fn one_long_line_writes_the_runs_near_the_caret_and_the_rest_of_the_line_in_two() {
        // One line of 400,000 characters: 1,569 runs, the caret in run 1,000.
        let text = "abcd ".repeat(80_000);
        let caret = 1_000 * RUN_CHARACTERS + 3;
        let (found, selection) = runs(&text, 0, caret);
        assert_eq!(
            found.len(),
            3 * RUN_BLOCK + 2,
            "the line's head, runs and tail"
        );
        assert_eq!(found[0].0, NodeId(HEAD as u64));
        assert_eq!(found[0].1.next_on_line(), Some(NodeId(512)));
        assert_eq!(found[1].1.previous_on_line(), Some(NodeId(HEAD as u64)));
        let tail = &found.last().unwrap().1;
        assert_eq!(tail.previous_on_line(), Some(NodeId(1279)));
        assert_eq!(
            found[found.len() - 2].1.next_on_line(),
            Some(NodeId(TAIL as u64))
        );
        assert_eq!(values(&found).concat(), text);
        assert_eq!(selection.focus.node, NodeId(1_000));
        assert_eq!(selection.focus.character_index, 3);
        assert_eq!(selection.anchor.node, NodeId(HEAD as u64));
        assert_eq!(selection.anchor.character_index, 0);
        assert!(found.iter().all(|(id, run)| {
            run.character_lengths().len() <= RUN_CHARACTERS
                || [HEAD, TAIL].contains(&(id.0 as usize))
        }));
    }

    /// One frame of the cache's text with the selection from byte `anchor`
    /// to byte `focus`: at most `3 * RUN_BLOCK + 4` runs, each written once,
    /// holding the text whole, and each end of the selection in a run the
    /// frame writes, at the character its byte is.
    fn assert_written(cache: &Rc<Cache>, anchor: usize, focus: usize) {
        let text = &cache.segmented;
        let (found, selection) =
            AccessibleText::new(cache.clone(), anchor, focus).runs(|key| NodeId(key as u64));
        let case = format!("{} bytes, {anchor} to {focus}", text.value.len());
        assert!(
            found.len() <= 3 * RUN_BLOCK + 4,
            "{case}: {} runs",
            found.len()
        );
        assert_eq!(values(&found).concat(), &text.value[..], "{case}");
        let mut written = std::collections::HashMap::new();
        let mut before = 0;
        for (id, run) in &found {
            let characters = run.character_lengths().len();
            assert!(
                written.insert(*id, (before, characters)).is_none(),
                "{case}: {id:?} written twice"
            );
            before += characters;
        }
        for (end, byte) in [(selection.anchor, anchor), (selection.focus, focus)] {
            let Some(&(start, characters)) = written.get(&end.node) else {
                panic!("{case}: byte {byte} names {:?}, never written", end.node);
            };
            assert!(end.character_index <= characters, "{case}: byte {byte}");
            assert_eq!(
                start + end.character_index,
                text.character_at(byte.min(text.value.len())),
                "{case}: byte {byte}"
            );
        }
    }

    #[test]
    fn a_selection_from_a_long_last_lines_end_names_runs_written() {
        // One unterminated line, its caret at 0 and its anchor at its end
        // (End, then Shift+Home): the caret's window is the line's first
        // 512 runs, 130,560 characters, and a longer line's rest goes in its
        // tail, there being no line after it to write.
        for (length, key, index) in [(130_560, 511, 255), (130_561, TAIL, 1)] {
            let text = "a".repeat(length);
            let (found, selection) = runs(&text, length, 0);
            assert_eq!(selection.focus.node, NodeId(0));
            assert_eq!(selection.anchor.node, NodeId(key as u64), "{length}");
            assert_eq!(selection.anchor.character_index, index, "{length}");
            assert!(found.iter().any(|(id, _)| *id == selection.anchor.node));
            assert_written(&Rc::new(Cache::new(0, &Rope::from(text))), length, 0);
        }
    }

    #[test]
    fn every_position_a_frame_reports_names_a_run_it_writes() {
        // A deterministic sequence of byte offsets.
        let mut seed = 29u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize
        };
        let clusters = format!("a{} ", "\u{301}".repeat(300));
        let shapes = [
            String::new(),
            "a".to_owned(),
            "\n".to_owned(),
            "one two\nthree\n".to_owned(),
            "line\n".repeat(2_000),
            format!("{}line", "line\n".repeat(1_999)),
            "ab\r\n".repeat(700),
            format!("{}end", "ab\rcd\r".repeat(400)),
            "\r".repeat(600),
            format!("{}{}", "x\n".repeat(10), "ab ".repeat(60_000)),
            format!("{}{}\n", "x\n".repeat(10), "ab ".repeat(60_000)),
            format!("{}{}", "line\n".repeat(2_000), "w ".repeat(100_000)),
            format!("{}\n{}", "w ".repeat(100_000), "s\n".repeat(1_000)),
            format!("{}\n", "y".repeat(299)).repeat(1_500),
            "abcd ".repeat(40_000),
            format!("{}{}", clusters.repeat(40), "🙂👩‍👩‍👧‍👦é語\t".repeat(2_000)),
        ];
        for text in shapes {
            let cache = Rc::new(Cache::new(0, &Rope::from(text.as_str())));
            let end = text.len();
            let last_line = text.rfind('\n').map_or(0, |at| at + 1);
            let byte = |at: usize| {
                let mut at = at % (end + 1);
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                at
            };
            let mut pairs = vec![
                (end, 0),
                (0, end),
                (end, end),
                (end, byte(last_line)),
                (end, byte(last_line + 1)),
                (byte(end / 2), end),
            ];
            for _ in 0..12 {
                let (a, b) = (byte(next()), byte(next()));
                pairs.extend([(end, a), (a, end), (0, a), (a, b)]);
            }
            for (anchor, focus) in pairs {
                assert_written(&cache, anchor, focus);
            }
        }
    }
}
