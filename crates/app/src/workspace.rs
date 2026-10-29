use crate::*;
use gitturtle_core::{ChangeArea, ChangeStatus, StatusEntry, WriteCommand};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder;
use preferences::CommitDraft;
use std::cell::RefCell;

#[derive(Clone, Copy)]
pub(super) enum WorkingRow {
    Heading(ChangeArea, usize),
    Directory(usize, ChangeArea),
    File(usize, ChangeArea),
}

struct WorkingInspectorLayout {
    height: Pixels,
    /// The inspector's width as last laid out, which its rows fit
    /// (`WorkingForms`).
    width: Pixels,
    list_size: Option<Size<Pixels>>,
    /// The whole inspector's height as last laid out, which no form of its
    /// rows changes, so it can choose the form (`WorkingFit`).
    inspector: Pixels,
}

/// A Git write's outcome in the status line ([`GitTurtle::record_outcome`]),
/// held while the control that asked confirms it. Window-lived and never
/// retained by a tab: it is drawn only while its repository is shown, and
/// its timer ends it.
pub(super) struct StatusOutcome {
    repository: PathBuf,
    text: String,
    _expiry: Task<()>,
}

/// The Changes header's height in the full form.
const WORKING_HEADER: f32 = 44.;

/// What the Working list says where its path filter matches nothing, as
/// the Tags list says "No tags match this filter".
pub(super) const WORKING_NO_MATCH: &str = "No files match this filter";
/// [`WORKING_NO_MATCH`] where it would wrap, as the commit's changed files
/// say it: three lines at 18 pt in a half tile left no room for Clear
/// filter below them.
pub(super) const WORKING_NO_MATCH_SHORT: &str = "No matches";
/// The no-match state's action, which empties the filter.
const CLEAR_FILTER: &str = "Clear filter";

/// How the Working Changes inspector's rows fit its height, chosen before
/// layout from the inspector's measured height and the heights its rows
/// would take, so no row is cut and no frame shows one form and then the
/// other.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WorkingFit {
    /// The filter, the selection controls and the Changes header give way
    /// to one row: the filter beside Actions (and Refresh, where the filter
    /// keeps its room beside it), the selection controls moving into the
    /// Actions menu, so the file list keeps its floor.
    tight: bool,
    /// The composer keeps its pinned heading; in the tightest inspector it
    /// leaves, its branch staying in the commit action's tooltip and the
    /// action bar, its count in the Staged heading and the commit action.
    heading: bool,
}

/// The least height the composer keeps: its top border, the pinned heading
/// row with the padding above it (unless `heading` has left), the title's
/// label and one line of its field with the gaps around them, and the
/// commit action with the padding below it, each snapped to device pixels as
/// layout snaps them. The list/composer body keeps it and the file list's
/// floor (`working_list_floor`), so the controls above the list give way
/// first (`WorkingFit`).
fn composer_minimum(short: bool, heading: bool, window: &Window) -> f32 {
    f32::from(ComposerMetrics::new(short, heading, window).minimum())
}

/// The heights the composer's rows take, snapped to device pixels as layout
/// snaps them, from which its minimum (`composer_minimum`), its preferred
/// height, its Description section's field (`description_field`) and its
/// drawn height (`content`) are predicted before layout.
struct ComposerMetrics {
    /// The composer's top border.
    border: Pixels,
    /// The padding above the heading row (or, without it, above the fields)
    /// and below the commit action.
    padding: Pixels,
    /// Whether the pinned heading is drawn (`WorkingFit::heading`).
    heading: bool,
    /// The heading's line beside its 16 px icon and the staged count's
    /// badge; nothing where the heading has left.
    heading_row: Pixels,
    /// The gap between the fields' rows, which also leads them below the
    /// heading and ends them above the commit action, so its rows and the
    /// action keep one rhythm: a short composer's scaled 4 px, a taller one's
    /// 0.5 rem.
    gap: Pixels,
    /// A taller composer's branch line with the gap below it; nothing in a
    /// short one.
    branch: Pixels,
    /// A label's line: "Title", "Description".
    label: Pixels,
    /// The title's one-line field.
    field: Pixels,
    /// The commit action.
    action: Pixels,
    /// A small line's height: the hint's and a guidance note's.
    small: Pixels,
    /// A line of the description at its 12 px, whole device pixels or a
    /// little more, so a field of whole lines never cuts its last.
    text: Pixels,
}

impl ComposerMetrics {
    fn new(short: bool, heading: bool, window: &Window) -> Self {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let line = |size: Pixels| snap(window.text_style().line_height.to_pixels(size.into(), rem));
        let heading_row = if heading {
            line(crate::appearance::ui_text(13.))
                .max(px(16.))
                .max(snap(rem * 0.125) * 2. + line(crate::appearance::ui_text(10.)))
        } else {
            // Without it the fields lead with the composer's own padding.
            px(0.)
        };
        // The fields keep this gap from the heading, between themselves and
        // above the commit action: a scaled 4 px in a short composer, 0.5 rem
        // in a taller one, whose fields lead with the branch line.
        let gap = snap(if short {
            crate::appearance::ui_size(4.)
        } else {
            rem * 0.5
        });
        let branch = if short {
            px(0.)
        } else {
            line(crate::appearance::ui_text(11.)).max(px(13.)) + gap
        };
        Self {
            border: snap(px(1.)),
            padding: snap(rem * if short { 0.5 } else { 0.75 }),
            heading,
            heading_row,
            gap,
            branch,
            label: line(crate::appearance::ui_text(11.)),
            field: snap(rem * 2.),
            action: snap(crate::appearance::ui_size(36.)),
            small: line(crate::appearance::ui_text(10.)),
            text: {
                let scale = window.scale_factor();
                let text = window
                    .text_style()
                    .line_height
                    .to_pixels(crate::appearance::ui_text(12.).into(), rem);
                px((f32::from(text) * scale).ceil() / scale)
            },
        }
    }

    /// The composer's least height: its border, the heading row with the
    /// padding above it, the title's label and one line of its field with
    /// the gaps around them, and the commit action with the padding below.
    fn minimum(&self) -> Pixels {
        // Without the heading the composer's padding leads the fields.
        let lead = if self.heading { self.gap } else { px(0.) };
        let fields = lead + self.branch + self.label + self.gap + self.field + self.gap;
        self.border + self.padding + self.heading_row + fields + self.action + self.padding
    }

    /// The composer drawn with its rows whole and nothing more: its chrome,
    /// the fields' `rows` (`GitTurtle::composer_rows`) and the description's
    /// `field`, or, where the Description section has left, those rows less
    /// the section's own (`GitTurtle::description_section`).
    fn content(&self, rows: Pixels, section: Pixels, field: Option<f32>) -> Pixels {
        self.chrome() + field.map_or(rows - section, |field| rows + px(field))
    }

    /// The scrolling fields' box in a composer `height` tall: what its
    /// border, the pinned heading and the commit action leave.
    fn fields_box(&self, height: Pixels) -> Pixels {
        height - self.chrome()
    }

    /// The fields' rows through the title's field, with the padding that
    /// leads them: the gap below the heading, or without it the composer's
    /// padding.
    fn title_rows(&self) -> Pixels {
        let lead = if self.heading { self.gap } else { self.padding };
        lead + self.branch + self.label + self.gap + self.field
    }

    /// What the composer takes beside its scrolling fields: its border, the
    /// pinned heading and the commit action.
    fn chrome(&self) -> Pixels {
        let heading = if self.heading {
            self.padding + self.heading_row
        } else {
            px(0.)
        };
        self.border + heading + self.action + self.padding
    }

    /// The description's least field: one line inside its 8 px above and
    /// below.
    fn description_least(&self) -> Pixels {
        px(16.) + self.text
    }

    /// The description's field in a composer `height` tall whose fields'
    /// other rows take `rows` (`GitTurtle::composer_rows`): `preferred`
    /// where it fits, else the whole lines that fit, so its Description
    /// section is whole. `None` where not even one line fits, so the section
    /// leaves whole, out of the Tab order and the accessibility tree, rather
    /// than show its label cut at the commit action or a field drawn past it.
    fn description_field(
        &self,
        height: Pixels,
        rows: Pixels,
        preferred: f32,
        window: &Window,
    ) -> Option<f32> {
        let room = self.fields_box(height) - rows;
        if room >= px(preferred) {
            return Some(preferred);
        }
        let lines = ((room - px(16.)) / self.text).floor();
        let field = px(16.) + self.text * lines;
        (lines >= 1.).then(|| f32::from(window.pixel_snap(field).min(room)))
    }
}

/// How many of the file list's rows it keeps whole whatever else gives way:
/// those through its first file row, so the shortest list shows a file and
/// not only the headings above it (Staged, Unstaged while nothing is staged,
/// and a directory's label when grouped). A list without files keeps one.
fn working_floor_rows(rows: &[WorkingRow]) -> usize {
    rows.iter()
        .take(4)
        .position(|row| matches!(row, WorkingRow::File(..)))
        .map_or(1, |index| index + 1)
}

/// The file list's least height: `rows` whole rows inside its border.
fn working_list_floor(rows: usize, density: crate::appearance::Density, window: &Window) -> f32 {
    f32::from(
        window.pixel_snap(px(density.file_row_height())) * rows as f32
            + window.pixel_snap(px(1.)) * 2.,
    )
}

/// The width the file list's no-match state has in an inspector `width`
/// wide: the list stands 1 px in from the inspector's divider, and the state
/// keeps the rows' text inset inside the list's border.
fn working_no_match_room(width: Pixels, window: &Window) -> Pixels {
    width - px(1.) - window.pixel_snap(px(1.)) * 2. - window.rem_size() * 0.75 * 2.
}

/// The no-match line the file list draws in `room`: the whole line where it
/// fits on one line, otherwise its short form, which the whole line names.
fn working_no_match_line(room: Pixels, window: &Window) -> &'static str {
    if wrapped_lines(WORKING_NO_MATCH, empty_state::line_text(), room, window) == 1 {
        WORKING_NO_MATCH
    } else {
        WORKING_NO_MATCH_SHORT
    }
}

/// The file list's least height while it says that its path filter matches
/// nothing, in an inspector `width` wide: the line, wrapped in its room, and
/// Clear filter below it, inside the state's padding and the list's border,
/// so the state is drawn whole wherever the list keeps its floor.
fn working_no_match_floor(width: Pixels, window: &Window) -> f32 {
    let room = working_no_match_room(width, window);
    let line = working_no_match_line(room, window);
    let lines = wrapped_lines(line, empty_state::line_text(), room, window);
    f32::from(
        empty_state::block_height(lines, true, window)
            + appearance::ui_size(empty_state::GAP / 2.) * 2.
            + window.pixel_snap(px(1.)) * 2.,
    )
}

/// How the Working Changes inspector's rows fit its width, measured from the
/// interface font at the interface size. Where a row's labelled form fits,
/// it is drawn as before; narrower, its controls keep only their icons, with
/// their accessible names and tooltips that lead with those names, and its
/// texts leave whole rather than show a lone ellipsis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WorkingForms {
    /// Directories, Stage and Unstage as 28 px icon controls on one row,
    /// where Stage and Unstage do not fit a row of their own labelled.
    pub(super) compact_selection: bool,
    /// The rows Directories, Stage and Unstage take: one, or two where the
    /// labelled pair wraps below Directories.
    selection_rows: usize,
    /// In the tight form's one row, Refresh stays beside Actions where the
    /// filter keeps a scaled 80 px beside both; otherwise it joins the
    /// Actions menu.
    toolbar_refresh: bool,
    /// The Changes header's Actions keeps only its caret, beside the icon-only
    /// Refresh, and "Refreshing…" leaves.
    compact_actions: bool,
    /// The Changes header's icon, then "Changes" with its count, each whole
    /// or absent: the icon leaves first, then the label and the count
    /// together, since the Changes tab or toggle in the header above already
    /// reads the count.
    header_icon: bool,
    header_label: bool,
    /// The Staged and Unstaged headings, fitted together so both keep one
    /// form: Unstage all and Stage all keep only their icons first, then the
    /// leading glyph leaves, then the group's name, the glyph returning beside
    /// the count to tell the groups apart, and last the glyph again. The count
    /// stays, naming its group in its tooltip once the name has left.
    section_action_label: bool,
    section_icon: bool,
    section_label: bool,
    /// A file row's status word, its 26 px status tile or, in its place,
    /// the tile's bare 16 px glyph, and its Stage or Unstage control, in the
    /// first of `FILE_FORMS` that keeps a short name such as "README.md"
    /// whole beside them, on every row together: the word leaves first, the
    /// tile gives way to its glyph before the control leaves, and the glyph
    /// leaves last. The word joins the path's tooltip, the action the row's
    /// menu; the status stays in the tooltip and the row's accessible name.
    file_status: bool,
    file_tile: bool,
    file_action: bool,
    file_glyph: bool,
    /// The short composer heading's icon, then its staged badge, leave where
    /// the heading beside them could not keep "Commit to" and the branch's
    /// first character and ellipsis, so it truncates only the branch.
    heading_icon: bool,
    heading_badge: bool,
    /// The title counter keeps "suggested" after its count where the line
    /// beside "Title" has room; without it the word is in its tooltip and
    /// accessible name.
    counter_word: bool,
    /// The description's label keeps "· optional" after "Description" where
    /// they share the line; without it the field's accessible name keeps
    /// the word ("Commit description, optional").
    description_note: bool,
    /// The commit action reads "Commit" where its label does not fit beside
    /// its icon, with that label as its accessible name and leading its
    /// tooltip.
    compact_commit: bool,
}

/// The filter's, the title's and the description's placeholders, of which
/// each field shows as much as fits its width (`placeholder_fit`).
pub(super) const FILTER_PLACEHOLDER: &str = "Filter working paths…";
/// The filter's placeholder where the whole one does not fit: a shorter
/// whole word reads better than a cut one ("Filte…"). The History
/// inspector's changed-file filter shortens to it too.
pub(super) const FILTER_SHORT_PLACEHOLDER: &str = "Filter";
/// The filter's accessible name, whole whatever form its placeholder takes,
/// so a screen reader never hears only "Filter".
pub(super) const FILTER_NAME: &str = "Filter working paths";
pub(super) const TITLE_PLACEHOLDER: &str = "Summarize the change";
/// The title's placeholder where the whole one does not fit, rather than
/// "Summarize t…".
pub(super) const TITLE_SHORT_PLACEHOLDER: &str = "Summarize";
pub(super) const DESCRIPTION_PLACEHOLDER: &str = "Explain why this change is needed…";

/// The composer's texts below its title, whose wrapped lines the
/// composer's rows count (`GitTurtle::composer_rows`).
const LONG_TITLE_NOTE: &str = "A shorter title is easier to scan. This title is still valid.";
/// How long a busy spell runs before it looks busy (`GitTurtle::busy_shown`).
pub(crate) const BUSY_LOOK_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// The look of a busy spell: the progress label, Cancel, the tab's
/// "· working", "Refreshing…" and the disabled styling of the controls a
/// write or the reads after it make unavailable. It waits
/// [`BUSY_LOOK_DELAY`], so a write that lands with its rows sooner never
/// flashes it on controls the person did not touch. Acceptance does not
/// wait: those controls refuse at once (`write_from`, `depart_working_row`
/// and their own handlers).
#[derive(Default)]
pub(crate) struct BusyLook {
    shown: bool,
    /// Shows the look once the delay passes; dropping it cancels that.
    reveal: Option<Task<()>>,
}

const UNSTAGED_HINT: &str = "Stage files to include in your commit";
const STAGED_HINT: &str = "Only staged changes will be committed";
const DRAFT_SAVE_NOTE: &str =
    "Draft could not be saved on this computer. Keep the app open and retry.";
const CONFLICT_NOTE: &str = "Resolve conflicted files before committing.";

thread_local! {
    static SELECTION_LABELS: crate::views::TextWidth = const { RefCell::new(None) };
    static CHANGES_COUNT: crate::views::TextWidth = const { RefCell::new(None) };
    static STAGED_BADGE: crate::views::TextWidth = const { RefCell::new(None) };
    static HEADING_MIN: crate::views::TextWidth = const { RefCell::new(None) };
    static CHANGES_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static ACTIONS_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static REFRESHING_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static DIRECTORIES_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    /// The wrapped lines of the selection controls' texts, by text, size and
    /// width, so a steady render lays none of them out.
    static WRAPPED_LINES: RefCell<Vec<WrappedText>> = const { RefCell::new(Vec::new()) };
    /// "Staged", "Unstaged", "Unstage all" and "Stage all".
    static SECTION_LABELS: RefCell<Option<(crate::views::LineKey, [Pixels; 4])>> =
        const { RefCell::new(None) };
    static STAGED_HEADING_COUNT: crate::views::TextWidth = const { RefCell::new(None) };
    static UNSTAGED_HEADING_COUNT: crate::views::TextWidth = const { RefCell::new(None) };
    /// A file row's name floor and its widest status word.
    static FILE_ROW_TEXTS: RefCell<Option<(crate::views::LineKey, [Pixels; 2])>> =
        const { RefCell::new(None) };
    static TITLE_NAME: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static TITLE_COUNTER: crate::views::TextWidth = const { RefCell::new(None) };
    /// "Description" and "· optional".
    static DESCRIPTION_LABEL: RefCell<Option<(crate::views::LineKey, Pixels)>> =
        const { RefCell::new(None) };
    static COMMIT_LABEL: crate::views::TextWidth = const { RefCell::new(None) };
    static CLEAR_FILTER_LABEL: crate::views::TextWidth = const { RefCell::new(None) };
    /// Each placeholder as it fits the width its field last painted at.
    static TITLE_FIT: PlaceholderFit = const { RefCell::new(None) };
    static DESCRIPTION_FIT: PlaceholderFit = const { RefCell::new(None) };
    static FILTER_FIT: PlaceholderFit = const { RefCell::new(None) };
}

/// A text's wrapped line count, with what it was laid out with: the font and
/// sizes, the text, its size and the width it wrapped in.
type WrappedText = (crate::views::LineKey, String, Pixels, Pixels, usize);

