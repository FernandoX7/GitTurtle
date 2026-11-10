//! Explicit LFS preview downloads with captured pointer/source review.
use crate::native_accessibility::{DrawnButton, LiveRegion as _};
use crate::*;
use gitturtle_core::{LfsDownloadPlan, LfsDownloadTarget, WriteCommand};
use gpui_kit::{component::WindowExt, prelude::FluentBuilder};

use crate::native_accessibility::text as label;

pub(super) fn preview_targets(
    content: &Content,
    file: &FileChange,
) -> Vec<(usize, LfsDownloadTarget)> {
    let pointers = match content {
        Content::Images { old, new } => [old.lfs_pointer.clone(), new.lfs_pointer.clone()],
        // Only a side whose object is truly absent: a pointer whose object
        // is local, whatever kept it from showing, has nothing to download.
        Content::Text {
            old,
            new,
            lfs_missing,
            ..
        } => [(old, lfs_missing[0]), (new, lfs_missing[1])]
            .map(|(bytes, missing)| missing.then(|| bytes.as_bytes().to_vec())),
        // Only a side whose object is truly absent: a model's unusable local
        // object, kept as a retryable pointer, has nothing to download either.
        Content::Rich(preview) => [&preview.old, &preview.new].map(|side| {
            side.lfs_missing
                .then(|| side.captured.as_deref())
                .flatten()
                .and_then(|bytes| {
                    gitturtle_preview::detect_lfs_pointer(bytes).map(|_| bytes.to_vec())
                })
        }),
        _ => [None, None],
    };
    pointers
        .into_iter()
        .enumerate()
        .filter_map(|(index, pointer)| {
            let pointer = pointer?;
            let path = if index == 0 {
                file.old_path.clone()
            } else {
                file.new_path.clone()
            }?;
            let blob_oid = if index == 0 {
                file.old_oid.clone()
            } else {
                file.new_oid.clone()
            };
            Some((
                index,
                LfsDownloadTarget {
                    path,
                    blob_oid,
                    pointer,
                },
            ))
        })
        .collect()
}

/// What is wrong with a side's Git LFS object, in the words that side says
/// wherever it stays its pointer: a whole clause, to follow the side's name.
pub(super) fn failure_words(failure: gitturtle_core::LfsObjectFailure) -> &'static str {
    match failure {
        gitturtle_core::LfsObjectFailure::TooLarge => {
            "this Git LFS object is over the 32 MiB preview limit"
        }
        gitturtle_core::LfsObjectFailure::Corrupt => {
            "the local copy of this Git LFS object is damaged"
        }
        gitturtle_core::LfsObjectFailure::StoreUnreadable => {
            "the local Git LFS store could not be read"
        }
    }
}

/// [`failure_words`] for two sides that failed alike.
fn both_failure_words(failure: gitturtle_core::LfsObjectFailure) -> &'static str {
    match failure {
        gitturtle_core::LfsObjectFailure::TooLarge => {
            "both Git LFS objects are over the 32 MiB preview limit"
        }
        gitturtle_core::LfsObjectFailure::Corrupt => {
            "the local copies of both Git LFS objects are damaged"
        }
        gitturtle_core::LfsObjectFailure::StoreUnreadable => {
            "the local Git LFS store could not be read"
        }
    }
}

/// [`failure_words`] opening a sentence, where no side's name comes first.
pub(super) fn failure_opening(failure: gitturtle_core::LfsObjectFailure) -> String {
    let words = failure_words(failure);
    format!("{}{}", words[..1].to_ascii_uppercase(), &words[1..])
}

/// What a side says in its own place, an image's or a model's: the whole
/// statement, with nothing about a download, which could not help.
pub(super) fn failure_sentence(failure: gitturtle_core::LfsObjectFailure) -> String {
    format!("{}.", failure_opening(failure))
}

/// What the failing sides of a text comparison say above it, the sides named
/// `names`, old then new, as one line however many failed; `None` where
/// neither did.
pub(super) fn side_failures_line(
    failed: [Option<gitturtle_core::LfsObjectFailure>; 2],
    names: [&str; 2],
) -> Option<String> {
    let side = |name: &str, failure| {
        format!(
            "{name}: {}, so its pointer is shown.",
            failure_words(failure)
        )
    };
    Some(match failed {
        [None, None] => return None,
        [Some(failure), None] => side(names[0], failure),
        [None, Some(failure)] => side(names[1], failure),
        [Some(old), Some(new)] if old == new => format!(
            "{} and {}: {}, so their pointers are shown.",
            names[0],
            names[1],
            both_failure_words(old)
        ),
        [Some(old), Some(new)] => format!(
            "{}: {}. {}: {}.",
            names[0],
            failure_words(old),
            names[1],
            failure_words(new)
        ),
    })
}

/// The line a comparison's failing sides say above it ([`side_failures_line`]),
/// which a view keeps from frame to frame: as tall as its words where the
/// room allows, and where the comparison needs the room it scrolls inside
/// its own height rather than being cut (DESIGN.md, "Empty and edge states").
pub(super) struct FailureLine {
    /// The element its words are written on.
    id: &'static str,
    pub(super) scroll: ScrollHandle,
    pub(super) focus: FocusHandle,
    /// The comparison last drawn ([`comparison_key`]): another one starts
    /// from its top, even where it says the same words.
    shown: std::cell::RefCell<ComparisonKey>,
}

/// Which comparison a line is drawn above: its file, its sides' objects and
/// their modes, as the read that prepared it was keyed. Told apart by its
/// memory address, a comparison read again could land where the one it
/// replaced was freed and keep that one's scroll.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ComparisonKey(Option<FileChange>);

/// Which comparison the shown `file` is.
pub(super) fn comparison_key(file: Option<&FileChange>) -> ComparisonKey {
    ComparisonKey(file.cloned())
}

impl FailureLine {
    pub(super) fn new(id: &'static str, cx: &mut App) -> Self {
        Self {
            id,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            shown: Default::default(),
        }
    }

