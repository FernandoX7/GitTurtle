//! Contextual revision browsing with immutable anchors and retained UI context.
//! Git history and preview preparation use the existing replaceable worker.
use crate::*;
use focus_reveal::{FocusReveal, Trigger};
use gitturtle_core::{FileHistoryEntry, FileHistoryPage};
use gpui_kit::prelude::FluentBuilder;

const PAGE_SIZE: usize = 100;

/// The id File History's message panel is revealed under as one region
/// (`FocusReveal::region`).
const MESSAGE_REGION: &str = "file-history-message";

/// The panel's title, at the header's 12 pt semibold.
const TITLE: &str = "File history";
/// Close's label where it keeps one.
const CLOSE_LABEL: &str = "Back";
/// Close's accessible name in every form.
const CLOSE_NAME: &str = "Close file history and restore previous view";
/// Close's tooltip where it keeps only its icon: its label, then its name.
const CLOSE_TOOLTIP: &str = "Back · Close file history and restore previous view";
/// Newer page's label, and its name and tooltip where it keeps only its
/// icon.
const NEWER_PAGE: &str = "Newer page";
/// Older page's, as Newer page's.
const OLDER_PAGE: &str = "Older page";

thread_local! {
    static TITLE_WIDTH: std::cell::RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { std::cell::RefCell::new(None) };
    static CLOSE_LABEL_WIDTH: std::cell::RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { std::cell::RefCell::new(None) };
    /// The wider of Newer page's and Older page's labels.
    static PAGE_LABEL_WIDTH: std::cell::RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { std::cell::RefCell::new(None) };
    /// The anchor's line, "From ‹hash› · first-parent lineage", as one line.
    static FROM_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// The least heights of File History's parts, so the content stays whole
/// first (DESIGN.md's responsive order): the header and the footer, which
/// scroll their own rows, give way down to their first row, and the body
/// keeps the message's toolbar and first line and one whole revision row.
/// A failed page's notice keeps a line of its message and Retry page whole,
/// its message scrolling inside it, and the revision row stays whole beside
/// it, the notice giving way down to that least first. A panel shorter than
/// all three scrolls. At 18 pt in a 461 × 490 window the header's and footer's
/// shares left the revisions 55 px, part of one 94 px row, and at 461 × 420
/// a failed page's notice left them about 31 px.
struct PanelLeast {
    /// The header's padding and its top row, with its bottom border.
    header: Pixels,
    /// The message's toolbar and first line, where a revision is shown, a
    /// failed page's notice, where one shows, and one revision row; or,
    /// where taller, the body whose rest beside the message panel's whole
    /// share (`commit_message::MESSAGE_SHARE`), which a long message
    /// takes, still holds the notice and the row.
    body: Pixels,
    /// A failed page's notice: its padding, a line of its message and Retry
    /// page; none where no page failed.
    failure: Pixels,
    /// A line of a failed page's message, as its text lays one out on a
    /// device pixel, which the message keeps as its notice gives way.
    error_line: Pixels,
    /// One whole revision row, which the revisions keep.
    row: Pixels,
    /// The footer's padding and its status line, with its top border.
    footer: Pixels,
}

impl PanelLeast {
    /// The least heights for a panel whose message panel, where a revision
    /// is shown, keeps at least `message` (`Toolbar::revision_message_least`).
    fn of(message: Option<Pixels>, failed: bool, window: &Window) -> Self {
        let rem = window.rem_size();
        let edge = window.pixel_snap(px(1.)) + rem * 1.5;
        let line = |size: f32| {
            window
                .text_style()
                .line_height
                .to_pixels(crate::appearance::ui_text(size).into(), rem)
        };
        let row = crate::appearance::ui_size(68.);
        let error_line = text_line(11., window);
        // Its `py_2` and `gap_2`, a line of its message and Retry page.
        let failure = if failed {
            rem * 1.5 + error_line + crate::appearance::ui_size(28.)
        } else {
            px(0.)
        };
        let below = failure + row;
        let body = if let Some(least) = message {
            // A body taller than its least gives the message panel up to its
            // share, which a long message takes, so the room below it grows
            // by the rest alone: at 18 pt in a 461 × 420 window a failed
            // page's notice made the body tall enough to give the message
            // more than its least, which the revisions gave up.
            (least + below).max(below / (1. - commit_message::MESSAGE_SHARE))
        } else {
            below
        };
        Self {
            header: edge + crate::appearance::ui_size(28.),
            body,
            failure,
            error_line,
            row,
            footer: edge + line(10.),
        }
    }

    /// The panel's least height: its header's, its body's and its footer's.
    fn total(&self) -> Pixels {
        self.header + self.body + self.footer
    }

    /// The heights the header and the footer take in a panel `tall`, where
    /// their rows are `header` and `footer` tall with their padding and
    /// border, as a column's flex items take them (CSS Flexbox, resolving
    /// flexible lengths): each as tall as its rows, at most 30% and 25% of
    /// the panel and at least its least, and where the three would not fit,
    /// the body keeping its least, the two shrink in proportion to their
    /// rows' heights, each down to its least, the panel scrolling past that.
    fn shares(&self, tall: Pixels, header: Pixels, footer: Pixels) -> [Pixels; 2] {
        let bases = [header, footer];
        let least = [self.header, self.footer];
        let most = [tall * 0.30, tall * 0.25];
        let clamp = |part: usize, height: Pixels| height.min(most[part]).max(least[part]);
        let hypothetical = [clamp(0, header), clamp(1, footer)];
        if hypothetical[0] + hypothetical[1] + self.body <= tall {
            return hypothetical;
        }
        let mut target = hypothetical;
        // A part whose rows are shorter than its least keeps its least; the
        // body, whose basis is none, keeps its own.
        let mut frozen = [header < hypothetical[0], footer < hypothetical[1]];
        while frozen.contains(&false) {
            let mut free = tall - self.body;
            let mut scaled = 0.;
            for part in 0..2 {
                if frozen[part] {
                    free -= target[part];
                } else {
                    free -= bases[part];
                    scaled += f32::from(bases[part]);
                }
            }
            let mut violations = [Pixels::ZERO; 2];
            for part in (0..2).filter(|part| !frozen[*part]) {
                let wanted = if scaled > 0. {
                    bases[part] - free.abs() * (f32::from(bases[part]) / scaled)
                } else {
                    bases[part]
                };
                target[part] = clamp(part, wanted);
                violations[part] = target[part] - wanted;
            }
            let violation = violations[0] + violations[1];
            for part in 0..2 {
                if frozen[part] {
                    continue;
                }
                frozen[part] = if violation > Pixels::ZERO {
                    violations[part] > Pixels::ZERO
                } else if violation < Pixels::ZERO {
                    violations[part] < Pixels::ZERO
                } else {
                    true
                };
            }
        }
        target
    }
}

/// The note beneath the anchor's line, which wraps in the header's room.
const FOLLOWS: &str = "Follows renames and the first parent of each merge.";

/// The heights File History's header and footer take, and whether each
/// scrolls its own rows, decided from the room their rows have without a
/// scroll cue's gutter and the height the window lays the panel out in,
/// never from whether they scrolled in their last frame: each is as tall
/// as its rows laid out in that room, within its share of the panel
/// (`PanelLeast::shares`); Newer page and Older page keep their labels
/// where the footer then holds them, else their icons where it holds
/// those (DESIGN.md's responsive order), and only otherwise does the footer
/// scroll, keeping their icons, laid out in the room its gutter leaves.
/// Each scrolls exactly where its rows overflow it so. Laid out in the
/// gutter's room once it had scrolled, the footer kept Newer page and Older
/// page on two rows reached from a shorter window and on one from a taller
/// one at 13 pt in
/// 432 × 460, and at 18 pt its labels' two overflowing rows became one row
/// of icons inside the gutter, which fitted, in every other frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Regions {
    header: Pixels,
    footer: Pixels,
    /// The body's between them, at least its least (`PanelLeast::body`).
    body: Pixels,
    header_scrolls: bool,
    footer_scrolls: bool,
    page_labels: bool,
}

impl Regions {
    /// Whether these regions hold more of the header's or the footer's rows
    /// than `other` does: taller than `other`'s where its rows scroll.
    fn hold_more_than(&self, other: &Self) -> bool {
        (other.header_scrolls && self.header > other.header + SETTLED)
            || (other.footer_scrolls && self.footer > other.footer + SETTLED)
    }
}

/// What decides File History's message toolbar and its `Regions`
/// (`GitTurtle::message_toolbar`): the inspector's width and height and
/// whether the window pins it, the font, sizes, line height and scale factor
/// its rows are laid out in, the anchor's and the status line's texts, the
/// revision's number, and whether a revision is shown and a page failed. A
/// frame whose inputs are the last decision's, as every wheel step's are,
/// takes that decision and shapes nothing: measured again, the header's
/// and the footer's rows laid out six lines in each frame at 13 pt in a
/// 1,480 × 980 window, where History's did none.
#[derive(Clone, PartialEq)]
struct RegionsInputs {
    width: Pixels,
    tall: Pixels,
    pinned: bool,
    line: crate::views::LineKey,
    line_height: DefiniteLength,
    scale: u32,
    anchor: String,
    status: String,
    revision: Option<usize>,
    shown: bool,
    failed: bool,
}

impl RegionsInputs {
    fn of(lineage: &Lineage, width: Pixels, tall: Pixels, pinned: bool, window: &Window) -> Self {
        Self {
            width,
            tall,
            pinned,
            line: crate::views::LineKey::current(window),
            line_height: crate::views::LineKey::line_height(window),
            scale: window.scale_factor().to_bits(),
            anchor: lineage.anchor_line(),
            status: lineage.status(),
            revision: lineage.selected.map(|index| lineage.offset + index + 1),
            shown: lineage.entry().is_some(),
            failed: lineage.error.is_some(),
        }
    }
}

/// The last message toolbar and `Regions` File History's panel decided, with
/// the inputs it decided them from (`RegionsInputs`).
struct Decided {
    inputs: RegionsInputs,
    toolbar: commit_message::Toolbar,
    regions: Regions,
}

/// File History's header's and footer's rows laid out in one room without
/// their gutters, measured once for a frame's inputs (`RegionsInputs`), in
/// the room of the toolbar the panel weighs its forms in
/// (`GitTurtle::message_toolbar`), so weighing
/// Copy message's and Hash's labels against their icons shapes nothing
/// more: the two take the same width, and only the panel's least heights
/// differ between them (`Rows::regions`).
struct Rows {
    /// The header's rows, with its padding and its border.
    header: Pixels,
    /// The footer's with Newer page and Older page labelled, where the
    /// wider label fits the room (`page_labels_fit`).
    labelled: Option<Pixels>,
    /// The footer's with their icons.
    icons: Pixels,
}

impl Rows {
    /// `lineage`'s rows laid out `room` wide, the footer's status line
    /// wrapped once for both of its forms.
    fn measure(lineage: &Lineage, room: Pixels, window: &Window) -> Self {
        let status = text_line(10., window) * wrapped(lineage.status(), 10., room, window);
        Self {
            header: file_history_header_height(lineage, &lineage.anchor_line(), room, window),
            labelled: page_labels_fit(room, window)
                .then(|| file_history_footer_height(status, true, room, window)),
            icons: file_history_footer_height(status, false, room, window),
        }
    }

    /// The regions these rows take in a panel `tall` whose parts keep
    /// `least` (`PanelLeast::shares`): Newer page and Older page labelled
    /// where the footer then holds them, else their icons where it holds
    /// those, and otherwise the footer scrolls, keeping their icons
    /// (DESIGN.md's responsive order), its share taken as theirs.
    fn regions(&self, least: &PanelLeast, tall: Pixels) -> Regions {
        let held = |footer: Pixels| {
            let shares = least.shares(tall, self.header, footer);
            (footer <= shares[1] + SETTLED).then_some(shares)
        };
        let (shares, footer_scrolls, page_labels) =
            if let Some(shares) = self.labelled.and_then(held) {
                (shares, false, true)
            } else {
                // Their icons' shares, worked out once whether the footer
                // holds them or scrolls them. Labelled in the room its
                // gutter left, Older page lay below the footer's fold at
                // 15 pt in a 424 × 600 window.
                let shares = least.shares(tall, self.header, self.icons);
                (shares, self.icons > shares[1] + SETTLED, false)
            };
        Regions {
            header: shares[0],
            footer: shares[1],
            body: (tall - shares[0] - shares[1]).max(least.body),
            header_scrolls: self.header > shares[0] + SETTLED,
            footer_scrolls,
            page_labels,
        }
    }
}

/// How far rows measured from their parts may run past the height they are
/// given and still fit it: layout sums the same lengths in another order.
const SETTLED: Pixels = px(0.01);

/// The interface font's line at `size` pt, as a text element lays one out,
/// on a device pixel (`image_compare::text_line`).
fn text_line(size: f32, window: &Window) -> Pixels {
    crate::image_compare::text_line(crate::appearance::ui_text(size), window)
}

/// The lines `text` at `size` pt wraps to in `room`.
fn wrapped(text: impl Into<SharedString>, size: f32, room: Pixels, window: &Window) -> f32 {
    crate::image_compare::wrapped_lines(text, crate::appearance::ui_text(size), room, window)
        .clamp(1, 64) as f32
}

/// The height of the rows a wrapping flex row lays `items` out in, `room`
/// wide and `gap` apart both ways: each item `width` wide and `height`
/// tall, or, as a text that fills the rest of its row, at least `width`,
/// its whole line, which layout takes as a text's least width, and as tall
/// as its text wraps to there.
fn wrapped_rows(
    items: &[(Pixels, Result<Pixels, &str>)],
    room: Pixels,
    gap: Pixels,
    window: &Window,
) -> Pixels {
    let mut rows: Vec<Pixels> = Vec::new();
    let mut used: Option<Pixels> = None;
    for &(width, height) in items {
        let beside = used.filter(|used| {
            crate::image_compare::on_device_pixel(*used + gap + width, window) <= room + SETTLED
        });
        let (start, left) = match beside {
            Some(used) => (used + gap, room - used - gap),
            None => (Pixels::ZERO, room),
        };
        let height = match height {
            Ok(height) => height,
            Err(text) => text_line(10., window) * wrapped(text, 10., left, window),
        };
        match (beside, rows.last_mut()) {
            (Some(_), Some(row)) => *row = (*row).max(height),
            _ => rows.push(height),
        }
        used = Some(start + width);
    }
    rows.iter()
        .copied()
        .fold(Pixels::ZERO, |sum, row| sum + row)
        + gap * rows.len().saturating_sub(1) as f32
}

/// The width the shared `button` helper gives a control labelled `label`
/// with no icon, as layout places it.
fn labelled_width(label: &str, window: &Window) -> Pixels {
    crate::placed_button_width(
        crate::views::line_widths(&[(label, crate::appearance::ui_text(12.))], window),
        false,
        window,
    )
}

/// File History's header laid out `room` wide, its `py_3` above and below
/// its rows and its bottom border included: the top row, a 28 px control
/// tall; the anchor's path, one line; the anchor's line `from` beside its
/// copy control where both fit, else on its own line or as it wraps with
/// the control beneath; the note, as it wraps; and Newer, Older and the
/// revision's number, as their row wraps.
fn file_history_header_height(
    lineage: &Lineage,
    from: &str,
    room: Pixels,
    window: &Window,
) -> Pixels {
    let snap = |length: Pixels| window.pixel_snap(length);
    let rem = window.rem_size();
    let (pad, gap) = (snap(rem * 0.75), snap(rem * 0.5));
    let square = snap(crate::appearance::ui_size(28.));
    let line = |size: f32| text_line(size, window);
    let from_line = crate::views::laid_out_text_width(
        crate::views::line_widths(&[(from, crate::appearance::ui_text(10.))], window),
        window,
    );
    let from_row = if from_line > room {
        line(10.) * wrapped(from, 10., room, window) + gap + square
    } else {
        wrapped_rows(
            &[(from_line, Ok(line(10.))), (square, Ok(square))],
            room,
            gap,
            window,
        )
    };
    let revision = lineage.selected.map_or(String::new(), |index| {
        format!("Revision {}", lineage.offset + index + 1)
    });
    // The number keeps its whole line beside Newer and Older or takes a row
    // of its own: layout takes a text's least width as its unwrapped line,
    // never its widest word. Measured by its widest word, the number stayed
    // beside them at 13 pt in a 424 × 1,000 window, where layout set it on
    // a row of its own, 18.5 px below what the header was given.
    let number = crate::views::laid_out_text_width(
        crate::views::line_widths(&[(&revision, crate::appearance::ui_text(10.))], window),
        window,
    );
    let revision_row = wrapped_rows(
        &[
            (labelled_width("Newer", window), Ok(square)),
            (labelled_width("Older", window), Ok(square)),
            (number, Err(revision.as_str())),
        ],
        room,
        gap,
        window,
    );
    pad + square.max(line(12.))
        + gap
        + line(12.)
        + gap
        + from_row
        + gap
        + line(10.) * wrapped(SharedString::new_static(FOLLOWS), 10., room, window)
        + gap
        + revision_row
        + pad
        + snap(px(1.))
}

/// File History's footer laid out `room` wide, its `p_3` above and below its
/// rows and its top border included: its status line, `status` tall as it
/// wraps there, and Newer page and Older page, labelled where `labels`, else
/// 28 px icons, on one row where they fit side by side.
fn file_history_footer_height(
    status: Pixels,
    labels: bool,
    room: Pixels,
    window: &Window,
) -> Pixels {
    let snap = |length: Pixels| window.pixel_snap(length);
    let rem = window.rem_size();
    let (pad, gap) = (snap(rem * 0.75), snap(rem * 0.5));
    let square = snap(crate::appearance::ui_size(28.));
    let width = |label: &str| {
        if labels {
            labelled_width(label, window)
        } else {
            square
        }
    };
    pad + status
        + gap
        + wrapped_rows(
            &[
                (width(NEWER_PAGE), Ok(square)),
                (width(OLDER_PAGE), Ok(square)),
            ],
            room,
            gap,
            window,
        )
        + pad
        + snap(px(1.))
}

/// Whether Newer page and Older page keep their labels in a footer whose
/// rows are laid out `room` wide: each labelled as the shared `button`
/// helper draws it, 10 px either side of its label on whole device pixels,
/// measured as `GitTurtle::file_history_top_row` measures Close, fits the
/// room alone; otherwise both keep only their 28 px icons, the shared
/// chevron turned up and down (DESIGN.md's responsive order). A label too
/// long for its room ended in an ellipsis, with no tooltip to finish it:
/// Older page at 18 pt in a 461 × 490 window.
fn page_labels_fit(room: Pixels, window: &Window) -> bool {
    use crate::views::{LineKey, cached_width, line_widths};
    let key = LineKey::current(window);
    let scale = window.scale_factor();
    let label = cached_width(&PAGE_LABEL_WIDTH, &key, || {
        [NEWER_PAGE, OLDER_PAGE]
            .map(|label| line_widths(&[(label, crate::appearance::ui_text(12.))], window))
            .into_iter()
            .fold(Pixels::ZERO, Pixels::max)
    });
    let labelled = crate::appearance::ui_size(20.) + label;
    px((f32::from(labelled) * scale).ceil() / scale) <= room
}

/// Whether a failed page's message, `error`, scrolls in its notice in a
/// body `body` tall whose message panel takes `message` (none where no
/// revision is shown; `commit_message::State::revision_panel_height`),
/// decided from this frame's inputs, never from whether it scrolled in its
/// last: its text wrapped in the notice's `room` without the cue's gutter,
/// the notice's `py_2` and `gap_2` and Retry page around it, as the
/// notice's column lays them out (CSS Flexbox): as tall as its rows, at most
/// 35% of the body, giving way to the message panel and the revisions'
/// least row down to its own least, the message taking what is left of it,
/// at most 100 px and at least its line. Decided from its last frame, a
/// 150-character message at 13 pt in a 540 px window between 554 and
/// 698 px tall, which wraps to 90 px without the gutter, kept the gutter
/// and scrolled at 100 px reached from 300 px tall and drew 90 px with no
/// cue reached from 1,000 px, Retry page 10 px lower in one than the other.
fn failure_error_scrolls(
    error: &str,
    least: &PanelLeast,
    body: Pixels,
    message: Pixels,
    room: Pixels,
    window: &Window,
) -> bool {
    let edges = window.rem_size() * 1.5;
    let retry = crate::appearance::ui_size(28.);
    let cap = px(100.);
    let text = text_line(11., window) * wrapped(error, 11., room, window);
    let notice = (edges + text.min(cap) + retry)
        .min(body * 0.35)
        .min(body - message - least.row)
        .max(least.failure);
    text > (notice - edges - retry).min(cap).max(least.error_line) + SETTLED
}