/// The lines `text` at `size` wraps to in `width`, laid out again only when
/// the text, the font, the sizes or the width change.
fn wrapped_lines(text: &str, size: Pixels, width: Pixels, window: &Window) -> usize {
    let key = crate::views::LineKey::current(window);
    WRAPPED_LINES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&(.., lines)) = cache
            .iter()
            .find(|(k, t, s, w, _)| *k == key && t == text && *s == size && *w == width)
        {
            return lines;
        }
        #[cfg(test)]
        crate::views::LINE_LAYOUTS.with(|count| count.set(count.get() + 1));
        let style = window.text_style();
        let lines = window
            .text_system()
            .shape_text(
                SharedString::from(text.to_owned()),
                size,
                &[style.to_run(text.len())],
                Some(width),
                None,
            )
            .map_or(1, |lines| {
                lines
                    .iter()
                    .map(|line| line.wrap_boundaries.len() + 1)
                    .sum()
            });
        if cache.len() >= 8 {
            cache.remove(0);
        }
        cache.push((key, text.to_owned(), size, width, lines));
        lines
    })
}

/// A placeholder as it fits a field's width, with the font and sizes and the
/// width it was fitted to.
pub(super) type PlaceholderFit = RefCell<Option<(crate::views::LineKey, Pixels, SharedString)>>;

/// The status words a file row shows (`render_working_row`).
const FILE_STATUS_WORDS: [&str; 6] = ["Conflict", "New", "Modified", "Deleted", "Renamed", "Type"];

/// The name a file row keeps whole at least, whatever leaves beside it: a
/// short name that nearly every repository's root holds, its capitals wider
/// than a lowercase name such as "notes.txt".
const FILE_NAME_FLOOR: &str = "README.md";

/// A file row's forms, widest first, as (status word, status tile, Stage or
/// Unstage control, bare status glyph): see `WorkingForms::file_status`.
/// The word leaves first; then the tile gives way to its glyph, so the
/// control stays; then the control leaves, and the tile returns where it
/// fits without it; last the glyph leaves.
const FILE_FORMS: [(bool, bool, bool, bool); 6] = [
    (true, true, true, false),
    (false, true, true, false),
    (false, false, true, true),
    (false, true, false, false),
    (false, false, false, true),
    (false, false, false, false),
];

/// The Staged and Unstaged headings' forms, widest first, as
/// (labelled action, glyph, name): see `WorkingForms::section_action_label`.
const SECTION_FORMS: [(bool, bool, bool); 5] = [
    (true, true, true),
    (false, true, true),
    (false, false, true),
    (false, true, false),
    (false, false, false),
];

impl GitTurtle {
    /// `WorkingForms` for an inspector `width` wide. Each text is laid out
    /// again only when it, the font or the sizes change, so a steady render
    /// asks the text system nothing. `short` is the composer's short form,
    /// whose heading names the branch.
    fn working_forms(&self, width: Pixels, short: bool, window: &Window, cx: &App) -> WorkingForms {
        use crate::appearance::{ui_size, ui_text};
        use crate::views::{LineKey, cached_text_width, cached_width, shaped_width};
        let key = LineKey::current(window);
        let rem = window.rem_size();
        let regular = window.text_style().font_weight;
        let measure = |text: &str, size: Pixels, weight: FontWeight| {
            shaped_width(text, size, weight, window).ceil()
        };
        let (gap, tight) = (rem * 0.5, rem * 0.25);
        // Every row's `px_3`, the inspector's one leading inset.
        let room = width - rem * 0.75 * 2.;
        let control = ui_size(28.);
        let sum = |items: &[Pixels], gap: Pixels| {
            items.iter().copied().sum::<Pixels>() + gap * items.len().saturating_sub(1) as f32
        };

        // Directories, Stage and Unstage. Stage and Unstage wrap together,
        // below Directories, before they keep only their icons.
        let (staged, unstaged) = self.selection_counts();
        let (stage, unstage) = (format!("Stage {unstaged}"), format!("Unstage {staged}"));
        let pair = cached_text_width(
            &SELECTION_LABELS,
            &format!("{stage}\n{unstage}"),
            window,
            || measure(&stage, ui_text(12.), regular) + measure(&unstage, ui_text(12.), regular),
        );
        let compact_selection = ui_size(10.) * 4. + pair + tight > room;
        let directories = cached_width(&DIRECTORIES_LABEL, &key, || {
            measure("Directories", ui_text(12.), regular)
        }) + ui_size(10.) * 2.;
        let selection_rows = if compact_selection
            || directories + tight + ui_size(10.) * 4. + pair + tight <= room
        {
            1
        } else {
            2
        };
        // The tight row: the filter, then Actions' caret and Refresh, each a
        // 28 px control a `gap_1` apart.
        let toolbar_refresh = room - (control + tight) * 2. >= ui_size(80.);

        // The Changes header: its icon, label and count, then Actions and
        // Refresh at the trailing edge.
        let total = self
            .work_status
            .as_ref()
            .map_or(0, |status| status.entries.len());
        let count = total.to_string();
        let chip = rem * 0.5 * 2.
            + cached_text_width(&CHANGES_COUNT, &count, window, || {
                measure(&count, ui_text(11.), FontWeight::MEDIUM)
            });
        let label = cached_width(&CHANGES_LABEL, &key, || {
            measure("Changes", ui_text(13.), FontWeight::SEMIBOLD)
        });
        let icon = px(17.);
        // The labelled compact helper: its padding either side, its label,
        // its 6 px gap and the caret, 0.875 rem square.
        let actions = (cached_width(&ACTIONS_LABEL, &key, || {
            shaped_width("Actions", ui_text(12.), regular, window)
        }) + ui_size(10.) * 2.
            + ui_size(6.)
            + rem * 0.875)
            .ceil();
        let refreshing = if self.status_task.is_some() {
            cached_width(&REFRESHING_LABEL, &key, || {
                measure("Refreshing…", ui_text(10.), regular)
            }) + gap
        } else {
            px(0.)
        };
        let full = sum(&[icon, label, chip], gap) + gap + refreshing + actions + gap + control;
        let compact_actions = full > room;
        let (mut header_icon, mut header_label) = (true, true);
        if compact_actions {
            let trailing = control * 2. + tight;
            let fits = |items: &[Pixels]| sum(items, gap) + gap + trailing <= room;
            if !fits(&[icon, label, chip]) {
                header_icon = false;
                header_label = fits(&[label, chip]);
            }
        }

        // The Staged and Unstaged headings, inside the file list's border and
        // its 1 px margin: the glyph, the group's name and count, and Unstage
        // all or Stage all at the trailing edge. Both take the first form in
        // which each fits.
        let (mut section_action_label, mut section_icon, mut section_label) = (true, true, true);
        if let Some((staged_rows, unstaged_rows)) = self.section_counts() {
            let [staged_name, unstaged_name, unstage_all, stage_all] =
                cached_width(&SECTION_LABELS, &key, || {
                    let name = |text| measure(text, ui_text(12.), FontWeight::SEMIBOLD);
                    let action = |text| measure(text, ui_text(12.), regular);
                    [
                        name("Staged"),
                        name("Unstaged"),
                        action("Unstage all"),
                        action("Stage all"),
                    ]
                });
            let count = |cache, rows: usize| {
                let text = rows.to_string();
                cached_text_width(cache, &text, window, || {
                    measure(&text, ui_text(11.), regular)
                })
            };
            // The labelled helper: its padding either side, icon and gap.
            let labelled = |text: Pixels| ui_size(10.) * 2. + ui_size(16.) + ui_size(6.) + text;
            let rows = [
                (
                    staged_name,
                    count(&STAGED_HEADING_COUNT, staged_rows),
                    labelled(unstage_all),
                ),
                (
                    unstaged_name,
                    count(&UNSTAGED_HEADING_COUNT, unstaged_rows),
                    labelled(stage_all),
                ),
            ];
            let section_room = room - px(3.);
            let fits = |(action_label, glyph, label): (bool, bool, bool)| {
                rows.iter().all(|&(name, count, action)| {
                    let glyph = if glyph { px(15.) + gap } else { px(0.) };
                    let name = if label { name + gap } else { px(0.) };
                    glyph + name + count + gap + if action_label { action } else { control }
                        <= section_room
                })
            };
            (section_action_label, section_icon, section_label) = SECTION_FORMS
                .into_iter()
                .find(|&form| fits(form))
                .unwrap_or((false, false, false));
        }

        // A file row, inside the list's border and its 1 px margin: `pl` 10
        // px with the 2 px selection edge, the 26 px status tile, the name,
        // then the status word and the Stage or Unstage control, and `pr_3`.
        // So the name keeps its floor, the row takes the first of
        // `FILE_FORMS` that fits: decoration gives way before an action,
        // and the name, the content, last.
        let [name_floor, status_word] = cached_width(&FILE_ROW_TEXTS, &key, || {
            let widest = FILE_STATUS_WORDS
                .iter()
                .map(|word| measure(word, ui_text(10.), regular))
                .fold(px(0.), Pixels::max);
            [
                measure(FILE_NAME_FLOOR, ui_text(12.), FontWeight::MEDIUM),
                widest,
            ]
        });
        let file_room = width - px(3.) - px(12.) - rem * 0.75;
        let fits = |(word, tile, action, glyph): (bool, bool, bool, bool)| {
            let lead = if tile {
                px(26.) + gap
            } else if glyph {
                px(16.) + gap
            } else {
                px(0.)
            };
            let word = if word { gap + status_word } else { px(0.) };
            let action = if action { gap + control } else { px(0.) };
            lead + name_floor + word + action <= file_room
        };
        let (file_status, file_tile, file_action, file_glyph) = FILE_FORMS
            .into_iter()
            .find(|&form| fits(form))
            .unwrap_or((false, false, false, false));

        // The composer heading: "Commit to" and at least the branch's first
        // character and ellipsis, a truncated line keeping the characters
        // that fit strictly before its ellipsis, as `scope_name_min_width`
        // measures a cut scope name.
        let (mut heading_icon, mut heading_badge) = (true, true);
        if short {
            let branch = self
                .work_status
                .as_ref()
                .and_then(|s| s.branch.as_deref())
                .unwrap_or("detached HEAD");
            let heading = format!("Commit to {branch}");
            let heading_min = cached_text_width(&HEADING_MIN, &heading, window, || {
                let size = ui_text(13.);
                let whole = shaped_width(&heading, size, FontWeight::SEMIBOLD, window).ceil();
                branch.chars().next().map_or(whole, |first| {
                    let cut = format!("Commit to {first}…");
                    (shaped_width(&cut, size, FontWeight::SEMIBOLD, window).floor() + px(1.))
                        .min(whole)
                })
            });
            let text = format!("{} staged", self.working_paths.staged_count);
            let badge = rem * 0.375 * 2.
                + cached_text_width(&STAGED_BADGE, &text, window, || {
                    measure(&text, ui_text(10.), FontWeight::MEDIUM)
                });
            if sum(&[px(16.), heading_min, badge], gap) > room {
                heading_icon = false;
                heading_badge = sum(&[heading_min, badge], gap) <= room;
            }
        }

        // The title's line: "Title", then its counter at the trailing edge.
        let title = cached_width(&TITLE_NAME, &key, || {
            measure("Title", ui_text(11.), FontWeight::MEDIUM)
        });
        let counter = title_counter(self.commit_title.read(cx).value().chars().count());
        let counter = cached_text_width(&TITLE_COUNTER, &counter, window, || {
            measure(&counter, ui_text(11.), regular)
        });
        let counter_word = title + gap + counter <= room;
        // The description's label: "Description", `gap_1`, "· optional".
        let description_note = cached_width(&DESCRIPTION_LABEL, &key, || {
            measure("Description", ui_text(11.), FontWeight::MEDIUM)
                + tight
                + measure("· optional", ui_text(11.), regular)
        }) <= room;

        // The commit action, as wide as the composer: the kit's Button at its
        // default size, `px_2p5` either side, its 16 px icon, `gap_2` and its
        // label at 1 rem, inside the 1 px edge a disabled action keeps.
        let label = self.commit_action_label(cx);
        let label = cached_text_width(&COMMIT_LABEL, &label, window, || {
            measure(&label, rem, regular)
        });
        let compact_commit = rem * 0.625 * 2. + px(16.) + gap + label + px(2.) > room;
        WorkingForms {
            compact_selection,
            selection_rows,
            toolbar_refresh,
            compact_actions,
            header_icon,
            header_label,
            section_action_label,
            section_icon,
            section_label,
            file_status,
            file_tile,
            file_action,
            file_glyph,
            heading_icon,
            heading_badge,
            counter_word,
            description_note,
            compact_commit,
        }
    }

    /// The Changes filter's placeholder fitted to its field, for the field's
    /// positioned wrapper in either form: the one row of the tight form,
    /// where the field can be 86 px at 18 pt, or its own row above the
    /// selection controls.
    pub(super) fn fit_filter_placeholder(&self) -> impl IntoElement {
        placeholder_fit(
            self.working_filter.clone(),
            &FILTER_FIT,
            &[FILTER_PLACEHOLDER, FILTER_SHORT_PLACEHOLDER],
            12.,
        )
    }

    /// The commit action's label: what it commits, or why it cannot.
    pub(super) fn commit_action_label(&self, cx: &App) -> String {
        let staged = self.working_paths.staged_count;
        if self.operation_busy.is_some() {
            "Working…".to_owned()
        } else if self.work_status.is_none()
            || self.working_filter_waiting()
            || self.working_paths.error.is_some()
        {
            // Rows kept while new ones are prepared keep their counts, and
            // the action keeps its label (disabled until they land).
            if self.working_filter_waiting() {
                "Filtering working paths…"
            } else if self.status_task.is_some() {
                "Reading working changes…"
            } else {
                "Changes unavailable"
            }
            .to_owned()
        } else if self.working_paths.conflicted {
            "Resolve conflicts to commit".to_owned()
        } else if !self
            .profile
            .as_ref()
            .is_some_and(|p| !p.name.is_empty() && !p.email.is_empty())
        {
            "Set your commit identity".to_owned()
        } else if staged == 0 {
            "Stage files to commit".to_owned()
        } else if self.commit_title.read(cx).value().trim().is_empty() {
            "Write a commit title".to_owned()
        } else {
            format!(
                "Commit {staged} {}",
                if staged == 1 { "file" } else { "files" }
            )
        }
    }

    /// The full selection controls' height at the inspector's `width`, as
    /// `render_working_selection` lays them out: its padding, the filter's
    /// field, the selection rows, the selection count's wrapped lines and any
    /// filtering notice, each a `gap_1` apart.
    fn working_controls_height(&self, forms: WorkingForms, width: Pixels, window: &Window) -> f32 {
        use crate::appearance::{ui_size, ui_text};
        let rem = window.rem_size();
        let room = width - rem * 0.75 * 2.;
        let gap = rem * 0.25;
        let line = |size: Pixels| window.text_style().line_height.to_pixels(size.into(), rem);
        let rows = forms.selection_rows as f32;
        let mut height = rem * 0.5 * 2. + rem * 2. + gap + ui_size(28.) * rows + gap * (rows - 1.);
        let (staged, unstaged) = self.selection_counts();
        let count = format!(
            "{} selected · filtering clears selection",
            staged + unstaged
        );
        let mut texts = vec![(count, ui_text(10.))];
        if self.working_filter_waiting() {
            texts.push(("Filtering working paths…".into(), ui_text(11.)));
        }
        if let Some(error) = &self.working_paths.error {
            texts.push((error.clone(), ui_text(11.)));
        }
        for (text, size) in texts {
            height += gap + line(size) * wrapped_lines(&text, size, room, window) as f32;
        }
        f32::from(height)
    }

    /// The inspector's `WorkingFit` at its measured `height` and `width`,
    /// with a list floor of `floor` and, where `composer`, the composer.
    #[allow(clippy::too_many_arguments)]
    fn working_fit(
        &self,
        forms: WorkingForms,
        height: Pixels,
        width: Pixels,
        floor: f32,
        composer: bool,
        short: bool,
        window: &Window,
    ) -> WorkingFit {
        let height = f32::from(height);
        let body = |heading: bool| {
            floor
                + if composer {
                    composer_minimum(short, heading, window)
                } else {
                    0.
                }
        };
        let full = self.working_controls_height(forms, width, window) + WORKING_HEADER;
        if full + body(true) <= height {
            return WorkingFit {
                tight: false,
                heading: true,
            };
        }
        WorkingFit {
            tight: true,
            heading: working_toolbar_height(window) + body(true) <= height,
        }
    }

    /// The height the composer's scrolling fields take, `width` wide, but for
    /// the description's field: the title's rows and any note below them,
    /// the Description section's label and hint with the gaps around its
    /// field (`description_section`), the guidance after it, and the gap
    /// above the commit action, wrapped texts laid out once per text, size
    /// and width (`wrapped_lines`). `shown`: the list shows a status's rows,
    /// its own or those kept while a newer one's are prepared.
    fn composer_rows(
        &self,
        metrics: &ComposerMetrics,
        width: Pixels,
        identity_ready: bool,
        shown: bool,
        window: &Window,
        cx: &App,
    ) -> Pixels {
        use crate::appearance::{ui_size, ui_text};
        let rem = window.rem_size();
        let gap = metrics.gap;
        // Every row's `px_3`, the inspector's one leading inset.
        let room = width - rem * 0.75 * 2.;
        let lines =
            |text: &str, size: Pixels, room: Pixels| wrapped_lines(text, size, room, window) as f32;
        let mut rows = metrics.title_rows();
        if self.commit_title.read(cx).value().chars().count() > 72 {
            rows += gap + metrics.small * lines(LONG_TITLE_NOTE, ui_text(10.), room);
        }
        rows += self.description_section(metrics, width, window);
        // The guidance after it, which the section leaves room for.
        if self.draft_save_error.is_some() {
            rows += gap
                + metrics.small * lines(DRAFT_SAVE_NOTE, ui_text(10.), room)
                + rem * 0.25
                + ui_size(28.);
        }
        if !identity_ready && shown {
            rows += gap + ui_size(28.);
        }
        if self.working_paths.conflicted {
            let text = room - px(15.) - rem * 0.5;
            rows += gap + px(15.).max(metrics.label * lines(CONFLICT_NOTE, ui_text(11.), text));
        }
        rows + metrics.gap
    }

    /// The Description section's rows in the composer's fields, `width`
    /// wide, but for its field: the gap that leads it, its label, and the
    /// hint, a gap either side of the field.
    fn description_section(
        &self,
        metrics: &ComposerMetrics,
        width: Pixels,
        window: &Window,
    ) -> Pixels {
        let gap = metrics.gap;
        // Every row's `px_3`, the inspector's one leading inset.
        let room = width - window.rem_size() * 0.75 * 2.;
        let hint = if self.working_paths.staged_count == 0 {
            UNSTAGED_HINT
        } else {
            STAGED_HINT
        };
        let size = crate::appearance::ui_text(10.);
        gap + metrics.label
            + gap
            + gap
            + metrics.small * wrapped_lines(hint, size, room, window) as f32
    }