    /// The line saying `sentence` of the comparison `key`. While it
    /// overflows it is a Tab stop that Up and Down scroll, ringing its own
    /// edge, as the rewritten-series preview's over-tall failure does.
    fn render<V: 'static>(
        &self,
        key: ComparisonKey,
        sentence: String,
        ring: Option<&crate::focus_ring::FocusRing>,
        window: &Window,
        cx: &mut Context<V>,
    ) -> Div {
        if *self.shown.borrow() != key {
            self.scroll.set_offset(point(px(0.), px(0.)));
            self.shown.replace(key);
        }
        let id = self.id;
        let scroll = &self.scroll;
        let cue = crate::tags::ScrollCue::new(scroll, Pixels::ZERO);
        let overflows = cue.kept();
        let focus = &self.focus;
        let tracked = overflows || focus.is_focused(window);
        let (warning, accent) = (appearance::labels(cx).warning, palette(cx).accent);
        let keys = scroll.clone();
        let edge = ring.and_then(|ring| ring.edge(focus, rgb(accent)));
        cue.list(
            div()
                .on_children_prepainted(cue.observe(|_, _, _| {}))
                .id(SharedString::from(format!("{id}-region")))
                .debug_selector(move || format!("{id}-region"))
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .when(tracked, |region| {
                    region
                        // The handle carries the Tab stop: GPUI ignores the
                        // `tab_stop` of the div that tracks it.
                        .track_focus(&focus.clone().tab_stop(overflows))
                        .role(Role::Group)
                        .aria_label("Git LFS preview warning")
                        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                            let down = match event.keystroke.key.as_str() {
                                "up" => false,
                                "down" => true,
                                _ => return,
                            };
                            let line = crate::empty_state::block_height(1, false, window);
                            let at = -keys.offset().y;
                            let end = keys.max_offset().y.max(Pixels::ZERO);
                            let to =
                                (if down { at + line } else { at - line }).clamp(Pixels::ZERO, end);
                            if to != at {
                                keys.set_offset(point(keys.offset().x, -to));
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }))
                })
                .child(
                    label(id, sentence)
                        .px_3()
                        .pt_2()
                        .text_size(appearance::ui_text(12.))
                        .text_color(rgb(warning)),
                ),
            "lfs-failure-line-scrollbar",
        )
        .relative()
        // Gives up its height before anything beside it does.
        .flex_shrink(crate::views::GIVES_FIRST)
        // Outside the scrolled text, so a line that overflows keeps its
        // distance from a Download row beneath it.
        .pb_2()
        .min_h(crate::empty_state::block_height(1, false, window) + appearance::ui_size(16.))
        .children(edge)
    }
}

/// `body` beneath the line [`side_failures_line`] made, if it made one: the
/// body keeps its rows ([`crate::views::comparison_rows_floor`]) and the line
/// gives way first.
pub(super) fn above_comparison<V: 'static>(
    line: &FailureLine,
    key: ComparisonKey,
    sentence: Option<String>,
    body: AnyElement,
    ring: Option<&crate::focus_ring::FocusRing>,
    window: &Window,
    cx: &mut Context<V>,
) -> AnyElement {
    above_comparison_with_row(line, key, sentence, None, body, ring, window, cx)
}

