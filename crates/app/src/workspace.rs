use crate::*;
use gitturtle_core::{ChangeArea, ChangeStatus, WriteCommand};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
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

/// The Changes header's height in the full form.
const WORKING_HEADER: f32 = 44.;

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
/// height, its Description section's field (`description_field`) and a
/// short composer's drawn height (`content`) are predicted before layout.
struct ComposerMetrics {
    /// The composer's top border.
    border: Pixels,
    /// The padding above the heading row (or, without it, above the fields)
    /// and below the commit action.
    padding: Pixels,
    /// The space between the fields' last row and the commit action: a short
    /// composer's gap, so its label, field and action keep one scaled 4 px
    /// rhythm, and a taller one's padding.
    end: Pixels,
    /// Whether the pinned heading is drawn (`WorkingFit::heading`).
    heading: bool,
    /// The heading's line beside its 16 px icon and the staged count's
    /// badge; nothing where the heading has left.
    heading_row: Pixels,
    /// The gap between the fields' rows, which also leads them below the
    /// heading.
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
        // A short composer's fields keep a scaled 4 px from the heading,
        // between themselves and above the commit action; a taller one's lead
        // with the branch line.
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
        let padding = snap(rem * if short { 0.5 } else { 0.75 });
        Self {
            border: snap(px(1.)),
            padding,
            end: if short { gap } else { padding },
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
    /// the gaps around them and the `end` below, and the commit action with
    /// the padding below.
    fn minimum(&self) -> Pixels {
        // Without the heading the composer's padding leads the fields.
        let lead = if self.heading { self.gap } else { px(0.) };
        let fields = lead + self.branch + self.label + self.gap + self.field + self.end;
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
        // The kit's small labelled Button: `px_2` either side, its label at
        // 0.875 rem, then `gap_1` and the caret, 0.875 rem square.
        let actions = (cached_width(&ACTIONS_LABEL, &key, || {
            shaped_width("Actions", rem * 0.875, regular, window)
        }) + rem * 2.125)
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
        )
    }