    /// The counts the Staged and Unstaged headings show, the first row and
    /// the first Unstaged one, found by bisection: the rows hold every Staged
    /// row before every Unstaged one.
    fn section_counts(&self) -> Option<(usize, usize)> {
        let area = |row: &WorkingRow| match row {
            WorkingRow::Heading(area, _)
            | WorkingRow::Directory(_, area)
            | WorkingRow::File(_, area) => *area,
        };
        let unstaged = self
            .working_rows
            .partition_point(|row| area(row) == ChangeArea::Staged);
        match (self.working_rows.first(), self.working_rows.get(unstaged)) {
            (Some(WorkingRow::Heading(_, staged)), Some(WorkingRow::Heading(_, unstaged))) => {
                Some((*staged, *unstaged))
            }
            _ => None,
        }
    }

    /// Brings the heading whose Stage all or Unstage all confirms its write
    /// into the list's view once the rows that write produced arrive
    /// (`schedule_working_filter` after the explicit refresh that follows
    /// it), so its Staged or Unstaged is seen on the control that was
    /// pressed. Stage all moves every file into Staged above its own
    /// heading, which went past the viewport's foot with its confirmation
    /// still on it, and centring a selected file could carry Unstage all's
    /// heading above the top. The list moves in one frame, the least that
    /// shows the heading whole, and only so far as `selected`, the selected
    /// file's row, stays in view, as it does when lists change: where the
    /// two cannot be seen together, the selected file keeps the place the
    /// refresh gives it, and the outcome is still announced and kept in the
    /// status line.
    pub(super) fn reveal_confirming_heading(
        &self,
        selected: Option<usize>,
        window: &Window,
        cx: &App,
    ) {
        let Some(heading) = self.confirming_heading(cx) else {
            return;
        };
        // A list never laid out has no viewport to reveal anything in.
        let Some(size) = self.working_scroll.0.borrow().last_item_size else {
            return;
        };
        reveal_heading(
            &self.working_scroll,
            size.item.height,
            window.pixel_snap(px(self.settings.density.file_row_height())),
            self.working_rows.len(),
            selected,
            heading,
        );
    }

    /// The row of the heading whose Stage all or Unstage all reads its
    /// confirmation now, if any.
    fn confirming_heading(&self, cx: &App) -> Option<usize> {
        [ChangeArea::Staged, ChangeArea::Unstaged]
            .into_iter()
            .filter(|area| copy_feedback::confirmed(&confirmations::all(&self.path, *area), cx))
            .find_map(|confirming| {
                self.working_rows.iter().position(
                    |row| matches!(row, WorkingRow::Heading(area, _) if *area == confirming),
                )
            })
    }
}

/// Scrolls `scroll`, a list of `rows` rows `row` tall in a `viewport` tall
/// view, to show row `heading` as [`heading_reveal`] decides, beside the
/// `selected` row. The move replaces any scroll the list was about to make,
/// such as the refresh's centring of the selected row, which the decision
/// accounts for, and lands when the list is next laid out, with its viewport
/// then.
fn reveal_heading(
    scroll: &UniformListScrollHandle,
    viewport: Pixels,
    row: Pixels,
    rows: usize,
    selected: Option<usize>,
    heading: usize,
) {
    let top = -scroll.0.borrow().base_handle.offset().y;
    match heading_reveal(top, viewport, row, rows, selected, heading) {
        Some(Reveal::Nearest) => scroll.scroll_to_item(heading, ScrollStrategy::Nearest),
        Some(Reveal::Edge(strategy)) => scroll.scroll_to_item_strict(heading, strategy),
        None => {}
    }
}

/// How the Working list scrolls to show a confirming heading whole
/// ([`GitTurtle::reveal_confirming_heading`]). The list places the heading
/// when it is next laid out, with the viewport it is drawn in then, which
/// the frames around a write change from the one last drawn: the busy
/// composer and the controls above the list are measured again, and a
/// 1000 × 1400 test window's list went from 681 to 660 px as Stage all's
/// rows arrived.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reveal {
    /// The least move that shows it, which keeps a selected file the list
    /// shows now in view.
    Nearest,
    /// The edge of the viewport on the selected file's side, `Bottom` below
    /// it or `Top` above it, for a selected file the list does not show
    /// now and would centre.
    Edge(ScrollStrategy),
}

/// How a list of `rows` rows `row` tall, last drawn `viewport` tall and
/// scrolled to `top`, shows row `heading`: where no file is selected, by
/// the least move; with the `selected` row, only where the two fit in the
/// viewport together with half a row to spare for the viewport's next
/// height, by the least move where the list shows that row now and
/// otherwise with the heading at the edge beside it. `None` leaves the
/// selected row where the refresh puts it.
fn heading_reveal(
    top: Pixels,
    viewport: Pixels,
    row: Pixels,
    rows: usize,
    selected: Option<usize>,
    heading: usize,
) -> Option<Reveal> {
    let Some(selected) = selected else {
        return Some(Reveal::Nearest);
    };
    let span = row * (heading.abs_diff(selected) + 1) as f32;
    if span > viewport - row / 2. {
        return None;
    }
    let top = top.clamp(
        Pixels::ZERO,
        (row * rows as f32 - viewport).max(Pixels::ZERO),
    );
    let start = row * selected as f32;
    // Within a hundredth of a pixel, as layout rounds.
    if start >= top - px(0.01) && start + row <= top + viewport + px(0.01) {
        Some(Reveal::Nearest)
    } else if heading > selected {
        Some(Reveal::Edge(ScrollStrategy::Bottom))
    } else {
        Some(Reveal::Edge(ScrollStrategy::Top))
    }
}

/// The tight form's one row: the filter's field beside 28 px controls, with
/// a `py_1` above and below that holds a focused control's ring.
fn working_toolbar_height(window: &Window) -> f32 {
    let rem = window.rem_size();
    f32::from(rem * 0.25 * 2. + (rem * 2.).max(crate::appearance::ui_size(28.)))
}

/// The title counter's whole text, for a title `length` characters long.
fn title_counter(length: usize) -> String {
    format!("{length}/72 suggested")
}

/// `full`, or where it is wider than `room` at `size`, the most of it that
/// fits with an ellipsis, cut at a character boundary after any spaces and
/// ellipsis it ends with; empty where not even its first character does, so
/// no lone ellipsis shows.
fn ellipsized(full: &str, room: Pixels, size: Pixels, window: &Window) -> SharedString {
    let weight = window.text_style().font_weight;
    let fits = |text: &str| crate::views::shaped_width(text, size, weight, window) <= room;
    if fits(full) {
        return SharedString::from(full.to_owned());
    }
    let stem = full.trim_end_matches('…');
    // The byte ends of `stem`'s first one, two, … characters, short of all.
    let ends: Vec<usize> = stem.char_indices().skip(1).map(|(end, _)| end).collect();
    let cut = |characters: usize| format!("{}…", stem[..ends[characters]].trim_end());
    let (mut fitting, mut low, mut high) = (SharedString::default(), 0, ends.len());
    while low < high {
        let middle = (low + high) / 2;
        let text = cut(middle);
        if fits(&text) {
            fitting = text.into();
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    fitting
}

/// The first of `forms` that fits `room` whole at `size`, each a shorter
/// wording of the one before it in whole words; where none does, the last
/// as [`ellipsized`] fits it, so a word is cut only where even the shortest
/// form is too wide.
fn whole_form(forms: &[&str], room: Pixels, size: Pixels, window: &Window) -> SharedString {
    let weight = window.text_style().font_weight;
    let (shortest, wider) = forms.split_last().expect("a placeholder");
    wider
        .iter()
        .find(|form| crate::views::shaped_width(form, size, weight, window) <= room)
        .map_or_else(
            || ellipsized(shortest, room, size, window),
            |form| SharedString::from(form.to_string()),
        )
}

/// A canvas that, painted after `field`, fits its placeholder to the width
/// the field just painted its text at: the first of `forms`, its whole-word
/// wordings from the longest, that fits, else the shortest ending with an
/// ellipsis rather than clipping a glyph (`whole_form`). It sets the
/// placeholder only when that changes, and then draws again, since refresh is
/// ignored during paint. Its parent is positioned; it takes no room. The
/// placeholder is no name: the field keeps its whole accessible name
/// (`aria_label`) whichever form shows. `text` is the size the field draws
/// its text at, at the default interface size: 12 pt for most fields, 11 pt
/// for the navigation's filter.
pub(super) fn placeholder_fit<M: gpui_kit::base::input::InputModeKind + 'static>(
    field: Entity<gpui_kit::base::input::InputBaseState<M>>,
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &'static [&'static str],
    text: f32,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let Some(bounds) = field.read(cx).text_bounds() else {
                return;
            };
            let fit = fitted_placeholder(cache, forms, bounds.size.width, text, window);
            if *field.read(cx).presentation().placeholder() != fit {
                field.update(cx, |state, cx| state.set_placeholder(fit, window, cx));
                window.defer(cx, |window, _| window.refresh());
            }
        },
    )
    .absolute()
    .size_0()
}

/// `forms` at `text` pt as they fit `room` (`whole_form`), laid out again
/// only when the room, the font or the sizes change.
fn fitted_placeholder(
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &[&str],
    room: Pixels,
    text: f32,
    window: &Window,
) -> SharedString {
    let key = crate::views::LineKey::current(window);
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((cached, width, text)) if *cached == key && *width == room => text.clone(),
            _ => {
                let fit = whole_form(forms, room, crate::appearance::ui_text(text), window);
                *cache = Some((key, room, fit.clone()));
                fit
            }
        }
    })
}

fn commit_composer_height(available: f32, row_height: f32, preferred: f32) -> f32 {
    let available = available.max(0.);
    // Reserve several visible rows at ordinary sizes and share very short
    // bodies equally. The scrolling composer must never consume the list.
    let list_reserve = (row_height * 3.5)
        .max(available * 0.30)
        .min(available * 0.50);
    // The cap scales with the interface size, as the composer's rows do.
    preferred
        .min(f32::from(crate::appearance::ui_size(440.)))
        .min(available - list_reserve)
}

impl GitTurtle {
    pub(super) fn current_commit_draft(&self, cx: &App) -> CommitDraft {
        CommitDraft {
            title: self.commit_title.read(cx).value().to_string(),
            description: self.commit_message.read(cx).value().to_string(),
        }
    }