/// [`above_comparison`] with a Download row, `row` ([`download_row`]),
/// between the line and `body`, or above `body` where no side failed: as in
/// Compare, the row keeps its height, the line gives way first and the body
/// keeps one whole row beside it.
#[allow(clippy::too_many_arguments)]
pub(super) fn above_comparison_with_row<V: 'static>(
    line: &FailureLine,
    key: ComparisonKey,
    sentence: Option<String>,
    row: Option<AnyElement>,
    body: AnyElement,
    ring: Option<&crate::focus_ring::FocusRing>,
    window: &Window,
    cx: &mut Context<V>,
) -> AnyElement {
    if sentence.is_none() && row.is_none() {
        return body;
    }
    let floor = if row.is_some() {
        crate::views::comparison_floor(1., window)
    } else {
        crate::views::comparison_rows_floor(window)
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .children(sentence.map(|sentence| line.render(key, sentence, ring, window, cx)))
        .children(row)
        .child(
            div()
                .debug_selector(|| "lfs-comparison".into())
                .flex_1()
                .min_h(floor)
                .child(body),
        )
        .into_any_element()
}

/// What a Download row says, on one line beside its buttons where it fits
/// there ([`sentence_fits_beside`]), otherwise, as beside a failing side's
/// line, in each button's tooltip and accessible description.
pub(crate) const DOWNLOAD_EXPLANATION: &str = "LFS preview objects require an explicit download.";

/// What a download's write is called, which its progress reads until Git
/// LFS reports more: the room a Download row that says its own progress
/// keeps for it whole beside Cancel download ([`buttons_share_line`]).
const DOWNLOAD_OBJECT: &str = "Download object";
const CANCEL_DOWNLOAD: &str = "Cancel download";
/// The least a failure's words keep beside the buttons: their first word
/// and the ellipsis they end in where they give way.
const FAILED: &str = "Failed…";

const DOWNLOAD_BEFORE: &str = "Download Before…";
const DOWNLOAD_AFTER: &str = "Download After…";
const DOWNLOAD_SOURCE: &str = "Download Source…";

/// What a Download button reads once its word "Download" has given way to
/// the comparison's row ([`download_row`]'s `short`), its pull icon and its
/// accessible name and tooltip still saying the whole.
fn short_label(full: &'static str) -> &'static str {
    full.strip_prefix("Download ").unwrap_or(full)
}

#[cfg(test)]
thread_local! {
    /// Whether a Download row's sentence fits beside its buttons, in place
    /// of [`sentence_fits_beside`]'s measurement, for a test of a
    /// measurement that disagrees with layout.
    pub(crate) static SENTENCE_FITS: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// How many download forms have opened, for a test of the presses that
    /// open one.
    pub(crate) static FORMS_OPENED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

thread_local! {
    static DOWNLOAD_BEFORE_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static DOWNLOAD_AFTER_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static DOWNLOAD_SOURCE_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static EXPLANATION_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static BEFORE_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static AFTER_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static SOURCE_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static PROGRESS_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static CANCEL_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static FAILED_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// `text` at the row's 12 pt as layout places it, laid out once per font
/// and size in `cache`.
fn row_text_width(
    text: &'static str,
    cache: &'static std::thread::LocalKey<crate::views::TextWidth>,
    window: &Window,
) -> Pixels {
    let width = crate::views::cached_text_width(cache, text, window, || {
        crate::views::line_widths(&[(text, appearance::ui_text(12.))], window)
    });
    crate::views::laid_out_text_width(width, window)
}

/// The label a Download button reads, with the cache of its width.
fn download_label(
    quick_source: bool,
    side: usize,
) -> (
    &'static str,
    &'static std::thread::LocalKey<crate::views::TextWidth>,
) {
    if quick_source {
        (DOWNLOAD_SOURCE, &DOWNLOAD_SOURCE_WIDTH)
    } else if side == 0 {
        (DOWNLOAD_BEFORE, &DOWNLOAD_BEFORE_WIDTH)
    } else {
        (DOWNLOAD_AFTER, &DOWNLOAD_AFTER_WIDTH)
    }
}

/// Whether a Download row `column` wide draws its sentence, which it draws
/// only on one line beside the buttons of all its `sides`, measured from
/// the interface font in the frame the row is drawn, each side in its
/// widest state where the row says what it `asks` for ([`side_widths`]).
/// Wrapped inside the pane, at 18 pt in a 461 × 490 window, it took three
/// lines above one button and left the comparison 4.5 px, and above two
/// stacked buttons it pushed Compare's footer under the status bar;
/// wherever it would not fit beside them, what it says moves to the
/// buttons, as it does beside a line.
fn sentence_fits_beside(
    quick_source: bool,
    sides: &[usize],
    asks: bool,
    column: Pixels,
    window: &Window,
) -> bool {
    let (gap, room) = row_room(column, window);
    let sentence = row_text_width(DOWNLOAD_EXPLANATION, &EXPLANATION_WIDTH, window);
    let [_, widest] = side_widths(quick_source, sides, false, asks, window);
    sentence + widest.iter().map(|&width| gap + width).sum::<Pixels>() <= room
}

/// A Download row's gap between its items and the room its line has
/// between the row's insets, in a row `column` wide.
fn row_room(column: Pixels, window: &Window) -> (Pixels, Pixels) {
    let rem = window.rem_size();
    let gap = window.pixel_snap(rem * 0.5);
    (gap, column - window.pixel_snap(rem * 0.75) * 2. + px(1e-3))
}

/// The width of each side's Download among `sides`, its word given way
/// where `short`, measured as layout places them in the frame the row is
/// drawn, and each side's widest state: where the row says what it `asks`
/// for, the review's, that is its Download or, while its download runs,
/// the running form, the progress text whole beside Cancel download,
/// whichever is wider.
fn side_widths(
    quick_source: bool,
    sides: &[usize],
    short: bool,
    asks: bool,
    window: &Window,
) -> [Vec<Pixels>; 2] {
    let (gap, _) = row_room(px(0.), window);
    let buttons = sides
        .iter()
        .map(|&side| {
            let (label, cache) = download_label(quick_source, side);
            if !short {
                return crate::labelled_button_width(label, cache, window);
            }
            let cache = if quick_source {
                &SOURCE_WIDTH
            } else if side == 0 {
                &BEFORE_WIDTH
            } else {
                &AFTER_WIDTH
            };
            crate::labelled_button_width(short_label(label), cache, window)
        })
        .collect::<Vec<_>>();
    let running = asks.then(|| {
        row_text_width(DOWNLOAD_OBJECT, &PROGRESS_WIDTH, window)
            + gap
            + crate::labelled_button_width(CANCEL_DOWNLOAD, &CANCEL_WIDTH, window)
    });
    let widest = buttons
        .iter()
        .map(|&button| running.map_or(button, |running| button.max(running)))
        .collect();
    [buttons, widest]
}

/// Whether the sides of `sides`, their Download buttons' word given way
/// where `short`, share one line in a row `column` wide, measured as layout
/// places them in the frame the row is drawn. Where they do not, they
/// stack one to a line. A row that says what it `asks` for, the review's,
/// decides it from each side's widest state ([`side_widths`]), and keeps
/// its failure's words at least "Failed…" wide beside the Downloads: where
/// the sides would not share a line in every state they stack, so a
/// download starting, failing or ending never changes the row's lines.
/// Decided from the Downloads alone, at 18 pt in a 461 × 490 window with
/// both sides missing, Before's running form drew Cancel download over
/// After… by 7 px, its progress 0 px wide.
fn buttons_share_line(
    quick_source: bool,
    sides: &[usize],
    short: bool,
    asks: bool,
    column: Pixels,
    window: &Window,
) -> bool {
    let (gap, room) = row_room(column, window);
    let [buttons, widest] = side_widths(quick_source, sides, short, asks, window);
    let line = |widths: &[Pixels]| widths.iter().fold(-gap, |line, &width| line + gap + width);
    line(&widest) <= room
        && (!asks || row_text_width(FAILED, &FAILED_WIDTH, window) + gap + line(&buttons) <= room)
}

/// `row`, which notes, where it is drawn with its buttons' word given way,
/// that it is, for a test: a row measured and not drawn notes nothing.
fn noted_short(row: Div, short: bool) -> Div {
    #[cfg(test)]
    if short {
        use gpui_kit::base::ElementExt as _;
        return row.on_prepaint(|_, _, _| {
            crate::views::note_compact_label("download-lfs-row", &DOWNLOAD_BEFORE.into())
        });
    }
    let _ = short;
    row
}

/// Who asked for an LFS download: Compare, whose header shows the write's
/// progress and Cancel, or the rewritten-series review the id names, which
/// covers that header and shows them in its own Download row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asker {
    Compare,
    Review(EntityId),
}

impl Asker {
    /// Where the confirmation says Cancel stands while the download runs.
    fn cancel_place(self) -> &'static str {
        match self {
            Asker::Compare => "The download can be cancelled from the operation bar.",
            Asker::Review(_) => {
                "While it runs, Cancel download stands in the review's Download row."
            }
        }
    }
}

/// A window's LFS downloads: the form open, the caller whose form last
/// opened its confirmation, the caller whose download runs, and how the last
/// one ended.
#[derive(Default)]
pub(crate) struct Downloads {
    form: Option<WeakEntity<LfsDownloadForm>>,
    confirming: Option<Asker>,
    running: Option<Asker>,
    ended: Option<(Asker, Option<SharedString>)>,
}

impl Downloads {
    /// Who asked for the download that runs.
    pub(crate) fn running(&self) -> Option<Asker> {
        self.running
    }

    /// How the download `asker` asked for last ended: `Some(None)` where it
    /// succeeded, `Some(Some(words))` with the failure's words.
    pub(crate) fn ended(&self, asker: Asker) -> Option<Option<SharedString>> {
        self.ended
            .as_ref()
            .filter(|(by, _)| *by == asker)
            .map(|(_, failure)| failure.clone())
    }
}

/// What a caller that covers the operation bar, the rewritten-series review,
/// says in its Download row of the download it asked for: Compare's header
/// says it for Compare, which passes none.
pub(crate) struct Asked {
    /// The focus each side's Download tracks.
    pub(crate) focus: [FocusHandle; 2],
    /// Cancel download's focus.
    pub(crate) cancel: FocusHandle,
    pub(crate) state: AskedState,
    /// Cancels the running write: the window's `cancel_operation`.
    pub(crate) on_cancel: OnCancel,
}

/// What Cancel download calls.
pub(crate) type OnCancel = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

pub(crate) enum AskedState {
    /// Nothing it asked for runs or failed: the row as Compare draws it.
    Resting,
    /// Side `side`'s download runs; once the write looks busy
    /// (`workspace::BusyLook`), `progress`, the window's progress text,
    /// stands where its Download stood, beside Cancel download.
    Running {
        side: usize,
        progress: Option<SharedString>,
    },
    /// It failed, saying these words, and its Download is back.
    Failed(SharedString),
}

/// The row offering a download of each side among `targets`, whose object
/// is missing, where `quick_source` calls the one side Source, beneath a
/// failing side's line if `line_above`, in a column `column` wide; where
/// `short`, its buttons' word "Download" has given way, so two that stack
/// with it share a line without it (DESIGN.md, "One responsive order"). It looks
/// refused while a write runs (`refused`, and `dimmed` once the write looks
/// busy, `workspace::BusyLook`), and a press asks `download` for the side's
/// target. Its frames stand on the pane's text edge, the line's above it: a
/// 0.75 rem bar holds neither a ghost Button's pull nor its ring (DESIGN.md,
/// "One left edge"). Compare and the rewritten-series review draw it alike;
/// the review, which covers the operation bar, also says there what it
/// `asked` for: while its download runs, where the pressed Download stood,
/// the progress and Cancel download, the row keeping its height and place,
/// and once it fails, the failure's words in the sentence's place.
#[allow(clippy::too_many_arguments)]
pub(crate) fn download_row(
    targets: Vec<(usize, LfsDownloadTarget)>,
    quick_source: bool,
    line_above: bool,
    short: bool,
    column: Pixels,
    (refused, dimmed): (bool, bool),
    asked: Option<Asked>,
    download: impl Fn(usize, LfsDownloadTarget, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    if targets.is_empty() {
        return div().into_any_element();
    }
    let sides = targets.iter().map(|(side, _)| *side).collect::<Vec<_>>();
    let asks = asked.is_some();
    let fits = sentence_fits_beside(quick_source, &sides, asks, column, window);
    #[cfg(test)]
    let fits = SENTENCE_FITS.get().unwrap_or(fits);
    let (failure, running) = match asked.as_ref().map(|asked| &asked.state) {
        Some(AskedState::Failed(words)) => (Some(words.clone()), None),
        Some(AskedState::Running {
            side,
            progress: Some(progress),
        }) => (None, Some((*side, progress.clone()))),
        _ => (None, None),
    };
    // The line the sentence is drawn on, where it is, or its failure's:
    // it gives way before the buttons' word does.
    let one_line = !line_above && fits && !short;
    let sentence_moves = !one_line || failure.is_some();
    let warning = appearance::labels(cx).warning;
    let share = buttons_share_line(quick_source, &sides, short, asks, column, window);
    let download = std::rc::Rc::new(download);
    let asked = asked.map(std::rc::Rc::new);
    // What stands in the sentence's place: the sentence, where it is drawn
    // (beside a line, or where it would not fit on one line beside the
    // buttons, it moves to each button's tooltip and description), or the
    // failure's words. Either shrinks, ending in an ellipsis and whole in
    // its tooltip, so the buttons keep their size and their line: should
    // the measurement that stood the sentence there disagree with layout,
    // it ends inside the pane rather than running past it, and a failure
    // landing never grows the row (wrapped, the words took a line of their
    // own, about 25 px at 18 pt, as they landed).
    let said = if let Some(words) = failure {
        let whole = words.clone();
        Some(
            label("lfs-download-failure", words)
                .debug_selector(|| "lfs-download-failure".into())
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(warning))
                .tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx))
                .into_any_element(),
        )
    } else {
        (!sentence_moves).then(|| {
            label("lfs-preview-download-explanation", DOWNLOAD_EXPLANATION)
                .debug_selector(|| "lfs-preview-download-explanation".into())
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .tooltip(|window, cx| Tooltip::new(DOWNLOAD_EXPLANATION).build(window, cx))
                .text_size(appearance::ui_text(12.))
                .into_any_element()
        })
    };
    let items = targets.into_iter().map(|(side, target)| {
        if let (Some((_, progress)), Some(asked)) = (
            running.as_ref().filter(|(running, _)| *running == side),
            asked.as_ref(),
        ) {
            // Where the pressed Download stood.
            return running_form(progress.clone(), &asked.cancel, asked.on_cancel.clone(), cx)
                .into_any_element();
        }
        let download = download.clone();
        let focus = asked.as_ref().map(|asked| asked.focus[side].clone());
        let full = download_label(quick_source, side).0;
        div()
            .debug_selector(move || format!("download-lfs-side-{side}"))
            .flex_none()
            .child(
                button(
                    ("download-lfs-side", side),
                    if short { short_label(full) } else { full },
                    "pull",
                    false,
                )
                .when_some(focus, |button, focus| button.track_focus(&focus))
                .when(sentence_moves, |button| {
                    crate::branch_actions::Described(button)
                        .aria_description(DOWNLOAD_EXPLANATION)
                        .0
                })
                // Its whole name stays in its accessible name and
                // tooltip, beside the sentence where that moved here.
                .when(short, |button| button.accessibility_label(full))
                .map(|button| match (short, sentence_moves) {
                    (true, true) => button.tooltip(format!("{full} · {DOWNLOAD_EXPLANATION}")),
                    (true, false) => button.tooltip(full),
                    (false, true) => button.tooltip(DOWNLOAD_EXPLANATION),
                    (false, false) => button,
                })
                // `open_lfs_download` refuses while a write runs.
                .map(|button| {
                    crate::workspace::refusing(
                        button,
                        || format!("download-lfs-side-{side}"),
                        refused,
                        dimmed,
                    )
                })
                .on_click(move |_, window, cx| download(side, target.clone(), window, cx))
                .node_recorded(),
            )
            .into_any_element()
    });
    // Its lines are its buttons': one where they share it, otherwise one
    // each, with what stands in the sentence's place beside the first; so
    // the row keeps its lines whether it rests, runs or failed.
    let mut lines = if share {
        vec![items.collect::<Vec<_>>()]
    } else {
        items.map(|item| vec![item]).collect()
    };
    if let (Some(said), Some(first)) = (said, lines.first_mut()) {
        first.insert(0, said);
    }
    noted_short(div(), short)
        .flex_none()
        .px_3()
        // The line above already keeps this row's distance.
        .when(!line_above, |row| row.pt_2())
        .pb_2()
        .flex()
        .flex_col()
        .gap_2()
        .children(lines.into_iter().map(|line| {
            div()
                .min_w_0()
                .flex()
                .flex_nowrap()
                .items_center()
                .gap_2()
                .children(line)
        }))
        .into_any_element()
}