/// What scrolls each of File History's Tab stops into view, whole, as its
/// focus comes from the keyboard (DESIGN.md: whole focus). A panel shorter
/// than its parts' least heights scrolls as a whole (`PanelLeast`), and its
/// header, its failure and its footer each scroll their own rows as well,
/// so a control there is revealed in its own region first and then in the
/// panel, where that region has just scrolled it
/// (`FocusReveal::control_within`). At 18 pt in a 461 × 420 window Tab
/// otherwise moved onto the anchor path's copy control hidden in the
/// header's scroll, and, after a failed page, onto Older page below the
/// panel's fold. Kept with the inspection, so Back to an earlier one finds
/// its panel where it was left.
#[derive(Clone)]
struct Reveals {
    /// The whole panel, with its overflow cue (`tags::ScrollCue`).
    panel: FocusReveal,
    header: FocusReveal,
    failure: FocusReveal,
    footer: FocusReveal,
}

impl Default for Reveals {
    fn default() -> Self {
        let keyboard = || FocusReveal::new(Trigger::Keyboard);
        Self {
            panel: keyboard(),
            header: keyboard(),
            failure: keyboard(),
            footer: keyboard(),
        }
    }
}

impl Reveals {
    /// `button`, tracked as `id`, revealed whole with its ring's `room` in
    /// `region`'s own scroll and in the panel where `region` has scrolled
    /// it in the same frame.
    fn track(
        &self,
        region: &FocusReveal,
        id: &'static str,
        button: Button,
        room: Pixels,
        cx: &mut App,
    ) -> Button {
        let focus = region.focus(id, cx);
        let own = region.control(focus.clone(), room);
        let panel = self.panel.control_within(focus.clone(), room, region);
        gpui_kit::base::ElementExt::on_prepaint(
            button.track_focus(&focus).debug_selector(move || id.into()),
            move |bounds, window, cx| {
                own(vec![bounds], window, cx);
                panel(vec![bounds], window, cx);
            },
        )
    }
}

/// How the header's top row, the title and Close, fits the room inside the
/// header, in DESIGN.md's responsive order: Close becomes its icon, then
/// the title goes, so Close is never cut by the panel's edge. At 18 pt in a
/// 461 × 490 window "File history" and the labelled Close ran 79 px past
/// the window's edge, Close's ring and label cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopRow {
    /// The title and Close with its label.
    Labelled,
    /// The title and Close's icon, its label in its tooltip.
    Icon,
    /// Close's icon alone.
    Bare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    First,
    Last,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageRequest {
    generation: u64,
    offset: usize,
    position: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Revision(usize),
    Page(usize, Position),
}

struct Lineage {
    repository: PathBuf,
    anchor: String,
    path: PathBuf,
    page: Option<FileHistoryPage>,
    offset: usize,
    selected: Option<usize>,
    generation: u64,
    pending: Option<PageRequest>,
    last_request: Option<PageRequest>,
    /// The scroll of a failed page's message, its own with its cue inside
    /// the notice's; each failure's message starts at its top.
    error_scroll: ScrollHandle,
    error: Option<String>,
}

impl Lineage {
    fn new(repository: PathBuf, anchor: String, path: PathBuf) -> Self {
        Self {
            repository,
            anchor,
            path,
            page: None,
            offset: 0,
            selected: None,
            generation: 0,
            pending: None,
            last_request: None,
            error_scroll: ScrollHandle::new(),
            error: None,
        }
    }

    fn begin(&mut self, offset: usize, position: Position) -> PageRequest {
        self.generation = self.generation.wrapping_add(1);
        let request = PageRequest {
            generation: self.generation,
            offset,
            position,
        };
        self.pending = Some(request);
        self.last_request = Some(request);
        self.error = None;
        request
    }

    fn accept(&mut self, request: PageRequest, page: FileHistoryPage) -> bool {
        if self.pending != Some(request) || page.anchor != self.anchor || page.path != self.path {
            return false;
        }
        self.selected = match request.position {
            Position::First => (!page.entries.is_empty()).then_some(0),
            Position::Last => page.entries.len().checked_sub(1),
        };
        self.offset = request.offset;
        self.page = Some(page);
        self.pending = None;
        self.error = None;
        true
    }

    fn fail(&mut self, request: PageRequest, message: String) {
        if self.pending == Some(request) {
            self.pending = None;
            self.error = Some(message);
            self.error_scroll = ScrollHandle::new();
        }
    }

    fn entry(&self) -> Option<&FileHistoryEntry> {
        self.page.as_ref()?.entries.get(self.selected?)
    }

    /// The anchor's line in the header, "From ‹hash› · first-parent
    /// lineage".
    fn anchor_line(&self) -> String {
        format!("From {} · first-parent lineage", short_oid(&self.anchor))
    }

    /// The footer's status line: the page being read, none loaded, or the
    /// revisions loaded and whether more follow.
    fn status(&self) -> String {
        let count = self.page.as_ref().map_or(0, |page| page.entries.len());
        if self.pending.is_some() {
            "Reading the requested page…".to_owned()
        } else if count == 0 {
            "No revisions loaded".to_owned()
        } else {
            let more = self.page.as_ref().and_then(|page| page.next_offset);
            format!(
                "Revisions {}–{}{}",
                self.offset + 1,
                self.offset + count,
                if more.is_some() {
                    " · more available"
                } else {
                    " · end of lineage"
                }
            )
        }
    }

    fn destination(&self, direction: i32, edge: bool) -> Option<Destination> {
        if self.pending.is_some() {
            return None;
        }
        let page = self.page.as_ref()?;
        if page.entries.is_empty() {
            return None;
        }
        let current = self.selected.unwrap_or(0);
        if edge {
            return Some(Destination::Revision(if direction < 0 {
                0
            } else {
                page.entries.len() - 1
            }));
        }
        if direction < 0 {
            if current > 0 {
                Some(Destination::Revision(current - 1))
            } else {
                (self.offset > 0).then(|| {
                    Destination::Page(self.offset.saturating_sub(PAGE_SIZE), Position::Last)
                })
            }
        } else if current + 1 < page.entries.len() {
            Some(Destination::Revision(current + 1))
        } else {
            page.next_offset
                .map(|offset| Destination::Page(offset, Position::First))
        }
    }
}

/// Holds the existing editors and scroll handles, so closing revision history
/// restores selection and viewport without rebuilding the previous preview.
pub(super) struct ReturnContext {
    inspector_message: commit_message::State,
    files: Vec<FileChange>,
    file_filter: String,
    selected_file: Option<usize>,
    preferred_file: Option<PathBuf>,
    content: Option<Arc<Content>>,
    conflict_view: Option<Entity<conflicts::ConflictView>>,
    conflict_subscription: Option<Subscription>,
    patch_editor: Option<Entity<EditorState>>,
    patch_decoration: Option<editor_find::PatchDecorations>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    partial_subscription: Option<Subscription>,
    split_view: Option<Entity<split_diff::SplitView>>,
    markdown_view: Option<Entity<markdown_view::View>>,
    before_editor: Option<Entity<EditorState>>,
    after_editor: Option<Entity<EditorState>>,
    images: [Option<Arc<RenderImage>>; 2],
    image_scroll: ScrollHandle,
    image_comparison: image_compare::State,
    blame_visible: bool,
    zoom: f32,
    text_mode: TextMode,
    review: crate::text_review::State,
    mode: WorkspaceMode,
    pane: Pane,
    sidebar: bool,
    history_sidebar: bool,
    focus: Option<FocusHandle>,
    status: String,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    lineage: Option<Lineage>,
    retained: Option<Box<ReturnContext>>,
    task: Option<Task<()>>,
    scroll: UniformListScrollHandle,
    reveals: Reveals,
    /// The header's and the footer's regions, decided with the message
    /// toolbar from the height the window lays the panel out in this frame,
    /// the inspector's whole height (`GitTurtle::message_toolbar`); `None`
    /// where no window lays the inspector out.
    regions: std::cell::Cell<Option<Regions>>,
    /// The last toolbar and regions decided, which a frame with the same
    /// inputs takes rather than measuring the rows again (`RegionsInputs`).
    decided: std::cell::RefCell<Option<Decided>>,
    previous: Option<Box<State>>,
}

impl State {
    pub(super) fn rescale_lists(&self, scales: settings::ListScales) {
        settings::rescale_list_scroll(&self.scroll, scales.lineage);
        if let Some(previous) = &self.previous {
            previous.rescale_lists(scales);
        }
    }
    pub(super) fn pause_for_tab(&mut self) {
        self.task = None;
        if let Some(lineage) = &mut self.lineage {
            lineage.generation = lineage.generation.wrapping_add(1);
            if lineage.pending.take().is_some() {
                lineage.error =
                    Some("History read paused on tab switch; use Retry to continue.".into());
            }
        }
        if let Some(retained) = &mut self.retained {
            retained.pause_for_tab();
        }
        if let Some(previous) = &mut self.previous {
            previous.pause_for_tab();
        }
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.retained
            .as_ref()
            .map_or(0, |context| context.retained_bytes())
            + self
                .previous
                .as_ref()
                .map_or(0, |previous| previous.retained_bytes())
            + self
                .lineage
                .as_ref()
                .and_then(|lineage| lineage.page.as_ref())
                .map_or(0, |page| {
                    page.entries
                        .iter()
                        .map(|entry| {
                            entry.commit.history_bytes()
                                + repository_tabs::file_bytes(&entry.change)
                        })
                        .sum()
                })
    }
    fn depth(&self) -> usize {
        usize::from(self.is_active())
            + self
                .previous
                .as_ref()
                .map_or(0, |previous| previous.depth())
    }

    pub(super) fn is_active(&self) -> bool {
        self.retained.is_some()
    }

    pub(super) fn rescale_code(&self, ratio: f32, cx: &mut App) {
        if let Some(previous) = &self.previous {
            previous.rescale_code(ratio, cx);
        }
        if let Some(retained) = &self.retained {
            retained.rescale_code(ratio, cx);
        }
    }

    pub(super) fn refresh_theme(&self, cx: &mut App) {
        if let Some(previous) = &self.previous {
            previous.refresh_theme(cx);
        }
        if let Some(retained) = &self.retained {
            retained.refresh_theme(cx);
        }
    }
}

impl ReturnContext {
    pub(super) fn pause_for_tab(&mut self) {
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
        self.image_comparison = self.image_comparison.clone();
    }

    pub(super) fn retained_bytes(&self) -> usize {
        // The preview accounts captured data, decoded pixels and presentations.
        // Reserve extra copies for native editors and uploaded image surfaces.
        self.content
            .as_ref()
            .map_or(0, |content| content.bytes().saturating_mul(4))
            + self
                .files
                .iter()
                .map(repository_tabs::file_bytes)
                .sum::<usize>()
            + self.file_filter.capacity()
            + self.inspector_message.retained_bytes()
    }
    pub(super) fn rescale_code(&self, ratio: f32, cx: &mut App) {
        for editor in [&self.patch_editor, &self.before_editor, &self.after_editor]
            .into_iter()
            .flatten()
        {
            text::rescale_editor(editor, ratio, cx);
        }
        if let Some(view) = &self.split_view {
            view.update(cx, |view, cx| view.rescale_code(ratio, cx));
        }
        if let Some(view) = &self.conflict_view {
            view.update(cx, |view, cx| view.rescale_code(ratio, cx));
        }
    }
    pub(super) fn refresh_theme(&self, cx: &mut App) {
        if let (Some(collection), Some(Content::Text { presentation, .. })) =
            (&self.patch_decoration, self.content.as_deref())
        {
            text::refresh_theme(collection, presentation, cx);
        }
        if let Some(split) = &self.split_view {
            split.update(cx, |view, cx| view.refresh_theme(cx));
        }
    }
}

impl GitTurtle {
    pub(super) fn file_history_entry(&self) -> Option<&FileHistoryEntry> {
        self.file_history.lineage.as_ref()?.entry()
    }
    pub(super) fn file_history_preview_origins(&self) -> Option<markdown_view::Origins> {
        let entry = self.file_history.lineage.as_ref()?.entry()?;
        Some(markdown_view::Origins::revisions(
            entry.commit.parents.first().cloned(),
            Some(entry.commit.oid.clone()),
        ))
    }
    pub(super) fn take_inspection_context(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ReturnContext {
        ReturnContext {
            inspector_message: self.inspector_message.take(cx),
            files: std::mem::take(&mut self.files),
            file_filter: self.file_filter.read(cx).value().to_string(),
            selected_file: self.selected_file.take(),
            preferred_file: self.preferred_file.take(),
            content: self.content.take(),
            conflict_view: self.conflict_view.take(),
            conflict_subscription: self.conflict_subscription.take(),
            patch_editor: self.patch_editor.take(),
            patch_decoration: self.patch_decoration.take(),
            patch_view: self.patch_view.take(),
            partial_subscription: self.partial_subscription.take(),
            split_view: self.split_view.take(),
            markdown_view: self.markdown_view.take(),
            before_editor: self.before_editor.take(),
            after_editor: self.after_editor.take(),
            images: std::mem::take(&mut self.images),
            image_scroll: std::mem::take(&mut self.image_scroll),
            image_comparison: self.image_comparison.clone(),
            blame_visible: self.blame.is_visible(),
            zoom: self.zoom,
            text_mode: self.text_mode,
            review: std::mem::take(&mut self.review),
            mode: self.mode,
            pane: self.pane,
            sidebar: self.sidebar,
            history_sidebar: self.history_sidebar,
            focus: self.inspection_opener(window, cx),
            status: self.status.clone(),
            error: self.error.take(),
        }
    }

    /// The inspector's message toolbar for an inspector `width` wide and
    /// `tall` (`commit_message::Toolbar::within`), which the window lays out
    /// before it draws one, and File History's header's and footer's
    /// regions with it (`Regions`). File History's panel decides its form
    /// from the room it has, never from whether it scrolled in its last
    /// frame (DESIGN.md's responsive order: actions become icons before the
    /// content gives way): at the inspector's whole width, Copy message and
    /// Hash keep their labels where the panel holds its parts with them and
    /// giving them up would not let the header or the footer hold more of
    /// its rows, else take their icons where the panel holds its parts with
    /// those; otherwise the toolbar is laid out in the room the panel's
    /// overflow cue's gutter leaves while it scrolls (`tags::ScrollCue`), so
    /// it never runs under the gutter, Copy message and Hash keeping their
    /// icons there where their labels take more rows. The header's and the
    /// footer's rows are measured once, in the room of the toolbar chosen
    /// (`Rows`), and both forms are weighed from them. Decided from the
    /// gutter its last frame kept, the panel considered icons only where it
    /// had not scrolled: at 18 pt in a 511 × 490 window the window drew
    /// every frame, the controls changing form in each, and a panel that
    /// had scrolled, at 420 px tall or with a failed page, kept Copy
    /// message's and Hash's labels on two rows and its scrollbar at about
    /// 530 to 560 px wide, where a fresh open drew their icons and none.
    /// Weighed against the panel alone, their labels' second row left the
    /// footer only its status line where their icons gave it Newer page and
    /// Older page.
    pub(super) fn message_toolbar(
        &self,
        width: Pixels,
        tall: Pixels,
        pinned: bool,
        window: &Window,
    ) -> commit_message::Toolbar {
        let whole = commit_message::Toolbar::within(width, tall, pinned, window);
        self.file_history.regions.set(None);
        let Some(lineage) = self
            .file_history
            .lineage
            .as_ref()
            .filter(|_| self.file_history.is_active())
        else {
            return whole;
        };
        // Decided once for its inputs (`RegionsInputs`): a wheel step over
        // the revisions takes the last decision and lays out no line.
        let inputs = RegionsInputs::of(lineage, width, tall, pinned, window);
        if let Some(decided) = self
            .file_history
            .decided
            .borrow()
            .as_ref()
            .filter(|decided| decided.inputs == inputs)
        {
            self.file_history.regions.set(Some(decided.regions));
            return decided.toolbar;
        }
        // The panel takes the inspector's whole height, which the window
        // lays out in this frame: read from the panel's last frame, a
        // one-step resize from 511 × 490 to 511 × 520 at 18 pt kept the
        // icons the shorter window took where the labels fit, and nothing
        // drew another frame.
        let failed = lineage.error.is_some();
        let least = |toolbar: &commit_message::Toolbar| {
            let message = lineage.entry().map(|_| toolbar.revision_message_least());
            PanelLeast::of(message, failed, window)
        };
        let icons = lineage.entry().and_then(|_| whole.with_icon_copies());
        let labelled_least = least(&whole);
        let (toolbar, regions) = if labelled_least.total() <= tall {
            let rows = Rows::measure(lineage, whole.room(), window);
            let labelled = rows.regions(&labelled_least, tall);
            icons
                .map(|icons| (icons, rows.regions(&least(&icons), tall)))
                .filter(|(_, regions)| regions.hold_more_than(&labelled))
                .unwrap_or((whole, labelled))
        } else if let Some((icons, icons_least)) = icons
            .map(|icons| (icons, least(&icons)))
            .filter(|(_, least)| least.total() <= tall)
        {
            let rows = Rows::measure(lineage, icons.room(), window);
            (icons, rows.regions(&icons_least, tall))
        } else {
            // The scrolling panel holds the more of its parts the shorter
            // its toolbar.
            let gutter = commit_message::Toolbar::within(
                width - crate::tags::ScrollCue::gutter(),
                tall,
                pinned,
                window,
            );
            let gutter = lineage
                .entry()
                .and_then(|_| gutter.with_icon_copies())
                .unwrap_or(gutter);
            let rows = Rows::measure(lineage, gutter.room(), window);
            (gutter, rows.regions(&least(&gutter), tall))
        };
        self.file_history.regions.set(Some(regions));
        self.file_history.decided.replace(Some(Decided {
            inputs,
            toolbar,
            regions,
        }));
        toolbar
    }

    /// The width the header's rows are laid out in: the room the message
    /// toolbar has (`commit_message::State::details_room`), inside the
    /// header's `px_3` as inside the toolbar's, less `gutter`, what the
    /// header's own scroll cue keeps at its right in place of its trailing
    /// `px_3` while it scrolls (`tags::ScrollCue::list`): its scrollbar's
    /// gutter and the ring's room before it, none where it fits. `None`
    /// where no window lays the inspector out.
    fn file_history_header_room(&self, gutter: Pixels, window: &Window) -> Option<Pixels> {
        let room = self.inspector_message.details_room()?;
        Some(if gutter > Pixels::ZERO {
            room + window.rem_size() * 0.75 - gutter
        } else {
            room
        })
    }

    /// The header's top row for the room the inspector gives it, measured
    /// from the interface font at the interface size (`TopRow`): inside the
    /// panel's left border and the header's `px_3`, the room the message
    /// toolbar has (`commit_message::State::details_room`), the title, the
    /// row's half-rem gap and Close, as the shared `button` helper draws it
    /// (10 px either side of its 16 px icon, 6 px gap and label, or a 28 px
    /// square), each on whole device pixels. Close's focus ring lies in the
    /// gap and the header's padding, inside the header's clip, or, while the
    /// header scrolls its own rows, its ring's room before the cue's gutter
    /// (`file_history_header_room`). Labelled where no window lays the
    /// inspector out.
    fn file_history_top_row(&self, gutter: Pixels, window: &Window) -> TopRow {
        use crate::views::{LineKey, cached_width, line_widths, shaped_width};
        let Some(room) = self.file_history_header_room(gutter, window) else {
            return TopRow::Labelled;
        };
        let key = LineKey::current(window);
        let scale = window.scale_factor();
        let drawn = |width: Pixels| px((f32::from(width) * scale).ceil() / scale);
        let title = drawn(cached_width(&TITLE_WIDTH, &key, || {
            shaped_width(
                TITLE,
                crate::appearance::ui_text(12.),
                FontWeight::SEMIBOLD,
                window,
            )
        }));
        let label = cached_width(&CLOSE_LABEL_WIDTH, &key, || {
            line_widths(&[(CLOSE_LABEL, crate::appearance::ui_text(12.))], window)
        });
        let gap = window.rem_size() * 0.5;
        let labelled = drawn(crate::appearance::ui_size(20. + 16. + 6.) + label);
        if title + gap + labelled <= room {
            TopRow::Labelled
        } else if title + gap + drawn(crate::appearance::ui_size(28.)) <= room {
            TopRow::Icon
        } else {
            TopRow::Bare
        }
    }

    /// Whether Newer page and Older page keep their labels in the footer,
    /// which lays its rows out in the room the header's are
    /// (`file_history_header_room`, `gutter` its own cue's;
    /// `page_labels_fit`), where no regions were decided for the frame
    /// (`Regions`). Labelled where no window lays the inspector out.
    fn file_history_page_labels(&self, gutter: Pixels, window: &Window) -> bool {
        self.file_history_header_room(gutter, window)
            .is_none_or(|room| page_labels_fit(room, window))
    }

    /// The control an inspection opened now gives focus back to as it
    /// closes (`ReturnContext::focus`): the focused control, or, where a
    /// row's menu holds focus as its item opens the inspection, the list the
    /// menu returns focus to (File history… from a changed file's
    /// right-click or keyboard menu). Such a menu is drawn inside the
    /// window's tree, and its handle, kept, went with the menu: closing File
    /// history focused it once the menu was gone, where AT-SPI read only the
    /// window's frame focused and no key, Escape and the command palette's
    /// shortcut among them, reached anything until a click. A popup outside
    /// the window's tree gives back the shown pane's list. While Settings or
    /// Projects shows, as a tab is left from there (`retain_active_tab`), it
    /// is the control Back would return focus to, so the tab shown again
    /// gives focus where Back gives it: the other page's control, which the
    /// tab does not draw, gave way to the shown mode's list.
    pub(super) fn inspection_opener(&self, window: &Window, cx: &App) -> Option<FocusHandle> {
        if self.page != AppPage::Repository
            && let Some(back) = &self.page_return_focus
        {
            return Some(back.clone());
        }
        if let Some(list) = [&self.file_menu, &self.commit_menu, &self.nav_menu]
            .into_iter()
            .flatten()
            .find_map(|menu| menu.returns_focus_to(window, cx))
        {
            return Some(list.clone());
        }
        if let Some(previous) = crate::native_accessibility::pointer_menu_return(window, cx) {
            return previous;
        }
        if self.app_focus.contains_focused(window, cx) {
            window.focused(cx)
        } else {
            Some(if self.pane == Pane::Files {
                self.files_focus().clone()
            } else {
                self.focus.clone()
            })
        }
    }

    pub(super) fn restore_inspection_context(
        &mut self,
        retained: ReturnContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_filter.update(cx, |input, cx| {
            input.set_value(retained.file_filter.clone(), window, cx)
        });
        self.inspector_message
            .replace(retained.inspector_message, cx);
        self.files = retained.files;
        self.refresh_file_filter(window, cx);
        self.selected_file = retained.selected_file;
        self.preferred_file = retained.preferred_file;
        // A restored preview is a new showing of it.
        self.preview_showing = crate::system_preview::Showing::next();
        self.content = retained.content;
        self.conflict_view = retained.conflict_view;
        self.conflict_subscription = retained.conflict_subscription;
        self.patch_editor = retained.patch_editor;
        self.patch_decoration = retained.patch_decoration;
        self.patch_view = retained.patch_view;
        self.partial_subscription = retained.partial_subscription;
        self.split_view = retained.split_view;
        self.markdown_view = retained.markdown_view;
        self.before_editor = retained.before_editor;
        self.after_editor = retained.after_editor;
        self.images = retained.images;
        self.image_scroll = retained.image_scroll;
        self.image_comparison = retained.image_comparison;
        self.blame.set_visible(retained.blame_visible);
        self.zoom = retained.zoom;
        self.text_mode = retained.text_mode;
        self.review = retained.review;
        self.mode = retained.mode;
        self.pane = retained.pane;
        self.sidebar = retained.sidebar;
        self.history_sidebar = retained.history_sidebar;
        self.status = retained.status;
        self.error = retained.error;
        // The window's root, which a click on an empty spot focuses, counts
        // as nothing to give back (`saved_focus`): on the repository page
        // focus goes to the shown mode's own list, which the window names as
        // its pane. Given back, the root left File history's close on
        // nothing a person could see, where Down moved nothing.
        let focus = self.saved_focus(retained.focus);
        if self.page != AppPage::Repository {
            // Restored behind Settings or Projects, as a write Settings
            // saves closes File history, focus stays on that page, and Back
            // returns it where the restore would have, or to the shown
            // mode's own list where it had nothing to return to
            // (`resume_repository_page`). Given at once, focus rested on a
            // control behind the page, which drew it nowhere and took the
            // next key.
            self.page_return_focus = focus;
        } else if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            // Behind a dialog or sheet, focus stays in it, and its close
            // lands where the restore would have put it (`return_modal_to`).
            self.return_modal_to(focus);
        } else {
            let focus = focus.unwrap_or_else(|| self.take_mode_list());
            self.give_inspection_focus_back(focus, window, cx);
        }
    }