    pub(super) fn persist_commit_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(worktree) = self.draft_repository.clone() else {
            return;
        };
        let draft = self.current_commit_draft(cx);
        self.save_commit_draft(worktree, draft, window, cx);
        cx.notify();
    }

    fn save_commit_draft(
        &mut self,
        worktree: PathBuf,
        draft: CommitDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if draft.is_empty() {
            self.commit_drafts.remove(&worktree);
        } else {
            self.commit_drafts.insert(worktree.clone(), draft.clone());
        }
        let Some(response) = self
            .draft_saver
            .queue(&self.preferences_writer, worktree, draft)
        else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.draft_save_error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn restore_commit_draft(
        &mut self,
        worktree: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_repository.as_ref() == Some(&worktree) {
            return;
        }
        self.persist_commit_draft(window, cx);
        let draft = self
            .commit_drafts
            .get(&worktree)
            .cloned()
            .unwrap_or_default();
        self.draft_repository = Some(worktree);
        self.commit_title
            .update(cx, |input, cx| input.set_value(draft.title, window, cx));
        self.commit_message.update(cx, |input, cx| {
            input.set_value(draft.description, window, cx)
        });
    }

    fn clear_committed_draft(
        &mut self,
        worktree: &PathBuf,
        submitted_message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft_repository.as_ref() == Some(worktree) {
            if self.current_commit_draft(cx).message() != submitted_message {
                return;
            }
            self.commit_title
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.commit_message
                .update(cx, |input, cx| input.set_value("", window, cx));
        } else if self
            .commit_drafts
            .get(worktree)
            .is_none_or(|draft| draft.message() != submitted_message)
        {
            return;
        }
        self.save_commit_draft(worktree.clone(), CommitDraft::default(), window, cx);
    }

    pub(super) fn project_event(
        &mut self,
        event: &projects::ProjectEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Naming a project changes app data only, so it stays available while
        // a Git operation runs on some repository.
        if let projects::ProjectEvent::Rename(path) = event {
            self.open_rename_project(path.clone(), window, cx);
            return;
        }
        if self.operation_busy.is_some() {
            return;
        }
        match event {
            projects::ProjectEvent::Rename(_) => {}
            projects::ProjectEvent::Open(path) => {
                if self
                    .repository
                    .as_ref()
                    .is_some_and(|repo| repo.path() == path)
                {
                    self.return_from_page(window, cx);
                } else {
                    self.limit = 500;
                    self.open(path.clone(), None, window, cx);
                }
            }
            projects::ProjectEvent::Back => self.return_from_page(window, cx),
            projects::ProjectEvent::Clone {
                source,
                destination,
            } => {
                let source = source.clone();
                let destination = destination.clone();
                self.create_project(
                    "Cloning repository…",
                    destination.clone(),
                    move || GitRepository::clone_repository(&source, &destination),
                    window,
                    cx,
                );
            }
            projects::ProjectEvent::Create {
                destination,
                branch,
            } => {
                let destination = destination.clone();
                let branch = branch.clone();
                self.create_project(
                    "Creating repository…",
                    destination.clone(),
                    move || GitRepository::init(&destination, &branch),
                    window,
                    cx,
                );
            }
        }
    }

    fn create_project(
        &mut self,
        label: &'static str,
        destination: PathBuf,
        operation: impl FnOnce() -> anyhow::Result<GitRepository> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let accepted_path = self.path.clone();
        let accepted_page = self.page;
        self.operation_repository = Some(destination.clone());
        self.operation_busy = Some(label);
        self.operation_error = None;
        self.operation_notice = None;
        self.hub.update(cx, |hub, cx| hub.set_busy(true, cx));
        let activity_id =
            self.activity
                .begin(&destination, label, &destination.display().to_string());
        self.save_activity(window, cx);
        let control = self.begin_operation_control(window, cx);
        let activity_control = control.clone();
        let response = self.operations.submit_controlled(control, operation);
        self.operation_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Repository operation stopped without a result. Inspect the destination before retrying.")));
            let _ = this.update_in(cx, |this, window, cx| {
                let explanation = result.as_ref().map(|_| "Repository operation completed.".into()).unwrap_or_else(|error| format!("{error:#}"));
                this.activity.finish(activity_id, result.is_ok(), activity_control.is_cancelled(), explanation.contains("without a result") || explanation.contains("timed out"), &explanation);
                this.save_activity(window, cx);
                this.operation_busy = None;
                this.operation_repository = None;
                this.finish_operation_control(window, cx);
                this.hub.update(cx, |hub, cx| hub.set_busy(false, cx));
                match result {
                    Ok(repo) => {
                        let message = format!("{} · {}", if label.starts_with("Cloning") { "Repository cloned" } else { "Repository created" }, this.project_name(repo.path()));
                        if this.path == accepted_path && this.page == accepted_page {
                            this.limit = 500; this.open(repo.path().to_owned(), None, window, cx);
                            this.operation_notice = Some(message);
                        } else {
                            this.register_completed_repository(repo.path().to_owned(), message, window, cx);
                        }
                    }
                    Err(error) => {
                        let message = format!("{error:#}");
                        this.hub.update(cx, |hub, cx| hub.set_error(Some(message.clone()), cx));
                        if this.path == accepted_path && this.page == accepted_page {
                            this.operation_error = Some(message);
                            this.operation_notice = None;
                        } else {
                            this.status = format!("Repository operation failed in {}: {message}",destination.display());
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn show_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_image_drag();
        self.cancel_branch_action();
        self.cancel_interactive_rebase_action();
        self.cancel_recovery_read();
        self.cancel_discard_action();
        if self.operation_busy.is_some() {
            return;
        }
        self.capture_page_return_focus(window, cx);
        self.page = AppPage::Projects;
        self.page_origin = AppPage::Repository;
        self.hub.update(cx, |hub, cx| {
            hub.set_busy(false, cx);
            hub.set_error(None, cx);
            hub.set_can_go_back(self.repository.is_some(), cx);
        });
        // Focus opens on the selected action tab, with its ring, inside the
        // page so the application's keys still reach the window's handlers.
        self.hub
            .update(cx, |hub, cx| hub.focus_selected_mode(window, cx));
        self.repaint_page(window, cx);
    }

    pub(super) fn remember_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let recent = self.begin_recent_project_save(&path);
        let response = self.preferences_writer.submit(move || {
            // Remembering rebuilds the whole store from disk; nothing is read here first.
            let mut prefs = Preferences::default();
            prefs.remember_repository(&path)?;
            Ok(prefs)
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(result) = response.await {
                let _ = this.update_in(cx, |this, window, cx| match result {
                    Ok(prefs) => {
                        this.absorb_remembered_project(recent, &prefs, window, cx);
                        this.hub
                            .update(cx, |hub, cx| hub.set_recent(prefs.recent_repositories, cx));
                    }
                    Err(error) => {
                        this.operation_error =
                            Some(format!("Could not save recent projects: {error:#}"))
                    }
                });
            }
        })
        .detach();
    }

    /// The UI generation rejects late replies, while the worker cancellation
    /// also bounds obsolete computation when no replacement preview is needed.
    pub(super) fn invalidate_read(&mut self) {
        self.pause_history_search_for_read();
        self.cancel_automatic_read();
        self.generation = self.generation.wrapping_add(1);
        self.worker.cancel();
        self.task = None;
        self.loading = None;
    }

    pub(super) fn refresh_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        self.cancel_automatic_read();
        self.ensure_local_watcher(true, window, cx);
        if self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            self.clear_preview();
            self.files.clear();
            self.file_paths.reset();
            self.selected_file = None;
        }
        self.work_generation = self.work_generation.wrapping_add(1);
        let generation = self.work_generation;
        let path = repo.path().to_owned();
        self.status_task = None;
        let response = self
            .operations
            .submit_read(move || worker::WorkingState::read(&repo));
        self.status_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Working-copy refresh ended without a result"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.work_generation != generation || this.path.as_ref() != Some(&path) {
                    return;
                }
                this.status_task = None;
                match result {
                    Ok(state) => this.apply_worktree_state(state, false, window, cx),
                    Err(error) => {
                        this.work_status = None;
                        this.working_paths.reset();
                        this.integration_state = None;
                        this.working_rows.clear();
                        this.working_travel = Default::default();
                        this.working_selected = None;
                        if this.mode == WorkspaceMode::Working {
                            this.invalidate_read();
                            this.clear_preview();
                            this.files.clear();
                            this.file_paths.reset();
                            this.selected_file = None;
                        }
                        this.operation_error
                            .get_or_insert_with(|| format!("Working tree: {error:#}"));
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn apply_worktree_state(
        &mut self,
        state: worker::WorkingState,
        quiet: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let worker::WorkingState {
            status,
            profile,
            remotes,
            operation,
        } = state;
        let path = self.path.clone();
        self.integration_state = match operation {
            Ok(state) => state,
            Err(error) => {
                self.operation_error
                    .get_or_insert_with(|| format!("Operation state: {error:#}"));
                None
            }
        };
        let preferred = self.working_selected.and_then(|(index, area)| {
            self.work_status
                .as_ref()?
                .entries
                .get(index)
                .map(|entry| (entry.path.clone(), area))
        });
        if !quiet {
            self.conflict_drafts.retain(|(repository, file), _| {
                Some(repository) != path.as_ref()
                    || status
                        .entries
                        .iter()
                        .any(|entry| entry.conflicted && &entry.path == file)
            });
        }
        let entries_changed = self
            .work_status
            .as_ref()
            .is_none_or(|previous| previous.entries != status.entries);
        let working_anchor = if entries_changed && quiet {
            self.working_scroll_anchor(window)
        } else {
            None
        };
        if entries_changed {
            // The shown rows index the earlier status; they stay on screen,
            // with the cursor among them, until the new status's land.
            let shown = self
                .work_status
                .clone()
                .filter(|_| !self.working_rows.is_empty())
                .map(|status| (status, self.working_selected));
            self.working_paths.supersede(shown);
            self.working_selected = preferred.as_ref().and_then(|(path, area)| {
                status.entries.iter().enumerate().find_map(|(i, entry)| {
                    if &entry.path != path {
                        return None;
                    }
                    retained_area(
                        *area,
                        entry.staged.is_some(),
                        entry.unstaged.is_some() || entry.untracked || entry.conflicted,
                    )
                    .map(|area| (i, area))
                })
            });
        }
        let remotes = match remotes {
            Ok(remotes) => remotes,
            Err(error) => {
                self.operation_error
                    .get_or_insert_with(|| format!("Remote configuration: {error:#}"));
                Vec::new()
            }
        };
        let branch_changed = self.work_status.as_ref().is_none_or(|previous| {
            previous.branch != status.branch || previous.upstream != status.upstream
        });
        let (default_remote, default_branch) = remote_defaults(&status, &remotes);
        if branch_changed || self.remote_name.read(cx).value().is_empty() {
            self.remote_name
                .update(cx, |input, cx| input.set_value(default_remote, window, cx));
        }
        if branch_changed || self.remote_branch.read(cx).value().is_empty() {
            self.remote_branch
                .update(cx, |input, cx| input.set_value(default_branch, window, cx));
        }
        self.profile = match profile {
            Ok(profile) => Some(profile),
            Err(error) => {
                self.operation_error
                    .get_or_insert_with(|| format!("Git identity: {error:#}"));
                None
            }
        };
        self.work_status = Some(Arc::new(status));
        // A quiet refresh that changed nothing leaves the line to whatever it
        // last reported, an operation's outcome included.
        if self.mode == WorkspaceMode::Working && (entries_changed || !quiet) {
            self.status = self.working_summary();
        }
        if entries_changed {
            self.schedule_working_filter(working_anchor, !quiet, window, cx);
        } else if !self.working_paths.pending {
            // No new rows: a departed row whose write has finished did not
            // move. Rows still being prepared for an earlier status, as a
            // quiet refresh's after the write's own can find, may yet land
            // it, and they settle the departure when they do.
            self.land_working_row(None, cx);
        }
        self.remotes = remotes;
        if !quiet && self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            if let Some((index, area)) = self.working_selected {
                self.select_working(index, area, window, cx);
                if let Some(position) = self.actionable_working_rows().iter().position(
                    |row| matches!(row, WorkingRow::File(i, a) if *i == index && *a == area),
                ) {
                    self.working_scroll
                        .scroll_to_item(position, ScrollStrategy::Center);
                }
            } else {
                self.clear_preview();
                self.files.clear();
                self.file_paths.reset();
                self.selected_file = None;
            }
        }
    }

    pub(super) fn show_working(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_inspections(window, cx);
        if self.operation_busy.is_some() {
            return;
        }
        if self.repository.is_none() {
            self.show_projects(window, cx);
            return;
        }
        if self.mode != WorkspaceMode::Working {
            if self.mode == WorkspaceMode::History {
                self.history_sidebar = self.sidebar;
            }
            self.retained_history_files =
                Some((std::mem::take(&mut self.files), self.selected_file.take()));
            self.invalidate_read();
            self.clear_preview();
        }
        self.mode = WorkspaceMode::Working;
        self.status = self.working_summary();
        self.page = AppPage::Repository;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.refresh_worktree(window, cx);
        cx.notify();
    }

    /// Records `notice`, the outcome Git reported for a write in
    /// `repository`, in the status line, which keeps it for
    /// [`copy_feedback::RESTORE_AFTER`], the time the control that asked
    /// reads its confirmation, over what the reads after the write report:
    /// the refresh that follows it once wrote Changes' working summary over
    /// "Staged notes.txt" before the row's Staged had gone. Then the line
    /// reads what they reported. A newer outcome replaces it and restarts the
    /// time, and a write submitted ends it.
    pub(super) fn record_outcome(
        &mut self,
        repository: &std::path::Path,
        notice: String,
        cx: &mut Context<Self>,
    ) {
        self.status = notice.clone();
        let expiry = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(copy_feedback::RESTORE_AFTER)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status_outcome = None;
                cx.notify();
            });
        });
        self.status_outcome = Some(StatusOutcome {
            repository: repository.to_owned(),
            text: notice,
            _expiry: expiry,
        });
    }

    /// The status bar's line on the Repository page: a read's error, else a
    /// write's outcome while it lasts and its repository is shown
    /// ([`Self::record_outcome`]), else the label of a read in progress,
    /// else the status.
    pub(super) fn status_line(&self) -> &str {
        let outcome = self
            .status_outcome
            .as_ref()
            .filter(|outcome| self.path.as_ref() == Some(&outcome.repository))
            .map(|outcome| outcome.text.as_str());
        self.error
            .as_deref()
            .or(outcome)
            .or(self.loading)
            .unwrap_or(&self.status)
    }

    /// The status bar's account of the working tree. Changes sets it when it
    /// is shown, refreshed or previewed, because History's figures describe a
    /// commit's files and a working preview holds only the previewed one.
    pub(super) fn working_summary(&self) -> String {
        let Some(status) = &self.work_status else {
            return "Reading working changes…".into();
        };
        let changes = status.entries.len();
        let staged = status
            .entries
            .iter()
            .filter(|entry| entry.staged.is_some())
            .count();
        let plural = if changes == 1 { "" } else { "s" };
        match (changes, staged) {
            (0, _) => "No working changes".into(),
            (_, 0) => format!("{changes} working change{plural}"),
            _ => format!("{changes} working change{plural} · {staged} staged"),
        }
    }

    pub(super) fn select_working(
        &mut self,
        index: usize,
        area: ChangeArea,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        self.close_blame(window, cx);
        let (Some(repo), Some(status)) = (&self.repository, &self.work_status) else {
            return;
        };
        let Some(entry) = status.entries.get(index).cloned() else {
            return;
        };
        let available = if area == ChangeArea::Staged {
            entry.staged.is_some()
        } else {
            entry.unstaged.is_some() || entry.untracked || entry.conflicted
        };
        if !available {
            self.invalidate_read();
            self.working_selected = None;
            self.clear_preview();
            self.files.clear();
            self.file_paths.reset();
            self.selected_file = None;
            return;
        }
        let repo = repo.clone();
        self.mode = WorkspaceMode::Working;
        self.working_selected = Some((index, area));
        self.clear_preview();
        self.files.clear();
        self.file_paths.reset();
        self.selected_file = None;
        self.zoom = 0.;
        self.image_scroll.set_offset(point(px(0.), px(0.)));
        self.interaction_started = Some(Instant::now());
        window.focus(&self.file_focus, cx);
        self.request(
            Job::WorkingPreview {
                repo,
                entry,
                area,
                head: self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.head.clone()),
            },
            "Reading working comparison…",
            window,
            cx,
        );
    }

    pub(super) fn move_working_selection(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files: Vec<_> = self
            .actionable_working_rows()
            .iter()
            .enumerate()
            .filter_map(|(position, row)| match row {
                WorkingRow::File(i, area) => Some((position, *i, *area)),
                _ => None,
            })
            .collect();
        if files.is_empty() {
            return;
        }
        let current = files
            .iter()
            .position(|(_, i, area)| Some((*i, *area)) == self.working_selected)
            .unwrap_or(0);
        let next = if edge {
            if direction < 0 { 0 } else { files.len() - 1 }
        } else {
            (current as i32 + direction).clamp(0, files.len() as i32 - 1) as usize
        };
        let (position, index, area) = files[next];
        self.choose_working_selection(index, area, false, false, window, cx);
        self.working_scroll
            .scroll_to_item(position, ScrollStrategy::Center);
    }

    /// One file row's Stage, or Unstage where `staged`, from its control or,
    /// where the control has left a narrow row, its menu. `path`, the row's
    /// file, confirms on its row's control in the area it moves to.
    fn stage_working_paths(
        &mut self,
        paths: Vec<PathBuf>,
        path: &std::path::Path,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (command, label, arrives) = if staged {
            (
                WriteCommand::Unstage { paths },
                "Unstaging file…",
                ChangeArea::Unstaged,
            )
        } else {
            (
                WriteCommand::Stage { paths },
                "Staging file…",
                ChangeArea::Staged,
            )
        };
        let origin = confirmations::file(&self.path, path, arrives);
        self.write_from(command, label, Some(origin.into()), window, cx);
    }

    /// Submits `command` with no control to confirm it: its outcome reaches
    /// the window-wide notice, or the operation error.
    pub(super) fn write(
        &mut self,
        command: WriteCommand,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.write_from(command, label, None, window, cx);
    }

    /// Whether the window is busy: a write in flight, a status read, or a
    /// list's rows being prepared. A write's own read and rows follow it
    /// without a gap, so its spell runs from the press to the rows' landing.
    fn busy_spell(&self) -> bool {
        self.operation_busy.is_some() || self.status_task.is_some() || self.working_paths.pending
    }

    /// Follows the busy spell as each frame is built: one that begins asks
    /// for a frame [`BUSY_LOOK_DELAY`] later, which shows its look if it
    /// still runs, and one that ends drops its look.
    pub(super) fn track_busy_look(&mut self, cx: &mut Context<Self>) {
        if !self.busy_spell() {
            self.busy_look = BusyLook::default();
            return;
        }
        if self.busy_look.shown || self.busy_look.reveal.is_some() {
            return;
        }
        self.busy_look.reveal = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(BUSY_LOOK_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.busy_look.shown = true;
                cx.notify();
            });
        }));
    }

    /// Whether the busy spell has lasted [`BUSY_LOOK_DELAY`], so what it
    /// makes unavailable looks unavailable.
    pub(super) fn busy_shown(&self) -> bool {
        self.busy_look.shown && self.busy_spell()
    }

    /// The write in flight, once its spell looks busy: the progress label,
    /// Cancel and the tab's "· working" wait for it.
    pub(super) fn operation_shown(&self) -> Option<&'static str> {
        self.operation_busy.filter(|_| self.busy_shown())
    }

    /// Submits `command`, asked for from `origin`. Once Git reports success,
    /// while its repository is still the one shown, the control `origin`
    /// marks, if any, confirms it where it was touched
    /// (`copy_feedback::confirm`), the outcome is announced once and the
    /// status line records it, and the window-wide notice stays down. A
    /// failure, an uncertain result, or a write with no `origin` or whose
    /// repository is no longer shown keeps today's error or notice; nothing
    /// is retried.
    pub(super) fn write_from(
        &mut self,
        command: WriteCommand,
        label: &'static str,
        origin: Option<confirmations::Origin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        let downloading_lfs = matches!(&command, WriteCommand::DownloadLfs(_));
        let lfs_selection = if downloading_lfs {
            self.selected_file
                .and_then(|index| self.files.get(index))
                .cloned()
                .map(|file| (file, self.mode, self.generation))
        } else {
            None
        };
        if !downloading_lfs {
            self.close_inspections(window, cx);
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let path = repo.path().to_owned();
        let ending_integration = matches!(
            &command,
            WriteCommand::Integration(
                gitturtle_core::IntegrationCommand::Abort { .. }
                    | gitturtle_core::IntegrationCommand::Quit { .. }
            )
        );
        let resolved_path = match &command {
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                ..
            }) if !matches!(
                &command,
                WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                    resolution: gitturtle_core::ConflictResolution::Save { .. },
                    ..
                })
            ) =>
            {
                Some(expected.path.clone())
            }
            _ => None,
        };
        let submitted_message = match &command {
            WriteCommand::Commit { message } => Some(message.clone()),
            _ => None,
        };
        let submitted_branch = match &command {
            WriteCommand::Checkout { branch } => Some(branch.clone()),
            WriteCommand::CreateBranch { name, .. } => Some(name.clone()),
            _ => None,
        };
        let success_notice = match &command {
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                resolution: gitturtle_core::ConflictResolution::Save { .. },
            }) => Some(format!(
                "Saved {} · remains unresolved until explicitly staged",
                expected.path.display()
            )),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Resolve {
                expected,
                ..
            }) => Some(format!("Resolved and staged {}", expected.path.display())),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Continue {
                expected,
            }) => Some(format!(
                "Continued {} on {}",
                expected.kind.label().to_lowercase(),
                expected.branch
            )),
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Abort { expected }) => {
                Some(format!(
                    "Aborted {} on {}",
                    expected.kind.label().to_lowercase(),
                    expected.branch
                ))
            }
            WriteCommand::Integration(gitturtle_core::IntegrationCommand::Quit { expected }) => {
                Some(format!(
                    "Stopped {} and kept the current files",
                    expected.kind.label().to_lowercase()
                ))
            }
            WriteCommand::Stage { paths } => Some(match paths.as_slice() {
                [path] => format!("Staged {}", path.display()),
                _ => "Staged selected changes".into(),
            }),
            WriteCommand::Unstage { paths } => Some(match paths.as_slice() {
                [path] => format!("Unstaged {}", path.display()),
                _ => "Unstaged selected changes".into(),
            }),
            WriteCommand::StageAll => Some("Staged all working changes".into()),
            WriteCommand::UnstageAll => Some("Unstaged all changes".into()),
            WriteCommand::Checkout { branch } => Some(format!("Switched to {branch}")),
            WriteCommand::CreateBranch { name, .. } => {
                Some(format!("Created and switched to {name}"))
            }
            WriteCommand::Fetch { remote } => Some(format!("Fetched {remote}")),
            WriteCommand::Pull { remote, branch } => Some(format!("Pulled {remote}/{branch}")),
            WriteCommand::Push {
                remote,
                local_branch,
                remote_branch,
            } => Some(format!("Pushed {local_branch} to {remote}/{remote_branch}")),
            WriteCommand::Branch(branch) => match branch.as_ref() {
                gitturtle_core::BranchCommand::CreateTracking { plan } => {
                    let upstream = plan.remote_ref.trim_start_matches("refs/remotes/");
                    Some(if plan.checkout {
                        format!("Created and switched to {} · tracks {upstream}", plan.name)
                    } else {
                        format!("Created {} · tracks {upstream}", plan.name)
                    })
                }
                _ => None,
            },
            _ => None,
        };
        // A Fetch or Pull that brought nothing new (`WriteOutcome::up_to_date`)
        // says so in the status line and its announcement, naming what it
        // read: a Fetch the remote, whose every ref it compared, a Pull the
        // branch it pulled. Its control still reads Fetched or Pulled, whose
        // width is the control's own.
        let up_to_date_notice = match &command {
            WriteCommand::Fetch { remote } => {
                Some(format!("Up to date with {remote} · nothing new"))
            }
            WriteCommand::Pull { remote, branch } => {
                Some(format!("Up to date with {remote}/{branch} · nothing new"))
            }
            _ => None,
        };
        let submitted_profile = if let WriteCommand::ApplyProfile(plan) = &command {
            self.submitted_profile(plan)
        } else {
            None
        };
        let tag_command = if let WriteCommand::Tag(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let worktree_command = if let WriteCommand::Worktree(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let recovery = if let WriteCommand::Recovery(command) = &command {
            Some(Arc::clone(command))
        } else {
            None
        };
        let refresh_history = matches!(
            &command,
            WriteCommand::Commit { .. }
                | WriteCommand::InteractiveRebase(_)
                | WriteCommand::Worktree(_)
                | WriteCommand::RecoverReflog(_)
                | WriteCommand::Branch(_)
                | WriteCommand::Tag(_)
                | WriteCommand::Checkout { .. }
                | WriteCommand::CreateBranch { .. }
                | WriteCommand::Fetch { .. }
                | WriteCommand::Pull { .. }
                | WriteCommand::Push { .. }
        ) || matches!(&command, WriteCommand::Integration(operation) if !matches!(operation, gitturtle_core::IntegrationCommand::Resolve { .. }))
            || matches!(&command, WriteCommand::Recovery(command) if matches!(command.as_ref(), gitturtle_core::RecoveryCommand::Amend { .. } | gitturtle_core::RecoveryCommand::Undo { .. } | gitturtle_core::RecoveryCommand::Revert { .. } | gitturtle_core::RecoveryCommand::CherryPick { .. }));
        self.operation_repository = Some(path.clone());
        self.operation_busy = Some(label);
        self.operation_error = None;
        self.operation_notice = None;
        self.status_outcome = None;
        self.work_generation = self.work_generation.wrapping_add(1);
        self.status_task = None;
        if self.mode == WorkspaceMode::Working && !downloading_lfs {
            self.invalidate_read();
            self.clear_preview();
            self.files.clear();
            self.file_paths.reset();
            self.selected_file = None;
        }
        let target = activity::target(
            &command,
            self.work_status
                .as_ref()
                .and_then(|s| s.branch.as_deref())
                .unwrap_or("Current worktree"),
        );
        let activity_id = self.activity.begin(&path, label, &target);
        self.save_activity(window, cx);
        let control = self.begin_operation_control(window, cx);
        let activity_control = control.clone();
        let rewrite_capture = self.prepare_rewrite_capture(&command);
        let response = self.operations.submit_controlled(control, move || {
            if let Some(capture) = rewrite_capture {
                futures::executor::block_on(capture).map_err(|_| {
                    anyhow::anyhow!(
                        "Original-series save ended without a result. Git was not started"
                    )
                })??;
            }
            repo.execute(&command)
        });
        self.operation_task = Some(cx.spawn_in(window, async move |this,cx| {
            let result=response.await.unwrap_or_else(|_|Err(anyhow::anyhow!("Operation ended without a result. Refresh before retrying; repository state may have changed.")));
            let _=this.update_in(cx,|this,window,cx| {
                let (success, explanation) = match &result {
                    Ok(_) => (true, "Git reported that the operation completed.".to_owned()),
                    Err(error) => (false, format!("{error:#}")),
                };
                let uncertain = explanation.contains("without a result") || explanation.contains("timed out") || explanation.contains("deadline") || explanation.contains("exceeded");
                this.activity.finish(activity_id, success, activity_control.is_cancelled(), uncertain, &explanation);
                this.save_activity(window, cx);
                this.operation_busy=None;
                this.operation_repository=None;
                this.finish_operation_control(window, cx);
                let succeeded=result.is_ok();
                match result {
                    Ok(outcome) => {
                        if let Some(oid) = &outcome.commit_oid { this.tab_commit_receipt(&path, oid.clone()); }
                        let notice = if let (Some(oid), Some(message)) = (outcome.commit_oid.as_ref(), submitted_message.as_deref()) {
                            format!("Committed {} · {}", short_oid(oid), message.lines().next().unwrap_or_default())
                        } else if let Some(notice) = up_to_date_notice.as_ref().filter(|_| outcome.up_to_date) {
                            notice.clone()
                        } else if let Some(notice) = &success_notice {
                            notice.clone()
                        } else {
                            outcome.message.lines().find(|line| !line.trim().is_empty()).unwrap_or("Git operation completed").to_owned()
                        };
                        match origin.filter(|_| this.path.as_ref() == Some(&path) && this.page == AppPage::Repository) {
                            Some(origin) => {
                                this.tab_operation_feedback(&path, None, None, cx);
                                this.record_outcome(&path, notice.clone(), cx);
                                copy_feedback::confirm(window, cx, origin.mark(), notice);
                            }
                            None => this.tab_operation_feedback(&path, Some(notice), None, cx),
                        }
                    }
                    Err(error) => {
                        this.tab_operation_feedback(&path, None, Some(format!("{error:#}")), cx);
                    }
                }
                if let Some(profile) = submitted_profile { this.finish_profile_write(&path, profile, succeeded, window, cx); }
                if let Some(command) = &tag_command { this.finish_tag_write(&path, command, succeeded, cx); }
                if let Some(command) = &worktree_command { this.finish_worktree_write(&path, command, succeeded, window, cx); }
                if let Some(command) = &recovery { this.finish_recovery_write(&path, command, succeeded, cx); }
                if succeeded && ending_integration { this.conflict_drafts.retain(|(repository, _), _| repository != &path); }
                if succeeded && let Some(resolved) = &resolved_path {
                    this.conflict_drafts.remove(&(path.clone(), resolved.clone()));
                }
                if succeeded && let Some(message) = &submitted_message {
                    this.clear_committed_draft(&path, message, window, cx);
                }
                if this.path.as_ref()==Some(&path) {
                    if succeeded && submitted_branch.as_deref().is_some_and(|branch| this.branch_name.read(cx).value().trim() == branch) {
                        this.branch_name.update(cx,|input,cx|input.set_value("",window,cx));
                    }
                    if refresh_history { this.refresh_after_write(path, window, cx); }
                    else if downloading_lfs {
                        if succeeded && lfs_selection.as_ref().is_some_and(|(file, mode, generation)| this.mode == *mode && this.generation == *generation && this.selected_file.and_then(|index| this.files.get(index)) == Some(file)) {
                            if this.mode == WorkspaceMode::Working {
                                if let Some((index, area)) = this.working_selected { this.select_working(index, area, window, cx); }
                            } else if !this.reload_tracked_inspection(window, cx) && let Some(index) = this.selected_file { this.load_file(index, window, cx); }
                        }
                    } else { this.refresh_worktree(window,cx); }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn refresh_after_write(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.path.as_ref() != Some(&path) {
            return;
        }
        // A write changes repository state, not the user's browsing intent.
        // Reopening discarded the selected scope and restarted a retained query
        // over all refs. Quiet refresh resolves that same scope again while
        // keeping pinned search results, immutable comparisons and scroll/focus.
        self.queue_automatic_refresh(
            local_refresh::LocalChange {
                git: true,
                worktree: true,
                ..Default::default()
            },
            window,
            cx,
        );
    }

    /// `width` is the inspector's width where the window pins it, otherwise
    /// the width it is expected to take until it is laid out.
    pub(super) fn render_working_inspector(
        &self,
        width: Pixels,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let previous_list = self.working_scroll.0.borrow().last_item_size;
        // Measure the list/composer body after Targets, feedback, the Changes
        // header and wrapping path/selection controls have taken their space.
        let layout = window.use_keyed_state("working-inspector-layout", cx, |window, _| {
            WorkingInspectorLayout {
                height: (window.viewport_size().height - px(400.)).max(px(160.)),
                width,
                list_size: previous_list.map(|size| size.item),
                inspector: (window.viewport_size().height - px(200.)).max(px(160.)),
            }
        });
        let available = f32::from(layout.read(cx).height);
        let short = available < 560.;
        let fitted_width = if pinned { width } else { layout.read(cx).width };
        let forms = self.working_forms(fitted_width, short, window, cx);
        // The measured body's height must not change the header height, which
        // would oscillate at a size threshold on successive layout passes.
        let header_height = WORKING_HEADER;
        let normal_description = if self.settings.density == appearance::Density::Compact {
            100.
        } else {
            128.
        };
        let description_height = ((available - 440.) * 0.5 + 64.).clamp(64., normal_description);
        let list_layout = layout.clone();
        let list_scroll = self.working_scroll.clone();
        let selected_row = self.actionable_working_rows().iter().position(
            |row| matches!(row, WorkingRow::File(index, area) if Some((*index, *area)) == self.working_selected),
        );
        let confirming_heading = self.confirming_heading(cx);
        let (reveal_row, row_count) = (
            window.pixel_snap(px(self.settings.density.file_row_height())),
            self.working_rows.len(),
        );
        // GPUI retains this content geometry in the scroll handle even while
        // Settings hides the inspector. Density changes must not reuse a pixel
        // offset that now points at a different set of rows. Rows that change
        // in number, as when a write's rows replace the empty list of its
        // pending filter, take this path too, so a confirming heading is
        // shown as the rows' arrival showed it.
        if let Some(size) =
            previous_list.filter(|size| size.contents.height != reveal_row * row_count as f32)
        {
            if let Some(heading) = confirming_heading {
                reveal_heading(
                    &self.working_scroll,
                    size.item.height,
                    reveal_row,
                    row_count,
                    selected_row,
                    heading,
                );
            } else if let Some(selected) = selected_row {
                self.working_scroll
                    .scroll_to_item(selected, ScrollStrategy::Nearest);
            }
        }
        let staged = self.working_paths.staged_count;
        // Of the status the shown rows index, as their counts are.
        let total = self
            .shown_working_status()
            .map_or(0, |status| status.entries.len());
        let conflicted = self.working_paths.conflicted;
        let identity_ready = self
            .profile
            .as_ref()
            .is_some_and(|p| !p.name.is_empty() && !p.email.is_empty());
        let busy = self.operation_busy.is_some();
        let refreshing = self.status_task.is_some();
        // What a busy spell makes unavailable refuses at once but looks it
        // only once the spell has lasted a moment (`BusyLook`).
        let looks_busy = self.busy_shown();
        // A result is on screen: the status's own rows, or, while a newer
        // status's are prepared, the rows kept from before, which keep the
        // layout they had.
        let shown = self.work_status.is_some()
            && !self.working_filter_waiting()
            && self.working_paths.error.is_none();
        // The commit waits for the rows it counts to be the status's own.
        let status_ready = shown && !self.working_paths.pending;
        // With nothing to list, one line says why (`empty_state`).
        let clean = (total == 0).then(|| {
            if shown {
                EmptyState::new(
                    "working-clean",
                    empty_state::working_clean_line(self.work_status.as_deref(), false),
                )
                .pane(cx)
            } else if refreshing {
                EmptyState::new("working-reading", "Reading the working tree…").pane(cx)
            } else {
                // Refresh, in the header just above, reads it again.
                EmptyState::new("working-unavailable", "Changes unavailable")
                    .tooltip("Refresh to read the working tree again.")
                    .pane(cx)
            }
        });
        // Where the rows would start, at their text inset, the line and
        // Clear filter below it, which empties the field and moves to the
        // list (`empty_state`). Above and below, half the gap: the list is
        // as short as its floor in a half tile at 18 pt. Where the line would
        // wrap, as there, it takes its short form, and where its label does
        // not fit the list the action is its icon, named and tooltipped by
        // its label.
        let no_match = total > 0 && self.working_no_match();
        let unmatched = no_match.then(|| {
            let room = working_no_match_room(fitted_width, window);
            let labelled = room >= labelled_button_width(CLEAR_FILTER, &CLEAR_FILTER_LABEL, window);
            let clear = if labelled {
                empty_state::action("working-clear-filter", CLEAR_FILTER, "close")
            } else {
                crate::views::compact_control_name(
                    empty_state::action("working-clear-filter", "", "close"),
                    "working-clear-filter",
                    CLEAR_FILTER,
                )
                .tooltip(CLEAR_FILTER)
            };
            div()
                .px_3()
                .py(appearance::ui_size(empty_state::GAP / 2.))
                .child(
                    EmptyState::new("working-no-match", working_no_match_line(room, window))
                        .named(WORKING_NO_MATCH)
                        .action(clear.on_click(
                            cx.listener(|this, _, window, cx| {
                                this.clear_working_filter(window, cx)
                            }),
                        ))
                        .block(cx),
                )
        });
        let branch = self
            .work_status
            .as_ref()
            .and_then(|s| s.branch.as_deref())
            .unwrap_or("detached HEAD");
        let summary_length = self.commit_title.read(cx).value().chars().count();
        // The commit waits for a write in flight and for the rows it counts
        // (`status_ready`), which its click refuses at once, and looks
        // unavailable for them once the spell looks busy.
        let commit_disabled = ((busy || !status_ready) && (looks_busy || !shown))
            || staged == 0
            || conflicted
            || !identity_ready
            || self.commit_title.read(cx).value().trim().is_empty();
        // An operation in progress replaces the composer with its banner.
        let composer = self
            .work_status
            .as_ref()
            .is_none_or(|status| status.operation.is_none());
        // A path filter that matches nothing: the list says so in its own
        // bounds, with Clear filter, and draws neither group's heading, as
        // with nothing to list; it keeps the room to show both whole.
        let floor = if no_match {
            working_no_match_floor(fitted_width, window)
        } else {
            working_list_floor(
                working_floor_rows(&self.working_rows),
                self.settings.density,
                window,
            )
        };
        let fit = self.working_fit(
            forms,
            layout.read(cx).inspector,
            fitted_width,
            floor,
            composer,
            short,
            window,
        );
        let metrics = ComposerMetrics::new(short, fit.heading, window);
        let minimum = f32::from(metrics.minimum());
        // The fields' rows but the description's field, at the interface
        // size: the composer prefers room for them whole with that field.
        let rows = self.composer_rows(&metrics, fitted_width, identity_ready, shown, window, cx);
        let description_height = description_height.max(f32::from(metrics.description_least()));
        let composer_height = commit_composer_height(
            available,
            self.settings.density.file_row_height(),
            (description_height + if short { 180. } else { 244. })
                .max(f32::from(metrics.chrome() + rows) + description_height),
        );
        // The list keeps its floor beside the composer's minimum.
        let composer_height = composer_height.min(available - floor).max(minimum);
        // Where its rows overflow it, the list is a whole number of rows tall,
        // so no row is cut through its text at the list's edge: the composer
        // takes the remainder, or, without one, it stays below the list. At
        // its floor the list is whole rows already.
        let row = f32::from(window.pixel_snap(px(self.settings.density.file_row_height())));
        let border = f32::from(window.pixel_snap(px(1.))) * 2.;
        // What is left below the list's last whole row where it has `inner`
        // inside its border, or nothing where its rows do not overflow it.
        let leftover = |inner: f32| {
            if total > 0
                && !no_match
                && inner > floor - border
                && inner < row * self.working_rows.len() as f32
            {
                (inner - ((inner + 0.01) / row).floor() * row).max(0.)
            } else {
                0.
            }
        };
        let remainder = leftover(available - if composer { composer_height } else { 0. } - border);
        let composer_height = composer_height + if composer { remainder } else { 0. };
        // The Description section, whole or absent: where the composer is too
        // short for it, it leaves rather than show its label cut at the
        // commit action, and its field leaves the Tab order with it.
        let description = composer
            .then(|| {
                metrics.description_field(px(composer_height), rows, description_height, window)
            })
            .flatten();
        // Its rows keep their rhythm down to the commit action, a scaled 4 px
        // in a short composer and 0.5 rem in a taller one, rather than the
        // height the composer's share leaves over gathering between the last
        // field and the action. A taller composer with its Description
        // section keeps its share and its description's field grows to take
        // what its rows leave: the field is the composer's one row that reads
        // better taller, and the action stays at the composer's foot. A short
        // composer, or one whose section has left, is drawn no taller than its
        // rows: what it does not use goes back to the list in whole rows, and
        // what is left, less than a row, lies between the list's last row and
        // the composer.
        let section = self.description_section(&metrics, fitted_width, window);
        let content = f32::from(metrics.content(rows, section, description));
        let (drawn, description) = match description {
            Some(field) if composer && !short => (
                composer_height,
                Some(field + (composer_height - content).max(0.)),
            ),
            _ if composer => (composer_height.min(content), description),
            _ => (composer_height, description),
        };
        let below = if !composer {
            remainder
        } else if drawn < composer_height {
            leftover(available - drawn - border)
        } else {
            0.
        };
        // A focused description that leaves hands its focus to the title, so
        // focus never rests on a field that is not drawn.
        if description.is_none()
            && self
                .commit_message
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            let title = self.commit_title.read(cx).focus_handle(cx);
            window.defer(cx, move |window, cx| window.focus(&title, cx));
        }
        // A description the composer cannot show still goes into the commit,
        // so the commit action says so.
        let hidden_description =
            description.is_none() && !self.commit_message.read(cx).value().trim().is_empty();
        // A row that its Stage or Unstage moved travels to its slot above
        // the list, which keeps the slot empty until it lands (`motion`).
        let flight = self.working_flight(cx);
        let landing = flight
            .as_ref()
            .map(|flight| (flight.position, flight.slot.clone()));
        let row_height = px(self.settings.density.file_row_height());
        let list = uniform_list(
            "working-files",
            self.working_rows.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                range
                    .map(|i| match &landing {
                        Some((position, slot)) if *position == i => {
                            crate::motion::slot(slot.clone(), row_height)
                        }
                        _ => this.render_working_row(i, forms, window, cx),
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.working_scroll);
        let travelling = flight.map(|flight| {
            let row = self.render_working_row(flight.position, forms, window, cx);
            crate::motion::TravellingRow::new(flight, row)
        });
        let refresh = button("refresh-working", "", "refresh", false)
            .debug_selector(|| "refresh-working".into())
            .accessibility_label("Refresh working changes")
            .tooltip(if refreshing && looks_busy {
                "Refresh working changes · Refreshing…"
            } else {
                "Refresh working changes"
            })
            .disabled((busy || refreshing) && looks_busy)
            .on_click(cx.listener(|this, _, window, cx| {
                // Refused at once, whatever it looks like yet.
                if this.operation_busy.is_none() && this.status_task.is_none() {
                    this.refresh_worktree(window, cx)
                }
            }));
        // Where the inspector is too short for these controls whole beside
        // the header, the list's floor and the composer's minimum, the
        // filter, Actions and Refresh share one row and the selection
        // controls move into the Actions menu (`WorkingFit`), so no control
        // is cut.
        let controls = if fit.tight {
            vec![
                div()
                    .debug_selector(|| "working-toolbar".into())
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(|| "working-filter".into())
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(
                                Input::new(&self.working_filter)
                                    .aria_label(FILTER_NAME)
                                    .text_size(crate::appearance::ui_text(12.)),
                            )
                            // Painted after the field, it fits its placeholder.
                            .child(self.fit_filter_placeholder()),
                    )
                    .child(self.render_working_recovery_menu(
                        true,
                        Some(!forms.toolbar_refresh),
                        cx,
                    ))
                    .when(forms.toolbar_refresh, |el| el.child(refresh))
                    .into_any_element(),
            ]
        } else {
            vec![
                // The selection controls fit whole; the scroll only guards a
                // prediction that missed.
                native_accessibility::clips_children(
                    div()
                        .id("working-selection")
                        .debug_selector(|| "working-selection".into())
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(self.render_working_selection(forms.compact_selection, cx)),
                )
                .into_any_element(),
                div()
                    .debug_selector(|| "working-header".into())
                    .h(px(header_height))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .when(forms.header_icon, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "working-header-icon".into())
                                .child(icon("commit", 17., p.accent)),
                        )
                    })
                    .when(forms.header_label, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "working-header-label".into())
                                .text_size(crate::appearance::ui_text(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Changes"),
                        )
                    })
                    // The count leaves with its label: alone it would repeat
                    // the Changes tab or toggle's count above it.
                    .when(forms.header_label, |el| {
                        el.child(
                            div()
                                .debug_selector(|| "working-header-count".into())
                                .px_2()
                                .py_0p5()
                                .rounded(px(5.))
                                .bg(rgb(p.hover))
                                .text_size(crate::appearance::ui_text(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(crate::appearance::labels(cx).count))
                                .child(total.to_string()),
                        )
                    })
                    // Actions and Refresh hold the trailing edge; the texts
                    // before them give way, so neither is pushed out.
                    .child(
                        div()
                            .ml_auto()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_2()
                            .when(forms.compact_actions, |el| el.gap_1())
                            .when(refreshing && looks_busy && !forms.compact_actions, |el| {
                                el.child(
                                    div()
                                        .debug_selector(|| "working-refreshing".into())
                                        .text_size(crate::appearance::ui_text(10.))
                                        .text_color(rgb(p.muted))
                                        .child("Refreshing…"),
                                )
                            })
                            .child(self.render_working_recovery_menu(
                                forms.compact_actions,
                                None,
                                cx,
                            ))
                            .child(refresh),
                    )
                    .into_any_element(),
            ]
        };
        let inspector_layout = layout.clone();
        div()
            .size_full().min_h_0().relative().flex().flex_col().bg(rgb(p.panel))
            // The inspector's own height, which no form of its rows changes,
            // chooses the form.
            .child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
                inspector_layout.update(cx, |layout, cx| {
                    if layout.inspector != bounds.size.height {
                        layout.inspector = bounds.size.height;
                        cx.notify();
                        window.defer(cx, |window, _| window.refresh());
                    }
                });
            }).absolute().inset_0())
            .children(controls)
            .child(div().debug_selector(|| "working-body".into()).relative().flex_1().min_h(px(floor + if composer { minimum } else { 0. })).flex().flex_col()
            .child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
                layout.update(cx, |layout, cx| {
                    if layout.height != bounds.size.height || layout.width != bounds.size.width {
                        layout.height = bounds.size.height;
                        layout.width = bounds.size.width;
                        cx.notify();
                        // Refresh is ignored during paint. Defer it so the
                        // cached workspace consumes the measured body height.
                        window.defer(cx, |window, _| window.refresh());
                    }
                });
            }).absolute().inset_0())
            .child(native_accessibility::clips_children(
                div().id("working-pane").debug_selector(|| "working-pane".into()).role(Role::ListBox).aria_label("Working tree files")
                    .tab_stop(true).key_context("GitTurtleList").track_focus(&self.file_focus)
                    .on_action(cx.listener(|this, _: &gpui_kit::component::input::Copy, window, cx| {
                        this.copy_list_item(crate::views::ListCopy::WorkingPaths, window, cx)
                    }))
                    .on_action(cx.listener(move |this, _: &native_accessibility::RowActions, window, cx| {
                        this.open_working_row_menu(forms.file_action, window, cx)
                    }))
                    .border_1().border_color(rgb(p.border)).when(self.focus_ring.shows(&self.file_focus), |pane| pane.border_color(rgb(p.accent)))
                    // One pixel in from the inspector's divider, which the kit
                    // paints over the inspector's first pixel: the list's own
                    // left edge and focus ring stay whole beside it, as the
                    // commit inspector's list does beside its own edge.
                    .ml(px(1.))
                    // Its floor, whatever else gives way, so its focus ring
                    // surrounds a list that shows a file.
                    .relative().flex_1().min_h(px(floor)).overflow_hidden()
                    .when(below > 0., |el| {
                        el.max_h(px(available - if composer { drawn } else { 0. } - below))
                    })
                    .when(total > 0 && !no_match, |el| el.child(list))
                    .children(travelling.filter(|_| total > 0 && !no_match))
                    .children(clean)
                    .children(unmatched)
                    .children(self.file_menu.as_ref().map(|menu| {
                        menu.render("working-row-menu-overlay", Self::dismiss_file_menu, cx)
                    }))
                    .child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
                        list_layout.update(cx, |layout, cx| {
                            if layout.list_size != Some(bounds.size) {
                                // The list's height settles a frame after a
                                // write's rows arrive, as the composer is
                                // measured again: a confirming heading is
                                // shown again in the height it now has.
                                if layout.list_size.is_some() && let Some(heading) = confirming_heading {
                                    reveal_heading(&list_scroll, bounds.size.height, reveal_row, row_count, selected_row, heading);
                                } else if layout.list_size.is_some() && let Some(selected) = selected_row {
                                    list_scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
                                }
                                layout.list_size = Some(bounds.size);
                                cx.notify();
                                window.defer(cx, |window, _| window.refresh());
                            }
                        });
                    }).absolute().inset_0()),
            ))
            .when(composer, |el| el.child(
                div().debug_selector(|| "commit-composer".into())
                    .relative().flex_shrink_0().min_h_0().h(px(drawn)).flex().flex_col()
                    // It ends at the body's foot, above the status bar: what
                    // the list's whole rows leave, less than a row, lies
                    // between the list's last row and the composer rather
                    // than below the commit action.
                    .mt_auto()
                    .bg(rgb(p.subtle)).border_t_1().border_color(rgb(p.border))
                    // The heading is pinned above the scrolling fields, as the
                    // commit action is below them; where the body is short the
                    // file list gives up the room for both, down to its floor.
                    // In the tightest inspector the heading leaves
                    // (`WorkingFit`).
                    .when(fit.heading, |el| el.child(div().flex_shrink_0().flex().items_center().gap_2().px_3().pt_3()
                        .when(short, |el| el.pt_2())
                        .when(forms.heading_icon, |el| el.child(div().debug_selector(|| "commit-composer-icon".into()).child(icon("commit", 16., p.accent))))
                        // Truncating, it keeps "Commit to" and cuts the branch.
                        .child(div().id("commit-composer-heading").debug_selector(|| "commit-composer-heading".into()).flex_1().min_w_0().truncate().text_size(crate::appearance::ui_text(13.)).font_weight(FontWeight::SEMIBOLD)
                            .tooltip({
                                // Without its badge the heading names the count.
                                let tooltip = if forms.heading_badge { format!("Commit to {branch}") } else { format!("Commit to {branch} · {staged} staged") };
                                move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .child(if short { format!("Commit to {branch}") } else { "Create a commit".into() }))
                        .when(forms.heading_badge, |el| el.child(div().debug_selector(|| "commit-staged-count".into()).flex_shrink_0().px_1p5().py_0p5().rounded(px(4.))
                            .bg(rgb(if staged > 0 { p.added_background } else { p.hover }))
                            .text_size(crate::appearance::ui_text(10.)).font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if staged > 0 { crate::appearance::labels(cx).added } else { crate::appearance::labels(cx).count }))
                            .child(format!("{staged} staged"))))))
                    .child(native_accessibility::clips_children(div().id("commit-composer-fields").debug_selector(|| "commit-composer-fields".into()).flex_1().min_h_0().overflow_y_scroll()
                    // The gap that followed the heading leads the fields;
                    // without the heading, the composer's own padding does.
                    // The fields end a gap above the commit action, as they
                    // keep it between themselves (`ComposerMetrics::gap`).
                    .child(div().flex().flex_col().gap_2().px_3().pt_2().pb_2()
                    .when(short, |el| el.gap(crate::appearance::ui_size(4.)).pt(crate::appearance::ui_size(4.)).pb(crate::appearance::ui_size(4.)))
                    .when(!fit.heading, |el| if short { el.pt_2() } else { el.pt_3() })
                    .when(!short, |el| el.child(div().flex().items_center().gap_1p5().min_w_0().text_size(crate::appearance::ui_text(11.))
                        .text_color(rgb(p.muted)).child(icon("branch", 13., p.muted))
                        .child(div().truncate().child(branch.to_owned()))))
                    .child(div().debug_selector(|| "commit-title-label".into()).flex().items_center().gap_2().text_size(crate::appearance::ui_text(11.))
                        .child(div().debug_selector(|| "commit-title-name".into()).flex_1().font_weight(FontWeight::MEDIUM).child("Title"))
                        .child({
                            let counter = div().id("commit-title-counter").debug_selector(|| "commit-title-counter".into())
                                .text_color(rgb(if summary_length > 72 { p.warning } else { p.muted }));
                            let whole = SharedString::from(title_counter(summary_length));
                            if forms.counter_word {
                                counter.child(whole).into_any_element()
                            } else {
                                // Without "suggested" the counter is read by its whole text.
                                crate::views::note_compact_label("commit-title-counter", &whole);
                                native_accessibility::static_text(counter, whole.clone())
                                    .tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx))
                                    .child(format!("{summary_length}/72"))
                                    .into_any_element()
                            }
                        }))
                    .child(div().debug_selector(|| "commit-title".into()).child(Input::new(&self.commit_title).readonly(busy).aria_label("Commit title").text_size(crate::appearance::ui_text(12.))))
                    .when(summary_length > 72, |el| el.child(
                        div().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted)).child(LONG_TITLE_NOTE),
                    ))
                    // Its label, field and hint, whole or not at all
                    // (`ComposerMetrics::description_field`).
                    .when_some(description, |el, field| el
                        .child(div().debug_selector(|| "commit-description-label".into()).flex().items_center().gap_1().text_size(crate::appearance::ui_text(11.))
                            .child(div().debug_selector(|| "commit-description-name".into()).font_weight(FontWeight::MEDIUM).child("Description"))
                            .when(forms.description_note, |el| el.child(div().debug_selector(|| "commit-description-optional".into()).text_color(rgb(p.muted)).child("· optional"))))
                        // Tab and Shift-Tab move focus on and back, as on every
                        // other stop, rather than indenting.
                        .child(shortcuts::tab_leaves(Textarea::new(&self.commit_message).readonly(busy)
                                .aria_label("Commit description, optional")
                                .h(px(field))
                                .text_size(crate::appearance::ui_text(12.)))
                            .debug_selector(|| "commit-description".into()))
                        .child(div().debug_selector(|| "commit-description-hint".into()).flex().items_center().gap_2().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted))
                            // Shrinking below its one line, it wraps as `description_section`
                            // predicts rather than running past the column at 18 pt.
                            .child(div().flex_1().min_w_0().child(if staged == 0 { UNSTAGED_HINT } else { STAGED_HINT }))))
                    .when_some(self.draft_save_error.as_ref(), |el, error| el.child(
                        div().flex().flex_col().gap_1().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.warning))
                            .child(DRAFT_SAVE_NOTE)
                            .child(button("retry-commit-draft", "Retry saving draft", "refresh", false)
                                .tooltip(error.clone())
                                .on_click(cx.listener(|this, _, window, cx| this.persist_commit_draft(window, cx)))),
                    ))
                    .when(!identity_ready && shown, |el| el.child(
                        button("configure-identity", "Set your commit identity", "", false)
                            .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx))),
                    ))
                    .when(conflicted, |el| el.child(
                        div().flex().items_center().gap_2().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))
                            .child(icon("file-conflict", 15., p.warning))
                            // Shrinking below its one line, it wraps as `composer_rows`
                            // predicts rather than running past the column.
                            .child(div().debug_selector(|| "commit-conflict-note".into()).flex_1().min_w_0().child(CONFLICT_NOTE)),
                    )))))
                    .child(div().flex_shrink_0().px_3().pb_3().when(short, |el| el.pb_2()).child({
                        // Disabled, it rests on the composer's own `subtle`, so it
                        // keeps its shape with an edge; it is as wide as the composer.
                        let commit = crate::appearance::keep_disabled_shape(
                            Button::new("commit-staged").debug_selector(|| "commit-staged".into()).primary().w_full().h(crate::appearance::ui_size(36.)),
                            commit_disabled, p.subtle, None, cx)
                            .icon(Icon::default().path("icons/commit.svg").size(px(16.)));
                        let label = self.commit_action_label(cx);
                        let detail = format!(
                            "Commit staged changes to {branch}{}",
                            if hidden_description { " · Includes the description, shown in a larger window" } else { "" }
                        );
                        // Where its label does not fit, it reads "Commit" and
                        // keeps the label as its name and leading its tooltip.
                        if forms.compact_commit {
                            let tooltip = format!("{label} · {detail}");
                            crate::views::compact_control_label(commit, "commit-staged", label)
                                .label("Commit")
                                .tooltip(tooltip)
                        } else {
                            commit.label(label).tooltip(detail)
                        }
                        .disabled(commit_disabled)
                        .on_click(cx.listener(|this, _, window, cx| {
                            // Refused at once, whatever it looks like yet.
                            if this.working_paths.pending {
                                return;
                            }
                            let message = this.current_commit_draft(cx).message();
                            this.write(WriteCommand::Commit { message }, "Creating commit…", window, cx);
                        }))
                    }))
                    // Painted after both fields, it fits their placeholders.
                    .child(placeholder_fit(
                        self.commit_title.clone(),
                        &TITLE_FIT,
                        &[TITLE_PLACEHOLDER, TITLE_SHORT_PLACEHOLDER],
                        12.,
                    ))
                    .child(placeholder_fit(
                        self.commit_message.clone(),
                        &DESCRIPTION_FIT,
                        &[DESCRIPTION_PLACEHOLDER],
                        12.,
                    )),
            )))
            .into_any_element()
    }

    fn render_working_row(
        &self,
        position: usize,
        forms: WorkingForms,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        // A write in flight, or the rows the reads after it are preparing,
        // leave these controls inert at once (`write_from`,
        // `depart_working_row`) and dimmed once the spell looks busy.
        let looks_busy = self.busy_shown();
        // The shown rows index the shown status, an earlier one while a
        // newer status's rows are prepared.
        match self.working_rows[position] {
            WorkingRow::Directory(index, _) => {
                let parent = self
                    .shown_working_status()
                    .and_then(|s| s.entries.get(index))
                    .and_then(|entry| entry.path.parent())
                    .filter(|path| !path.as_os_str().is_empty())
                    .map_or("Repository root".into(), |path| path.display().to_string());
                div()
                    .h(px(self.settings.density.file_row_height()))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(parent)
                    .into_any_element()
            }
            WorkingRow::Heading(area, count) => {
                let staged = area == ChangeArea::Staged;
                let name = if staged { "Staged" } else { "Unstaged" };
                let (selector, action, detail) = if staged {
                    (
                        "unstage-all",
                        "Unstage all",
                        "Remove all staged changes from the next commit",
                    )
                } else {
                    (
                        "stage-all",
                        "Stage all",
                        "Include all local changes in the next commit",
                    )
                };
                // Where its label does not fit, the action keeps its icon,
                // with its name and a tooltip that leads with it. Either way
                // it reads Staged or Unstaged where it was touched once Git
                // reports success, in its label's place or in its tooltip.
                let (mark, confirmation) = (
                    confirmations::all(&self.path, area),
                    if staged {
                        confirmations::UNSTAGE_ALL
                    } else {
                        confirmations::STAGE_ALL
                    },
                );
                let origin = mark.clone();
                let confirming = copy_feedback::confirmed(&mark, cx);
                let unavailable = ((busy || self.working_paths.pending) && looks_busy)
                    || count == 0
                    || self.working_paths.error.is_some()
                    || !self.working_filter.read(cx).value().is_empty();
                let control = if forms.section_action_label {
                    copy_feedback::feedback_button(mark, confirmation, action, detail, cx)
                } else {
                    crate::views::compact_control_name(
                        copy_feedback::feedback_button(
                            mark,
                            confirmation,
                            "",
                            format!("{action} · {detail}"),
                            cx,
                        ),
                        selector,
                        action,
                    )
                };
                let part = move |part: &'static str| -> String {
                    format!(
                        "{}-heading-{part}",
                        if staged { "staged" } else { "unstaged" }
                    )
                };
                div()
                    .debug_selector(move || format!("working-row-{position}"))
                    .w_full()
                    .h(px(self.settings.density.file_row_height()))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.subtle))
                    .when(forms.section_icon, |el| {
                        el.child(div().debug_selector(move || part("icon")).child(icon(
                            if staged { "check" } else { "code" },
                            15.,
                            if staged { p.added } else { p.modified },
                        )))
                    })
                    .when(forms.section_label, |el| {
                        el.child(
                            div()
                                .debug_selector(move || part("label"))
                                .text_size(crate::appearance::ui_text(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(name),
                        )
                    })
                    .child(
                        div()
                            .id(if staged {
                                "staged-heading-count"
                            } else {
                                "unstaged-heading-count"
                            })
                            .debug_selector(move || part("count"))
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            // Without its name the count says what it counts.
                            .when(!forms.section_label, |el| {
                                el.tooltip(move |window, cx| {
                                    Tooltip::new(format!("{name} · {count}")).build(window, cx)
                                })
                            })
                            .child(count.to_string()),
                    )
                    .child(
                        control
                            .debug_selector(move || selector.into())
                            .ml_auto()
                            .disabled(unavailable)
                            // The write it confirms leaves it unavailable, its
                            // group emptied or the reads after it pending, and
                            // the disabled colour, 3.19:1 on Midnight's heading,
                            // read the outcome as unavailable. While it confirms
                            // it stays inert in its resting label colour, which
                            // the kit replays over the disabled one; it takes
                            // the disabled look when the confirmation ends.
                            .when(unavailable && confirming, |control| {
                                control.text_color(rgb(p.control_label()))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                // Refused at once while the rows it counts
                                // are prepared, whatever it looks like yet.
                                if this.working_paths.pending {
                                    return;
                                }
                                this.write_from(
                                    if staged {
                                        WriteCommand::UnstageAll
                                    } else {
                                        WriteCommand::StageAll
                                    },
                                    if staged {
                                        "Unstaging files…"
                                    } else {
                                        "Staging files…"
                                    },
                                    Some(origin.clone().into()),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .into_any_element()
            }
            WorkingRow::File(index, area) => {
                let entry = &self.shown_working_status().unwrap().entries[index];
                let paths = entry.paths();
                let row_path = entry.path.clone();
                let ignore_entry = entry.clone();
                let ignore_owner = cx.entity().downgrade();
                let ignore_repository = self.path.clone();
                let name = entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let parent = entry
                    .path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| !p.is_empty())
                    .unwrap_or("Repository root".into());
                let active = self.working_is_selected(index, area);
                let staged = area == ChangeArea::Staged;
                let path_detail = entry.original_path.as_ref().map_or_else(
                    || entry.path.display().to_string(),
                    |original| format!("{} → {}", original.display(), entry.path.display()),
                );
                let stage_label = format!(
                    "{} {}",
                    if staged { "Unstage" } else { "Stage" },
                    entry.path.display()
                );
                let change = if staged { entry.staged } else { entry.unstaged };
                let (symbol, status_label, color, background) = if entry.conflicted {
                    ("file-conflict", "Conflict", p.warning, p.hover)
                } else if entry.untracked || change == Some(ChangeStatus::Added) {
                    ("file-added", "New", p.added, p.added_background)
                } else {
                    match change.unwrap_or(ChangeStatus::Modified) {
                        ChangeStatus::Added => ("file-added", "New", p.added, p.added_background),
                        ChangeStatus::Modified => {
                            ("file-modified", "Modified", p.modified, p.hover)
                        }
                        ChangeStatus::Deleted => {
                            ("file-deleted", "Deleted", p.removed, p.removed_background)
                        }
                        ChangeStatus::Renamed => ("file-renamed", "Renamed", p.renamed, p.hover),
                        ChangeStatus::TypeChanged => ("file-type", "Type", p.warning, p.hover),
                    }
                };
                // Without its status word the row's tooltip names the status.
                let path_detail = if forms.file_status {
                    path_detail
                } else {
                    format!("{path_detail} · {status_label}")
                };
                div()
                    .id((
                        if staged {
                            "staged-file"
                        } else {
                            "unstaged-file"
                        },
                        index,
                    ))
                    .role(Role::ListBoxOption)
                    .aria_label(format!(
                        "{} · {} · {}",
                        entry.path.display(),
                        status_label,
                        if staged { "Staged" } else { "Unstaged" }
                    ))
                    .debug_selector(move || format!("working-row-{position}"))
                    .aria_selected(active)
                    .when(self.shown_working_cursor() == Some((index, area)), |row| {
                        row.aria_active_descendant()
                    })
                    .h(px(self.settings.density.file_row_height()))
                    .w_full()
                    .pl(px(10.))
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(if active { p.selected } else { p.panel }))
                    .border_l_2()
                    .border_color(if active { rgb(p.accent) } else { rgba(0) })
                    .hover(move |s| s.bg(rgb(p.row_hover(active))))
                    .active(move |s| s.bg(rgb(p.selected)))
                    .cursor_pointer()
                    // Where the name needs its room, the tile gives way to
                    // its bare glyph, and then the glyph leaves.
                    .when(forms.file_tile, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("working-tile-{position}"))
                                .size(px(26.))
                                .flex_shrink_0()
                                .rounded(px(7.))
                                .bg(rgb(background))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(symbol, 16., color)),
                        )
                    })
                    .when(forms.file_glyph, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("working-glyph-{position}"))
                                .flex_shrink_0()
                                .child(icon(symbol, 16., color)),
                        )
                    })
                    .child(
                        div()
                            .id(("working-path", position))
                            .debug_selector(move || format!("working-path-{position}"))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            // GPUI's existing tooltip can outlive an occluding
                            // menu. Removing its builder clears that tooltip.
                            .when(
                                !window
                                    .context_stack()
                                    .iter()
                                    .any(|context| context.contains("PopupMenu")),
                                |path| {
                                    path.tooltip(move |window, cx| {
                                        let text = path_detail.clone();
                                        Tooltip::element(move |window, _| {
                                            div()
                                                .id("working-path-tooltip")
                                                .debug_selector(|| "working-path-tooltip".into())
                                                .max_w(
                                                    px(420.).min(
                                                        window.viewport_size().width - px(80.),
                                                    ),
                                                )
                                                .max_h(window.viewport_size().height * 0.5)
                                                .overflow_hidden()
                                                .whitespace_normal()
                                                .child(text.clone())
                                        })
                                        .build(window, cx)
                                    })
                                },
                            )
                            // Each line fits whole or ends in an ellipsis
                            // inside the row, never cut by half a pixel.
                            .child(
                                crate::views::ellipsis_end(
                                    div()
                                        .debug_selector(move || format!("working-name-{position}")),
                                )
                                .text_size(crate::appearance::ui_text(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                            )
                            .child(
                                crate::views::ellipsis_end(div())
                                    .text_size(crate::appearance::ui_text(10.))
                                    .text_color(rgb(p.muted))
                                    .child(parent),
                            ),
                    )
                    .when(forms.file_status, |row| {
                        row.child(
                            div()
                                .debug_selector(move || format!("working-status-{position}"))
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_text(10.))
                                .text_color(rgb(p.text))
                                .child(status_label),
                        )
                    })
                    .when(forms.file_action, |row| {
                        // Icon-only, it reads its confirmation in its tooltip
                        // once the Stage or Unstage that moved its file here
                        // succeeded, and keeps its name.
                        row.child(
                            copy_feedback::feedback_button(
                                confirmations::file(&self.path, &entry.path, area),
                                if staged {
                                    confirmations::STAGED_FILE
                                } else {
                                    confirmations::UNSTAGED_FILE
                                },
                                "",
                                stage_label.clone(),
                                cx,
                            )
                            .debug_selector(move || format!("working-stage-{position}"))
                            .accessibility_label(stage_label)
                            .disabled(busy && looks_busy)
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.depart_working_row(position, window, cx);
                                    this.stage_working_paths(
                                        paths.clone(),
                                        &row_path,
                                        staged,
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        let modifiers = event.modifiers();
                        this.choose_working_selection(
                            index,
                            area,
                            modifiers.platform,
                            modifiers.shift,
                            window,
                            cx,
                        )
                    }))
                    .app_context_menu(move |menu, _, _| {
                        working_row_menu(
                            menu,
                            ignore_owner.clone(),
                            ignore_repository.clone(),
                            MenuRow {
                                entry: ignore_entry.clone(),
                                row: position,
                                staged,
                            },
                            forms.file_action,
                            busy,
                        )
                    })
                    .into_any_element()
            }
        }
    }

    /// Opens the Working Changes cursor row's menu beside the row, from
    /// Shift-F10 or the Menu key: the menu a right-click on the row opens,
    /// which leads with Stage or Unstage where the row has no room for that
    /// control (`stage_control`, the rows' form when the list was drawn).
    fn open_working_row_menu(
        &mut self,
        stage_control: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_menu.is_some() || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some((index, area)) = self.working_selected else {
            return;
        };
        let Some(row) = self
            .actionable_working_rows()
            .iter()
            .position(|row| matches!(row, WorkingRow::File(i, a) if (*i, *a) == (index, area)))
        else {
            return;
        };
        let Some(entry) = self
            .work_status
            .as_ref()
            .and_then(|status| status.entries.get(index))
            .cloned()
        else {
            return;
        };
        let position = native_accessibility::KeyboardMenu::below_row(
            &self.working_scroll,
            row,
            window.pixel_snap(px(self.settings.density.file_row_height())),
            appearance::ui_size(appearance::PANE_INSET),
        );
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        let busy = self.operation_busy.is_some();
        self.file_menu = Some(native_accessibility::KeyboardMenu::open(
            position,
            self.file_focus.clone(),
            move |menu| {
                working_row_menu(
                    menu,
                    owner,
                    repository,
                    MenuRow {
                        entry,
                        row,
                        staged: area == ChangeArea::Staged,
                    },
                    stage_control,
                    busy,
                )
            },
            Self::dismiss_file_menu,
            window,
            cx,
        ));
        cx.notify();
    }

    /// Closes the file list's keyboard menu. A press outside it leaves focus
    /// in the menu, so focus returns to the list; an item or Escape has moved
    /// it already, to the list or to the dialog the item opened.
    pub(super) fn dismiss_file_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.file_menu.take() {
            if menu.has_focus(window, cx) || window.focused(cx).is_none() {
                window.focus(&self.file_focus, cx);
            }
            cx.notify();
        }
    }

    /// The Git action bar. `compact`, in a window too short for its full form
    /// (`views::Chrome`), it is one row of 28 px controls: the branch picker
    /// and sync state give up their width first, and Fetch, Pull, Push and
    /// Targets keep only their icons, their accessible names, and tooltips
    /// that lead with those names. Their actions, states and order are the
    /// full form's.
    /// `edge_form` is how the repository edge is drawn
    /// (`views::GitTurtle::edge_form`); the compact form draws its barest
    /// form, its shortest line or, where even that would be cut, its glyph,
    /// and a detached HEAD's picker names the commit. An action drawn as its
    /// icon keeps its label as its accessible name and tooltip, and a line
    /// without its caption, or a glyph, keeps the whole line as its name and
    /// in its tooltip.
    pub(super) fn render_git_actions(
        &self,
        compact: bool,
        edge_form: crate::empty_state::EdgeForm,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        // A write in flight leaves the actions inert at once (`write_from`,
        // `run_edge_action`) and dimmed once its spell looks busy.
        let looks_busy = self.operation_shown().is_some();
        let branch = self
            .work_status
            .as_ref()
            .and_then(|s| s.branch.clone())
            .unwrap_or("Detached HEAD".into());
        let selected_remote = self.remote_name.read(cx).value().trim().to_owned();
        let selected_branch = self.remote_branch.read(cx).value().trim().to_owned();
        let remote = self
            .remotes
            .iter()
            .find(|remote| remote.name == selected_remote);
        let target = format!("{selected_remote}/{selected_branch}");
        let branch_empty = self.branch_name.read(cx).value().trim().is_empty();
        let detached = self.work_status.as_ref().is_none_or(|s| s.branch.is_none());
        let branch_picker = self.render_branch_picker(compact, cx);
        // A compact row's texts truncate rather than push the actions away.
        let shrinking = |text: Div| {
            if compact {
                text.min_w_0().truncate()
            } else {
                text
            }
        };
        // A repository at an edge (detached HEAD, no remote, no commits yet
        // or no upstream) says so in the counts' place, and its next action, if any,
        // joins the actions (`empty_state`).
        let edge = self.repository_edge();
        // The edge's action joins the actions, after Push and before
        // Targets, so in a half tile it wraps with Targets rather than
        // pushing Push to a row of its own.
        let edge_action =
            edge.as_ref().and_then(|edge| edge.action()).map(|action| {
                let labelled = edge_form.labelled && !compact;
                let control = if labelled {
                    button(action.id, action.label, action.symbol, false).secondary()
                } else {
                    crate::views::compact_control_name(
                        button(action.id, "", action.symbol, false).secondary(),
                        action.id,
                        action.label,
                    )
                    .tooltip(action.label)
                };
                let inset = appearance::ui_size(if labelled {
                    crate::LABEL_INSET
                } else {
                    crate::ICON_INSET
                });
                appearance::keep_disabled_shape(control, looks_busy, p.panel, Some(inset), cx)
                    .debug_selector(move || action.id.into())
                    .disabled(looks_busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_edge_action(action, window, cx)
                    }))
            });
        let sync_state = self.work_status.as_ref().map(|status| {
            if let Some(edge) = &edge {
                let caption = edge_form.caption && !compact;
                return div()
                    .px_2()
                    .when(compact, |el| el.min_w_0())
                    .child(
                        EmptyState::new(
                            "branch-edge",
                            if compact {
                                edge.compact_line()
                            } else if caption {
                                edge.line()
                            } else {
                                edge.short_line()
                            },
                        )
                        .named(edge.line())
                        .tooltip(edge.explanation())
                        .glyph((compact && edge_form.glyph).then(|| edge.glyph()))
                        .truncating(compact)
                        .inline(cx),
                    )
                    .into_any_element();
            }
            let explanation = status.upstream.as_ref().map_or_else(
                || "No upstream is configured.".to_owned(),
                |upstream| {
                    format!("Compared with local {upstream}. Fetch to update remote information.")
                },
            );
            // Truncated, the counts stay readable in the tooltip.
            let tooltip = if compact {
                format!("{} · {explanation}", sync_summary(status))
            } else {
                explanation
            };
            div()
                .id("branch-sync-state")
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .text_size(crate::appearance::ui_text(11.))
                .when(compact, |el| el.min_w_0().overflow_hidden())
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .when(status.upstream.is_some(), |el| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(compact, |el| el.min_w_0())
                            .text_color(rgb(if status.ahead > 0 { p.added } else { p.muted }))
                            .child(icon(
                                "push",
                                12.,
                                if status.ahead > 0 { p.added } else { p.muted },
                            ))
                            .child(shrinking(div()).child(format!("{} ahead", status.ahead))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(compact, |el| el.min_w_0())
                            .text_color(rgb(if status.behind > 0 {
                                p.modified
                            } else {
                                p.muted
                            }))
                            .child(icon(
                                "pull",
                                12.,
                                if status.behind > 0 {
                                    p.modified
                                } else {
                                    p.muted
                                },
                            ))
                            .child(shrinking(div()).child(format!("{} behind", status.behind))),
                    )
                })
                .into_any_element()
        });
        let operation_state = self
            .work_status
            .as_ref()
            .and_then(|s| s.operation.as_ref())
            .map(|operation| {
                div()
                    .px_2()
                    .py_1()
                    .rounded(px(5.))
                    .bg(rgb(p.hover))
                    .text_size(crate::appearance::ui_text(11.))
                    .when(compact, |el| el.min_w_0().truncate())
                    .text_color(rgb(p.warning))
                    .child(format!("{operation} in progress"))
            });
        let actions = [("fetch", "Fetch"), ("pull", "Pull"), ("push", "Push")]
            .into_iter()
            .map(|(id, label)| {
                let unavailable = looks_busy
                    || remote.is_none()
                    || (id != "fetch" && (selected_branch.is_empty() || detached));
                let tooltip = if looks_busy {
                    self.operation_busy.unwrap_or_default().to_owned()
                } else if remote.is_none() {
                    "No configured remote selected · Check Targets".to_owned()
                } else if id != "fetch" && detached {
                    "Switch to a local branch before pulling or pushing".to_owned()
                } else if id != "fetch" && selected_branch.is_empty() {
                    "Choose a remote branch in Targets".to_owned()
                } else {
                    match id {
                        "fetch" => format!("Fetch from {selected_remote}"),
                        "pull" => format!("Pull {target} into {branch} · fast-forward only"),
                        _ => format!("Push {branch} to {target}"),
                    }
                };
                let name = format!("{label}: {tooltip}");
                // Each reads its confirmation where it was touched once Git
                // reports success, for the target it acted on: labelled in its
                // label's place, as an icon in its tooltip. Its name stays.
                let (confirmation, target) = match id {
                    "fetch" => (confirmations::FETCH, vec![selected_remote.as_str()]),
                    "pull" => (
                        confirmations::PULL,
                        vec![selected_remote.as_str(), selected_branch.as_str()],
                    ),
                    _ => (
                        confirmations::PUSH,
                        vec![
                            branch.as_str(),
                            selected_remote.as_str(),
                            selected_branch.as_str(),
                        ],
                    ),
                };
                let mark = confirmations::sync(id, &self.path, &target);
                let control = if compact {
                    let tooltip = format!("{label} · {tooltip}");
                    crate::views::compact_control_label(
                        copy_feedback::feedback_button(mark, confirmation, "", tooltip, cx),
                        id,
                        name.clone(),
                    )
                    .map(|button| {
                        if id == "push" {
                            button.primary()
                        } else {
                            button.secondary()
                        }
                    })
                } else {
                    // Its rest width decides where the bar wraps, so the
                    // past tense takes its icon's and label's room alone.
                    copy_feedback::confirming(
                        action_frame(id),
                        rems(0.5),
                        copy_feedback::Fit::Within,
                        mark,
                        confirmation,
                        label,
                        tooltip,
                        cx,
                    )
                    .when(id == "push", |button| button.primary())
                };
                // Disabled, each rests on the neutral `subtle`, 1.046:1 off
                // Midnight's toolbar, so there it keeps its shape with an edge
                // that takes its pixel from the padding.
                let inset = if compact {
                    appearance::ui_size(crate::ICON_INSET)
                } else {
                    action_inset()
                };
                appearance::keep_disabled_shape(control, unavailable, p.panel, Some(inset), cx)
                    .debug_selector(move || id.into())
                    .disabled(unavailable)
                    .accessibility_label(name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let remote = this.remote_name.read(cx).value().trim().to_owned();
                        let branch = this.remote_branch.read(cx).value().trim().to_owned();
                        let local_branch = this
                            .work_status
                            .as_ref()
                            .and_then(|s| s.branch.clone())
                            .unwrap_or_default();
                        let target = match id {
                            "fetch" => vec![remote.as_str()],
                            "pull" => vec![remote.as_str(), branch.as_str()],
                            _ => vec![local_branch.as_str(), remote.as_str(), branch.as_str()],
                        };
                        let origin = confirmations::sync(id, &this.path, &target);
                        let command = match id {
                            "fetch" => WriteCommand::Fetch { remote },
                            "pull" => WriteCommand::Pull { remote, branch },
                            _ => WriteCommand::Push {
                                remote,
                                local_branch,
                                remote_branch: branch,
                            },
                        };
                        this.write_from(
                            command,
                            match id {
                                "fetch" => "Fetching remote…",
                                "pull" => "Pulling fast-forward…",
                                _ => "Pushing branch…",
                            },
                            Some(origin.into()),
                            window,
                            cx,
                        );
                    }))
            });
        let open = self.git_actions_open;
        let targets = if compact {
            crate::views::compact_control_name(
                button("git-actions-toggle", "", "sliders", open),
                "git-actions-toggle",
                "Targets",
            )
            .tooltip(if open {
                "Targets · Hide branch and remote targets"
            } else {
                "Targets · Show branch and remote targets"
            })
        } else {
            button("git-actions-toggle", "Targets", "sliders", open).tooltip(if open {
                "Hide branch and remote targets"
            } else {
                "Show branch and remote targets"
            })
        }
        .debug_selector(|| "git-actions-toggle".into())
        .toggled(open)
        .on_click(cx.listener(|this, _, _, cx| {
            this.git_actions_open = !this.git_actions_open;
            cx.notify();
        }));
        let row = if compact {
            // The branch picker and sync state share a group that wraps as if
            // it had no width, so the row breaks only when the actions alone
            // overflow it; the group then gives up its room. The header's
            // compact row keeps the room a focused control's ring takes above
            // these, so this row keeps it only below them.
            div()
                .px_3()
                .pb(appearance::button_ring_room(cx))
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(branch_picker)
                        .children(sync_state)
                        .children(operation_state),
                )
                .children(actions)
                .children(edge_action)
                .child(targets)
        } else {
            div()
                .min_h(crate::appearance::ui_size(46.))
                .px_3()
                .py_1()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(branch_picker)
                .children(sync_state)
                .children(operation_state)
                .child(div().flex_1())
                .children(actions)
                .children(edge_action)
                .child(targets)
        };
        div().debug_selector(|| "git-actions".into()).flex().flex_col().flex_shrink_0().bg(rgb(p.panel)).border_b_1().border_color(rgb(p.border))
            .child(row)
            .when(self.git_actions_open, |el| el.child(
                div().px_4().pb_3().flex().flex_col().gap_2()
                    .child(div().flex().flex_wrap().items_end().gap_3()
                        .child(div().flex().flex_col().gap_1()
                            .child(action_field_label("Find or create a branch", p.muted))
                            .child(div().flex().items_center().gap_1()
                                .child(div().w(appearance::ui_size(170.)).child(Input::new(&self.branch_name).aria_label("Branch to switch to or create").text_size(crate::appearance::ui_text(12.)).disabled(busy)))
                                // Each reads Switched or Created where it was
                                // touched once Git reports success, though the
                                // name field it read has emptied.
                                .child(copy_feedback::feedback_button(confirmations::branch_form("checkout-branch", &self.path), confirmations::SWITCH, "Switch", "Switch to the named local branch", cx).disabled(looks_busy || branch_empty)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let branch = this.branch_name.read(cx).value().trim().to_owned();
                                        let origin = confirmations::branch_form("checkout-branch", &this.path);
                                        this.write_from(WriteCommand::Checkout { branch }, "Switching branch…", Some(origin.into()), window, cx);
                                    })))
                                .child(copy_feedback::feedback_button(confirmations::branch_form("create-branch", &self.path), confirmations::CREATE, "Create", "Create and switch to a branch from HEAD", cx).disabled(looks_busy || branch_empty)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let name = this.branch_name.read(cx).value().trim().to_owned();
                                        let origin = confirmations::branch_form("create-branch", &this.path);
                                        this.write_from(WriteCommand::CreateBranch { name, start_point: None }, "Creating branch…", Some(origin.into()), window, cx);
                                    })))))
                        .child(div().w(px(1.)).h(crate::appearance::ui_size(32.)).mx_1().bg(rgb(p.border)))
                        .child(div().w(appearance::ui_size(120.)).flex().flex_col().gap_1()
                            .child(action_field_label("Remote", p.muted))
                            .child(Input::new(&self.remote_name).aria_label("Git remote target").text_size(crate::appearance::ui_text(12.)).disabled(busy)))
                        .child(div().w(appearance::ui_size(200.)).flex().flex_col().gap_1()
                            .child(action_field_label("Remote branch", p.muted))
                            .child(Input::new(&self.remote_branch).aria_label("Remote branch target").text_size(crate::appearance::ui_text(12.)).disabled(busy))))
                    .child(div().flex().items_center().gap_1p5().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))
                        .child(icon("remote", 12., p.muted))
                        .child(div().min_w_0().truncate().child(remote.map_or_else(
                            || "No remote configured · branch and commit actions are available locally".to_owned(),
                            |remote| format!("Push to {} · {}", target, display_remote_url(&remote.push_url)),
                        )))),
            ))
            .into_any_element()
    }
}