/// A running download's progress and its Cancel download, as the
/// rewritten-series review draws them for the download it asked for: in its
/// Download row where the pressed Download stood, and at the top of its
/// body while it shows another file or commit. The progress, the window's
/// progress text, gives way first, ending in an ellipsis, whole in its
/// tooltip.
pub(crate) fn running_form(
    progress: SharedString,
    cancel: &FocusHandle,
    on_cancel: OnCancel,
    cx: &App,
) -> Div {
    let whole = progress.clone();
    let text = div()
        .id("lfs-download-progress")
        .debug_selector(|| "lfs-download-progress".into())
        .role(Role::Status)
        .aria_label(progress.clone())
        .live_region(gpui::accesskit::Live::Off, false)
        .tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx))
        .min_w_0()
        .truncate()
        .text_size(appearance::ui_text(12.))
        .text_color(rgb(palette(cx).accent))
        .child(progress);
    // Tests assemble the window's tree from what they record.
    #[cfg(test)]
    crate::native_accessibility::written::built(&text);
    div()
        .flex_shrink(1.)
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .child(text)
        .child(
            div().flex_none().child(
                button("cancel-lfs-download", CANCEL_DOWNLOAD, "close", false)
                    .debug_selector(|| "cancel-lfs-download".into())
                    .track_focus(cancel)
                    .on_click(move |_, window, cx| on_cancel(window, cx))
                    .node_recorded(),
            ),
        )
}