    /// The commit action's label: what it commits, or why it cannot.
    pub(super) fn commit_action_label(&self, cx: &App) -> String {
        let staged = self.working_paths.staged_count;
        if self.operation_busy.is_some() {
            "Working…".to_owned()
        } else if self.work_status.is_none()
            || self.working_paths.pending
            || self.working_paths.error.is_some()
        {
            if self.working_paths.pending {
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
    fn working_controls_height(
        &self,
        forms: WorkingForms,
        width: Pixels,
        window: &Window,
        cx: &App,
    ) -> f32 {
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
        if self.working_paths.pending {
            texts.push(("Filtering working paths…".into(), ui_text(11.)));
        }
        if !self.working_paths.pending
            && self.working_paths.error.is_none()
            && self.working_paths.visible_count == 0
            && !self.working_filter.read(cx).value().is_empty()
        {
            texts.push(("No working paths match this filter".into(), ui_text(11.)));
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
        cx: &App,
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
        let full = self.working_controls_height(forms, width, window, cx) + WORKING_HEADER;
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
    /// field (`description_section`), the guidance after it, and the `end`
    /// above the commit action, wrapped texts laid out once per text, size
    /// and width (`wrapped_lines`).
    fn composer_rows(
        &self,
        metrics: &ComposerMetrics,
        width: Pixels,
        identity_ready: bool,
        status_ready: bool,
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
        if !identity_ready && status_ready {
            rows += gap + ui_size(28.);
        }
        if self.working_paths.conflicted {
            let text = room - px(15.) - rem * 0.5;
            rows += gap + px(15.).max(metrics.label * lines(CONFLICT_NOTE, ui_text(11.), text));
        }
        rows + metrics.end
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
/// (`aria_label`) whichever form shows.
pub(super) fn placeholder_fit<M: gpui_kit::base::input::InputModeKind + 'static>(
    field: Entity<gpui_kit::base::input::InputBaseState<M>>,
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &'static [&'static str],
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let Some(text) = field.read(cx).text_bounds() else {
                return;
            };
            let fit = fitted_placeholder(cache, forms, text.size.width, window);
            if *field.read(cx).presentation().placeholder() != fit {
                field.update(cx, |state, cx| state.set_placeholder(fit, window, cx));
                window.defer(cx, |window, _| window.refresh());
            }
        },
    )
    .absolute()
    .size_0()
}

/// `forms` as they fit `room` (`whole_form`), laid out again only when the
/// room, the font or the sizes change.
fn fitted_placeholder(
    cache: &'static std::thread::LocalKey<PlaceholderFit>,
    forms: &[&str],
    room: Pixels,
    window: &Window,
) -> SharedString {
    let key = crate::views::LineKey::current(window);
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((cached, width, text)) if *cached == key && *width == room => text.clone(),
            _ => {
                let text = whole_form(forms, room, crate::appearance::ui_text(12.), window);
                *cache = Some((key, room, text.clone()));
                text
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
        window.focus(&self.app_focus, cx);
        self.hub.update(cx, |hub, cx| {
            hub.set_busy(false, cx);
            hub.set_error(None, cx);
            hub.set_can_go_back(self.repository.is_some(), cx);
        });
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
            self.working_paths.reset();
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
        }
        self.remotes = remotes;
        if !quiet && self.mode == WorkspaceMode::Working {
            self.invalidate_read();
            if let Some((index, area)) = self.working_selected {
                self.select_working(index, area, window, cx);
                if let Some(position) = self.working_rows.iter().position(
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
            .working_rows
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
    /// where the control has left a narrow row, its menu.
    fn stage_working_paths(
        &mut self,
        paths: Vec<PathBuf>,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if staged {
            self.write(
                WriteCommand::Unstage { paths },
                "Unstaging file…",
                window,
                cx,
            );
        } else {
            self.write(WriteCommand::Stage { paths }, "Staging file…", window, cx);
        }
    }

    pub(super) fn write(
        &mut self,
        command: WriteCommand,
        label: &'static str,
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
                        } else if let Some(notice) = &success_notice {
                            notice.clone()
                        } else {
                            outcome.message.lines().find(|line| !line.trim().is_empty()).unwrap_or("Git operation completed").to_owned()
                        };
                        this.tab_operation_feedback(&path, Some(notice), None);
                    }
                    Err(error) => {
                        this.tab_operation_feedback(&path, None, Some(format!("{error:#}")));
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
        let selected_row = self.working_rows.iter().position(
            |row| matches!(row, WorkingRow::File(index, area) if Some((*index, *area)) == self.working_selected),
        );
        // GPUI retains this content geometry in the scroll handle even while
        // Settings hides the inspector. Density changes must not reuse a pixel
        // offset that now points at a different set of rows.
        if previous_list.is_some_and(|size| {
            size.contents.height
                != window.pixel_snap(px(self.settings.density.file_row_height()))
                    * self.working_rows.len() as f32
        }) && let Some(selected) = selected_row
        {
            self.working_scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
        let staged = self.working_paths.staged_count;
        let total = self
            .work_status
            .as_ref()
            .map_or(0, |status| status.entries.len());
        let conflicted = self.working_paths.conflicted;
        let identity_ready = self
            .profile
            .as_ref()
            .is_some_and(|p| !p.name.is_empty() && !p.email.is_empty());
        let busy = self.operation_busy.is_some();
        let refreshing = self.status_task.is_some();
        let status_ready = self.work_status.is_some()
            && !self.working_paths.pending
            && self.working_paths.error.is_none();
        let branch = self
            .work_status
            .as_ref()
            .and_then(|s| s.branch.as_deref())
            .unwrap_or("detached HEAD");
        let summary_length = self.commit_title.read(cx).value().chars().count();
        let commit_disabled = busy
            || !status_ready
            || staged == 0
            || conflicted
            || !identity_ready
            || self.commit_title.read(cx).value().trim().is_empty();
        // An operation in progress replaces the composer with its banner.
        let composer = self
            .work_status
            .as_ref()
            .is_none_or(|status| status.operation.is_none());
        let floor = working_list_floor(
            working_floor_rows(&self.working_rows),
            self.settings.density,
            window,
        );
        let fit = self.working_fit(
            forms,
            layout.read(cx).inspector,
            fitted_width,
            floor,
            composer,
            short,
            window,
            cx,
        );
        let metrics = ComposerMetrics::new(short, fit.heading, window);
        let minimum = f32::from(metrics.minimum());
        // The fields' rows but the description's field, at the interface
        // size: the composer prefers room for them whole with that field.
        let rows = self.composer_rows(
            &metrics,
            fitted_width,
            identity_ready,
            status_ready,
            window,
            cx,
        );
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
            if total > 0 && inner > floor - border && inner < row * self.working_rows.len() as f32 {
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
        // A short composer is drawn no taller than its rows, so its title's
        // label, field and commit action (and the Description section where it
        // shows) keep their scaled 4 px rhythm, rather than the height its
        // share leaves over gathering between the last field and the action.
        // What it does not use goes back to the list in whole rows, and what
        // is left, less than a row, lies between the list's last row and the
        // composer.
        let section = self.description_section(&metrics, fitted_width, window);
        let drawn = if composer && short {
            composer_height.min(f32::from(metrics.content(rows, section, description)))
        } else {
            composer_height
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
        let list = uniform_list(
            "working-files",
            self.working_rows.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                range
                    .map(|i| this.render_working_row(i, forms, window, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.working_scroll);
        let refresh = button("refresh-working", "", "refresh", false)
            .debug_selector(|| "refresh-working".into())
            .accessibility_label("Refresh working changes")
            .tooltip(if refreshing {
                "Refresh working changes · Refreshing…"
            } else {
                "Refresh working changes"
            })
            .disabled(busy || refreshing)
            .on_click(cx.listener(|this, _, window, cx| this.refresh_worktree(window, cx)));
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
                            .when(refreshing && !forms.compact_actions, |el| {
                                el.child(
                                    div()
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
            .child(div().relative().flex_1().min_h(px(floor + if composer { minimum } else { 0. })).flex().flex_col()
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
                    .when(total > 0, |el| el.child(list))
                    .when(total == 0, |el| el.child(
                        div().size_full().flex().flex_col().items_center().justify_center().p_5().gap_2()
                            .child(div().size(px(40.)).rounded(px(12.))
                                .bg(rgb(if status_ready { p.added_background } else { p.hover }))
                                .flex().items_center().justify_center()
                                .child(icon(if status_ready { "check" } else if refreshing { "refresh" } else { "changes" },
                                    22., if status_ready { p.added } else { p.muted })))
                            .child(div().text_size(crate::appearance::ui_text(13.)).font_weight(FontWeight::MEDIUM)
                                .child(if status_ready { "Working tree clean" } else if refreshing { "Reading working tree…" } else { "Changes unavailable" }))
                            .child(div().max_w(px(220.)).text_center().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))
                                .child(if status_ready { "No staged changes or local edits. Your next changes will appear here." }
                                    else if refreshing { "Checking staged changes and local edits." }
                                    else { "Refresh to try reading this working tree again." })),
                    ))
                    .child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
                        list_layout.update(cx, |layout, cx| {
                            if layout.list_size != Some(bounds.size) {
                                if layout.list_size.is_some() && let Some(selected) = selected_row {
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
                    .child(div().flex().flex_col().gap_2().px_3().pt_2().pb_3()
                    // A short composer's fields end a gap above the commit
                    // action (`ComposerMetrics::end`).
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
                                counter.child(whole)
                            } else {
                                // Without "suggested" the counter names it.
                                crate::views::note_compact_label("commit-title-counter", &whole);
                                counter.role(Role::Label).aria_label(whole.clone())
                                    .tooltip(move |window, cx| Tooltip::new(whole.clone()).build(window, cx))
                                    .child(format!("{summary_length}/72"))
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
                        .child(div().debug_selector(|| "commit-description".into()).child(Textarea::new(&self.commit_message).readonly(busy)
                            .aria_label("Commit description, optional")
                            .h(px(field))
                            .text_size(crate::appearance::ui_text(12.))))
                        .child(div().debug_selector(|| "commit-description-hint".into()).flex().items_center().gap_2().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted))
                            .child(div().flex_1().child(if staged == 0 { UNSTAGED_HINT } else { STAGED_HINT }))))
                    .when_some(self.draft_save_error.as_ref(), |el, error| el.child(
                        div().flex().flex_col().gap_1().text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.warning))
                            .child(DRAFT_SAVE_NOTE)
                            .child(button("retry-commit-draft", "Retry saving draft", "refresh", false)
                                .tooltip(error.clone())
                                .on_click(cx.listener(|this, _, window, cx| this.persist_commit_draft(window, cx)))),
                    ))
                    .when(!identity_ready && status_ready, |el| el.child(
                        button("configure-identity", "Set your commit identity", "", false)
                            .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx))),
                    ))
                    .when(conflicted, |el| el.child(
                        div().flex().items_center().gap_2().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))
                            .child(icon("file-conflict", 15., p.warning))
                            .child(CONFLICT_NOTE),
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
                            let message = this.current_commit_draft(cx).message();
                            this.write(WriteCommand::Commit { message }, "Creating commit…", window, cx);
                        }))
                    }))
                    // Painted after both fields, it fits their placeholders.
                    .child(placeholder_fit(
                        self.commit_title.clone(),
                        &TITLE_FIT,
                        &[TITLE_PLACEHOLDER, TITLE_SHORT_PLACEHOLDER],
                    ))
                    .child(placeholder_fit(
                        self.commit_message.clone(),
                        &DESCRIPTION_FIT,
                        &[DESCRIPTION_PLACEHOLDER],
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
        match self.working_rows[position] {
            WorkingRow::Directory(index, _) => {
                let parent = self
                    .work_status
                    .as_ref()
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
                let (selector, action, symbol, detail) = if staged {
                    (
                        "unstage-all",
                        "Unstage all",
                        "minus",
                        "Remove all staged changes from the next commit",
                    )
                } else {
                    (
                        "stage-all",
                        "Stage all",
                        "plus",
                        "Include all local changes in the next commit",
                    )
                };
                // Where its label does not fit, the action keeps its icon,
                // with its name and a tooltip that leads with it.
                let control = if forms.section_action_label {
                    button(selector, action, symbol, false).tooltip(detail)
                } else {
                    crate::views::compact_control_name(
                        button(selector, "", symbol, false),
                        selector,
                        action,
                    )
                    .tooltip(format!("{action} · {detail}"))
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
                            .disabled(
                                busy || count == 0
                                    || self.working_paths.pending
                                    || self.working_paths.error.is_some()
                                    || !self.working_filter.read(cx).value().is_empty(),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.write(
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
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .into_any_element()
            }
            WorkingRow::File(index, area) => {
                let entry = &self.work_status.as_ref().unwrap().entries[index];
                let paths = entry.paths();
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
                let menu_paths = paths.clone();
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
                    .when(self.working_selected == Some((index, area)), |row| {
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
                            .child(
                                div()
                                    .debug_selector(move || format!("working-name-{position}"))
                                    .truncate()
                                    .text_size(crate::appearance::ui_text(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .truncate()
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
                        row.child(
                            button(
                                (if staged { "unstage" } else { "stage" }, index),
                                "",
                                if staged { "minus" } else { "plus" },
                                false,
                            )
                            .debug_selector(move || format!("working-stage-{position}"))
                            .accessibility_label(stage_label.clone())
                            .tooltip(stage_label)
                            .disabled(busy)
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.stage_working_paths(paths.clone(), staged, window, cx);
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
                    .context_menu(move |menu, _, _| {
                        let owner = ignore_owner.clone();
                        let repository = ignore_repository.clone();
                        let entry = ignore_entry.clone();
                        let discard_owner = owner.clone();
                        let discard_repository = repository.clone();
                        let discard_entry = entry.clone();
                        // Without its Stage or Unstage control the row's menu
                        // leads with that action.
                        let menu = if forms.file_action {
                            menu
                        } else {
                            let (owner, repository) = (owner.clone(), repository.clone());
                            let paths = menu_paths.clone();
                            menu.item(
                                PopupMenuItem::new(if staged { "Unstage" } else { "Stage" })
                                    .disabled(busy)
                                    .on_click(move |_, window, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            if this.path == repository {
                                                this.stage_working_paths(
                                                    paths.clone(),
                                                    staged,
                                                    window,
                                                    cx,
                                                );
                                            }
                                        });
                                    }),
                            )
                        };
                        menu.item(
                            PopupMenuItem::new(if entry.untracked {
                                "Ignore…"
                            } else {
                                "Ignore applies to untracked files"
                            })
                            .disabled(busy || staged || !entry.untracked)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.path == repository {
                                        this.open_ignore(entry.clone(), window, cx);
                                    }
                                });
                            }),
                        )
                        // Discard reverts the whole file to HEAD, so both the
                        // staged and unstaged rows offer the same action.
                        .item(
                            PopupMenuItem::new(if discard_entry.conflicted {
                                "Discard applies after conflict resolution"
                            } else if discard_entry.untracked {
                                "Delete untracked file…"
                            } else {
                                "Discard changes…"
                            })
                            .disabled(busy || discard_entry.conflicted)
                            .on_click(move |_, window, cx| {
                                let _ = discard_owner.update(cx, |this, cx| {
                                    if this.path == discard_repository {
                                        this.open_discard(discard_entry.clone(), window, cx);
                                    }
                                });
                            }),
                        )
                    })
                    .into_any_element()
            }
        }
    }

    /// The Git action bar. `compact`, in a window too short for its full form
    /// (`views::Chrome`), it is one row of 28 px controls: the branch picker
    /// and sync state give up their width first, and Fetch, Pull, Push and
    /// Targets keep only their icons, their accessible names, and tooltips
    /// that lead with those names. Their actions, states and order are the
    /// full form's.
    pub(super) fn render_git_actions(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
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
        let sync_state = self.work_status.as_ref().map(|status| {
            let explanation = status.upstream.as_ref().map_or_else(
                || {
                    "No upstream is configured. Choose a remote and branch in Targets to push."
                        .to_owned()
                },
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
                .when(status.upstream.is_none(), |el| {
                    el.child(
                        shrinking(div())
                            .text_color(rgb(p.muted))
                            .child("No upstream"),
                    )
                })
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
                let unavailable = busy
                    || remote.is_none()
                    || (id != "fetch" && (selected_branch.is_empty() || detached));
                let tooltip = if busy {
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
                let control = if compact {
                    let tooltip = format!("{label} · {tooltip}");
                    crate::views::compact_control_label(button(id, "", id, false), id, name.clone())
                        .map(|button| {
                            if id == "push" {
                                button.primary()
                            } else {
                                button.secondary()
                            }
                        })
                        .tooltip(tooltip)
                } else {
                    action_button(id, label, id)
                        .when(id == "push", |button| button.primary())
                        .tooltip(tooltip)
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
                        let command = match id {
                            "fetch" => WriteCommand::Fetch { remote },
                            "pull" => WriteCommand::Pull { remote, branch },
                            _ => WriteCommand::Push {
                                remote,
                                local_branch: this
                                    .work_status
                                    .as_ref()
                                    .and_then(|s| s.branch.clone())
                                    .unwrap_or_default(),
                                remote_branch: branch,
                            },
                        };
                        this.write(
                            command,
                            match id {
                                "fetch" => "Fetching remote…",
                                "pull" => "Pulling fast-forward…",
                                _ => "Pushing branch…",
                            },
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
                                .child(button("checkout-branch", "Switch", "branch", false).disabled(busy || branch_empty)
                                    .tooltip("Switch to the named local branch")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let branch = this.branch_name.read(cx).value().trim().to_owned();
                                        this.write(WriteCommand::Checkout { branch }, "Switching branch…", window, cx);
                                    })))
                                .child(button("create-branch", "Create", "branch-add", false).disabled(busy || branch_empty)
                                    .tooltip("Create and switch to a branch from HEAD")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let name = this.branch_name.read(cx).value().trim().to_owned();
                                        this.write(WriteCommand::CreateBranch { name, start_point: None }, "Creating branch…", window, cx);
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
fn action_inset() -> Pixels {
    appearance::ui_text(13.) * 0.75
}

fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>, symbol: &str) -> Button {
    Button::new(id)
        .secondary()
        .h(crate::appearance::ui_size(34.))
        .px(action_inset())
        .text_size(crate::appearance::ui_text(12.))
        .font_weight(FontWeight::MEDIUM)
        .label(label)
        .icon(
            Icon::default()
                .path(format!("icons/{symbol}.svg"))
                .size(appearance::ui_size(16.)),
        )
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