/// The action bar's sync state as one line: the ahead and behind counts, or
/// that no upstream is configured.
fn sync_summary(status: &gitturtle_core::RepositoryStatus) -> String {
    if status.upstream.is_some() {
        format!("{} ahead, {} behind", status.ahead, status.behind)
    } else {
        "No upstream".into()
    }
}

/// The space beside a labelled toolbar action's content: the 0.75 rem it took
/// as `px_3` at the root rem the app sets, the interface text's 13 px.
/// The file row a Working Changes menu acts on: its status entry, its place
/// among the list's rows, where a Stage or Unstage departs from, and whether it
/// is the staged row.
struct MenuRow {
    entry: StatusEntry,
    row: usize,
    staged: bool,
}

/// A Working Changes file row's menu, which a right-click on the row and
/// Shift-F10 or the Menu key on the list's cursor row open alike. Without its
/// Stage or Unstage control (`stage_control`) the row's menu leads with that
/// action. Each item acts only on the repository the menu was built for.
fn working_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    target: MenuRow,
    stage_control: bool,
    busy: bool,
) -> PopupMenu {
    let MenuRow { entry, row, staged } = target;
    let discard_owner = owner.clone();
    let discard_repository = repository.clone();
    let discard_entry = entry.clone();
    let menu = if stage_control {
        menu
    } else {
        let (owner, repository) = (owner.clone(), repository.clone());
        let paths = entry.paths();
        let path = entry.path.clone();
        menu.item(
            PopupMenuItem::new(if staged { "Unstage" } else { "Stage" })
                .disabled(busy)
                .on_click(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.path == repository {
                            this.depart_working_row(row, window, cx);
                            this.stage_working_paths(paths.clone(), &path, staged, window, cx);
                        }
                    });
                }),
        )
    };
    // An action the row cannot take reads as its command with the condition
    // it waits for apart from it, not as one phrase.
    let ignore = if entry.untracked {
        PopupMenuItem::new("Ignore…")
    } else {
        native_accessibility::detailed_item("Ignore…", "Untracked files only")
    };
    let discard = if discard_entry.conflicted {
        native_accessibility::detailed_item("Discard changes…", "After conflict resolution")
    } else if discard_entry.untracked {
        PopupMenuItem::new("Delete untracked file…")
    } else {
        PopupMenuItem::new("Discard changes…")
    };
    menu.item(
        ignore
            .disabled(busy || staged || !entry.untracked)
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.path == repository {
                        this.open_ignore(entry.clone(), window, cx);
                    }
                });
            }),
    )
    // Discard reverts the whole file to HEAD, so both the staged and
    // unstaged rows offer the same action.
    .item(
        discard
            .disabled(busy || discard_entry.conflicted)
            .on_click(move |_, window, cx| {
                let _ = discard_owner.update(cx, |this, cx| {
                    if this.path == discard_repository {
                        this.open_discard(discard_entry.clone(), window, cx);
                    }
                });
            }),
    )
}