    /// Focuses `focus` as an inspection closes on the repository page, and,
    /// once a frame has drawn the restored page, the shown mode's own list
    /// (History's commit list, else the changed-file list, named as the
    /// window's pane) where `focus` still holds focus but that frame did not
    /// draw it, or where nothing holds focus: the brief's "if gone, History's
    /// selected row". A control the inspection was opened from can be gone
    /// as it closes: a dropdown's menu drawn inside the window's tree, as
    /// the narrow Compare's File actions, which held focus as its File
    /// history… or Blame ran, or the wide Compare's File history button,
    /// which no frame draws while File history is open, so its handle is
    /// dropped. Focus stayed on that handle, where no key reached anything
    /// until a click. Focus the person moves meanwhile, or a dialog or sheet
    /// opened meanwhile, is left where it is.
    pub(super) fn give_inspection_focus_back(
        &mut self,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&focus, cx);
        let (owner, path) = (cx.entity().downgrade(), self.path.clone());
        let given = focus.downgrade();
        // The callbacks run as a frame starts: the second runs once the
        // first frame has drawn the restored page.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.page != AppPage::Repository
                        || this.path != path
                        || window.has_active_dialog(cx)
                        || window.has_active_sheet(cx)
                    {
                        return;
                    }
                    let stranded = match window.focused(cx) {
                        None => true,
                        Some(now) => {
                            given.upgrade().as_ref() == Some(&now)
                                && !this.app_focus.contains(&now, window)
                        }
                    };
                    if stranded {
                        let list = this.take_mode_list();
                        window.focus(&list, cx);
                        cx.notify();
                    }
                });
            });
        });
    }

    /// A page may finish while Settings or Projects is visible. Accept its
    /// metadata then, but construct/focus the selected preview only on return.
    pub(super) fn resume_file_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lineage) = &self.file_history.lineage else {
            return;
        };
        if self.page != AppPage::Repository || self.blame.is_visible() || lineage.pending.is_some()
        {
            return;
        }
        if let (Some(index), Some(entry)) = (lineage.selected, lineage.entry())
            && (self.files.first() != Some(&entry.change) || self.content.is_none())
        {
            self.select_file_revision(index, window, cx);
        }
    }

    pub(super) fn open_file_history(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository
            || self.mode == WorkspaceMode::Working
            || self.file_history.is_active()
            || self.operation_busy.is_some()
        {
            return;
        }
        if self.revision_inspection.is_active() {
            if let Some((anchor, path)) = self.inspection_history_target(index) {
                self.open_file_history_at(anchor, path, window, cx);
            } else {
                self.error = Some(if self.is_quick_source() {
                    "The current HEAD is unavailable. Refresh Working Changes before opening this tracked file's history."
                } else {
                    "This file is absent on the selected comparison side. Choose its existing side to inspect history."
                }.into());
                cx.notify();
            }
            return;
        }
        let (Some(commit), Some(file)) = (
            self.selected_commit
                .and_then(|index| self.commits.get(index)),
            self.files.get(index),
        ) else {
            return;
        };
        let anchor = commit.oid.clone();
        let path = file.path().to_owned();
        self.open_file_history_at(anchor, path, window, cx);
    }

    pub(super) fn file_history_blame_target(&self) -> Option<gitturtle_core::BlameTarget> {
        let entry = self.file_history.lineage.as_ref()?.entry()?;
        Some(gitturtle_core::BlameTarget::Committed {
            oid: entry.commit.oid.clone(),
            path: entry.change.path().to_owned(),
        })
    }

    pub(super) fn open_file_history_at(
        &mut self,
        anchor: String,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository || self.operation_busy.is_some() {
            return;
        }
        if self.file_history.depth() >= 4 {
            self.error = Some(
                "Four file-history comparisons are already open. Use Back before opening another."
                    .into(),
            );
            cx.notify();
            return;
        }
        let Some(repo) = &self.repository else {
            return;
        };
        let lineage = Lineage::new(repo.path().to_owned(), anchor, path);
        self.invalidate_read();
        let retained = self.take_inspection_context(window, cx);
        self.blame.hide();
        self.clear_preview();
        let previous = self
            .file_history
            .is_active()
            .then(|| Box::new(std::mem::take(&mut self.file_history)));
        self.file_history = State {
            lineage: Some(lineage),
            previous,
            retained: Some(Box::new(retained)),
            ..Default::default()
        };
        self.mode = WorkspaceMode::Compare;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.load_file_history_page(0, Position::First, window, cx);
    }

    /// An explicit target change exits the entire transient navigation stack.
    /// Back itself still pops one view, retaining the prior selection/viewport.
    pub(super) fn close_inspections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        loop {
            if self.close_blame(window, cx) {
                continue;
            }
            if self.close_file_history(window, cx) {
                continue;
            }
            if self.close_revision_inspection(window, cx) {
                continue;
            }
            break;
        }
        self.blame = blame::State::default();
    }

    /// Returns true if Back handled a transient file-history context.
    pub(super) fn close_file_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(retained) = self.file_history.retained.take() else {
            return false;
        };
        self.invalidate_read();
        self.file_history = self
            .file_history
            .previous
            .take()
            .map(|previous| *previous)
            .unwrap_or_default();
        self.clear_preview();
        let retained = *retained;
        self.restore_inspection_context(retained, window, cx);
        if self.mode == WorkspaceMode::Compare
            && !self.blame.is_visible()
            && self.content.is_none()
            && self.error.is_none()
        {
            self.read_selected_preview(window, cx);
        }
        // Page transitions and explicit writes may close this pane too. Wait
        // until their current handler finishes before resuming queued changes.
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| this.try_automatic_refresh(window, cx));
        });
        cx.notify();
        true
    }

    fn load_file_history_page(
        &mut self,
        offset: usize,
        position: Position,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let Some(lineage) = self.file_history.lineage.as_ref() else {
            return;
        };
        if repo.path() != lineage.repository || self.page != AppPage::Repository {
            return;
        }
        self.invalidate_read();
        let lineage = self.file_history.lineage.as_mut().unwrap();
        let request = lineage.begin(offset, position);
        let repository = lineage.repository.clone();
        let anchor = lineage.anchor.clone();
        let path = lineage.path.clone();
        self.forget_failed_page(window, cx);
        let generation = self.generation;
        let response = self.worker.submit(Job::FileHistory {
            repo,
            anchor: anchor.clone(),
            path: path.clone(),
            offset,
            limit: PAGE_SIZE,
        });
        self.file_history.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!("File history was interrupted. Retry this page."))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                let Some(lineage) = this.file_history.lineage.as_mut() else {
                    return;
                };
                if this.path.as_ref() != Some(&repository)
                    || lineage.repository != repository
                    || lineage.anchor != anchor
                    || lineage.path != path
                    || lineage.pending != Some(request)
                {
                    return;
                }
                this.file_history.task = None;
                let selected = match result {
                    Ok(Output::FileHistory(page)) => {
                        if lineage.accept(request, page) {
                            lineage.selected
                        } else {
                            lineage.fail(request, "The returned file history did not match this revision. Retry this page.".into());
                            None
                        }
                    }
                    Ok(_) => {
                        lineage.fail(request, "The file-history read returned a different result. Retry this page.".into());
                        None
                    }
                    Err(error) => {
                        lineage.fail(request, format!("{error:#}"));
                        None
                    }
                };
                if let Some(selected) = selected && this.page == AppPage::Repository {
                    this.select_file_revision(selected, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Once a failed page's notice is no longer drawn, as its page is read
    /// again or lands or a revision is chosen, its reveal lets go of Retry
    /// page's focus handle and of where the notice drew it, so neither
    /// outlives the notice; a reveal kept for its next failure kept both
    /// for as long as File History stayed open. Focus on Retry page moves
    /// to the revisions first, as focus on an inspection's opener no frame
    /// draws falls back to the mode's list (`give_inspection_focus_back`):
    /// pressed, Retry page went with its notice as the page was read again,
    /// and focus stayed on its handle, where no key reached anything until
    /// the page landed, or, where it landed empty, until a click.
    fn forget_failed_page(&mut self, window: &mut Window, cx: &mut App) {
        if self.file_history.reveals.failure.holds_focus(window, cx) {
            window.focus(&self.file_focus, cx);
        }
        self.file_history.reveals.failure = FocusReveal::new(Trigger::Keyboard);
    }

    fn select_file_revision(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_blame(window, cx);
        let Some(lineage) = self.file_history.lineage.as_mut() else {
            return;
        };
        if lineage.pending.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(entry) = lineage
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
        else {
            return;
        };
        self.inspector_message.select(&entry.commit.oid, cx);
        let change = entry.change.clone();
        lineage.selected = Some(index);
        lineage.error = None;
        self.forget_failed_page(window, cx);
        self.file_history
            .scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.files = vec![change];
        self.refresh_file_filter(window, cx);
        self.selected_file = None;
        self.pane = Pane::Files;
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            // Under a dialog or sheet, as one opened while the first page
            // was read, focus stays in it and the revision list becomes
            // where its close lands (`return_modal_to`): focused at once,
            // the list took focus from behind the modal, drawn nowhere a
            // person could see, and the next key went to it.
            self.return_modal_to(Some(self.file_focus.clone()));
        } else {
            window.focus(&self.file_focus, cx);
        }
        self.load_file(0, window, cx);
    }

    pub(super) fn move_file_history_revision(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.file_history.is_active() {
            return false;
        }
        let destination = self
            .file_history
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.destination(direction, edge));
        match destination {
            Some(Destination::Revision(index)) => self.select_file_revision(index, window, cx),
            Some(Destination::Page(offset, position)) => {
                self.load_file_history_page(offset, position, window, cx)
            }
            None => {}
        }
        true
    }

    pub(super) fn render_file_history(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(lineage) = self.file_history.lineage.as_ref() else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let busy = lineage.pending.is_some();
        let newer = lineage.destination(-1, false).is_some();
        let older = lineage.destination(1, false).is_some();
        let count = lineage.page.as_ref().map_or(0, |page| page.entries.len());
        let next_page = lineage.page.as_ref().and_then(|page| page.next_offset);
        let previous_page = lineage.offset.checked_sub(PAGE_SIZE);
        let retry = lineage.last_request;
        let entry = lineage.entry();
        let copy_path = lineage.path.display().to_string();
        let path = copy_path.clone();
        let path_mark = copy_feedback::Mark::new("copy-history-path", &lineage.path);
        let path_copy = path_mark.clone();
        let message = entry.map(|_| {
            self.inspector_message
                .revision_message_least()
                .unwrap_or_default()
        });
        let least = PanelLeast::of(message, lineage.error.is_some(), window);
        let reveals = &self.file_history.reveals;
        let room = crate::appearance::button_ring_room(cx);
        // Each region reveals the control focus moves onto in its own rows;
        // the panel's reveal runs the region's again first where focus lies
        // there, so both show it whole in the frame that first draws it.
        for region in [&reveals.header, &reveals.failure, &reveals.footer] {
            region.reveal(window, cx);
        }
        reveals.panel.reveal(window, cx);
        // While it scrolls, the panel keeps its cue's gutter at its right,
        // which its toolbar and message are laid out without
        // (`GitTurtle::message_toolbar`).
        let cue = crate::tags::ScrollCue::revealing(&reveals.panel, Pixels::ZERO);
        // The header and the footer cue their own rows' scroll on the
        // panel's surface, a scrollbar along a track a thumb moves on and
        // otherwise a fade at each edge their rows continue past, their rows
        // keeping the ring's room before the gutter, as Blame's footer does.
        // Whether each scrolls, and so keeps the gutter, is decided with the
        // message toolbar from the room their rows have without it and the
        // panel's height in this frame, never from whether they scrolled in
        // their last (`Regions`, `GitTurtle::message_toolbar`).
        let from = lineage.anchor_line();
        let status = lineage.status();
        let regions = self.file_history.regions.get();
        // The message and details as measured, which the message panel's
        // height in this frame, and so a failed page's notice's, follows.
        let extent =
            entry.and_then(|entry| self.message_extent(&entry.commit, Some(entry), window, cx));
        let error_scrolls = regions
            .zip(self.file_history_header_room(Pixels::ZERO, window))
            .zip(lineage.error.as_deref())
            .and_then(|((regions, room), error)| {
                let message = match entry {
                    Some(_) => self
                        .inspector_message
                        .revision_panel_height(extent, regions.body)?,
                    None => Pixels::ZERO,
                };
                Some(failure_error_scrolls(
                    error,
                    &least,
                    regions.body,
                    message,
                    room,
                    window,
                ))
            });
        let decide = |cue: crate::tags::ScrollCue, scrolls: Option<bool>| match scrolls {
            Some(scrolls) => cue.decided(scrolls),
            None => cue,
        };
        let header_cue = decide(
            crate::tags::ScrollCue::revealing(&reveals.header, room)
                .on(crate::tags::Surface::Panel),
            regions.map(|regions| regions.header_scrolls),
        );
        let footer_cue = decide(
            crate::tags::ScrollCue::revealing(&reveals.footer, room)
                .on(crate::tags::Surface::Panel),
            regions.map(|regions| regions.footer_scrolls),
        );
        let header_gutter = if header_cue.kept() {
            crate::tags::ScrollCue::gutter() + room
        } else {
            Pixels::ZERO
        };
        let footer_gutter = if footer_cue.kept() {
            crate::tags::ScrollCue::gutter() + room
        } else {
            Pixels::ZERO
        };
        // The revisions cue their own scroll on the panel too, the track kept
        // the ring's room inside the list's edge, where its ring lies.
        let revisions_cue =
            crate::tags::ScrollCue::new(&self.file_history.scroll.0.borrow().base_handle, room)
                .on(crate::tags::Surface::Panel);
        let top_row = self.file_history_top_row(header_gutter, window);
        // Newer page and Older page, labelled where the wider label fits the
        // footer's room and the footer holds them, else each the shared
        // chevron turned up or down (`Regions`, `file_history_page_labels`),
        // named and tooltipped by its label.
        let page_labels = regions.map_or_else(
            || self.file_history_page_labels(footer_gutter, window),
            |regions| regions.page_labels,
        );
        let page_button = |id: &'static str, label: &'static str, turn: f32| {
            if page_labels {
                button(id, label, "", false).flex_shrink_0()
            } else {
                crate::views::compact_control_name(
                    button(id, "", "chevron", false)
                        .icon(
                            gpui_kit::component::Icon::default()
                                .path("icons/chevron.svg")
                                .size(crate::appearance::ui_size(16.))
                                .rotate(gpui_kit::percentage(turn)),
                        )
                        .tooltip(label)
                        .flex_shrink_0(),
                    id,
                    label,
                )
            }
        };
        // The anchor's line keeps its whole measured line beside its copy
        // control, or the header's whole row where that is narrower, and
        // below that the control moves beneath it, on its left edge: at 18 pt
        // in a 461 px window the line, given no least, was 61 to 80 px wide
        // beside the control and wrapped to about 135 px.
        let from_least = self
            .file_history_header_room(header_gutter, window)
            .map(|room| {
                let line = crate::views::laid_out_text_width(
                    crate::views::cached_text_width(&FROM_WIDTH, &from, window, || {
                        crate::views::line_widths(
                            &[(&from, crate::appearance::ui_text(10.))],
                            window,
                        )
                    }),
                    window,
                );
                (line <= room).then_some(line)
            });
        cue.list(div()
            .on_children_prepainted(cue.observe(reveals.panel.observe()))
            // A panel shorter than its parts' least heights scrolls.
            .id("file-history-panel")
            .debug_selector(|| "file-history-panel".into())
            .overflow_y_scroll()
            .track_scroll(reveals.panel.scroll())
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(p.panel))
            .border_l_1()
            .border_color(rgb(p.border))
            .child(
                header_cue.list(div()
                    .on_children_prepainted(header_cue.observe(reveals.header.observe()))
                    .id("file-history-header")
                    .overflow_y_scroll()
                    .track_scroll(reveals.header.scroll())
                    .min_h_0()
                    .px_3()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    // DESIGN.md's responsive order: Close becomes its icon,
                    // then the title goes; Close is never cut (`TopRow`).
                    .child(
                        div()
                            .debug_selector(|| "file-history-top-row".into())
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .when(top_row != TopRow::Bare, |row| {
                                row.child(
                                    div()
                                        .debug_selector(|| "file-history-title".into())
                                        .flex_shrink_0()
                                        .text_size(crate::appearance::ui_text(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(TITLE),
                                )
                            })
                            // Alone, it keeps the trailing edge it has beside the title.
                            .when(top_row == TopRow::Bare, |row| row.justify_end())
                            .child({
                                let close = if top_row == TopRow::Labelled {
                                    button("close-file-history", CLOSE_LABEL, "arrow-left", false)
                                } else {
                                    button("close-file-history", "", "arrow-left", false)
                                        .tooltip(CLOSE_TOOLTIP)
                                };
                                let close = crate::views::compact_control_name(
                                    close.flex_shrink_0(),
                                    "close-file-history",
                                    CLOSE_NAME,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.back_to_history(window, cx);
                                }));
                                reveals.track(
                                    &reveals.header,
                                    "close-file-history",
                                    close,
                                    room,
                                    cx,
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("file-history-anchor-path")
                            .debug_selector(|| "file-history-anchor-path".into())
                            // Its whole line, as `file_history_header_height`
                            // measures it: it clips its text, which leaves it
                            // no least height of its own, so a header shorter
                            // than its rows squeezed it, from 21 to 8 px at
                            // 14 pt in a 472 × 1,000 window, rather than
                            // scroll where it was decided to.
                            .flex_shrink_0()
                            .min_w_0()
                            .truncate()
                            .text_size(crate::appearance::ui_text(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                            .child(copy_path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(|| "file-history-from".into())
                                    .flex_1()
                                    // Its whole line, else the whole row, in
                                    // which it wraps rather than run past the
                                    // header; the control wraps beneath it.
                                    .map(|line| match from_least {
                                        Some(Some(least)) => line.min_w(least),
                                        Some(None) => line.min_w(relative(1.)),
                                        None => line.min_w_0(),
                                    })
                                    .text_size(crate::appearance::ui_text(10.))
                                    .text_color(rgb(p.muted))
                                    .child(from),
                            )
                            .child({
                                let copy = copy_feedback::copy_button(
                                    path_mark,
                                    "",
                                    "Copy file history's anchor path",
                                    cx,
                                )
                                .on_click(move |_, window, cx| {
                                    copy_feedback::copy(
                                        window,
                                        cx,
                                        Some(path_copy.clone()),
                                        "file path",
                                        copy_path.clone(),
                                    )
                                });
                                reveals.track(&reveals.header, "copy-history-path", copy, room, cx)
                            }),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(p.muted))
                            .child(FOLLOWS),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child({
                                let newer = button("newer-file-revision", "Newer", "", false)
                                    .disabled(!newer)
                                    .accessibility_label("Previous newer file revision")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.move_file_history_revision(-1, false, window, cx);
                                    }));
                                reveals.track(&reveals.header, "newer-file-revision", newer, room, cx)
                            })
                            .child({
                                let older = button("older-file-revision", "Older", "", false)
                                    .disabled(!older)
                                    .accessibility_label("Next older file revision")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.move_file_history_revision(1, false, window, cx);
                                    }));
                                reveals.track(&reveals.header, "older-file-revision", older, room, cx)
                            })
                            .child(
                                div()
                                    .debug_selector(|| "file-history-revision-number".into())
                                    .flex_1()
                                    .text_size(crate::appearance::ui_text(10.))
                                    .text_color(rgb(p.muted))
                                    .child(lineage.selected.map_or(String::new(), |index| {
                                        format!("Revision {}", lineage.offset + index + 1)
                                    })),
                            ),
                    ), "file-history-header-scrollbar")
                    // The cue's own node gives way and carries the divider,
                    // inside which its track lies, so it paints over no line.
                    .debug_selector(|| "file-history-header".into())
                    // As tall as `Regions` decided, or, where no window lays
                    // the inspector out, its rows within its share.
                    .map(|header| match regions {
                        Some(regions) => header.h(regions.header).flex_none(),
                        None => header.max_h(relative(0.30)).flex_shrink(1.).min_h(least.header),
                    })
                    .border_b_1()
                    .border_color(rgb(p.border)),
            )
            .child(div().flex_1().min_h(least.body).flex().flex_col()
                // As tall as the message and the revision's details where
                // they fit, as History's panel is, at most its share here.
                // Its controls keep their own focus handles, so the panel
                // reveals the message panel whole as one region.
                .children(entry.map(|entry| {
                    let message = self.render_commit_message(&entry.commit, true, extent, window, cx);
                    reveals.panel.region(MESSAGE_REGION, room, message, cx)
                }))
            .children(lineage.error.as_ref().map(|error| {
                // The notice and its message each cue their own scroll, as
                // every scrolling region does, their fades dissolving into
                // the notice's removed background: both once scrolled with
                // nothing to say so. The message gives way inside the notice
                // down to its line, so the notice holds Retry page whole and
                // only the message scrolls: at 18 pt in a 461 × 420 window
                // both scrolled, and a wheel over the message moved only it,
                // leaving Retry page below the notice's fold.
                let surface = crate::tags::Surface::Removed;
                let failure_cue = crate::tags::ScrollCue::revealing(&reveals.failure, room).on(surface);
                let error_cue = decide(
                    crate::tags::ScrollCue::new(&lineage.error_scroll, Pixels::ZERO).on(surface),
                    error_scrolls,
                );
                failure_cue.list(div()
                    .on_children_prepainted(failure_cue.observe(reveals.failure.observe()))
                    .id("file-history-failure")
                    .debug_selector(|| "file-history-failure".into())
                    .overflow_y_scroll()
                    .track_scroll(reveals.failure.scroll())
                    .min_h_0()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(error_cue.list(
                        div()
                            .on_children_prepainted(error_cue.observe(|_, _, _| {}))
                            .id("file-history-error")
                            .debug_selector(|| "file-history-error".into())
                            .max_h(px(100.))
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&lineage.error_scroll)
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(p.warning))
                            .child(error.clone()),
                        "file-history-error-scrollbar",
                    )
                    // The cue's own node gives way, down to a line.
                    .flex_shrink(1.)
                    .min_h(least.error_line))
                    .child({
                        let retry = button("retry-file-history", "Retry page", "refresh", false)
                            .disabled(busy || retry.is_none())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(request) = retry {
                                    this.load_file_history_page(
                                        request.offset,
                                        request.position,
                                        window,
                                        cx,
                                    );
                                }
                            }));
                        reveals.track(&reveals.failure, "retry-file-history", retry, room, cx)
                    }), "file-history-failure-scrollbar")
                    // The cue's own node gives way to the revisions' whole
                    // row down to its least, and carries the notice's fill.
                    .max_h(relative(0.35))
                    .flex_shrink(1.)
                    .min_h(least.failure)
                    .bg(rgb(p.removed_background))
            }))
            .child(
                crate::native_accessibility::clips_children(div()
                    // The list's rows report where it lies, so the panel
                    // reveals it whole in the frame Tab first draws it: the
                    // kit's `on_prepaint` canvas, absolute with no insets,
                    // lay at the list's foot and reported it a list lower.
                    .on_children_prepainted(revisions_cue.observe(
                        reveals.panel.control(self.file_focus.clone(), room),
                    ))
                    .id("file-revision-list").debug_selector(|| "file-revision-list".into())
                    .role(Role::ListBox)
                    .aria_label("File revisions, newest first")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.file_focus)
                    .on_action(cx.listener(
                        |this, _: &gpui_kit::component::input::Copy, window, cx| {
                            this.copy_list_item(crate::views::ListCopy::RevisionHash, window, cx)
                        },
                    ))
                    .flex_1()
                    .relative()
                    // One whole row, which a failed page's notice gives way to.
                    .min_h(least.row)
                    .child(if count == 0 {
                        empty(
                            if busy {
                                "Reading file history…"
                            } else {
                                "No revisions in this lineage"
                            },
                            "History starts at the chosen commit and follows its first parents.",
                        )
                    } else {
                        // Its rows end at the gutter the cue keeps while they
                        // overflow, and its track lies in that gutter inside
                        // the ring's room, so the ring on the list's edge
                        // stays whole: the list once scrolled with nothing
                        // to say so.
                        revisions_cue.around(
                            uniform_list(
                                "file-revisions",
                                count,
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    range
                                        .map(|index| this.render_revision_row(index, cx))
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .size_full()
                            .track_scroll(&self.file_history.scroll)
                            .pr(revisions_cue.trailing()),
                            "file-revisions-scrollbar",
                        )
                        .size_full()
                        .into_any_element()
                    })
                    // Its keyboard ring on its own edge, over the rows, so
                    // none of them moves.
                    .children(self.focus_ring.edge(&self.file_focus, rgb(p.accent)))),
            )
            )
            .child(
                footer_cue.list(div()
                    .on_children_prepainted(footer_cue.observe(reveals.footer.observe()))
                    .id("file-history-footer")
                    .overflow_y_scroll()
                    .track_scroll(reveals.footer.scroll())
                    .min_h_0()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(p.muted))
                            .child(status),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child({
                                // Whole inside the room the footer's gutter
                                // leaves, labelled or its icon: at 18 pt in
                                // a 461 × 490 window Older page, 128 px, ran
                                // 8 px into its 120 px room's gutter.
                                let newer = page_button("newer-file-page", NEWER_PAGE, 0.75)
                                    .disabled(busy || previous_page.is_none())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(offset) = previous_page {
                                            this.load_file_history_page(
                                                offset,
                                                Position::First,
                                                window,
                                                cx,
                                            );
                                        }
                                    }));
                                reveals.track(&reveals.footer, "newer-file-page", newer, room, cx)
                            })
                            .child({
                                let older = page_button("older-file-page", OLDER_PAGE, 0.25)
                                    .disabled(busy || next_page.is_none())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(offset) = next_page {
                                            this.load_file_history_page(
                                                offset,
                                                Position::First,
                                                window,
                                                cx,
                                            );
                                        }
                                    }));
                                reveals.track(&reveals.footer, "older-file-page", older, room, cx)
                            }),
                    ), "file-history-footer-scrollbar")
                    .debug_selector(|| "file-history-footer".into())
                    .map(|footer| match regions {
                        Some(regions) => footer.h(regions.footer).flex_none(),
                        None => footer.max_h(relative(0.25)).flex_shrink(1.).min_h(least.footer),
                    })
                    .border_t_1()
                    .border_color(rgb(p.border)),
            ), "file-history-panel-scrollbar")
            .size_full()
            .min_w_0()
            .into_any_element()
    }

    /// Each line is selectable after the message in `selection`'s document.
    pub(super) fn render_revision_metadata(
        &self,
        entry: &FileHistoryEntry,
        selection: &commit_message::Selection,
        cx: &App,
    ) -> AnyElement {
        let p = palette(cx);
        let before = entry
            .change
            .old_path
            .as_ref()
            .map_or("Absent".into(), |path| path.display().to_string());
        let after = entry
            .change
            .new_path
            .as_ref()
            .map_or("Absent (deleted)".into(), |path| path.display().to_string());
        let [author, revision, parent, paths] = selection.metadata([
            format!(
                "{} · {}",
                commit_message::summary(&entry.commit.author),
                full_date(entry.commit.timestamp)
            )
            .into(),
            format!(
                "{} · {}",
                short_oid(&entry.commit.oid),
                entry.change.status.label()
            )
            .into(),
            entry
                .parent_oid
                .as_ref()
                .map_or(
                    "Root revision · compared with an empty tree".into(),
                    |parent| format!("Compared with first parent {}", short_oid(parent)),
                )
                .into(),
            if before == after {
                before
            } else {
                format!("Before: {before}\nAfter: {after}")
            }
            .into(),
        ]);
        // Inset as the commit's metadata is (`render_commit_metadata`), and
        // ending at its last row, so details that fit draw no scrollbar.
        div()
            .id("file-revision-details")
            .debug_selector(|| "file-revision-details".into())
            .min_w_0()
            .px_3()
            .pt_3()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(appearance::ui_text(11.))
            .text_color(rgb(p.muted))
            .child(author)
            .child(revision)
            .child(parent)
            .child(
                div()
                    .debug_selector(|| "file-revision-paths".into())
                    .text_color(rgb(p.text))
                    .child(paths),
            )
            .into_any_element()
    }

    fn render_revision_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(lineage) = self.file_history.lineage.as_ref() else {
            return div().into_any_element();
        };
        let Some(entry) = lineage
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
        else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let active = lineage.selected == Some(index);
        let enabled = lineage.pending.is_none();
        let path = entry.change.path().display().to_string();
        let detail = format!(
            "{} · {} · {}",
            short_oid(&entry.commit.oid),
            entry.change.status.label(),
            commit_message::summary(&entry.commit.author)
        );
        let subject = commit_message::summary(&entry.commit.subject);
        let tooltip = format!("{subject}\n{detail}\n{path}");
        div()
            .id(("file-revision", index))
            .debug_selector(move || format!("file-revision-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .aria_label(format!("{subject} · {detail}"))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .h(crate::appearance::ui_size(68.))
            .w_full()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .border_l_2()
            .border_color(if active {
                self.selected_row_marker(cx)
            } else {
                rgb(p.panel)
            })
            .bg(rgb(if active { p.selected } else { p.panel }))
            .hover(|style| style.bg(rgb(p.row_hover(active))))
            .when(enabled, |element| element.cursor_pointer())
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(subject),
            )
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(detail),
            )
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(path),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.select_file_revision(index, window, cx)
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gitturtle_core::{ChangeStatus, Commit, FileChange, FileHistoryEntry, FileHistoryPage};
    use std::path::PathBuf;

    #[test]
    fn scaling_preserves_nested_file_history_list_rows() {
        use crate::settings::ListScales;
        use gpui_kit::{point, px};
        let state = super::State {
            previous: Some(Box::default()),
            ..Default::default()
        };
        state
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-6800.)));
        let previous = state.previous.as_ref().unwrap();
        previous
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-13600.)));
        state.rescale_lists(ListScales {
            history: 1.,
            files: 1.,
            navigation: 1.,
            lineage: 85. / 68.,
        });
        assert_eq!(state.scroll.0.borrow().base_handle.offset().y, px(-8500.));
        assert_eq!(
            previous.scroll.0.borrow().base_handle.offset().y,
            px(-17000.)
        );
    }

    fn change(old: Option<PathBuf>, new: Option<PathBuf>) -> FileChange {
        FileChange {
            old_oid: old.as_ref().map(|_| "a".repeat(40)),
            new_oid: new.as_ref().map(|_| "b".repeat(40)),
            status: if new.is_none() {
                ChangeStatus::Deleted
            } else if old.is_none() {
                ChangeStatus::Added
            } else if old != new {
                ChangeStatus::Renamed
            } else {
                ChangeStatus::Modified
            },
            old_mode: if old.is_some() { "100644" } else { "000000" }.into(),
            new_mode: if new.is_some() { "100644" } else { "000000" }.into(),
            old_path: old,
            new_path: new,
        }
    }

    fn page(lineage: &Lineage, count: usize, next_offset: Option<usize>) -> FileHistoryPage {
        FileHistoryPage {
            anchor: lineage.anchor.clone(),
            path: lineage.path.clone(),
            entries: (0..count)
                .map(|index| FileHistoryEntry {
                    commit: Commit {
                        oid: format!("{index:040x}"),
                        parents: vec!["c".repeat(40)],
                        author: "Author".into(),
                        timestamp: 0,
                        subject: format!("Revision {index}"),
                        body: String::new(),
                    },
                    parent_oid: Some("c".repeat(40)),
                    change: change(Some(lineage.path.clone()), Some(lineage.path.clone())),
                })
                .collect(),
            next_offset,
        }
    }

    fn lineage() -> Lineage {
        Lineage::new("/fixture".into(), "f".repeat(40), "current.txt".into())
    }

    #[gpui::test]
    fn rendered_messages_leave_revision_navigation_and_copy_the_selected_entry(
        cx: &mut TestAppContext,
    ) {
        use crate::commit_message::tests::{commit, draw, fixture};
        let (_, app, cx) = fixture(cx, vec![commit("a", "Original history", "Original body")]);
        let expected = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let retained = app.take_inspection_context(window, cx);
                let mut lineage = lineage();
                let request = lineage.begin(0, Position::First);
                let mut page = page(&lineage, 20, Some(PAGE_SIZE));
                page.entries[0].commit.subject = "Wrapped revision title 東京 🐢 ".repeat(100);
                page.entries[0].commit.body = "Paragraph.\n\n".repeat(1000);
                let expected = commit_message::full_message(&page.entries[0].commit);
                assert!(lineage.accept(request, page));
                app.file_history = State {
                    lineage: Some(lineage),
                    retained: Some(Box::new(retained)),
                    ..State::default()
                };
                expected
            })
        });
        for font in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(font, 12, window, cx));
            draw(cx);
            let revision_list = cx.debug_bounds("file-revision-list").unwrap();
            assert!(
                revision_list.size.height >= appearance::ui_size(68.),
                "at least one entire revision must remain usable: {revision_list:?}"
            );
            let inspector = cx.debug_bounds("inspector-test").unwrap();
            for id in [
                "copy-message",
                "copy-commit",
                "close-file-history",
                "newer-file-page",
                "older-file-page",
            ] {
                let control = cx.debug_bounds(id).unwrap();
                assert!(
                    control.left() >= inspector.left() && control.right() <= inspector.right(),
                    "{id}: {control:?}"
                );
            }
            let button = cx.debug_bounds("copy-message").unwrap();
            cx.simulate_click(button.center(), Modifiers::default());
            cx.read(|cx| {
                assert!(
                    cx.read_from_clipboard().unwrap().text().unwrap() == expected,
                    "copy must preserve the selected file revision message"
                )
            });
        }
        cx.update(|window, cx| {
            appearance::apply_text_sizes(13, 12, window, cx);
            app.update(cx, |app, cx| app.back_to_history(window, cx));
        });
        draw(cx);
        let copy = cx.debug_bounds("copy-message").unwrap();
        cx.simulate_click(copy.center(), Modifiers::default());
        cx.read(|cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "Original history\n\nOriginal body"
            )
        });
    }

    /// With the revision list focused, Copy Selected List Item takes the
    /// selected revision's full hash, unless message text is selected.
    #[gpui::test]
    fn list_copy_takes_the_selected_revision_hash(cx: &mut TestAppContext) {
        use crate::commit_message::tests::{commit, copy_chord, draw, fixture, message_selection};
        let (_, app, cx) = fixture(cx, vec![commit("a", "Original history", "")]);
        cx.update(|window, cx| {
            shortcuts::bind_keys(cx);
            app.update(cx, |app, cx| {
                let retained = app.take_inspection_context(window, cx);
                let mut lineage = lineage();
                let request = lineage.begin(0, Position::First);
                assert!(lineage.accept(request, page(&lineage, 5, None)));
                lineage.selected = Some(3);
                app.file_history = State {
                    lineage: Some(lineage),
                    retained: Some(Box::new(retained)),
                    ..State::default()
                };
            })
        });
        draw(cx);
        let focus = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let focus = app.read(cx).file_focus.clone();
                window.focus(&focus, cx);
            });
            draw(cx);
        };
        focus(cx);
        assert_eq!(copy_chord(cx), format!("{:040x}", 3));

        let selection = message_selection(&app, cx);
        cx.update(|window, cx| selection.select_all(window, cx));
        focus(cx);
        assert_eq!(copy_chord(cx), "Revision 3");
    }

    #[test]
    fn superseded_pages_cannot_replace_the_latest_navigation() {
        let mut state = lineage();
        let stale = state.begin(0, Position::First);
        let current = state.begin(PAGE_SIZE, Position::Last);
        assert!(!state.accept(stale, page(&state, PAGE_SIZE, Some(PAGE_SIZE))));
        state.fail(stale, "obsolete failure".into());
        assert_eq!(state.pending, Some(current));
        assert!(state.error.is_none());
        assert!(state.accept(current, page(&state, 3, None)));
        assert_eq!(state.offset, PAGE_SIZE);
        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn revision_arrows_cross_pages_without_skipping_boundary_entries() {
        let mut state = lineage();
        let request = state.begin(0, Position::Last);
        assert!(state.accept(request, page(&state, PAGE_SIZE, Some(PAGE_SIZE))));
        assert_eq!(
            state.destination(1, false),
            Some(Destination::Page(PAGE_SIZE, Position::First))
        );
        let next = state.begin(PAGE_SIZE, Position::First);
        assert_eq!(state.destination(-1, false), None);
        assert!(state.accept(next, page(&state, 3, None)));
        assert_eq!(state.selected, Some(0));
        assert_eq!(
            state.destination(-1, false),
            Some(Destination::Page(0, Position::Last))
        );
        assert_eq!(state.destination(1, false), Some(Destination::Revision(1)));
        state.selected = Some(2);
        assert_eq!(state.destination(1, false), None);
    }

    #[test]
    fn failed_page_keeps_the_previous_revision_and_retry_offset() {
        let mut state = lineage();
        let first = state.begin(0, Position::Last);
        assert!(state.accept(first, page(&state, PAGE_SIZE, Some(PAGE_SIZE))));
        let selected_oid = state.entry().unwrap().commit.oid.clone();
        let failed = state.begin(PAGE_SIZE, Position::First);
        state.fail(failed, "missing local object".into());
        assert_eq!(state.entry().unwrap().commit.oid, selected_oid);
        assert_eq!(state.offset, 0);
        assert_eq!(state.last_request, Some(failed));
        assert!(state.pending.is_none());
    }

    #[test]
    fn anchor_mismatch_and_empty_pages_do_not_invent_a_revision() {
        let mut state = lineage();
        let request = state.begin(0, Position::First);
        let mut wrong = page(&state, 2, None);
        wrong.anchor = "a".repeat(40);
        assert!(!state.accept(request, wrong));
        assert!(state.page.is_none());
        assert!(state.accept(request, page(&state, 0, None)));
        assert!(state.entry().is_none());
        assert_eq!(state.destination(1, false), None);
        assert_eq!(state.destination(-1, true), None);
    }

    #[test]
    fn revision_selection_preserves_rename_and_absent_deletion_sides() {
        let mut state = lineage();
        let request = state.begin(0, Position::First);
        let mut revisions = page(&state, 2, None);
        let before = PathBuf::from("older/name.txt");
        let after = state.path.clone();
        revisions.entries[0].change = change(Some(after.clone()), None);
        revisions.entries[1].change = change(Some(before.clone()), Some(after.clone()));
        assert!(state.accept(request, revisions));
        let deletion = &state.entry().unwrap().change;
        assert_eq!(deletion.old_path.as_ref(), Some(&after));
        assert!(deletion.new_path.is_none());
        assert!(deletion.new_oid.is_none());
        state.selected = Some(1);
        let rename = &state.entry().unwrap().change;
        assert_eq!(rename.old_path.as_ref(), Some(&before));
        assert_eq!(rename.new_path.as_ref(), Some(&after));
    }

    #[cfg(unix)]
    #[test]
    fn visually_identical_byte_paths_cannot_receive_each_others_pages() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let mut state = lineage();
        state.path = PathBuf::from(OsString::from_vec(b"file-\xff.txt".to_vec()));
        let request = state.begin(0, Position::First);
        let mut wrong = page(&state, 1, None);
        wrong.path = PathBuf::from(OsString::from_vec(b"file-\xfe.txt".to_vec()));
        assert_eq!(state.path.to_string_lossy(), wrong.path.to_string_lossy());
        assert!(!state.accept(request, wrong));
        assert!(state.page.is_none());
    }

    /// File history opened as the window leaves for Settings lands its first
    /// page while the repository page is hidden, which loads no revision;
    /// returning through the repository's own tab loads the selected one, as
    /// Back does (`return_from_page`). Through the tab, the list showed its
    /// selected revision beside no preview of it until one was chosen again.
    #[gpui::test]
    async fn returning_through_the_active_tab_resumes_file_history(cx: &mut TestAppContext) {
        use crate::views::tests::{history_script, history_window_from, settle};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.selected_file),
            Some(0),
            "the commit's file is highlighted"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_file_history(0, window, cx);
                app.show_settings(window, cx);
            })
        });
        first_page_lands(&app, cx);
        let revision = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.file_history
                    .lineage
                    .as_ref()
                    .and_then(Lineage::entry)
                    .map(|entry| entry.change.clone())
            })
        };
        let selected = revision(cx).expect("the page landed with a revision selected");
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.content.is_none()),
            "the page landed on Settings and loaded no revision"
        );
        let tab = app.read_with(cx, |app, _| {
            app.repository_tabs.active.expect("the repository's tab")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.switch_repository_tab(tab, window, cx))
        });
        settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Repository
                && app.file_history.is_active()
                && app.files.first() == Some(&selected)
                && app.content.is_some()),
            "back through its tab, File history loads its selected revision"
        );
    }

    /// A dialog, then a sheet, opened while File history's first page is
    /// read keeps focus as the page lands and selects its revision, and its
    /// close lands on the revision list (`return_modal_to`). The landing
    /// once focused the list behind the modal, drawn nowhere a person could
    /// see, and the next key went to it.
    #[gpui::test]
    async fn a_first_page_landing_under_a_dialog_moves_no_focus(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_script, history_window_from, settle};
        use gpui_kit::component::WindowExt;
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        for modal in ["dialog", "sheet"] {
            // The page held on the reader while the modal opens.
            let held = app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            });
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_file_history(0, window, cx)));
            draw(cx);
            cx.update(|window, cx| {
                if modal == "dialog" {
                    window.open_dialog(cx, |dialog, _, _| dialog.title("Probe"));
                } else {
                    window.open_sheet(cx, |sheet, _, _| sheet.title("Probe"));
                }
            });
            draw(cx);
            let in_modal = focused(cx);
            assert!(
                in_modal.is_some() && in_modal.as_ref() != Some(&list),
                "the {modal} takes focus"
            );
            drop(held);
            first_page_lands(&app, cx);
            settle(&app, cx).await;
            assert!(
                app.read_with(cx, |app, _| app
                    .file_history
                    .lineage
                    .as_ref()
                    .is_some_and(|lineage| lineage.selected.is_some())),
                "the page selected its revision under the {modal}"
            );
            assert_eq!(focused(cx), in_modal, "focus stays in the {modal}");
            cx.update(|window, cx| {
                if modal == "dialog" {
                    window.close_dialog(cx);
                } else {
                    window.close_sheet(cx);
                }
            });
            draw(cx);
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(400));
            draw(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)
                || window.has_active_sheet(cx)));
            assert_eq!(
                focused(cx),
                Some(list.clone()),
                "the {modal} closes onto the revision list"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.close_file_history(window, cx)));
            settle(&app, cx).await;
        }
    }

    /// Opens File History on the selected commit's first file and waits for
    /// its first page, which the window's reader reads on a thread
    /// `run_until_parked` cannot wait for, then for the preview of the
    /// revision the page selects, which [`settle`](crate::views::tests::settle)
    /// awaits as the window's read. `settle` alone does not wait for the
    /// page: opening ends the window's read, and the page lands through File
    /// history's own task, so on a slow reader `settle` found nothing to
    /// await and returned before the page selected its revision.
    pub(crate) async fn open_landed(
        app: &gpui_kit::Entity<GitTurtle>,
        cx: &mut gpui_kit::VisualTestContext,
    ) {
        use crate::views::tests::settle;
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_file_history(0, window, cx)));
        first_page_lands(app, cx);
        settle(app, cx).await;
    }

    /// Waits for File history's page, read on the window's reader, a thread
    /// `run_until_parked` cannot wait for, to land, and fails the test at the
    /// caller's line where it has not landed after eight rounds: a page that
    /// never landed once let the test carry on without it.
    #[track_caller]
    pub(crate) fn first_page_lands(
        app: &gpui_kit::Entity<GitTurtle>,
        cx: &mut gpui_kit::VisualTestContext,
    ) {
        let landed = |cx: &mut gpui_kit::VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.file_history
                    .lineage
                    .as_ref()
                    .is_some_and(|lineage| lineage.page.is_some() && lineage.pending.is_none())
            })
        };
        for _ in 0..8 {
            if landed(cx) {
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        assert!(landed(cx), "File history's page landed");
    }

    /// File History's message panel is as tall as the message and the
    /// revision's details where they fit, with one inset after the paths, as
    /// History's panel ends one inset below P1
    /// (`views::tests::changed_files_follow_the_last_parent`), so the
    /// revisions follow at the inspector's rhythm, at 13 pt in 1,000 × 980,
    /// 461 × 980 and 1,280 × 800 and at 18 pt in 1,480 × 1,200, where they
    /// fit its 45% share: it kept that fixed share of the room between File
    /// History's header and footer, which left 93 px from the paths to the
    /// revisions at 13 pt in 1,000 × 980. Nothing scrolls where everything
    /// fits. File History opened at 13 pt in 1,200 × 900, the size History
    /// was drawn at, with no resize or text size between, is measured for
    /// its revision, never with History's measurement of the same commit at
    /// the same width, bound and text, which lacks the revision's paths: it
    /// opens with no measurement (`take_inspection_context`), and were the
    /// two to share one, whose details it measured would tell them apart
    /// (`commit_message::Details`, pinned by
    /// `commit_message::tests::a_kept_measurement_stands_only_for_the_details_it_measured`).
    /// Every later step resizes first.
    #[gpui::test]
    async fn file_revisions_follow_the_revisions_paths(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_script, history_window_from, settle};
        use gpui_kit::px;
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.simulate_resize(gpui_kit::size(px(1200.), px(900.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let follows = |cx: &mut gpui_kit::VisualTestContext, at: &str| {
            let paths = cx.debug_bounds("file-revision-paths").unwrap();
            let revisions = cx.debug_bounds("file-revision-list").unwrap();
            // The inset, then the panel's 1 px bottom border.
            let rhythm = cx.update(|window, _| window.rem_size() * 0.75) + px(1.);
            let gap = revisions.top() - paths.bottom();
            assert!(
                (gap - rhythm).abs() <= px(0.5),
                "{at}: {gap:?} from the paths {paths:?} to the revisions {revisions:?}, \
                 not {rhythm:?}"
            );
            assert_eq!(
                cx.read(|cx| app.read(cx).inspector_message.overflow()),
                Some(px(0.)),
                "{at}: nothing to scroll"
            );
        };
        follows(cx, "at the size History was drawn at");
        for (text, width, height) in [
            (13, 1000., 980.),
            (13, 461., 980.),
            (13, 1280., 800.),
            (18, 1480., 1200.),
        ] {
            cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(gpui_kit::size(px(width), px(height)));
            draw(cx);
            follows(cx, &format!("{text} pt in {width} × {height}"));
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A revision's message too long for 45% of the room between File
    /// History's header and footer keeps that 45% and scrolls, as History's
    /// does (`views::tests::a_long_message_keeps_its_share_and_scrolls`).
    #[gpui::test]
    async fn a_long_revision_message_keeps_its_share_and_scrolls(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_window_from};
        use gpui_kit::px;
        let body: String = (1..=80).map(|line| format!("Body line {line}\n")).collect();
        let message = format!("fixture notes\n\n{body}");
        let script = format!(
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata {}\n{message}\nM 100644 inline notes.txt\ndata 5\nnote\n\ndone\n",
            message.len(),
        );
        let (_fixture, app, cx) = history_window_from(cx, &script).await;
        open_landed(&app, cx).await;
        cx.simulate_resize(gpui_kit::size(px(1000.), px(680.)));
        draw(cx);
        let panel = cx.debug_bounds("commit-message-panel").unwrap();
        // The panel tops the room it shares with the revisions, which end it.
        let revisions = cx.debug_bounds("file-revision-list").unwrap();
        let room = revisions.bottom() - panel.top();
        assert!(
            (panel.size.height - room * 0.45).abs() <= px(1.),
            "{panel:?} keeps 45% of the {room:?} above {revisions:?}"
        );
        assert!(
            cx.read(|cx| app.read(cx).inspector_message.overflow())
                .is_some_and(|overflow| overflow > px(0.)),
            "a long message scrolls"
        );
    }

    /// A revision's details keep the commit metadata's inset, the scaled
    /// 12 px beside them, and end at their last row, as the Inspector's do,
    /// at 18 pt in a 1,000 × 680 window and at the default size in a
    /// 1,480 × 980 one: their padding below the last row was scrolled as
    /// content, so details that fit drew a scrollbar for it.
    #[gpui::test]
    async fn file_history_details_end_at_their_last_row(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_script, history_window_from, settle};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        for (text, width, height) in [(18, 1000., 680.), (13, 1480., 980.)] {
            cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let details = cx
                .debug_bounds("file-revision-details")
                .unwrap_or_else(|| panic!("{at}: the revision's details"));
            let last = cx.debug_bounds("file-revision-paths").unwrap();
            let inset = cx.update(|window, _| window.rem_size() * 0.75);
            assert!(
                (details.bottom() - last.bottom()).abs() <= gpui_kit::px(0.5),
                "{at}: {details:?} ends at its last row {last:?}"
            );
            assert!(
                (last.left() - details.left() - inset).abs() <= gpui_kit::px(0.5),
                "{at}: {last:?} inset {inset:?} in {details:?}"
            );
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// File history opened from Compare's changed-file row by the row's
    /// right-click menu, at 18 pt in a 461 × 490 window, closes by Escape
    /// onto that row's list, drawn and taking the keys: the command
    /// palette's shortcut opens the palette, and with the palette closed a
    /// second Escape goes back to History. The menu, drawn inside the row,
    /// held focus as its item ran, and its handle was kept as the control to
    /// give back: closing focused that handle once the menu was gone, where
    /// AT-SPI read only the window's frame focused and no key reached
    /// anything until a click.
    #[gpui::test]
    async fn file_history_opened_from_a_rows_menu_closes_onto_its_list(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{Modifiers, MouseButton, px, size};
        const HISTORY: &str = "menu-item: File history… · MenuItem";
        let palette = if cfg!(target_os = "macos") {
            "cmd-shift-p"
        } else {
            "ctrl-shift-p"
        };
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|window, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
            cx.set_reduce_motion(true);
            crate::appearance::apply_text_sizes(18, 12, window, cx);
        });
        cx.simulate_resize(size(px(461.), px(490.)));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_commit(1, window, cx);
                app.pane = Pane::History;
                window.focus(&app.focus, cx);
            })
        });
        settle(&app, cx).await;
        // Enter on the commit opens its highlighted file in Compare.
        cx.simulate_keystrokes("enter");
        settle(&app, cx).await;
        let (files, root) = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Compare, "Enter opened Compare");
            (app.file_focus.clone(), app.app_focus.clone())
        });
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        assert_eq!(focused(cx), Some(files.clone()), "Compare's file list");
        let row = cx
            .debug_bounds("selected-changed-file")
            .expect("the selected file's row");
        cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
        draw(cx);
        let item = cx.debug_bounds(HISTORY).expect("the row's menu");
        cx.simulate_click(item.center(), Modifiers::default());
        first_page_lands(&app, cx);
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.file_history.is_active()));
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| !app.file_history.is_active()
            && app.mode == WorkspaceMode::Compare));
        assert_eq!(focused(cx), Some(files.clone()), "the row's list");
        assert!(
            cx.update(|window, _| root.contains(&files, window)),
            "the window draws the list"
        );
        assert!(app.read_with(cx, |app, _| app.pane == Pane::Files));
        cx.simulate_keystrokes(palette);
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "the palette opens"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(
            focused(cx),
            Some(files.clone()),
            "the palette closes onto it"
        );
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.mode == WorkspaceMode::History),
            "a second Escape goes back to History"
        );
        // The same from the keyboard: Shift-F10 on History's changed file,
        // whose menu opens on File history….
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.pane = Pane::Files;
                window.focus(&app.file_focus, cx);
            })
        });
        draw(cx);
        cx.simulate_keystrokes("shift-f10");
        draw(cx);
        assert!(
            cx.debug_bounds(HISTORY).is_some(),
            "the row's keyboard menu"
        );
        cx.simulate_keystrokes("enter");
        first_page_lands(&app, cx);
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.file_history.is_active()));
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| !app.file_history.is_active()
            && app.mode == WorkspaceMode::History));
        assert_eq!(focused(cx), Some(files), "the changed files' list");
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// With the window's root focused, as a click on an empty spot of the
    /// window leaves it and where AT-SPI reads only the window's frame
    /// focused, the window's own shortcuts reach it: in Compare the command
    /// palette's shortcut opens the palette, and Escape goes back to History.
    #[gpui::test]
    async fn the_windows_shortcuts_reach_its_focused_root(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{Modifiers, point, px};
        let palette = if cfg!(target_os = "macos") {
            "cmd-shift-p"
        } else {
            "ctrl-shift-p"
        };
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
        let root = app.read_with(cx, |app, _| app.app_focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let click_an_empty_spot = |cx: &mut VisualTestContext| {
            let bar = cx.debug_bounds("status-bar").expect("the status bar");
            cx.simulate_click(
                point(bar.left() + px(4.), bar.center().y),
                Modifiers::default(),
            );
            draw(cx);
        };
        click_an_empty_spot(cx);
        assert_eq!(focused(cx), Some(root.clone()), "the window's root");
        cx.simulate_keystrokes(palette);
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "the palette opens"
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        click_an_empty_spot(cx);
        assert_eq!(focused(cx), Some(root), "the window's root again");
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.mode == WorkspaceMode::History),
            "Escape goes back to History"
        );
    }

    /// File History at 18 pt in a 461 × 490 window gives way in DESIGN.md's
    /// responsive order: in its top row Close becomes its icon, and the
    /// title goes where the two do not fit, Close whole inside the header
    /// and the window with room for its ring,
    /// the anchor's line wraps inside the header, and the message keeps its
    /// first line whole below its toolbar, above the revisions. The labelled
    /// Close and the title ran 79 px past the window's edge, and the
    /// message's 15 px viewport drew nothing under the revision row. At 13 pt
    /// in a 1,480 × 980 window Close keeps its label.
    #[gpui::test]
    async fn file_history_gives_way_in_a_half_tile_at_18_pt(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| {
            crate::appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let slack = px(0.5);
        let ring = cx.update(|_, cx| crate::appearance::button_ring_room(cx));
        for (text, width, height) in [(18, 461., 490.), (18, 461., 420.), (13, 1480., 980.)] {
            cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let header = bounds(cx, "file-history-header");
            let close = bounds(cx, "close-file-history");
            assert!(
                close.left() - ring >= header.left() - slack
                    && close.right() + ring <= header.right() + slack
                    && close.right() + ring <= px(width),
                "{at}: Close {close:?} and its ring inside {header:?} and the window"
            );
            let square = crate::appearance::ui_size(28.);
            let title = cx.debug_bounds("file-history-title");
            if text == 18 {
                assert!(
                    (close.size.width - square).abs() <= slack,
                    "{at}: Close {close:?} keeps its icon"
                );
                // Where the title has not gone, it lies whole beside Close.
                if let Some(title) = title {
                    assert!(
                        title.right() <= close.left() && title.left() >= header.left(),
                        "{at}: the title {title:?} beside Close {close:?}"
                    );
                }
            } else {
                assert!(
                    close.size.width > square + slack && title.is_some(),
                    "{at}: Close {close:?} keeps its label beside the title"
                );
            }
            let from = bounds(cx, "file-history-from");
            assert!(
                from.right() <= header.right() + slack,
                "{at}: the anchor's line {from:?} inside {header:?}"
            );
            let viewport = bounds(cx, "commit-message-viewport");
            // The title's first line, a half rem below the piece's top, at
            // 15 pt and 1.35 lines, whole inside the viewport.
            let first = bounds(cx, "commit-message-piece-0");
            let line = cx.update(|window, _| {
                window.rem_size() * 0.5 + crate::appearance::ui_text(15.) * 1.35
            });
            assert!(
                (first.top() - viewport.top()).abs() <= slack
                    && first.top() + line <= viewport.bottom() + slack,
                "{at}: the message's first line, {line:?} of {first:?}, whole in {viewport:?}"
            );
            let panel = bounds(cx, "commit-message-panel");
            let revisions = bounds(cx, "file-revision-list");
            assert!(
                revisions.top() + slack >= panel.bottom(),
                "{at}: the revisions {revisions:?} below the message {panel:?}"
            );
            // At least one revision row whole.
            let row = crate::appearance::ui_size(68.);
            assert!(
                revisions.size.height + slack >= row,
                "{at}: the revisions {revisions:?} hold a whole {row:?} row; header {header:?}, \
                 message {panel:?}, footer {:?}",
                cx.debug_bounds("file-history-footer")
            );
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// Every Tab stop of File History is revealed whole as Tab or Shift+Tab
    /// moves onto it at 18 pt, in a 461 × 420 window, where the panel is
    /// shorter than its parts' least heights and scrolls as a whole, and in
    /// a 461 × 490 one, where the panel fits but its header and footer
    /// scroll their own rows: Close, the anchor path's copy control and the
    /// revision arrows in the header's own scroll, the message panel's Copy
    /// message and Hash, revealed with the message panel as one region, and,
    /// where a page failed and another follows, Retry page in the failure's
    /// own scroll and the page arrows in the footer's, each whole, its
    /// ring's room included, inside the part of the panel and of its region
    /// the window shows, and inside the panel's view in every frame painted
    /// after the key, the first that draws the focus included; so are the
    /// revisions where the keyboard places focus on them. A stop starts
    /// below its fold, so no walk passes by finding every stop in view. The
    /// scrolling panel draws its scrollbar, and the toolbar, message and top
    /// row are laid out for the room its gutter leaves. The revisions keep
    /// one whole row, a failed page's notice giving way above them. Tab once moved
    /// onto the anchor path's copy control hidden in the header's scroll and
    /// onto Older page below the panel's fold, showing neither. At 13 pt in
    /// a 1,480 × 973 window the panel fits, draws no scrollbar and is laid
    /// out as it was before it could scroll.
    #[gpui::test]
    async fn every_file_history_stop_is_revealed_whole_in_a_short_panel(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{Bounds, ElementId, point, px, size};
        const HEADER: [&str; 4] = [
            "close-file-history",
            "copy-history-path",
            "newer-file-revision",
            "older-file-revision",
        ];
        const FOOTER: [&str; 2] = ["newer-file-page", "older-file-page"];
        const RETRY: &str = "retry-file-history";
        const MESSAGE: &str = "commit-message-panel";
        const LIST: &str = "file-revision-list";
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| {
            crate::appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let reveals = app.read_with(cx, |app, _| app.file_history.reveals.clone());
        // The toolbar, the message and the top row are laid out for the room
        // the panel draws them in (`commit_message::State::details_room`),
        // the toolbar's own less its `px_3`, so a scrolling panel's gutter
        // is taken from both: laid out at the inspector's whole width, they
        // measured 19 px wider than drawn at 18 pt in a 461 × 420 window.
        let laid_out_for_drawn_room = |cx: &mut VisualTestContext, at: &str| {
            let room = app
                .read_with(cx, |app, _| app.inspector_message.details_room())
                .expect("a laid-out toolbar");
            let toolbar = cx
                .debug_bounds("commit-message-toolbar")
                .expect("the toolbar");
            let drawn = toolbar.size.width - cx.update(|window, _| window.rem_size()) * 1.5;
            // The toolbar's width is laid out on whole device pixels.
            assert!(
                (room - drawn).abs() <= px(1.),
                "{at}: laid out for {room:?}, drawn in {drawn:?}"
            );
        };
        // The guard: where the panel fits, it is laid out as before it could
        // scroll, and draws no scrollbar.
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(973.)));
        draw(cx);
        draw(cx);
        let laid_out = |x: f32, y: f32, width: f32, height: f32| {
            Some(Bounds::new(
                point(px(x), px(y)),
                size(px(width), px(height)),
            ))
        };
        for (selector, before) in [
            ("file-history-header", laid_out(1201., 136., 279., 181.5)),
            ("file-history-top-row", laid_out(1210.5, 145.5, 260., 28.)),
            ("file-history-title", laid_out(1210.5, 149.5, 87., 19.5)),
            ("close-file-history", laid_out(1399.5, 145.5, 71., 28.)),
            ("file-history-from", laid_out(1210.5, 212., 225.5, 16.)),
            (MESSAGE, laid_out(1201., 317.5, 279., 172.)),
            ("commit-message-toolbar", laid_out(1201., 317.5, 279., 34.)),
            (
                "commit-message-viewport",
                laid_out(1201., 351.5, 279., 137.),
            ),
            (LIST, laid_out(1201., 489.5, 279., 387.)),
            ("file-history-footer", laid_out(1201., 876.5, 279., 70.5)),
            ("newer-file-page", laid_out(1210.5, 909.5, 92., 28.)),
            ("older-file-page", laid_out(1309., 909.5, 92., 28.)),
        ] {
            assert_eq!(
                cx.debug_bounds(selector),
                before,
                "13 pt in 1480 × 973: {selector}"
            );
        }
        assert_eq!(
            reveals.panel.scroll().max_offset().y,
            px(0.),
            "13 pt in 1480 × 973: the panel fits"
        );
        assert!(
            cx.debug_bounds("file-history-panel-scrollbar").is_none(),
            "13 pt in 1480 × 973: no scrollbar"
        );
        laid_out_for_drawn_room(cx, "13 pt in 1480 × 973");

        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        let room = cx.update(|_, cx| crate::appearance::button_ring_room(cx));
        let slack = px(0.5);
        // The region whose own scroll holds `stop`, if any.
        let region = |stop: &str| {
            if HEADER.contains(&stop) {
                Some("file-history-header")
            } else if FOOTER.contains(&stop) {
                Some("file-history-footer")
            } else if stop == RETRY {
                Some("file-history-failure")
            } else {
                None
            }
        };
        // The stop holding focus. The message panel's controls keep their
        // own focus handles; focus on any of them lies in its region.
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                for (reveal, stops) in [
                    (&reveals.header, &HEADER[..]),
                    (&reveals.failure, &[RETRY][..]),
                    (&reveals.footer, &FOOTER[..]),
                ] {
                    if let Some(id) = reveal.focused(window) {
                        return stops
                            .iter()
                            .copied()
                            .find(|stop| ElementId::from(*stop) == id);
                    }
                }
                if reveals
                    .panel
                    .focus(MESSAGE_REGION, cx)
                    .contains_focused(window, cx)
                {
                    Some(MESSAGE)
                } else {
                    let list = app.read(cx).file_focus.clone();
                    list.is_focused(window).then_some(LIST)
                }
            })
        };
        // What the window shows of the place `stop` lies in: the panel's
        // view, and its region's where it lies in one.
        let shown = |cx: &mut VisualTestContext, stop: &str| {
            let panel = cx.debug_bounds("file-history-panel").expect("the panel");
            region(stop).map_or(panel, |region| {
                panel.intersect(&cx.debug_bounds(region).expect("the region"))
            })
        };
        let whole = |control: Bounds<Pixels>, view: Bounds<Pixels>| {
            control.top() + slack >= view.top() && control.bottom() <= view.bottom() + slack
        };
        // `key` until focus leaves File History: the stops in order, each
        // whole where it took focus.
        let walk = |cx: &mut VisualTestContext, key: &str, at: &str| {
            let mut stops = Vec::new();
            for _ in 0..16 {
                reveals.panel.take_painted();
                cx.simulate_keystrokes(key);
                draw(cx);
                let Some(stop) = focused(cx) else {
                    return stops;
                };
                let control = cx
                    .debug_bounds(stop)
                    .unwrap_or_else(|| panic!("{at}: {stop} drawn"));
                let view = shown(cx, stop);
                assert!(
                    whole(control.dilate(room), view),
                    "{at}, {key} onto {stop}: {control:?} with its ring whole in {view:?}"
                );
                for frame in reveals.panel.take_painted() {
                    if let Some(drawn) = frame.control {
                        assert!(
                            whole(drawn, frame.viewport),
                            "{at}, {key} onto {stop}: a frame drew it at {drawn:?} in {:?}",
                            frame.viewport
                        );
                    }
                }
                stops.push(stop);
            }
            panic!("{at}: {key} kept to File History through {stops:?}");
        };
        for (height, failed) in [(420., false), (420., true), (490., false)] {
            cx.simulate_resize(size(px(461.), px(height)));
            if failed {
                // A page failed, and another follows.
                app.update(cx, |app, cx| {
                    let lineage = app.file_history.lineage.as_mut().unwrap();
                    lineage.error = Some("The page could not be read.".into());
                    lineage.page.as_mut().unwrap().next_offset = Some(PAGE_SIZE);
                    cx.notify();
                });
            } else {
                app.update(cx, |app, cx| {
                    let lineage = app.file_history.lineage.as_mut().unwrap();
                    lineage.error = None;
                    lineage.page.as_mut().unwrap().next_offset = None;
                    cx.notify();
                });
            }
            draw(cx);
            draw(cx);
            let at = format!("18 pt in 461 × {height}, failed {failed}");
            let scrolls = height < 490.;
            assert_eq!(
                reveals.panel.scroll().max_offset().y > px(0.),
                scrolls,
                "{at}: the panel scrolls"
            );
            assert_eq!(
                cx.debug_bounds("file-history-panel-scrollbar").is_some(),
                scrolls,
                "{at}: the scrolling panel draws its scrollbar"
            );
            laid_out_for_drawn_room(cx, &at);
            // The revisions keep one whole row, below a failed page's notice
            // and above the footer: a notice once left them about 31 px.
            let revisions = cx.debug_bounds(LIST).expect("the revisions");
            let footer = cx.debug_bounds("file-history-footer").expect("the footer");
            let row = crate::appearance::ui_size(68.);
            assert!(
                revisions.size.height + slack >= row && revisions.bottom() <= footer.top() + slack,
                "{at}: the revisions {revisions:?} hold a whole {row:?} row above the footer \
                 {footer:?}"
            );
            if failed {
                let notice = cx.debug_bounds("file-history-failure").expect("the notice");
                assert!(
                    revisions.top() + slack >= notice.bottom(),
                    "{at}: the revisions {revisions:?} below the notice {notice:?}"
                );
            }
            // From Close, every region at its top.
            cx.update(|window, cx| {
                for reveal in [
                    &reveals.panel,
                    &reveals.header,
                    &reveals.failure,
                    &reveals.footer,
                ] {
                    reveal.scroll().set_offset(point(px(0.), px(0.)));
                }
                reveals
                    .header
                    .focus("close-file-history", cx)
                    .focus(window, cx);
            });
            draw(cx);
            // Copy message, Hash and the message's text, then the
            // revisions, below a failed page's notice. The text and the
            // revisions were no Tab stops, their handles made without it
            // where GPUI ignores the div's own, so Tab passed them by.
            let mut expected = vec![
                "close-file-history",
                "copy-history-path",
                "older-file-revision",
                MESSAGE,
                MESSAGE,
                MESSAGE,
            ];
            if failed {
                expected.extend([RETRY, LIST, "older-file-page"]);
            } else {
                expected.push(LIST);
            }
            // Not every stop is in view before the walk.
            let below = expected
                .iter()
                .copied()
                .filter(|stop| {
                    let fold = shown(cx, stop).bottom();
                    cx.debug_bounds(stop)
                        .is_some_and(|bounds| bounds.top() >= fold)
                })
                .collect::<Vec<_>>();
            assert!(!below.is_empty(), "{at}: a stop starts below its fold");
            let mut forward = vec!["close-file-history"];
            forward.extend(walk(cx, "tab", &at));
            assert_eq!(forward, expected, "{at}: Tab walks every stop");
            // Back from the control past the last stop.
            let mut backward = walk(cx, "shift-tab", &at);
            backward.reverse();
            assert_eq!(backward, expected, "{at}: Shift+Tab walks them back");
            // Focus the keyboard places on the revisions, as Escape out of a
            // dialog opened from them does, with the panel at its top.
            reveals.panel.scroll().set_offset(point(px(0.), px(0.)));
            draw(cx);
            cx.update(|window, cx| {
                let list = app.read(cx).file_focus.clone();
                window.focus(&list, cx);
            });
            draw(cx);
            assert_eq!(focused(cx), Some(LIST), "{at}");
            let list = cx.debug_bounds(LIST).expect("the revisions");
            let view = shown(cx, LIST);
            assert!(
                whole(list.dilate(room), view),
                "{at}: the revisions {list:?} with their ring whole in {view:?}"
            );
            if failed && scrolls {
                // The message panel, revealed as one region: from Retry page
                // with the panel at its foot, where the message panel lies
                // above the view, Shift+Tab onto Hash shows it whole.
                cx.update(|window, cx| {
                    let scroll = reveals.panel.scroll();
                    scroll.set_offset(point(px(0.), -scroll.max_offset().y));
                    reveals.failure.focus(RETRY, cx).focus(window, cx);
                });
                draw(cx);
                assert_eq!(focused(cx), Some(RETRY), "{at}");
                let message = cx.debug_bounds(MESSAGE).expect("the message panel");
                assert!(
                    !whole(message, shown(cx, MESSAGE)),
                    "{at}: the message panel {message:?} starts out of view"
                );
                reveals.panel.take_painted();
                cx.simulate_keystrokes("shift-tab");
                draw(cx);
                assert_eq!(focused(cx), Some(MESSAGE), "{at}: Shift+Tab onto Hash");
                let message = cx.debug_bounds(MESSAGE).expect("the message panel");
                let view = shown(cx, MESSAGE);
                assert!(
                    whole(message.dilate(room), view),
                    "{at}: the message panel {message:?} with its ring's room whole in {view:?}"
                );
                for frame in reveals.panel.take_painted() {
                    if let Some(drawn) = frame.control {
                        assert!(
                            whole(drawn, frame.viewport),
                            "{at}: a frame drew the message panel at {drawn:?} in {:?}",
                            frame.viewport
                        );
                    }
                }
            }
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// File History's header and its footer each cue their own scroll with
    /// the shared cue, as every other scrolling region does: at 18 pt in a
    /// 461 × 490 window and a 1,200 × 490 one both scroll, the header along
    /// a track a thumb moves on, so it draws its scrollbar, and the footer,
    /// shorter than the shortest track, by a fade at the edge its rows
    /// continue past, in the panel they lie on. What each holds ends short
    /// of the gutter the cue keeps, its ring included, and the scrolling
    /// footer keeps Newer page's and Older page's icons, at 1,200 px and at
    /// 13 pt in 461 × 490 too, where their labels would fit its row. Where
    /// they fit, at 13 pt in a 1,480 × 980 window, neither draws a cue. Both
    /// once scrolled hundreds of pixels with nothing to say so.
    #[gpui::test]
    async fn file_history_header_and_footer_cue_their_own_scrolls(cx: &mut TestAppContext) {
        use crate::tags::tests::painted_fades_on;
        use crate::tags::{SHORTEST_SCROLL_TRACK, ScrollCue};
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| {
            crate::appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let reveals = app.read_with(cx, |app, _| app.file_history.reveals.clone());
        let (surface, ring) = cx.update(|_, cx| {
            (
                crate::appearance::palette(cx).panel,
                crate::appearance::button_ring_room(cx),
            )
        });
        let slack = px(0.5);
        let regions = [
            (
                &reveals.header,
                "file-history-header-scrollbar",
                "close-file-history",
            ),
            (
                &reveals.footer,
                "file-history-footer-scrollbar",
                "older-file-page",
            ),
            (
                &reveals.footer,
                "file-history-footer-scrollbar",
                "newer-file-page",
            ),
        ];
        let square =
            |cx: &mut VisualTestContext| cx.update(|_, _| crate::appearance::ui_size(28.)) + slack;
        for (text, width, scrolls, labels) in [
            (18, 461., true, false),
            (18, 1200., true, false),
            (13, 461., false, false),
        ] {
            cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(490.)));
            draw(cx);
            draw(cx);
            // Newer page and Older page whole: their labels where the wider
            // fits the footer's room, else the chevron turned up and down,
            // each named and tooltipped by its label, never an ellipsis.
            let room = app
                .read_with(cx, |app, _| app.inspector_message.details_room())
                .expect("a laid-out toolbar");
            for (stop, name) in [
                ("newer-file-page", NEWER_PAGE),
                ("older-file-page", OLDER_PAGE),
            ] {
                let at = format!("{text} pt in {width} × 490, {stop}");
                let control = cx.debug_bounds(stop).expect("the page control");
                assert_eq!(
                    control.size.width > square(cx),
                    labels,
                    "{at}: {control:?} labelled where it fits {room:?}, its icon where not"
                );
                if !labels {
                    assert_eq!(
                        crate::views::COMPACT_NAMES.with(|names| names.borrow().get(stop).copied()),
                        Some(name),
                        "{at}: its icon named by its label"
                    );
                }
                let line = cx.update(|window, _| {
                    crate::views::line_widths(&[(name, crate::appearance::ui_text(12.))], window)
                });
                assert!(
                    !labels || control.size.width + slack >= line,
                    "{at}: {control:?} holds its whole label {line:?}"
                );
            }
            for (reveal, scrollbar, stop) in regions {
                let at = format!("{text} pt in {width} × 490, {scrollbar}, {stop}");
                let scroll = reveal.scroll();
                if !scrolls {
                    if scroll.max_offset().y > px(0.) {
                        let list = scroll.bounds();
                        let control = cx.debug_bounds(stop).expect("drawn");
                        assert!(
                            control.right() + ring <= list.right() - ScrollCue::gutter() + slack,
                            "{at}: {stop} {control:?} clear of the gutter of {list:?}"
                        );
                        assert!(
                            cx.debug_bounds(scrollbar).is_some()
                                || painted_fades_on(cx, list, surface) != (None, None),
                            "{at}: cued"
                        );
                    } else {
                        assert!(cx.debug_bounds(scrollbar).is_none(), "{at}: no scrollbar");
                    }
                    continue;
                }
                assert!(scroll.max_offset().y > px(0.), "{at}: scrolls its rows");
                let list = scroll.bounds();
                let drawn = cx.debug_bounds(scrollbar);
                let fades = painted_fades_on(cx, list, surface);
                if list.size.height >= SHORTEST_SCROLL_TRACK {
                    assert!(drawn.is_some(), "{at}: draws its scrollbar along {list:?}");
                    assert_eq!(fades, (None, None), "{at}: and no fade");
                } else {
                    assert!(drawn.is_none(), "{at}: no scrollbar along {list:?}");
                    assert!(
                        fades.0.is_none() && fades.1.is_some(),
                        "{at}: a fade at the edge its rows continue past, in the panel: {fades:?}"
                    );
                }
                let control = cx
                    .debug_bounds(stop)
                    .unwrap_or_else(|| panic!("{at}: {stop} drawn"));
                assert!(
                    control.right() + ring <= list.right() - ScrollCue::gutter() + slack,
                    "{at}: {stop} {control:?} with its ring clear of the gutter of {list:?}"
                );
            }
        }
        // A failed page's message cues its own scroll, fading into the
        // notice's removed background, and gives way inside the notice down
        // to its line, so only the message scrolls and the notice holds
        // Retry page whole: at 18 pt in a 461 × 420 window, with a message
        // longer than its 100 px. Both once scrolled, and a wheel over the
        // message moved only it, Retry page below the notice's fold.
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(420.)));
        app.update(cx, |app, cx| {
            let lineage = app.file_history.lineage.as_mut().unwrap();
            lineage.error = Some("The page could not be read. ".repeat(12));
            cx.notify();
        });
        draw(cx);
        draw(cx);
        let removed = cx.update(|_, cx| crate::appearance::palette(cx).removed_background);
        let (failure, error) = app.read_with(cx, |app, _| {
            let lineage = app.file_history.lineage.as_ref().unwrap();
            (
                app.file_history.reveals.failure.scroll().clone(),
                lineage.error_scroll.clone(),
            )
        });
        let at = "18 pt in 461 × 420, a failed page";
        assert_eq!(
            failure.max_offset().y,
            px(0.),
            "{at}: the notice holds its rows"
        );
        assert!(
            cx.debug_bounds("file-history-failure-scrollbar").is_none()
                && painted_fades_on(cx, failure.bounds(), removed) == (None, None),
            "{at}: the notice draws no cue"
        );
        let notice = cx.debug_bounds("file-history-failure").expect("the notice");
        let retry = cx.debug_bounds("retry-file-history").expect("Retry page");
        assert!(
            retry.top() + slack >= notice.top() && retry.bottom() <= notice.bottom() + slack,
            "{at}: Retry page {retry:?} whole in the notice {notice:?}"
        );
        assert!(error.max_offset().y > px(0.), "{at}: the message scrolls");
        let list = error.bounds();
        if list.size.height >= SHORTEST_SCROLL_TRACK {
            assert!(
                cx.debug_bounds("file-history-error-scrollbar").is_some(),
                "{at}: the message's scrollbar along {list:?}"
            );
        } else {
            assert!(
                cx.debug_bounds("file-history-error-scrollbar").is_none(),
                "{at}: no scrollbar along the message's {list:?}"
            );
            assert!(
                painted_fades_on(cx, list, removed).1.is_some(),
                "{at}: a fade at the message's foot in the notice's fill"
            );
        }
        app.update(cx, |app, cx| {
            app.file_history.lineage.as_mut().unwrap().error = None;
            cx.notify();
        });
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        draw(cx);
        for (reveal, scrollbar, _) in regions {
            let at = format!("13 pt in 1480 × 980, {scrollbar}");
            let scroll = reveal.scroll();
            assert_eq!(scroll.max_offset().y, px(0.), "{at}: fits");
            assert!(cx.debug_bounds(scrollbar).is_none(), "{at}: no scrollbar");
            assert_eq!(
                painted_fades_on(cx, scroll.bounds(), surface),
                (None, None),
                "{at}: no fade"
            );
        }
        cx.update(|_, cx| cx.set_reduce_motion(false));
    }

    /// File History's revisions cue their scroll as every scrolling list
    /// does (`tags::ScrollCue`): at 13 pt in a 1,480 × 980 window the
    /// fixture's revisions fit, draw no scrollbar and keep the list's whole
    /// width; with 30 revisions loaded they overflow, their scrollbar's
    /// thumb lies in the track at the list's right inside the ring's room,
    /// so the keyboard ring on the list's edge stays whole, and the rows end
    /// at the gutter before it. The list once scrolled with nothing to say
    /// so.
    #[gpui::test]
    async fn file_revisions_cue_their_scroll(cx: &mut TestAppContext) {
        use crate::tags::ScrollCue;
        use crate::tags::tests::painted_thumb;
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{point, px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| {
            crate::appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let scroll = app.read_with(cx, |app, _| {
            app.file_history.scroll.0.borrow().base_handle.clone()
        });
        let ring = cx.update(|_, cx| crate::appearance::button_ring_room(cx));
        let slack = px(0.5);
        let list = scroll.bounds();
        assert_eq!(
            scroll.max_offset().y,
            px(0.),
            "the fixture's revisions fit {list:?}"
        );
        assert!(
            cx.debug_bounds("file-revisions-scrollbar").is_none(),
            "no scrollbar where they fit"
        );
        let row = cx
            .debug_bounds("file-revision-0")
            .expect("the first revision");
        assert!(
            (row.right() - list.right()).abs() < slack,
            "{row:?} keeps the width of {list:?} where nothing scrolls"
        );
        app.update(cx, |app, cx| {
            let lineage = app.file_history.lineage.as_mut().unwrap();
            lineage.page = Some(page(lineage, 30, None));
            lineage.selected = Some(0);
            cx.notify();
        });
        draw(cx);
        let list = scroll.bounds();
        assert!(
            scroll.max_offset().y > px(0.),
            "30 revisions overflow {list:?}"
        );
        assert!(
            cx.debug_bounds("file-revisions-scrollbar").is_some(),
            "the revisions draw their scrollbar"
        );
        let column = list.dilate(-ring);
        let track = gpui_kit::Bounds::from_corners(
            point(
                column.right() - gpui_kit::base::Scrollbar::width(),
                column.top(),
            ),
            column.bottom_right(),
        );
        assert!(
            painted_thumb(cx, track).is_some(),
            "a thumb in {track:?}, the ring's {ring:?} kept inside {list:?}"
        );
        let row = cx
            .debug_bounds("file-revision-0")
            .expect("the first revision");
        assert!(
            row.right() <= list.right() - ring - ScrollCue::gutter() + slack,
            "{row:?} ends at the gutter of {list:?}"
        );
        cx.update(|_, cx| cx.set_reduce_motion(false));
    }

    /// File History's panel settles at every width from 500 to 560 px at
    /// 18 pt in a 490 px tall window: where Copy message's label fits the
    /// toolbar but Hash's does not fit beside it, their two labelled rows
    /// would leave the panel shorter than its parts, so they keep only their
    /// icons on one row, and the panel fits, draws no scrollbar and is laid
    /// out for the room it is drawn in. At 511 px the controls took their
    /// labels where the panel kept no gutter and their icons where it did,
    /// and the panel kept the gutter in every other frame: the window drew
    /// without end, which hung the test's draw.
    #[gpui::test]
    async fn file_historys_panel_settles_where_its_gutter_would_decide_the_copy_controls(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let reveals = app.read_with(cx, |app, _| app.file_history.reveals.clone());
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        let slack = px(0.5);
        for width in 500..=560 {
            let at = format!("18 pt in {width} × 490");
            cx.simulate_resize(size(px(width as f32), px(490.)));
            draw(cx);
            assert_eq!(
                reveals.panel.scroll().max_offset().y,
                px(0.),
                "{at}: the panel fits"
            );
            assert!(
                cx.debug_bounds("file-history-panel-scrollbar").is_none(),
                "{at}: no scrollbar"
            );
            let copy = cx.debug_bounds("copy-message").expect("Copy message");
            let hash = cx.debug_bounds("copy-commit").expect("Hash");
            assert!(
                (copy.top() - hash.top()).abs() <= slack,
                "{at}: Copy message {copy:?} and Hash {hash:?} share one row"
            );
            let room = app
                .read_with(cx, |app, _| app.inspector_message.details_room())
                .expect("a laid-out toolbar");
            let toolbar = cx
                .debug_bounds("commit-message-toolbar")
                .expect("the toolbar");
            let drawn = toolbar.size.width - cx.update(|window, _| window.rem_size()) * 1.5;
            assert!(
                (room - drawn).abs() <= px(1.),
                "{at}: laid out for {room:?}, drawn in {drawn:?}"
            );
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// The toolbars a test reads as laid out start with its last draw
    /// (`views::tests::draw`): at most the frames that draw drew, never an
    /// earlier draw's. Kept, the record grew by a frame for every frame any
    /// test on the thread drew.
    #[gpui::test]
    async fn the_laid_out_toolbars_start_with_each_draw(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        draw(cx);
        draw(cx);
        let frames = commit_message::State::take_laid_out_toolbars();
        assert!(
            !frames.is_empty() && frames.len() <= 3,
            "the toolbars of the last draw's three frames, not {} frames",
            frames.len()
        );
    }

    /// File History's message toolbar settles in one form for each of
    /// `sizes` at 18 pt, whatever the panel's last frame: Copy message and
    /// Hash lie where they do come to from the size before, from a 1,000 px
    /// tall window, where the panel fits, from a 400 px tall one, where it
    /// scrolls, and once a failed page's notice goes; the panel scrolls
    /// alike, and the toolbar is laid out for the room it is drawn in.
    /// Decided from the gutter the panel kept in its last frame, a panel that
    /// had scrolled kept their labels on two rows and its scrollbar at about
    /// 530 to 560 px wide in a 490 px tall window, where one that had fitted
    /// drew their icons and no scrollbar. Its cases are tests of their own
    /// (`file_historys_toolbar_settles_*`), each under about 15 s: the 123
    /// sizes took 31.9 s in one.
    async fn toolbar_settles(cx: &mut TestAppContext, sizes: &[(f32, f32)]) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let reveals = app.read_with(cx, |app, _| app.file_history.reveals.clone());
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        let fail = |cx: &mut VisualTestContext, failed: bool| {
            app.update(cx, |app, cx| {
                let lineage = app.file_history.lineage.as_mut().unwrap();
                lineage.error = failed.then(|| "The page could not be read.".into());
                cx.notify();
            });
        };
        let resize = |cx: &mut VisualTestContext, width: f32, height: f32| {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
        };
        // Where Copy message and Hash lie, whether the panel scrolls, and
        // whether the toolbar was laid out for the room it is drawn in.
        let form = |cx: &mut VisualTestContext| {
            let room = app
                .read_with(cx, |app, _| app.inspector_message.details_room())
                .expect("a laid-out toolbar");
            let toolbar = cx
                .debug_bounds("commit-message-toolbar")
                .expect("the toolbar");
            let drawn = toolbar.size.width - cx.update(|window, _| window.rem_size()) * 1.5;
            (
                cx.debug_bounds("copy-message"),
                cx.debug_bounds("copy-commit"),
                reveals.panel.scroll().max_offset().y > px(0.),
                (room - drawn).abs() <= px(1.),
            )
        };
        for &(width, height) in sizes {
            let at = format!("18 pt in {width} × {height}");
            resize(cx, width, height);
            let settled = form(cx);
            assert!(settled.3, "{at}: laid out for the room it is drawn in");
            for from in [1000., 400.] {
                resize(cx, width, from);
                resize(cx, width, height);
                assert_eq!(form(cx), settled, "{at}, from {from} px tall");
            }
            fail(cx, true);
            draw(cx);
            fail(cx, false);
            draw(cx);
            assert_eq!(form(cx), settled, "{at}, once a failed page's notice goes");
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// [`toolbar_settles`] at every width from 500 to 550 px in a 490 px
    /// tall window.
    #[gpui::test]
    async fn file_historys_toolbar_settles_at_500_to_550_wide(cx: &mut TestAppContext) {
        let sizes: Vec<_> = (500..=550).map(|width| (width as f32, 490.)).collect();
        toolbar_settles(cx, &sizes).await;
    }

    /// [`toolbar_settles`] at every width from 551 to 600 px in a 490 px
    /// tall window.
    #[gpui::test]
    async fn file_historys_toolbar_settles_at_551_to_600_wide(cx: &mut TestAppContext) {
        let sizes: Vec<_> = (551..=600).map(|width| (width as f32, 490.)).collect();
        toolbar_settles(cx, &sizes).await;
    }

    /// [`toolbar_settles`] at heights from 420 to 620 px, 20 px apart, at
    /// 511 and 540 px wide.
    #[gpui::test]
    async fn file_historys_toolbar_settles_from_420_to_620_tall(cx: &mut TestAppContext) {
        let sizes: Vec<_> = [511., 540.]
            .into_iter()
            .flat_map(|width| {
                (420..=620)
                    .step_by(20)
                    .map(move |height| (width, height as f32))
            })
            .collect();
        toolbar_settles(cx, &sizes).await;
    }

    /// Where File History's header and footer lay out each of their parts,
    /// and whether each scrolls, at `text` pt in each of `sizes`, reached
    /// from a 300 px tall window, where they scroll, and from a 1,000 px
    /// tall one, where they scroll less or fit (`header_and_footer_settle_*`):
    /// every pair is the same, and in those frames and the 1,000 px one each
    /// scrolls exactly where it keeps its cue's gutter, and one that fits
    /// ends at its last row.
    async fn header_and_footer_settle(cx: &mut TestAppContext, text: u8, sizes: &[(f32, f32)]) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        const PARTS: [&str; 13] = [
            "file-history-header",
            "file-history-top-row",
            "file-history-title",
            "close-file-history",
            "file-history-from",
            "copy-history-path",
            "newer-file-revision",
            "older-file-revision",
            "file-history-header-scrollbar",
            "file-history-footer",
            "newer-file-page",
            "older-file-page",
            "file-history-footer-scrollbar",
        ];
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let reveals = app.read_with(cx, |app, _| app.file_history.reveals.clone());
        cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
        let resize = |cx: &mut VisualTestContext, width: f32, height: f32| {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
        };
        let form = |cx: &mut VisualTestContext| {
            (
                PARTS.map(|part| cx.debug_bounds(part)),
                [&reveals.header, &reveals.footer]
                    .map(|region| region.scroll().max_offset().y > px(0.)),
            )
        };
        let measured =
            |cx: &mut VisualTestContext, at: &str| assert_regions_as_decided(&app, cx, at);
        for &(width, height) in sizes {
            let at = format!("{text} pt in {width} × {height}");
            resize(cx, width, 300.);
            resize(cx, width, height);
            let shorter = form(cx);
            resize(cx, width, 1000.);
            measured(cx, &format!("{text} pt in {width} × 1000"));
            resize(cx, width, height);
            let taller = form(cx);
            for (index, part) in PARTS.iter().enumerate() {
                assert_eq!(
                    shorter.0[index], taller.0[index],
                    "{at}: {part} reached from 300 px tall, then from 1,000 px tall"
                );
            }
            assert_eq!(
                shorter.1, taller.1,
                "{at}: whether the header and the footer scroll, from 300 px tall, then from 1,000"
            );
            measured(cx, &at);
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// File History's header and footer each scroll exactly where they were
    /// decided to (`Regions`), so each keeps its cue's gutter, and draws its
    /// scrollbar, only where its rows overflow it; one decided to fit ends at
    /// its last row and its padding, its rows as measured as tall as layout
    /// lays them out; the anchor's path keeps its whole line; a footer that
    /// scrolls keeps Newer page's and Older page's icons, and a panel that
    /// scrolls Copy message's and Hash's where their labels take two rows,
    /// so the shorter rows hold the more of what each shows.
    fn assert_regions_as_decided(
        app: &gpui_kit::Entity<GitTurtle>,
        cx: &mut gpui_kit::VisualTestContext,
        at: &str,
    ) {
        use gpui_kit::px;
        let pad = cx.update(|window, _| window.pixel_snap(window.rem_size() * 0.75));
        let (regions, reveals) = app.read_with(cx, |app, _| {
            (
                app.file_history.regions.get().expect("decided regions"),
                app.file_history.reveals.clone(),
            )
        });
        let path = cx
            .debug_bounds("file-history-anchor-path")
            .expect("the anchor's path");
        let line = cx.update(|window, _| text_line(12., window));
        assert!(
            path.size.height + px(0.5) >= line,
            "{at}: the anchor's path {path:?} keeps its whole {line:?} line"
        );
        let slack = px(0.5);
        let square = cx.update(|_, _| crate::appearance::ui_size(28.)) + slack;
        assert!(
            !(regions.footer_scrolls && regions.page_labels),
            "{at}: a footer decided to scroll keeps its page controls' icons, {regions:?}"
        );
        let older = cx.debug_bounds("older-file-page").expect("Older page");
        assert!(
            !(reveals.footer.scroll().max_offset().y > slack && older.size.width > square),
            "{at}: a scrolling footer draws Older page {older:?} as its icon"
        );
        let copy = cx.debug_bounds("copy-message").expect("Copy message");
        let hash = cx.debug_bounds("copy-commit").expect("Hash");
        assert!(
            !(reveals.panel.scroll().max_offset().y > slack
                && copy.size.width > square
                && (copy.top() - hash.top()).abs() > slack),
            "{at}: a scrolling panel keeps Copy message {copy:?} and Hash {hash:?} on one row"
        );
        // Labelled on two rows only where their icons would let neither the
        // header nor the footer hold more of its rows: one that scrolls
        // keeps its whole cap, 30% and 25% of the panel, which the icons'
        // shorter toolbar leaves as it is; below its cap, it gave way to the
        // body's least, which their icons lower.
        if copy.size.width > square && (copy.top() - hash.top()).abs() > slack {
            let tall = cx
                .debug_bounds("file-history-panel")
                .expect("the panel")
                .size
                .height;
            for (part, scrolls, height, cap) in [
                ("header", regions.header_scrolls, regions.header, 0.30),
                ("footer", regions.footer_scrolls, regions.footer, 0.25),
            ] {
                assert!(
                    !scrolls || height + slack >= tall * cap,
                    "{at}: Copy message {copy:?} and Hash {hash:?} keep their labels on two \
                     rows while the {part} scrolls below its cap in a {tall:?} panel, \
                     {regions:?}"
                );
            }
        }
        let ends = [
            ["older-file-revision", "file-history-revision-number"],
            ["older-file-page", "newer-file-page"],
        ];
        for ((region, scrolls), last) in [&reveals.header, &reveals.footer]
            .into_iter()
            .zip([regions.header_scrolls, regions.footer_scrolls])
            .zip(ends)
        {
            let scroll = region.scroll();
            let list = scroll.bounds();
            assert_eq!(
                scroll.max_offset().y > px(0.5),
                scrolls,
                "{at}: {list:?} scrolls {:?} exactly where it keeps its gutter, {regions:?}",
                scroll.max_offset().y
            );
            if !scrolls {
                let foot = last
                    .into_iter()
                    .filter_map(|part| cx.debug_bounds(part))
                    .map(|part| part.bottom())
                    .fold(px(0.), Pixels::max);
                assert!(
                    (list.bottom() - pad - foot).abs() < px(0.5),
                    "{at}: {list:?} ends at its last row, {foot:?}, and its padding"
                );
            }
        }
    }

    /// File History's header and footer scroll exactly where they were
    /// decided to (`assert_regions_as_decided`) at each of `texts` pt, from
    /// 424 to 664 px wide at 600 and 1,000 px tall
    /// (`file_history_regions_scroll_where_decided_*`).
    async fn regions_scroll_where_decided(cx: &mut TestAppContext, texts: &[u8]) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        for &text in texts {
            cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
            for width in (424..=664).step_by(8) {
                for height in [600., 1000.] {
                    cx.simulate_resize(size(px(width as f32), px(height)));
                    draw(cx);
                    assert_regions_as_decided(
                        &app,
                        cx,
                        &format!("{text} pt in {width} × {height}"),
                    );
                }
            }
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// File History's header and footer scroll exactly where they were
    /// decided to at 11 to 14 pt (`regions_scroll_where_decided`). The
    /// anchor's path, which clips its text, gave up its line to a header
    /// shorter than its rows: at 14 pt from 472 to 484 px wide in a 1,000 px
    /// tall window the header measured 238 px, was given about 226 and
    /// decided to scroll, and its path was squeezed from 21 to 8 px, so it
    /// kept a gutter with nothing to scroll and its rows narrowed for none.
    #[gpui::test]
    async fn file_history_regions_scroll_where_decided_at_11_to_14_pt(cx: &mut TestAppContext) {
        regions_scroll_where_decided(cx, &[11, 12, 13, 14]).await;
    }

    /// As at 11 to 14 pt
    /// (`file_history_regions_scroll_where_decided_at_11_to_14_pt`), at 15
    /// to 18 pt: at 15 pt from 532 to 632 px wide the path was squeezed from
    /// 22.5 to 9 px.
    #[gpui::test]
    async fn file_history_regions_scroll_where_decided_at_15_to_18_pt(cx: &mut TestAppContext) {
        regions_scroll_where_decided(cx, &[15, 16, 17, 18]).await;
    }

    /// File History's header and footer take each form and wrap from the
    /// room their rows have, never from whether they scrolled in their last
    /// frame (`header_and_footer_settle`): at 13 pt from 424 to 544 px wide
    /// at 400, 460 and 520 px tall. Decided from the gutter the footer kept,
    /// Newer page and Older page lay on two rows reached from a shorter
    /// window and on one from a taller one at 432 × 460 and from 464 to
    /// 520 × 460.
    #[gpui::test]
    async fn header_and_footer_settle_at_13_pt(cx: &mut TestAppContext) {
        let sizes: Vec<(f32, f32)> = [400., 460., 520.]
            .into_iter()
            .flat_map(|height| {
                (424..=544)
                    .step_by(8)
                    .map(move |width| (width as f32, height))
            })
            .collect();
        header_and_footer_settle(cx, 13, &sizes).await;
    }

    /// As at 13 pt (`header_and_footer_settle_at_13_pt`), at 18 pt from 461
    /// to 661 px wide at 420, 490 and 520 px tall. Decided from the gutter
    /// the footer kept, Newer page's and Older page's labels, on two rows
    /// that overflowed it, became one row of icons inside the gutter, which
    /// fitted, in every other frame, and the window drew without end.
    #[gpui::test]
    async fn header_and_footer_settle_at_18_pt(cx: &mut TestAppContext) {
        let sizes: Vec<(f32, f32)> = [420., 490., 520.]
            .into_iter()
            .flat_map(|height| {
                (461..=661)
                    .step_by(8)
                    .map(move |width| (width as f32, height))
            })
            .collect();
        header_and_footer_settle(cx, 18, &sizes).await;
    }

    /// File History's message toolbar takes its form from the height the
    /// window lays it out in, in the first frame a resize draws: at 18 pt, a
    /// one-step resize from 511 × 490 and from 520 × 490, where Copy message
    /// and Hash keep only their icons, to the shortest height, in 20 px
    /// steps from 520 px, where they keep their labels, draws them labelled
    /// in each frame it draws, the first included, with no frame drawn but
    /// the resize's own. Taken from the panel's last frame, the resize to
    /// 520 px, where the labels fitted the panel, kept their icons, and
    /// nothing drew another frame; there their icons now let the header
    /// hold more of its rows.
    #[gpui::test]
    async fn file_historys_toolbar_takes_its_form_from_the_height_a_resize_draws(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        let square = crate::appearance::ui_size(28.);
        let slack = px(0.5);
        let labelled = |cx: &mut VisualTestContext| {
            cx.debug_bounds("copy-message")
                .expect("Copy message")
                .size
                .width
                > square + slack
        };
        for width in [511., 520.] {
            let tall = (520..=1000)
                .step_by(20)
                .map(|height| height as f32)
                .find(|&height| {
                    cx.simulate_resize(size(px(width), px(height)));
                    draw(cx);
                    labelled(cx)
                })
                .expect("a height where Copy message keeps its label");
            cx.simulate_resize(size(px(width), px(490.)));
            draw(cx);
            assert!(
                !labelled(cx),
                "18 pt in {width} × 490: Copy message keeps its icon"
            );
            commit_message::State::take_laid_out_toolbars();
            crate::tags::settling::reset();
            // Only the frames the resize itself draws.
            cx.simulate_resize(size(px(width), px(tall)));
            let at = format!("18 pt from {width} × 490 to {width} × {tall}");
            let frames = commit_message::State::take_laid_out_toolbars();
            assert!(!frames.is_empty(), "{at}: the resize draws");
            assert!(
                frames.iter().all(|icons| !icons),
                "{at}: labelled in every frame the resize draws, its first included: {frames:?}"
            );
            let copy = cx.debug_bounds("copy-message").expect("Copy message");
            assert!(
                copy.size.width > square + slack,
                "{at}: Copy message {copy:?} labelled"
            );
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// The anchor's line keeps its whole line beside the anchor path's copy
    /// control, or the header's whole row where that is narrower, and below
    /// that the control moves beneath it, on the line's left edge: at 18 pt
    /// in a 461 × 490 window the line takes the header's row and the control
    /// stands below it, and at 18 pt in 1,200 × 980 and at 13 pt in
    /// 1,480 × 980 the line and the control share one row. Given no least,
    /// the line was 61 to 80 px wide beside the control at 18 pt in a 461 px
    /// window and wrapped to about 135 px.
    #[gpui::test]
    async fn the_anchor_line_keeps_its_least_width_beside_its_copy_control(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        let text = app.read_with(cx, |app, _| {
            let anchor = &app.file_history.lineage.as_ref().unwrap().anchor;
            format!("From {} · first-parent lineage", short_oid(anchor))
        });
        let slack = px(0.5);
        for (size_pt, width, height, beneath) in [
            (18, 461., 490., true),
            (18, 1200., 980., false),
            (13, 1480., 980., false),
        ] {
            let at = format!("{size_pt} pt in {width} × {height}");
            cx.update(|window, cx| crate::appearance::apply_text_sizes(size_pt, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let from = bounds(cx, "file-history-from");
            let copy = bounds(cx, "copy-history-path");
            // The top row above takes the header's whole row, as the
            // anchor's line's own row does.
            let row = bounds(cx, "file-history-top-row").size.width;
            let line = cx.update(|window, _| {
                crate::views::line_widths(&[(&text, crate::appearance::ui_text(10.))], window)
            });
            assert!(
                from.size.width + slack >= line.min(row),
                "{at}: the line {from:?} keeps {line:?} or the row's {row:?}"
            );
            if beneath {
                assert!(
                    copy.top() + slack >= from.bottom()
                        && (copy.left() - from.left()).abs() <= slack,
                    "{at}: the control {copy:?} beneath the line {from:?}, on its left edge"
                );
            } else {
                assert!(
                    copy.left() + slack >= from.right() && copy.top() < from.bottom(),
                    "{at}: the control {copy:?} beside the line {from:?}"
                );
            }
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// A failed page's notice leaves nothing in its reveal once no notice
    /// is drawn: a revision chosen ends the failure, and Retry page's focus
    /// handle and where the notice drew it go with the notice. Kept for the
    /// next failure, they stayed as long as File History did.
    #[gpui::test]
    async fn a_failed_pages_notice_leaves_no_reveal_behind(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        app.update(cx, |app, cx| {
            let lineage = app.file_history.lineage.as_mut().unwrap();
            lineage.error = Some("The page could not be read.".into());
            cx.notify();
        });
        draw(cx);
        let kept = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.file_history.reveals.failure.kept())
        };
        assert!(kept(cx) > 0, "the notice's Retry page is tracked");
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.move_file_history_revision(1, false, window, cx)
            })
        });
        settle(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("file-history-failure").is_none(),
            "the notice goes"
        );
        assert_eq!(kept(cx), 0, "the notice's reveal holds nothing");
    }

    /// Retry page, focused from the keyboard and pressed, goes as its notice
    /// does, the page read again, and focus moves to the revisions in the
    /// same step, so no frame draws focus on a control it does not draw;
    /// once the page lands, the notice's reveal holds no handle. Focus
    /// stayed on Retry page's handle, where no key reached anything until
    /// the page landed, or, where it landed empty, until a click.
    #[gpui::test]
    async fn retry_page_pressed_gives_focus_to_the_revisions(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{KeyDownEvent, KeyUpEvent, Keystroke, PlatformInput};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        app.update(cx, |app, cx| {
            let lineage = app.file_history.lineage.as_mut().unwrap();
            lineage.error = Some("The page could not be read.".into());
            cx.notify();
        });
        draw(cx);
        // Retry page by the keyboard, from Hash before it.
        let retry = app.read_with(cx, |app, cx| {
            app.file_history
                .reveals
                .failure
                .focus("retry-file-history", cx)
        });
        cx.update(|window, cx| retry.focus(window, cx));
        cx.simulate_keystrokes("shift-tab");
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            cx.update(|window, _| retry.is_focused(window)),
            "Retry page holds focus"
        );
        // Pressed: its handler runs, and the page's read has not answered.
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        let enter = Keystroke::parse("enter").unwrap();
        cx.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: enter.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke: enter }), cx);
        });
        let reading = app.read_with(cx, |app, _| {
            let lineage = app.file_history.lineage.as_ref().unwrap();
            lineage.pending.is_some() && lineage.error.is_none()
        });
        assert!(reading, "Retry page reads the page again");
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "the revisions take focus as Retry page goes"
        );
        first_page_lands(&app, cx);
        settle(&app, cx).await;
        assert!(cx.debug_bounds("retry-file-history").is_none(), "no notice");
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "the revisions"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.file_history.reveals.failure.kept()),
            0,
            "the notice's reveal holds no handle"
        );
        cx.update(|_, cx| cx.set_reduce_motion(false));
    }

    /// File history and Blame closed by Escape never leave focus on a
    /// control the restored page does not draw: opened from the narrow
    /// Compare's File actions menu at 18 pt in a 461 × 490 window, whose
    /// menu held focus as its item ran, each lands on the changed-file list,
    /// where the command palette's shortcut opens the palette; opened by
    /// the wide Compare's File history button, which no frame draws while
    /// File history is open, it lands on a drawn control. Focus stayed on
    /// the gone menu's or button's handle, where no key reached anything.
    #[gpui::test]
    async fn inspections_closed_from_gone_openers_land_on_a_drawn_control(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{Modifiers, px, size};
        let palette = if cfg!(target_os = "macos") {
            "cmd-shift-p"
        } else {
            "ctrl-shift-p"
        };
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|window, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
            cx.set_reduce_motion(true);
            crate::appearance::apply_text_sizes(18, 12, window, cx);
        });
        cx.simulate_resize(size(px(461.), px(490.)));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let (files, root) =
            app.read_with(cx, |app, _| (app.file_focus.clone(), app.app_focus.clone()));
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        for item in [
            "menu-item: File history… · MenuItem",
            "menu-item: Blame · MenuItem",
        ] {
            assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
            let trigger = cx
                .debug_bounds("file-actions-menu")
                .expect("the narrow Compare's File actions");
            cx.simulate_click(trigger.center(), Modifiers::default());
            draw(cx);
            let entry = cx
                .debug_bounds(item)
                .unwrap_or_else(|| panic!("File actions offers {item}"));
            cx.simulate_click(entry.center(), Modifiers::default());
            if item.contains("File history") {
                first_page_lands(&app, cx);
            } else {
                crate::blame::lands(&app, cx);
            }
            settle(&app, cx).await;
            assert!(app.read_with(cx, |app, _| app.file_history.is_active()
                || app.blame.is_visible()));
            cx.simulate_keystrokes("escape");
            settle(&app, cx).await;
            assert!(app.read_with(cx, |app, _| !app.file_history.is_active()
                && !app.blame.is_visible()
                && app.mode == WorkspaceMode::Compare));
            assert_eq!(focused(cx), Some(files.clone()), "{item}: the file list");
            cx.simulate_keystrokes(palette);
            draw(cx);
            assert!(
                cx.update(|window, cx| window.has_active_dialog(cx)),
                "{item}: the palette opens"
            );
            cx.update(|window, cx| window.close_all_dialogs(cx));
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(400));
            draw(cx);
        }
        // The wide Compare's File history button.
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let button = cx
            .debug_bounds("open-file-history")
            .expect("the wide Compare's File history button");
        cx.simulate_click(button.center(), Modifiers::default());
        first_page_lands(&app, cx);
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.file_history.is_active()));
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        let now = focused(cx).expect("a focused control");
        assert!(
            cx.update(|window, _| root.contains(&now, window)),
            "the window draws the control {now:?}"
        );
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }

    /// A failed page's message scrolls along a track that rests on its
    /// notice's removed background, as the pickers' tracks rest on their
    /// raised surface, and its thumb takes the thumb fitted to that surface
    /// (`Palette::removed_thumb`), 2:1 there; hovered, secondary text, as
    /// every thumb does: at 13 pt in a 1,480 × 980 window in Ember and
    /// Midnight, with a message taller than its 100 px. The kit's track was
    /// the canvas, a band across the notice, and the palette's thumb stood
    /// 1.66:1 on Ember's removed background and 1.74:1 on Midnight's.
    #[gpui::test]
    async fn a_failed_pages_message_scrolls_on_its_notices_surface(cx: &mut TestAppContext) {
        use crate::tags::tests::{painted_thumb, painted_thumb_fill, painted_track_fill};
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{Bounds, Hsla, point, px, rgb, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|window, cx| {
            cx.set_reduce_motion(true);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        app.update(cx, |app, cx| {
            let lineage = app.file_history.lineage.as_mut().unwrap();
            lineage.error = Some("The page could not be read. ".repeat(40));
            cx.notify();
        });
        let error = app.read_with(cx, |app, _| {
            app.file_history
                .lineage
                .as_ref()
                .unwrap()
                .error_scroll
                .clone()
        });
        let fill = |color: u32| Some(gpui_kit::Background::from(Hsla::from(rgb(color))));
        for choice in [
            crate::appearance::ThemeChoice::Ember,
            crate::appearance::ThemeChoice::Midnight,
        ] {
            cx.update(|_, cx| choice.apply(None, cx));
            draw(cx);
            let at = format!("{} at 13 pt in 1,480 × 980", choice.label());
            assert!(error.max_offset().y > px(0.), "{at}: the message scrolls");
            assert!(
                cx.debug_bounds("file-history-error-scrollbar").is_some(),
                "{at}: along its scrollbar"
            );
            let list = error.bounds();
            let track = Bounds::from_corners(
                point(
                    list.right() - gpui_kit::component::scroll::Scrollbar::width(),
                    list.top(),
                ),
                list.bottom_right(),
            );
            let (surface, thumb, shared, hovered) = cx.read(|cx| {
                let palette = crate::appearance::palette(cx);
                (
                    palette.removed_background,
                    crate::appearance::labels(cx).removed_thumb,
                    palette.scrollbar_thumb(),
                    palette.muted,
                )
            });
            assert_eq!(
                painted_track_fill(cx, track),
                fill(surface),
                "{at}: its track in the notice's #{surface:06x}"
            );
            assert_eq!(
                painted_thumb_fill(cx, track),
                fill(thumb),
                "{at}: its thumb in #{thumb:06x}, not the palette's #{shared:06x}"
            );
            assert_ne!(thumb, shared, "{at}: its thumb moves off the palette's");
            let center = painted_thumb(cx, track).expect("a thumb").center();
            cx.simulate_mouse_move(center, None, Modifiers::default());
            draw(cx);
            assert_eq!(
                painted_thumb_fill(cx, track),
                fill(hovered),
                "{at}: hovered, in secondary text #{hovered:06x}"
            );
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            draw(cx);
        }
        cx.update(|_, cx| cx.set_reduce_motion(false));
    }

    /// A failed page's message decides whether it scrolls from this frame's
    /// inputs (`failure_error_scrolls`): at 13 pt in a 540 px window from 480
    /// to 1,000 px tall, with a message that wraps to a line more inside its
    /// cue's gutter than without it and fits 100 px without it, the notice,
    /// its message and Retry page lie where they do reached from a 300 px
    /// tall window, where the message scrolls, and from a 1,000 px tall
    /// one, where it fits; the message scrolls exactly where decided; the
    /// notice holds its rows; and the message panel is as tall as decided
    /// for the frame (`commit_message::State::revision_panel_height`).
    /// Decided from its last frame, a 150-character message at 13 pt in
    /// 540 × 554 to 698 kept its gutter and scrolled at 100 px reached from
    /// 300 px tall and drew 90 px with no cue reached from 1,000 px, Retry
    /// page 10 px lower in one than the other.
    #[gpui::test]
    async fn a_failed_pages_message_scrolls_where_decided(cx: &mut TestAppContext) {
        use crate::views::tests::{
            draw, history_script, history_window_from, hold_quiet_reads_for_test, settle,
        };
        use gpui_kit::{px, size};
        let (_fixture, app, cx) = history_window_from(cx, history_script(4, 2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
        settle(&app, cx).await;
        open_landed(&app, cx).await;
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
        let width = 540.;
        let resize = |cx: &mut VisualTestContext, height: f32| {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
        };
        resize(cx, 1000.);
        // The longest message of whole words that fits 100 px without the
        // gutter and wraps to a line more inside it, measured from the
        // interface font, so the gutter alone would make it overflow.
        let room = app
            .read_with(cx, |app, _| app.inspector_message.details_room())
            .expect("a laid-out toolbar");
        let message = cx
            .update(|window, _| {
                let line = text_line(11., window);
                (1..60)
                    .map(|words| "unreadable ".repeat(words).trim_end().to_owned())
                    .rfind(|text| {
                        let lines = |room| wrapped(text, 11., room, window);
                        line * lines(room) <= px(100.)
                            && lines(room - crate::tags::ScrollCue::gutter()) > lines(room)
                    })
            })
            .expect("a message the gutter wraps to another line");
        app.update(cx, |app, cx| {
            app.file_history.lineage.as_mut().unwrap().error = Some(message.clone());
            cx.notify();
        });
        let (failure, error) = app.read_with(cx, |app, _| {
            (
                app.file_history.reveals.failure.scroll().clone(),
                app.file_history
                    .lineage
                    .as_ref()
                    .unwrap()
                    .error_scroll
                    .clone(),
            )
        });
        let slack = px(0.5);
        for height in (480..=1000).step_by(10) {
            let at = format!("13 pt in {width} × {height}");
            let mut seen = Vec::new();
            for from in [300., 1000.] {
                resize(cx, from);
                resize(cx, height as f32);
                seen.push((
                    [
                        "file-history-failure",
                        "file-history-error",
                        "retry-file-history",
                    ]
                    .map(|part| cx.debug_bounds(part)),
                    error.max_offset().y > slack,
                ));
                assert_eq!(
                    failure.max_offset().y,
                    px(0.),
                    "{at} from {from} px tall: the notice holds its rows"
                );
            }
            assert_eq!(
                seen[0], seen[1],
                "{at}: the notice, its message and Retry page from 300 px tall, then from 1,000"
            );
            let (regions, least, extent) = cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    let least = PanelLeast::of(
                        app.inspector_message.revision_message_least(),
                        true,
                        window,
                    );
                    let entry = app.file_history_entry().cloned().expect("a revision");
                    let extent = app.message_extent(&entry.commit, Some(&entry), window, cx);
                    (
                        app.file_history.regions.get().expect("decided regions"),
                        least,
                        extent,
                    )
                })
            });
            let panel = app
                .read_with(cx, |app, _| {
                    app.inspector_message
                        .revision_panel_height(extent, regions.body)
                })
                .expect("a laid-out toolbar");
            let drawn = cx
                .debug_bounds("commit-message-panel")
                .expect("the message panel")
                .size
                .height;
            // Within a pixel: layout rounds the header's and the footer's
            // heights, then the panel's edges, to device pixels.
            assert!(
                (drawn - panel).abs() <= px(1.),
                "{at}: the message panel drawn {drawn:?} as decided, {panel:?}"
            );
            // The room of this frame's toolbar, which a panel that scrolls
            // lays out inside its gutter.
            let room = app
                .read_with(cx, |app, _| app.inspector_message.details_room())
                .expect("a laid-out toolbar");
            let decided = cx.update(|window, _| {
                failure_error_scrolls(&message, &least, regions.body, panel, room, window)
            });
            assert_eq!(
                seen[1].1, decided,
                "{at}: the message scrolls exactly where decided, {:?}",
                seen[1].0
            );
        }
        cx.update(|window, cx| {
            cx.set_reduce_motion(false);
            crate::appearance::apply_text_sizes(13, 12, window, cx);
        });
    }
}