impl GitTurtle {
    /// What the failing sides of the shown text say above it, where they say
    /// it nowhere else: Rendered and Diagrams say it in the side's own place,
    /// so the line would say it twice and take the comparison's room. A
    /// download could not help, so none of them offers one.
    pub(super) fn lfs_failure_sentence(&self) -> Option<String> {
        let Some(Content::Text {
            lfs_failed,
            markdown,
            diagrams,
            ..
        }) = self.content.as_deref()
        else {
            return None;
        };
        let own_words = (self.text_mode == TextMode::Markdown && markdown.is_some())
            || (self.text_mode == TextMode::Diagrams && diagrams.is_some());
        if own_words {
            return None;
        }
        let names = if self.is_quick_source() {
            ["Source", "Source"]
        } else {
            ["Before", "After"]
        };
        side_failures_line(*lfs_failed, names)
    }

    /// The line above the comparison, where the failing sides say it there:
    /// a child of Compare's own column, which gives way first; a Download
    /// row beside it keeps its height and the comparison keeps one row.
    pub(super) fn render_lfs_failure_line(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let sentence = self.lfs_failure_sentence()?;
        let ring = self.focus_ring.clone();
        let key = comparison_key(self.selected_file.and_then(|index| self.files.get(index)));
        Some(
            self.lfs_failure_line
                .render(key, sentence, Some(&ring), window, cx),
        )
    }

    fn lfs_download_targets(&self) -> Vec<(usize, LfsDownloadTarget)> {
        let Some(file) = self.selected_file.and_then(|index| self.files.get(index)) else {
            return Vec::new();
        };
        self.content
            .as_deref()
            .map_or_else(Vec::new, |content| preview_targets(content, file))
    }