fn action_inset() -> Pixels {
    appearance::ui_text(13.) * 0.75
}

/// The Git writes that confirm on the control that asked for them
/// ([`GitTurtle::write_from`]): each control's icon at rest and the word it
/// reads once Git reports success, after the check or, in the action bar's
/// labelled Fetch, Pull and Push, alone (`copy_feedback::Fit`).
pub(super) mod confirmations {
    use crate::copy_feedback::{Action, Mark};
    use gitturtle_core::{BranchCommand, ChangeArea, WriteCommand};
    use std::path::{Path, PathBuf};

    /// A file row's control in Staged, which unstages at rest and reads
    /// Staged once that file's Stage succeeded.
    pub(crate) const STAGED_FILE: Action = Action {
        icon: "minus",
        done: "Staged",
        state: "staged",
    };
    /// A file row's control in Unstaged, which stages at rest and reads
    /// Unstaged once that file's Unstage succeeded.
    pub(crate) const UNSTAGED_FILE: Action = Action {
        icon: "plus",
        done: "Unstaged",
        state: "unstaged",
    };
    pub(crate) const STAGE_ALL: Action = Action {
        icon: "plus",
        done: "Staged",
        state: "staged",
    };
    pub(crate) const UNSTAGE_ALL: Action = Action {
        icon: "minus",
        done: "Unstaged",
        state: "unstaged",
    };
    pub(crate) const FETCH: Action = Action {
        icon: "fetch",
        done: "Fetched",
        state: "fetched",
    };
    pub(crate) const PULL: Action = Action {
        icon: "pull",
        done: "Pulled",
        state: "pulled",
    };
    pub(crate) const PUSH: Action = Action {
        icon: "push",
        done: "Pushed",
        state: "pushed",
    };
    pub(crate) const CREATE: Action = Action {
        icon: "branch-add",
        done: "Created",
        state: "created",
    };
    pub(crate) const SWITCH: Action = Action {
        icon: "branch",
        done: "Switched",
        state: "switched",
    };

    /// The control of `path`'s row in `area` of `repository`'s working
    /// changes. A file staged arrives in Staged, so its Stage confirms on the
    /// control of its row there, the row it moved to, and an Unstage on its
    /// row in Unstaged.
    pub(crate) fn file(repository: &Option<PathBuf>, path: &Path, area: ChangeArea) -> Mark {
        Mark::per_subject(
            match area {
                ChangeArea::Staged => "staged-file-action",
                ChangeArea::Unstaged => "unstaged-file-action",
            },
            (repository, path),
        )
    }

    /// Stage all, or in Staged Unstage all, of `repository`.
    pub(crate) fn all(repository: &Option<PathBuf>, area: ChangeArea) -> Mark {
        Mark::new(
            match area {
                ChangeArea::Staged => "unstage-all",
                ChangeArea::Unstaged => "stage-all",
            },
            repository,
        )
    }

    /// The action bar's Fetch, Pull or Push (`id`) of `repository` toward
    /// `target`, its remote and branches, so changing Targets leaves the
    /// next target's control at rest.
    pub(crate) fn sync(id: &'static str, repository: &Option<PathBuf>, target: &[&str]) -> Mark {
        Mark::new(id, (repository, target))
    }

    /// Targets' Create (`create-branch`) or Switch (`checkout-branch`) of
    /// `repository`, whose name field empties once the branch is created.
    pub(crate) fn branch_form(id: &'static str, repository: &Option<PathBuf>) -> Mark {
        Mark::new(id, repository)
    }