    /// Whether the shown file has a side to download: its row keeps its
    /// height, so the comparison gives way to it rather than cover it.
    pub(super) fn lfs_download_offered(&self) -> bool {
        !self.lfs_download_targets().is_empty()
    }

    /// Compare's Download row, its buttons' word given way where `short`.
    pub(super) fn render_lfs_download_actions(
        &self,
        column: Pixels,
        short: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        download_row(
            self.lfs_download_targets(),
            self.is_quick_source(),
            self.lfs_failure_sentence().is_some(),
            short,
            column,
            (
                self.operation_busy.is_some(),
                self.operation_shown().is_some(),
            ),
            // Its header shows the download's progress and Cancel.
            None,
            move |_, target, window, cx| {
                Self::open_lfs_download_from(&owner, target, Asker::Compare, window, cx);
            },
            window,
            cx,
        )
    }

    /// A download's write begins: it runs for the caller whose form opened
    /// its confirmation, and where Compare asked, the preview it was asked
    /// from, which its end reads again if still shown, is captured. Only
    /// the caller that asked reads again: a download the review asked for
    /// once read Compare's unchanged file again too.
    pub(super) fn lfs_download_begins(&mut self) -> Option<crate::workspace::LfsView> {
        let asker = self
            .lfs_downloads
            .confirming
            .take()
            .unwrap_or(Asker::Compare);
        self.lfs_downloads.running = Some(asker);
        self.lfs_downloads.ended = None;
        if asker != Asker::Compare {
            return None;
        }
        self.selected_file
            .and_then(|index| self.files.get(index))
            .cloned()
            .map(|file| (file, self.mode, self.generation))
    }

    /// The running download's write ends, having failed with `failure`'s
    /// words or succeeded: the caller that asked reads how.
    pub(super) fn lfs_download_ends(&mut self, failure: Option<SharedString>) {
        if let Some(asker) = self.lfs_downloads.running.take() {
            self.lfs_downloads.ended = Some((asker, failure));
        }
    }

    /// Whether a press asks the window for a download that it accepts,
    /// opening a form: a press it refuses records nothing
    /// ([`Self::open_lfs_download`]).
    pub(super) fn open_lfs_download_from(
        owner: &WeakEntity<Self>,
        target: LfsDownloadTarget,
        asker: Asker,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        owner
            .update(cx, |owner, cx| {
                owner.open_lfs_download(target, asker, window, cx)
            })
            .unwrap_or(false)
    }

    /// Starts a download's write as `write_from` does once `asker`'s
    /// confirmation is accepted, for a test, which cannot run Git LFS: the
    /// window busy with its label and its Cancel, the write held.
    #[cfg(test)]
    pub(crate) fn simulate_lfs_download(
        &mut self,
        asker: Asker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<crate::workspace::LfsView> {
        self.lfs_downloads.confirming = Some(asker);
        let selection = self.lfs_download_begins();
        self.operation_busy = Some(DOWNLOAD_OBJECT);
        let _ = self.begin_operation_control(window, cx);
        cx.notify();
        selection
    }

    /// Gives the open form a plan for its object from `remote`, as its
    /// source read does where Git LFS runs, for a test, which cannot run
    /// it: the plan stands in for a reviewed one, and its write refuses
    /// before any command runs.
    #[cfg(test)]
    pub(crate) fn stand_in_plan(&self, remote: &str, cx: &mut App) {
        let form = self
            .lfs_downloads
            .form
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .expect("the form is open");
        form.update(cx, |form, cx| {
            form.cancel();
            form.error = None;
            form.plan = Some(LfsDownloadPlan::stand_in(form.target.clone(), remote));
            cx.notify();
        });
    }

    /// Ends the write [`Self::simulate_lfs_download`] started, failing
    /// with `failure` or succeeding, as `write_from`'s end does.
    #[cfg(test)]
    pub(crate) fn simulate_lfs_download_end(
        &mut self,
        failure: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.operation_busy = None;
        self.finish_operation_control(window, cx);
        self.lfs_download_ends(failure.map(SharedString::from));
        cx.notify();
    }

    pub(super) fn open_lfs_download(
        &mut self,
        target: LfsDownloadTarget,
        asker: Asker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // A second press finds the form open and does nothing: two clicks
        // delivered before any frame opened two identical forms.
        let open = self
            .lfs_downloads
            .form
            .as_ref()
            .is_some_and(|form| form.upgrade().is_some());
        if self.operation_busy.is_some() || open {
            return false;
        }
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let owner = cx.entity().downgrade();
        let remote = self.remote_name.read(cx).value().trim().to_owned();
        let remotes = self
            .remotes
            .iter()
            .map(|remote| remote.name.clone())
            .collect();
        let form = cx.new(|cx| {
            LfsDownloadForm::new(owner, repo, (target, asker), remote, remotes, window, cx)
        });
        form.update(cx, |this, cx| this.prepare(window, cx));
        self.lfs_downloads.form = Some(form.downgrade());
        #[cfg(test)]
        FORMS_OPENED.set(FORMS_OPENED.get() + 1);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let shown = form.clone();
            // In the dialog's content, as the profile form is: the form takes
            // what the cap leaves it and scrolls the rest, revealing the
            // control Tab moves onto, with the ring's room below it, which
            // the footer's gap gives back. In the dialog's own scrolling
            // body, at 18 pt in a 461 × 490 window, the form was taller than
            // what the cap left, so that body cut Read configured source's
            // ring at its foot and revealed nothing.
            dialog
                .title(native_accessibility::dialog_title(
                    "lfs-download-title",
                    "Download an LFS preview",
                ))
                .width(appearance::dialog_width(px(650.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                .footer(crate::tags::dialog_footer(
                    "Review download",
                    Some("Cancel"),
                ))
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |this, cx| this.review(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
        });
        true
    }
}

struct LfsDownloadForm {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    target: LfsDownloadTarget,
    /// Who asked: the confirmation says where its Cancel stands.
    asker: Asker,
    remote: Entity<InputState>,
    remotes: Vec<String>,
    reader: operations::SerialExecutor,
    task: Option<Task<()>>,
    cancellation: gitturtle_core::HistoryCancellation,
    plan: Option<LfsDownloadPlan>,
    pending: bool,
    error: Option<String>,
    _subscription: Subscription,
    /// The form's scroll, which reveals the control Tab moves onto.
    body: crate::focus_reveal::FocusReveal,
}
impl Drop for LfsDownloadForm {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl LfsDownloadForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        // The object to download, and who asked for it.
        (target, asker): (LfsDownloadTarget, Asker),
        remote: String,
        remotes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remote = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(remote)
                .placeholder("Configured remote name")
        });
        // Return in the field goes to the dialog's default action alone,
        // which runs once per press, as the key goes down, never on a held
        // key's repeats, under X11 too: its review reads the source when
        // there is no plan. The field's own `PressEnter`, which it sends for
        // every Enter key-down, read the source as well, so every press read
        // it twice and each repeat cancelled and began the read again, or
        // said again to choose a remote.
        let subscription = cx.subscribe_in(
            &remote,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.cancel();
                    this.plan = None;
                }
                cx.notify();
            },
        );
        Self {
            owner,
            repo,
            target,
            asker,
            remote,
            remotes,
            reader: operations::SerialExecutor::new("lfs-preview-source"),
            task: None,
            cancellation: Default::default(),
            plan: None,
            pending: false,
            error: None,
            _subscription: subscription,
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
        }
    }
    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.task = None;
        self.pending = false;
    }
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel();
        self.plan = None;
        self.error = None;
        let remote = self.remote.read(cx).value().trim().to_owned();
        if remote.is_empty() {
            crate::tags::set_error_line(
                &mut self.error,
                "Choose a configured remote for this LFS object. Add a remote from the branch menu if this repository has none.",
                window,
                cx,
            );
            cx.notify();
            return;
        }
        self.pending = true;
        self.cancellation = Default::default();
        let cancellation = self.cancellation.clone();
        let repo = self.repo.clone();
        let target = self.target.clone();
        let response = self.reader.submit_read(move || {
            gitturtle_core::run_cancellable_inspection(cancellation, || {
                repo.lfs_download_plan(&target, &remote)
            })
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "LFS source inspection was interrupted. Read the source again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.pending = false;
                match result {
                    Ok(plan) => this.plan = Some(plan),
                    Err(error) => crate::tags::set_error_line(
                        &mut this.error,
                        format!("{error:#}"),
                        window,
                        cx,
                    ),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(plan) = self.plan.clone() else {
            self.prepare(window, cx);
            return;
        };
        let asker = self.asker;
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path.as_deref() != Some(this_path(&self.repo)) || owner.operation_busy.is_some() { crate::tags::set_error_line(&mut self.error, "The repository changed or another operation started. Reopen this download after it finishes.", window, cx); cx.notify(); return; }
            window.close_dialog(cx);
            // The write this confirmation starts runs for who asked.
            owner.lfs_downloads.confirming = Some(asker);
            owner.confirm_git_write("Download selected LFS object".into(), format!("File: {}\nObject: sha256:{}\nSize: {}\nRemote: {}\n{}\n\nThis explicit transfer fetches only this object's pointer. Git LFS uses your configured credentials and SSH host verification. {}\n\nGitTurtle verifies the object's SHA-256 and size before previewing it. Working files and the index remain unchanged; no checkout, smudge, prune, or push is performed.", plan.target.path.display(), plan.oid, crate::counted(plan.size, "byte", "bytes"), plan.remote, plan.source, asker.cancel_place()), DOWNLOAD_OBJECT, WriteCommand::DownloadLfs(Arc::new(plan)), window, cx);
        });
    }
}

fn this_path(repo: &GitRepository) -> &std::path::Path {
    repo.path()
}