    /// The branch picker while it shows `branch` of `repository`: a switch
    /// chosen from its menu confirms once the picker names that branch.
    pub(crate) fn picker(repository: &Option<PathBuf>, branch: &str) -> Mark {
        Mark::new("current-branch", (repository, branch))
    }

    /// The branch picker while it shows `branch` of `repository`, a tracking
    /// branch just created and switched to: it reads [`CREATE`]'s word
    /// rather than [`SWITCH`]'s.
    pub(crate) fn picker_created(repository: &Option<PathBuf>, branch: &str) -> Mark {
        Mark::new("current-branch", (repository, branch, CREATE.state))
    }

    /// Where a write confirms once Git reports success
    /// (`GitTurtle::write_from`); a write with none keeps the window-wide
    /// notice.
    #[derive(Clone, Debug)]
    pub(crate) enum Origin {
        /// The control that asked reads its word, the outcome is announced
        /// once and the status line records it.
        Control(Mark),
        /// No control reads anything: the outcome is announced once and the
        /// status line records it.
        StatusLine,
    }

    impl Origin {
        pub(crate) fn mark(self) -> Option<Mark> {
            match self {
                Self::Control(mark) => Some(mark),
                Self::StatusLine => None,
            }
        }
    }

    impl From<Mark> for Origin {
        fn from(mark: Mark) -> Self {
            Self::Control(mark)
        }
    }

    /// Where a write confirmed in a review dialog (`confirm_git_write`)
    /// confirms in `repository`. A tracking branch, created from the branch
    /// picker's menu or a remote branch's details, confirms on the picker
    /// once the picker names it where Git switched to it, and otherwise in
    /// the status line; every other reviewed write keeps the window-wide
    /// notice (`None`).
    pub(crate) fn reviewed(repository: &Option<PathBuf>, command: &WriteCommand) -> Option<Origin> {
        match command {
            WriteCommand::Branch(branch) => match branch.as_ref() {
                BranchCommand::CreateTracking { plan } if plan.checkout => {
                    Some(picker_created(repository, &plan.name).into())
                }
                BranchCommand::CreateTracking { .. } => Some(Origin::StatusLine),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The action bar's labelled Fetch, Pull or Push frame, 34 px high, without
/// content: `copy_feedback::confirming` draws its icon and label, `gap_2`
/// apart as the kit's medium Button sets them.
fn action_frame(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .secondary()
        .h(crate::appearance::ui_size(34.))
        .px(action_inset())
        .text_size(crate::appearance::ui_text(12.))
        .font_weight(FontWeight::MEDIUM)
}

fn action_field_label(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .text_size(crate::appearance::ui_text(10.))
        .text_color(rgb(color))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

pub(super) fn retained_area(
    preferred: ChangeArea,
    staged: bool,
    unstaged: bool,
) -> Option<ChangeArea> {
    match preferred {
        ChangeArea::Staged if staged => Some(ChangeArea::Staged),
        ChangeArea::Unstaged if unstaged => Some(ChangeArea::Unstaged),
        _ if staged => Some(ChangeArea::Staged),
        _ if unstaged => Some(ChangeArea::Unstaged),
        _ => None,
    }
}

fn remote_defaults(
    status: &gitturtle_core::RepositoryStatus,
    remotes: &[gitturtle_core::Remote],
) -> (String, String) {
    if let Some(upstream) = &status.upstream
        && let Some((remote, branch)) = remotes
            .iter()
            .filter_map(|remote| {
                upstream
                    .strip_prefix(&format!("{}/", remote.name))
                    .map(|branch| (remote, branch))
            })
            .max_by_key(|(remote, _)| remote.name.len())
    {
        return (remote.name.clone(), branch.into());
    }
    let remote = remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .or(remotes.first())
        .map(|remote| remote.name.clone())
        .unwrap_or_default();
    (remote, status.branch.clone().unwrap_or_default())
}

pub(super) fn display_remote_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    if let Some((scheme, remainder)) = without_query.split_once("://") {
        let (authority, path) = remainder
            .split_once('/')
            .map(|(authority, path)| (authority, format!("/{path}")))
            .unwrap_or((remainder, String::new()));
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        format!("{scheme}://{host}{path}")
    } else {
        without_query.into()
    }
}

#[cfg(test)]
mod confirmation_tests;

#[cfg(test)]
mod tests {
    use super::{commit_composer_height, display_remote_url, remote_defaults, retained_area};
    use core::prelude::v1::test;
    use gitturtle_core::ChangeArea;

    #[test]
    fn small_working_inspector_reserves_files_after_wrapping_controls() {
        // Reported 680-point content: 490-point inspector less 165 points of
        // filter/selection controls and header, with Compact rows at UI 18.
        let body = 490. - 165.;
        let row = 34. * 18. / 13.;
        let composer = commit_composer_height(body, row, 244.);
        assert!(body - composer >= 3. * row);
        assert!(composer >= 36. * 18. / 13. + 48.);
        for body in [0., 80., 160., 325., 600., 1000.] {
            let composer = commit_composer_height(body, row, 344.);
            assert!((0. ..=body).contains(&composer));
            assert!(body - composer >= body * 0.30);
        }
    }

    #[test]
    fn staging_preserves_path_selection_and_moves_area_only_when_needed() {
        assert_eq!(
            retained_area(ChangeArea::Unstaged, true, false),
            Some(ChangeArea::Staged)
        );
        assert_eq!(
            retained_area(ChangeArea::Staged, false, true),
            Some(ChangeArea::Unstaged)
        );
        assert_eq!(
            retained_area(ChangeArea::Unstaged, true, true),
            Some(ChangeArea::Unstaged)
        );
        assert_eq!(retained_area(ChangeArea::Staged, false, false), None);
    }

    #[test]
    fn remote_defaults_follow_the_new_branch_and_its_upstream() {
        let remotes = [
            gitturtle_core::Remote {
                name: "origin".into(),
                url: "local".into(),
                push_url: "local".into(),
            },
            gitturtle_core::Remote {
                name: "team/upstream".into(),
                url: "other".into(),
                push_url: "other".into(),
            },
        ];
        let mut status = gitturtle_core::RepositoryStatus {
            branch: Some("feature/new".into()),
            ..Default::default()
        };
        assert_eq!(
            remote_defaults(&status, &remotes),
            ("origin".into(), "feature/new".into())
        );
        status.upstream = Some("team/upstream/different-name".into());
        assert_eq!(
            remote_defaults(&status, &remotes),
            ("team/upstream".into(), "different-name".into())
        );
    }

    #[test]
    fn displayed_remote_address_does_not_expose_embedded_credentials_or_query() {
        assert_eq!(
            display_remote_url("https://user:secret@example.invalid/team/repo?token=private"),
            "https://example.invalid/team/repo"
        );
        assert_eq!(
            display_remote_url("git@example.invalid:team/repo.git"),
            "git@example.invalid:team/repo.git"
        );
    }
}

#[cfg(test)]
mod targets_focus_tests {
    use crate::tags::tests::{draw, tagged_repository, window};
    use crate::*;
    use core::prelude::v1::test;

    /// Targets' Switch turns disabled while it holds focus: a completed switch
    /// clears the branch field. Tab and Shift+Tab, as keystrokes through the
    /// window, still leave it for its neighbours, past the Create button that
    /// the empty field disables too.
    #[gpui::test]
    fn tab_and_shift_tab_leave_a_focused_switch_that_turns_disabled(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let set_branch = |cx: &mut VisualTestContext, value: &'static str| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.git_actions_open = true;
                    app.branch_name
                        .update(cx, |input, cx| input.set_value(value, window, cx));
                    cx.notify();
                })
            });
            draw(cx);
        };
        let press = |cx: &mut VisualTestContext, keys: &str| {
            cx.simulate_keystrokes(keys);
            draw(cx);
            cx.update(|window, cx| window.focused(cx))
                .expect("a focused control")
        };

        set_branch(cx, "topic");
        let (branch, remote) = cx.read(|cx| {
            let app = app.read(cx);
            (
                app.branch_name.read(cx).focus_handle(cx),
                app.remote_name.read(cx).focus_handle(cx),
            )
        });
        cx.update(|window, cx| branch.focus(window, cx));
        draw(cx);
        // Enabled, Targets reads: branch field, Switch, Create, Remote field.
        let switch = press(cx, "tab");
        let create = press(cx, "tab");
        assert!(switch != branch && create != switch && create != remote);
        assert_eq!(press(cx, "tab"), remote, "Create precedes the Remote field");
        assert_eq!(press(cx, "shift-tab"), create);
        assert_eq!(press(cx, "shift-tab"), switch);

        set_branch(cx, "");
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(switch.clone()),
            "the disabled Switch keeps focus until the keyboard moves it"
        );
        assert_eq!(
            press(cx, "tab"),
            remote,
            "Tab leaves the disabled Switch, past the disabled Create"
        );
        assert_eq!(
            press(cx, "shift-tab"),
            branch,
            "Shift+Tab passes over both disabled buttons"
        );

        set_branch(cx, "topic");
        assert_eq!(press(cx, "tab"), switch);
        set_branch(cx, "");
        assert_eq!(
            press(cx, "shift-tab"),
            branch,
            "Shift+Tab leaves the disabled Switch for the branch field"
        );
    }
}