impl Render for LfsDownloadForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pointer = gitturtle_preview::detect_lfs_pointer(&self.target.pointer);
        // The remotes and Read configured source, ghost Buttons at the
        // dialog's text edge, stand the label or icon of each line's first
        // on that edge, their frames their pull before it (DESIGN.md, "One
        // left edge"); the form keeps the pull with the ring's room on its
        // left, and the ring's room on its right, given back through its
        // margins, so its clip cuts neither them nor the field's ring. It
        // keeps the ring's room above and below its controls too, given
        // back through its margin, and a wrapper keeps the room below them
        // inside the dialog's clip.
        let room = appearance::button_ring_room(cx);
        let lead = crate::ghost_pull(window) + room;
        // A control Tab moves onto scrolls into view with its ring's room
        // (`focus_reveal`).
        self.body.reveal(window, cx);
        let field = self.remote.read(cx).focus_handle(cx);
        let read = self.body.focus("read-lfs-source", cx);
        let remotes: Vec<_> = (0..self.remotes.len().min(12))
            .map(|index| self.body.focus(("lfs-source-remote", index), cx))
            .collect();
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        // At most 450 px, and never more than the whole dialog's cap, its
        // title and footer included: a loose bound only. What holds the
        // body to the room the cap leaves it is the `min_h_0` chain from
        // the dialog's content down to this list, which shrinks it to that
        // room and scrolls the rest.
        let cap = px(450.).min(appearance::dialog_max_height(window));
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("lfs-download-review-content").flex().flex_col().gap_3().max_h(cap + room * 2.).min_h_0().overflow_y_scroll().track_scroll(self.body.scroll()).p(room).m(-room).pl(lead).ml(-lead)
            .child(label("lfs-download-file", format!("File: {}", self.target.path.display())).text_size(appearance::ui_text(13.)))
            .when_some(pointer.as_ref(), |element, pointer| element.child(label("lfs-download-object", format!("sha256:{}\n{}", pointer.oid, crate::counted(pointer.size, "byte", "bytes"))).text_size(appearance::ui_text(12.))))
            .child(label("lfs-download-source-label", "Choose the configured source").text_size(appearance::ui_text(12.)))
            // Reported from a wrapper of its own: a list that tracks its
            // scroll hands its own children's bounds to the scroll handle,
            // not to its `on_children_prepainted`.
            .child(div().flex().flex_col().on_children_prepainted(self.body.control(field, room)).child(Input::new(&self.remote).aria_label("Git LFS download remote")))
            .child(crate::edge_row(window).on_children_prepainted(self.body.controls(remotes.iter().cloned().map(Some).collect(), room)).children(self.remotes.iter().take(12).zip(&remotes).enumerate().map(|(index, (name, focus))| { let name = name.clone(); crate::edge_button(("lfs-source-remote", index), name.clone()).track_focus(focus).debug_selector(move || format!("lfs-source-remote-{index}")).disabled(self.pending).on_click(cx.listener(move |this, _, window, cx| { this.remote.update(cx, |input, cx| input.set_value(name.clone(), window, cx)); this.prepare(window, cx); })) })))
            .child(crate::edge_row(window).on_children_prepainted(self.body.controls(vec![Some(read.clone())], room)).child(button("read-lfs-source", "Read configured source", "refresh", false).track_focus(&read).debug_selector(|| "read-lfs-source".into()).flex_shrink(1.).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.prepare(window, cx)))))
            .when(self.pending, |element| element.child(label("lfs-source-loading", "Resolving the selected pointer, Git LFS tooling, and configured source…").text_size(appearance::ui_text(12.))))
            .when_some(self.plan.as_ref(), |element, plan| element.child(label("lfs-source-resolved", plan.source.clone()).text_size(appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| crate::tags::error_line("lfs-download-error", error, Some(appearance::ui_text(12.)), window, cx).text_color(rgb(p.warning))))
            .child(label("lfs-download-limit", "Normal browsing stays passive. This action is limited to one object up to 32 MiB; decoder and memory limits still apply. No credentials are stored in this form.").text_size(appearance::ui_text(12.)).text_color(rgb(p.muted))), "lfs-download-scrollbar").min_h_0())
    }
}

#[cfg(test)]
mod held_return_tests {
    use super::*;
    use crate::branch_actions::tests::{REPEATS, enter};
    use crate::tags::tests::{draw, tagged_repository, window};
    use ::core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    /// A held Return in the LFS download's source field reads the source,
    /// or asks for a remote, once per press, under X11 too: with no remote
    /// named the form says to choose one once per press, and with a remote
    /// the repository does not configure it reads the source once and says
    /// why it failed, the key's repeats (`REPEATS`) reading nothing more,
    /// each waited out until the form's reader has answered; after the
    /// key-up a new press is answered again. Each answer is an alert, by
    /// its generation. The field's own `PressEnter` read the source as well
    /// as the dialog's default action, whose review reads it when there is
    /// no plan: every press read it twice, or asked twice, and each repeat
    /// cancelled the read and began it again, or asked again.
    #[gpui::test]
    async fn a_held_return_in_the_lfs_source_field_reads_once_per_press(
        cx: &mut gpui::TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        // The form each opening builds.
        let opened: Rc<RefCell<Option<WeakEntity<LfsDownloadForm>>>> = Rc::default();
        let _opened = cx.update(|_, cx| {
            let opened = opened.clone();
            cx.observe_new(move |_: &mut LfsDownloadForm, _, cx| {
                *opened.borrow_mut() = Some(cx.entity().downgrade());
            })
        });
        let target = LfsDownloadTarget {
            path: "assets/large.bin".into(),
            blob_oid: None,
            pointer: format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 12\n",
                "a".repeat(64)
            )
            .into_bytes(),
        };
        // The generation of the latest alert the window draws.
        let alerts = |cx: &mut VisualTestContext| match crate::copy_feedback::tests::alerted(cx) {
            Some((ElementId::NamedInteger(_, generation), _)) => generation,
            other => panic!("an alert is drawn: {other:?}"),
        };
        for (case, remote) in [("no remote", ""), ("a remote not configured", "nowhere")] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.remote_name
                        .update(cx, |input, cx| input.set_value(remote, window, cx));
                    app.open_lfs_download(
                        target.clone(),
                        crate::lfs_download::Asker::Compare,
                        window,
                        cx,
                    )
                })
            });
            let form = opened
                .borrow_mut()
                .take()
                .and_then(|form| form.upgrade())
                .expect("the form opens");
            // The form's reader answers, and its answer lands.
            let lands = |cx: &mut VisualTestContext| {
                form.read_with(cx, |form, _| form.reader.drain());
                cx.run_until_parked();
                draw(cx);
            };
            lands(cx);
            cx.update(|window, cx| {
                let field = form.read(cx).remote.read(cx).focus_handle(cx);
                field.focus(window, cx);
            });
            draw(cx);
            for press in ["the first press", "a new press after the key-up"] {
                let after = alerts(cx) + 1;
                enter(cx, Some(false));
                lands(cx);
                assert_eq!(alerts(cx), after, "{case}, {press}: answered once");
                for (repeat, events) in REPEATS {
                    for down in events {
                        enter(cx, *down);
                    }
                    lands(cx);
                    cx.update(|window, cx| {
                        assert!(window.has_active_dialog(cx), "{case}, {repeat}");
                        let form = form.read(cx);
                        assert!(!form.pending, "{case}, {repeat}: nothing is read");
                        assert!(
                            form.remote.read(cx).focus_handle(cx).is_focused(window),
                            "{case}, {repeat}: focus stays in the field"
                        );
                    });
                    assert_eq!(
                        alerts(cx),
                        after,
                        "{case}, {repeat}: nothing more is read or said"
                    );
                }
                enter(cx, None);
                draw(cx);
            }
            cx.update(|window, cx| window.close_dialog(cx));
            draw(cx);
        }
    }
}
