//! Native tag browser, annotation inspector, and explicit captured actions.
use crate::*;
use focus_reveal::{FocusReveal, Trigger};
use gitturtle_core::{RemoteConfig, Tag, TagCommand, TagDetails, TagList, WriteCommand};
use gpui_kit::{
    component::{
        WindowExt,
        dialog::{Confirm, DialogFooter},
        input::Enter,
        scroll::{Scrollbar, ScrollbarHandle, ScrollbarMode},
    },
    prelude::FluentBuilder,
};
use std::{cell::Cell, rc::Rc};

pub(crate) const PREPARING: &str = "Reading tags…";
/// The gap the kit's dialog leaves between its body and its footer, its
/// default 16 px padding. A dialog whose body keeps the focus ring's room
/// below its last control gives the room back by narrowing this gap.
pub(crate) const DIALOG_FOOTER_GAP: Pixels = px(16.);

/// Sets a dialog form's error line, `slot`, to `text`, and has the window's
/// alert say it once, at once, bounded (`bounded_accessible_text`), in the
/// frame that draws the line, while the line stands in place. Each form
/// sets its line only here, as an action fails or is refused, so the alert
/// is said again only for a new answer, never as frames draw the line again.
/// The forms' lines were no live regions, so a failure was heard only by
/// moving onto it.
pub(crate) fn set_error_line(
    slot: &mut Option<String>,
    text: impl Into<String>,
    window: &mut Window,
    cx: &mut App,
) {
    alert_error_line(slot, text, window, cx);
}

/// [`set_error_line`], returning the alert, for a form whose line can stop
/// answering what it shows before a frame draws it: it keeps the alert to
/// take it back (`copy_feedback::withdraw`), as Stashes does.
pub(crate) fn alert_error_line(
    slot: &mut Option<String>,
    text: impl Into<String>,
    window: &mut Window,
    cx: &mut App,
) -> crate::copy_feedback::Alerted {
    let text = text.into();
    let alerted = crate::copy_feedback::alert(
        window,
        cx,
        native_accessibility::bounded_accessible_text(text.as_str()),
    );
    *slot = Some(text);
    alerted
}

/// The most lines a dialog form's error line draws ([`error_line`]).
pub(crate) const ERROR_LINES: usize = 3;

/// The most lines, and bytes, a cut error line's tooltip shows of its
/// report ([`error_line`]): a tooltip has no height of its own to keep.
const ERROR_TOOLTIP_LINES: usize = 12;
const ERROR_TOOLTIP_BYTES: usize = 1024;

/// A dialog form's error line, `id`, which is also its debug selector:
/// `text` at `size` (where `None`, the interface text size, `ui_text(13.)`,
/// the size a dialog's text inherits from the window's root) in at most
/// [`ERROR_LINES`] lines, the last ending in an ellipsis where it needs
/// more, its whole text, bounded (`bounded_accessible_text`), its
/// accessible value, as the alert that says it ([`set_error_line`]) says it
/// whole. Only a line that is cut has a tooltip, which shows the report's
/// first lines ([`error_tooltip`]); a whole line has none, as a whole label
/// has none. Whether it is cut is decided as each frame lays the line out,
/// from that frame's width, text size and report together, in the line's
/// own font ([`LaidOut`]), and the tooltip, asked for as a hover's delay ends, shows
/// what the latest frame decided: a resize that cuts the line, or makes it
/// whole, in one step answers the next hover without another frame, and a
/// text size or report that changes during the delay is never measured
/// against the width another frame laid out. Decided while the frame was
/// built, from the frame before, it answered for the old width until
/// something else drew the form again; decided as the tooltip was asked
/// for, it paired the latest width with the hovered frame's size and
/// report, and measured in GPUI's default font, outside any frame's text
/// style. A tooltip shown while the pointer rests on the line stays as it
/// was shown through a resize, as macOS keeps a shown tooltip until the
/// pointer moves off: the next hover answers for the new cut. Drawn whole,
/// a long Git report, as a hook's or a remote's stderr, grew the form by
/// each of its lines, past its fields and actions. The line's state key and
/// its bounded text are kept for its id ([`kept_line`]), so a frame at rest
/// formats and copies neither.
pub(crate) fn error_line(
    id: &'static str,
    text: &str,
    size: Option<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let (key, whole) = kept_line(id, text);
    // The size the line is drawn and measured at, set on the line itself,
    // so its prepaint never reads the window's text style for it: that read
    // builds GPUI's default text style, whose font features are an
    // allocation, and walks the style stack, which a line without a size of
    // its own (the profile form's) did on every frame, however steady.
    // Where none is given it is the size such a line inherited, the
    // interface size the window's render sets on its root (`views.rs`,
    // `GitTurtle::render`), which the kit's dialogs keep for their content.
    let size = size.unwrap_or_else(|| crate::appearance::ui_text(13.));
    // How the line was laid out last, which its prepaint records, kept
    // across frames so the hover a frame began reads the latest.
    let laid_out = window
        .use_keyed_state(key, cx, |_, _| {
            std::rc::Rc::new(std::cell::RefCell::new(None::<LaidOut>))
        })
        .read(cx)
        .clone();
    let recorded = laid_out.clone();
    let report = whole.clone();
    native_accessibility::static_text(
        div()
            .on_children_prepainted(move |bounds, window, _| {
                #[cfg(test)]
                let before = crate::test_support::allocations();
                // A steady frame compares what it lays out with what was
                // laid out last and reads nothing more; a change measures
                // inside the line's own text style, so its font measures it.
                let mut laid = recorded.borrow_mut();
                match bounds.first().map(|line| line.size.width) {
                    None => *laid = None,
                    Some(width) => {
                        if !laid
                            .as_ref()
                            .is_some_and(|laid| laid.is(&report, width, size))
                        {
                            *laid = Some(LaidOut {
                                cut: report_cut(&report, size, width, window),
                                report: report.clone(),
                                width,
                                size,
                            });
                        }
                    }
                }
                #[cfg(test)]
                PREPAINT_ALLOCATIONS.with(|counted| {
                    let (prepaints, allocations) = counted.get();
                    counted.set((
                        prepaints + 1,
                        allocations + crate::test_support::allocations() - before,
                    ));
                });
            })
            .id(id)
            .debug_selector(move || id.into())
            .text_size(size)
            .line_clamp(ERROR_LINES)
            .text_ellipsis()
            .tooltip(move |window, cx| {
                let cut = laid_out
                    .borrow()
                    .as_ref()
                    .filter(|laid| laid.cut)
                    .map(|laid| laid.report.clone());
                match cut {
                    Some(report) => {
                        gpui_kit::component::tooltip::Tooltip::new(error_tooltip(&report))
                            .build(window, cx)
                    }
                    // Nothing to add to a whole line: an empty view, drawn
                    // as nothing.
                    None => cx.new(|_| gpui::EmptyView).into(),
                }
            })
            .child(whole.clone()),
        whole,
    )
}

/// What the error line `id` drew last ([`kept_line`]): the key of the state
/// that keeps how it was laid out, `{id}/laid-out`, and its text as drawn,
/// bounded (`bounded_accessible_text`). One entry for each line's id, which
/// are static, each at most 16 KiB of text, so what is kept is bounded by
/// the forms the app has.
struct KeptLine {
    key: ElementId,
    whole: SharedString,
}

thread_local! {
    static KEPT_LINES: std::cell::RefCell<HashMap<&'static str, KeptLine>> =
        std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
thread_local! {
    /// Test-only: the error lines [`kept_line`] has found their key and
    /// text for, and the allocations it made doing so.
    static KEPT_LINE_TEXT: std::cell::Cell<(usize, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// The state key and the text the error line `id` draws for `text`, kept
/// for the id ([`KeptLine`]): the key formatted the first time the line is
/// drawn and the text copied again only when it changes. A form's render
/// builds its line in every frame that draws the form, so a frame at rest
/// builds neither: each frame formatted the key and copied the report,
/// which can be 16 KiB. Two windows drawing the same line with different
/// reports take turns with its entry, each copying its report as every
/// frame did before, never drawing the other's.
fn kept_line(id: &'static str, text: &str) -> (ElementId, SharedString) {
    #[cfg(test)]
    let before = crate::test_support::allocations();
    let kept = KEPT_LINES.with_borrow_mut(|lines| {
        let line = lines.entry(id).or_insert_with(|| KeptLine {
            key: SharedString::from(format!("{id}/laid-out")).into(),
            whole: SharedString::default(),
        });
        if !native_accessibility::carries_accessible_text(&line.whole, text) {
            line.whole = native_accessibility::bounded_accessible_text(text);
        }
        (line.key.clone(), line.whole.clone())
    });
    #[cfg(test)]
    KEPT_LINE_TEXT.with(|counted| {
        let (lines, allocations) = counted.get();
        counted.set((
            lines + 1,
            allocations + crate::test_support::allocations() - before,
        ));
    });
    kept
}

/// Empties [`KEPT_LINES`], for a test that counts what an error line
/// builds: a GPUI test's seeds share the thread and its cache.
#[cfg(test)]
fn forget_kept_lines() {
    KEPT_LINES.with_borrow_mut(HashMap::clear);
}

/// How the latest frame that drew an error line laid it out
/// ([`error_line`]): the report it drew, the width and text size it laid
/// the report out at, and whether the report needs more than
/// [`ERROR_LINES`] lines there, measured as that frame prepainted the line,
/// in the line's own font. Kept together, so a hover never pairs one
/// frame's width with another's size or report, and measured again only
/// when one of them changes, so a steady frame shapes nothing. The font is
/// not part of the key: the interface font is chosen as the app starts and
/// does not change while it runs.
struct LaidOut {
    report: SharedString,
    width: Pixels,
    size: Pixels,
    cut: bool,
}

impl LaidOut {
    /// Whether this is `report` laid out `width` wide at `size`.
    fn is(&self, report: &str, width: Pixels, size: Pixels) -> bool {
        self.width == width && self.size == size && self.report == report
    }
}

/// Whether `report` needs more than [`ERROR_LINES`] lines laid out `width`
/// wide at `size` in the window's text style, as an error line's prepaint
/// measures it ([`LaidOut`]), in work bounded however long the report is: a
/// report of more lines than that is cut at any width and is not shaped,
/// and the rest is wrapped only one line past the bound (GPUI's wrap budget
/// runs across a text's lines), from the report itself rather than a copy.
/// Measured whole, a live resize copied, shaped and wrapped a report of up
/// to 16 KiB in every frame whose width changed, where the line draws
/// three lines of it.
fn report_cut(report: &SharedString, size: Pixels, width: Pixels, window: &Window) -> bool {
    if report.split('\n').nth(ERROR_LINES).is_some() {
        return true;
    }
    let style = window.text_style();
    let Ok(lines) = window.text_system().shape_text(
        report.clone(),
        size,
        &[style.to_run(report.len())],
        Some(width),
        Some(ERROR_LINES + 1),
    ) else {
        return true;
    };
    let laid: usize = lines
        .iter()
        .map(|line| line.wrap_boundaries().len() + 1)
        .sum();
    #[cfg(test)]
    REPORT_LINES_LAID.with(|counted| {
        let (measured, lines) = counted.get();
        counted.set((measured + 1, lines + laid));
    });
    laid > ERROR_LINES
}

#[cfg(test)]
thread_local! {
    /// Test-only: the reports [`report_cut`] has shaped, and the lines it
    /// has laid out for them.
    static REPORT_LINES_LAID: std::cell::Cell<(usize, usize)> =
        const { std::cell::Cell::new((0, 0)) };
    /// Test-only: the error lines' prepaints ([`error_line`]), and the
    /// allocations they made.
    static PREPAINT_ALLOCATIONS: std::cell::Cell<(usize, u64)> =
        const { std::cell::Cell::new((0, 0)) };
}

/// What a cut error line's tooltip shows of `report`: its first
/// [`ERROR_TOOLTIP_LINES`] lines, within [`ERROR_TOOLTIP_BYTES`], with an
/// ellipsis where more follows; the line's accessible value keeps the rest.
pub(crate) fn error_tooltip(report: &str) -> SharedString {
    let mut lines = report.lines();
    let mut shown = lines
        .by_ref()
        .take(ERROR_TOOLTIP_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let mut more = lines.next().is_some();
    if shown.len() > ERROR_TOOLTIP_BYTES {
        // Cut between whole grapheme clusters.
        shown.truncate(crate::text_cut::floor_cluster(&shown, ERROR_TOOLTIP_BYTES));
        more = true;
    }
    if more {
        shown.push('…');
    }
    shown.into()
}

/// Why a dialog's action on its repository is refused once that repository
/// is no longer the one shown (`GitTurtle::dialog_refusal`).
pub(crate) const NOT_SHOWN_REASON: &str = "Return to this repository first";

/// Whether the kit's `Confirm` reaching a dialog's field comes from Return
/// in that field, for a dialog whose field acts on Return of its own: the
/// dialog sends it for a fresh press there, after the field's own `Enter`
/// has let the key go on, and Done's click sends it too
/// ([`dialog_footer`]), to whatever holds focus, a field included, since a
/// Button takes no focus on a press. The view notes the field's `Enter` as
/// it passes ([`Self::note`]) and acts on `Confirm` only where it took that
/// note ([`Self::take`]); any other `Confirm` is the dialog's own. Taken as
/// the field's, Done clicked with focus in the Reflog's filter read the
/// first entry the filter matched and left the Reflog open.
#[derive(Clone, Default)]
pub(crate) struct FieldReturn(Rc<Cell<bool>>);

impl FieldReturn {
    /// Notes a field's `Enter`, which the key going down sends: the note
    /// stands until that key's dispatch ends, so it never answers a later
    /// click, nor a press the dialog does not confirm.
    pub(crate) fn note(&self, cx: &mut App) {
        self.0.set(true);
        let noted = self.0.clone();
        cx.defer(move |_| noted.set(false));
    }

    /// Whether the `Confirm` being dispatched comes from the Enter noted,
    /// which it takes.
    pub(crate) fn take(&self) -> bool {
        self.0.replace(false)
    }
}

/// A dialog's own footer, the kit's OK and optional Cancel built on the app's
/// compact frame (`button_frame`, through `button`): 28 px high at the
/// interface size, with its padding, label size and focus ring, as every
/// other action in the app, in place of the kit's own 26 px Buttons, which
/// stood shorter than the shared controls in the dialog above them. OK, the
/// primary action, reads `ok`, and Cancel, where shown, `cancel`; each
/// dispatches the kit's `Confirm` or `Cancel`, as the kit's own did, so the
/// dialog's `on_ok` and `on_cancel` answer them, and Enter and Escape act as
/// before. Tests find them as `dialog-ok: {ok}` and `dialog-cancel:
/// {cancel}`.
pub(crate) fn dialog_footer(
    ok: impl Into<SharedString>,
    cancel: Option<&'static str>,
) -> DialogFooter {
    use gpui_kit::component::dialog::{Cancel, Confirm};
    let ok: SharedString = ok.into();
    DialogFooter::new()
        .when_some(cancel, |footer, cancel| {
            footer.child(
                dialog_cancel("cancel", cancel)
                    .debug_selector(move || format!("dialog-cancel: {cancel}"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
            )
        })
        .child(
            button("ok", ok.clone(), "", false)
                .primary()
                .debug_selector(move || format!("dialog-ok: {ok}"))
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                }),
        )
}

/// A dialog's footer whose one action only dismisses it, `label`, where the
/// dialog's body holds its own filled primary action, as Stashes' Review
/// restore… beside Close, or where the body's choices are equal, so none is
/// the default, as the mainline dialog's "Parent n" beside Cancel: the
/// footer's action takes Cancel's bordered style ([`dialog_cancel`]) rather
/// than a fill, since a macOS window has at most one default button and a
/// surface fills only the action its state leads to. It still dispatches
/// the kit's `Confirm`, so Enter and the dialog's `on_ok` act as before, and
/// stands in the filled frame's bounds ([`bordered_in_place`]), so nothing
/// moves. Tests find it as `dialog-ok: {label}`. (Decided 2026-11-02:
/// Stashes drew Review restore… and Close as two filled primaries, and the
/// mainline dialog filled its Cancel.)
pub(crate) fn dialog_dismiss_footer(label: &'static str) -> DialogFooter {
    use gpui_kit::component::dialog::Confirm;
    DialogFooter::new().child(
        bordered_in_place(
            button("ok", label, "", false),
            appearance::ui_size(LABEL_INSET),
        )
        .debug_selector(move || format!("dialog-ok: {label}"))
        .on_click(|_, window, cx| {
            window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
        }),
    )
}

/// `control`, which stood filled, in Cancel's bordered secondary style
/// ([`dialog_cancel`]) where its surface leads to another action: its 1 px
/// edge comes out of `inset`, the side padding its frame gives it, so its
/// bounds and its icon and label keep the filled Button's place. The border
/// alone widened a labelled Button by 2 px, which moved a footer's action
/// and the toolbar's Push between History and Working Changes.
pub(crate) fn bordered_in_place(control: Button, inset: Pixels) -> Button {
    control.secondary().border_1().px(inset - px(1.))
}

/// A dialog's Cancel, `id`, reading `label`, on the compact frame: the
/// secondary variant, its `subtle` fill bordered by the dialog's own edge,
/// 3:1 or more against the dialog (`Palette::dialog_edge`, which the palette
/// installs as the toolkit's secondary edge, so hover and press keep it), as
/// macOS draws Cancel as a bordered push button beside the filled default
/// action, and one Cancel style in every dialog, the ones `dialog_footer`
/// builds and those that build their own footers alike (Create tag, forced
/// worktree removal, Discard). The frame's transparent rest left Cancel
/// reading as plain text beside the filled primary, and the page's `border`
/// stood 1.28:1 off Braden's dialog. The border is the toolkit's 1 px in
/// every state, so nothing moves.
pub(crate) fn dialog_cancel(id: &'static str, label: &'static str) -> Button {
    button(id, label, "", false).secondary().border_1()
}

/// The cue a scrolling list gives the rows beyond its edge, the same for
/// every list the app cues: the branch chooser, the remote manager, Tags,
/// the tag inspector's Push to… list, the Reflog's entries, the worktree
/// manager's lists, Stashes' detail, the GitHub panel's body, a document
/// side's own header controls and a 3D model's details. Where the
/// scrollbar's thumb can move along its track it is the always-visible
/// scrollbar the review dialogs cue theirs with; along a shorter track it
/// is a soft fade at each edge the rows continue past ([`edge_fades`]).
///
/// While the list overflows, its rows keep a gutter at their right, and
/// along a track at least [`SHORTEST_SCROLL_TRACK`] tall the scrollbar
/// paints its track there, over the column the rows lie in: inside the
/// list, clear of the room it keeps around its rows for their focus rings,
/// so the track covers no row and no ring, and clear of the dialog's side
/// padding, where the dialog's own scrollbar runs. The gutter is the
/// scrollbar's width plus the room of the ring every palette installs, so a
/// row's ring ends where the track begins, and neither the rows nor the
/// track move with the ring's room, which the list gives back. A list whose
/// rows fit keeps no gutter and draws no scrollbar, so its rows keep their
/// width.
///
/// Along a shorter track the kit's thumb ran out of its track against the
/// scroll or barely moved, as in a side header's controls region at its
/// 36–44 px floor, so there the list draws no scrollbar, leaving no
/// scrollbar element behind, and fades the edges its rows continue past
/// instead. It keeps the gutter, so nothing it holds moves as it grows or
/// shrinks past that length: without it, a document header's controls
/// wrapped wider and moved up a line when a click's answer shortened the
/// header past it. It still scrolls under a wheel and as Tab reveals a
/// control, and still reports itself, as every cued list does, as a scroll
/// view that clips its rows (`native_accessibility::clips_children`).
///
/// GPUI lays a frame out before it knows whether the list overflows, so the
/// gutter follows the overflow the list's scroll handle kept from the last
/// frame, and the scrollbar the length of the track it kept, built only
/// where the track the frame paints is still that long: a list shrunk
/// below the shortest track draws no bar in the frame that shrinks it. A
/// frame whose layout differs draws once more after it is painted; a frame
/// draws the scrollbar only with the gutter, so a thumb never paints over a
/// row. The
/// fades read the list as it is painted, so they follow its offset, its
/// reach and its length in the frame that changes them: a list that grows
/// past the shortest track draws no fade before its scrollbar comes, and
/// one that shrinks below it fades in the frame that shrinks it.
///
/// A fade never lies over a focus indicator. In a list whose controls a
/// [`FocusReveal`] scrolls into view ([`ScrollCue::revealing`]), the fade at
/// an edge stops short of the focused control's ring, its ring's room
/// outside its edge, so the ring and the label inside it stay whole, and
/// it returns whole in the frame focus leaves. The reveal keeps its ring's
/// room alone: at a side header's floor, as in the model's 40 px region at
/// 13 pt, a 28 px control (about 39 px at 18 pt) cannot also keep a fade's
/// height clear of both edges, and a reveal that kept one edge's fade
/// clear brought the control under the other's, its second pass in the
/// frame scrolling it back.
pub(crate) struct ScrollCue {
    rows: RowColumn,
    /// Whether this frame keeps the gutter, and whether it draws the
    /// scrollbar in it ([`cues`]).
    cued: (bool, bool),
    /// What scrolls the list's focused control into view, whose ring the
    /// fades stop short of.
    reveal: Option<FocusReveal>,
    /// The track takes no fill and no edge ([`ScrollCue::over_rows`]).
    clear_track: bool,
    /// The fades dissolve into the panel, not the canvas
    /// ([`ScrollCue::on_panel`]).
    on_panel: bool,
}

/// The shortest track a scrollbar's thumb moves along: the kit's shortest
/// thumb, an unscaled 48 px (`MIN_THUMB_SIZE` in `gpui-base`'s scrollbar),
/// and its 4 px inset at each of the track's ends (`THUMB_INSET`). Along a
/// shorter track the kit's thumb ran out of the track against the scroll,
/// or moved a few pixels at most, and a drag divided by the track's length
/// less the thumb's, at most zero.
pub(crate) const SHORTEST_SCROLL_TRACK: Pixels = px(48. + 4. * 2.);

/// How tall an edge fade is at the default interface size: about the room
/// a 28 px control keeps above and below its label, with its ring's, so a
/// fade over a control at the edge softens its ring and frame and leaves
/// its label clear. It scales with the interface text size, as the rows it
/// lies over do, and takes at most a quarter of the list, so at either size
/// the list's middle half reads unfaded. Shared by every scrolling list's
/// cue ([`ScrollCue`]) and by the inspector's commit message viewport
/// (`commit_message::message_fades`), so their fades read as one.
pub(crate) const EDGE_FADE: f32 = 8.;

impl ScrollCue {
    /// The cue for the list `scroll` tracks, which keeps `room` around its
    /// rows for their rings, for the frame being rendered: for a list that
    /// holds no control focus can reach, such as a model's details.
    pub(crate) fn new(scroll: &ScrollHandle, room: Pixels) -> Self {
        let rows = RowColumn {
            scroll: scroll.clone(),
            room,
        };
        Self {
            cued: cues(&rows),
            rows,
            reveal: None,
            clear_track: false,
            on_panel: false,
        }
    }

    /// The cue for a list drawn on the palette's panel rather than its
    /// canvas, as File History's header and footer are: its fades dissolve
    /// into that panel, where the canvas's darker or lighter band would lie
    /// across its rows' edge.
    pub(crate) fn on_panel(mut self) -> Self {
        self.on_panel = true;
        self
    }

    /// The cue for a list whose rows, and the edge that draws its focus
    /// ring, run beneath its track, as the Working inspector's short body's
    /// do: the track takes no fill and no edge, so only its thumb lies over
    /// them. The kit fills a track with the theme's `scrollbar` colour, the
    /// canvas, which painted over that ring's right side, the selected row's
    /// end and the composer's divider for the track's whole height, as
    /// `shortcuts.rs` sets its own track's fill. Resting on the rows, the
    /// thumb takes the thumb fitted to them, 2:1 on the panel and on a
    /// hovered, selected and hovered selected row
    /// (`Palette::over_rows_thumb`): the palette's thumb, fitted to the
    /// track's surfaces, stood 1.46:1 on Midnight's selected row.
    pub(crate) fn over_rows(mut self) -> Self {
        self.clear_track = true;
        self
    }

    /// [`Self::new`] for a list whose controls `reveal` scrolls into view:
    /// its fades stop short of the focused control's ring.
    pub(crate) fn revealing(reveal: &FocusReveal, room: Pixels) -> Self {
        Self {
            reveal: Some(reveal.clone()),
            ..Self::new(reveal.scroll(), room)
        }
    }

    /// Whether this frame keeps the gutter: where the list overflowed when
    /// last laid out ([`cues`]).
    pub(crate) fn kept(&self) -> bool {
        self.cued.0
    }

    /// The gutter a list's rows keep while they overflow: the scrollbar's
    /// width and the room of the ring every palette installs, so a row's
    /// ring ends where the track begins.
    pub(crate) fn gutter() -> Pixels {
        let ring = appearance::BUTTON_FOCUS_RING;
        Scrollbar::width() + ring.gap + ring.width
    }

    /// For the list's `on_children_prepainted`: runs `prepainted`, then draws
    /// once more when this frame's layout cues otherwise than the cue
    /// assumed.
    pub(crate) fn observe(
        &self,
        prepainted: impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let (rows, cued) = (self.rows.clone(), self.cued);
        move |bounds, window, cx| {
            prepainted(bounds, window, cx);
            let settled = cues(&rows) == cued;
            #[cfg(test)]
            settling::observed(&rows, settled);
            if !settled {
                window.defer(cx, |window, _| window.refresh());
            }
        }
    }

    /// `list`, with the gutter while it overflows, the scrollbar `id` in it
    /// along a track a thumb moves on, and otherwise the fades at the edges
    /// its rows continue past. The list is a node that clips its rows
    /// (`native_accessibility::clips_children`), so a row scrolled out of
    /// view is out of reach as well as out of sight. Tests find the
    /// scrollbar, while it is drawn, by `id`.
    pub(crate) fn list(&self, list: Stateful<Div>, id: &'static str) -> Div {
        let gutter = Self::gutter();
        let room = self.rows.room;
        let (kept, scrollbar) = self.cued;
        let clear_track = self.clear_track;
        let cue = div()
            .flex()
            .flex_col()
            .child(crate::native_accessibility::clips_children(
                list.when(kept, |list| list.pr(room + gutter)),
            ))
            // In every frame, so a list that shrinks below the shortest
            // track fades in the frame that shrinks it.
            .child(edge_fades(
                self.rows.clone(),
                self.reveal.clone(),
                self.on_panel,
            ));
        if scrollbar {
            // The scrollbar is built as the frame paints (`in_frame`), after
            // the list has laid out in it, and only along a track a thumb
            // moves on then: a list that shrinks below the shortest track
            // draws no bar, and leaves no hitbox, in the frame that shrinks
            // it, where its fades already are. Built as the cue rendered,
            // from the length the list kept from the frame before, the bar
            // painted for that one frame over the new fade, and a pointer
            // could still meet it.
            let rows = self.rows.clone();
            cue.child(
                div().absolute().size_full().flex().flex_col().child(
                    crate::in_frame::in_frame(move |_, _, cx| {
                        if rows.viewport_bounds().size.height < SHORTEST_SCROLL_TRACK {
                            return div().into_any_element();
                        }
                        // The kit's scrollbar lays itself over the list
                        // already; this wrapper only carries its debug
                        // selector, which the kit's element takes none of.
                        // It stays: a test tells a list that draws no
                        // scrollbar from one that leaves a bar behind
                        // painting nothing, which a pointer would still
                        // meet, only by it, where a painted thumb shows only
                        // what is painted.
                        crate::in_frame::probed(
                            div().debug_selector(move || id.into()).size_full().child({
                                let bar = Scrollbar::vertical(&rows)
                                    .id(id)
                                    .mode(ScrollbarMode::Always);
                                if clear_track {
                                    // Its thumb rests on the rows, so it
                                    // takes the thumb fitted to them; hovered
                                    // and dragged, the palette's hovered
                                    // thumb, which the resting one set here
                                    // would otherwise replace.
                                    let clear = gpui_kit::transparent_black();
                                    let thumb = rgb(appearance::labels(cx).over_rows_thumb);
                                    let hovered = rgb(appearance::palette(cx).muted);
                                    bar.styles(|styles| {
                                        styles
                                            .track(|track| track.bg(clear).border_color(clear))
                                            .thumb(|style| style.bg(thumb))
                                            .thumb_hover(|style| style.bg(hovered))
                                            .thumb_active(|style| style.bg(hovered))
                                    })
                                } else {
                                    bar
                                }
                            }),
                            "scrollbar",
                        )
                        .into_any_element()
                    })
                    .named(id),
                ),
            )
        } else {
            cue
        }
    }
}

/// How tall the fades over the list `rows` reads are where its track is
/// too short for a thumb that moves, measured as it was last laid out: the
/// scaled [`EDGE_FADE`], at most a quarter of the list; zero along a track
/// a thumb moves on.
fn edge_fade(rows: &RowColumn) -> Pixels {
    if rows.viewport_bounds().size.height >= SHORTEST_SCROLL_TRACK {
        return Pixels::ZERO;
    }
    appearance::ui_size(EDGE_FADE).min(rows.scroll.bounds().size.height / 4.)
}

/// The fades over a list too short for its scrollbar's thumb to move
/// ([`ScrollCue`]): at each edge its rows continue past, a band as wide as
/// the list and [`EDGE_FADE`] tall, opaque at the edge in the theme's
/// background colour (the palette's canvas, which panes and dialogs are
/// drawn on, or its panel for a list on one, [`ScrollCue::on_panel`]) and
/// clear inward, so the rows there dissolve into the surface
/// rather than end at a cut, as macOS fades content scrolled beneath a
/// window's edge. It draws none at an end the list is scrolled to, and
/// none on a list whose rows fit.
///
/// It reads the list's bounds, track, offset and reach as it paints, after
/// the list has laid out and clamped its offset in the same frame, so a
/// wheel, a reveal, a resize or rows that arrive show their fades in the
/// first frame that draws them, and a list whose track has grown long
/// enough for a thumb draws none while its scrollbar is still to come; read
/// as the cue renders, rows that arrived drew no fade until something else
/// drew the list again, and a tall list drew a fade for one frame before
/// its scrollbar. It reads where `reveal`'s focused control lies then too,
/// and a fade whose band reaches that control's ring ends where the ring
/// begins ([`fade_reach`]). It inserts no hitbox,
/// so the pointer reaches what lies beneath it: a wheel over a fade scrolls
/// the list, and a row under one takes its hover and click. It does not
/// move: a fade comes and goes with the scroll that reveals an edge, in
/// that frame.
fn edge_fades(rows: RowColumn, reveal: Option<FocusReveal>, on_panel: bool) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            let scroll = &rows.scroll;
            let list = scroll.bounds();
            let fade = edge_fade(&rows);
            if fade <= Pixels::ZERO {
                return;
            }
            let ring = reveal
                .as_ref()
                .and_then(|reveal| reveal.focused_bounds(window, cx))
                .map(|control| control.dilate(appearance::button_ring_room(cx)));
            let (top_fade, bottom_fade) = fade_reach(list, fade, ring);
            // Half a pixel either way is an end: GPUI rounds the reach to
            // two decimals.
            let slack = px(0.5);
            let scrolled = -scroll.offset().y;
            let palette = appearance::palette(cx);
            let surface: Hsla = rgb(if on_panel {
                palette.panel
            } else {
                palette.canvas
            })
            .into();
            let band = |angle: f32| {
                linear_gradient(
                    angle,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )
            };
            if scrolled > slack && top_fade > Pixels::ZERO {
                // Toward the top, opaque at the list's top edge.
                let top = Bounds::new(list.origin, size(list.size.width, top_fade));
                window.paint_quad(fill(top, band(0.)));
            }
            if scrolled < scroll.max_offset().y - slack && bottom_fade > Pixels::ZERO {
                // Toward the bottom, opaque at the list's bottom edge.
                let bottom = Bounds::new(
                    point(list.left(), list.bottom() - bottom_fade),
                    size(list.size.width, bottom_fade),
                );
                window.paint_quad(fill(bottom, band(180.)));
            }
        },
    )
    .absolute()
    .size_full()
}

/// How far the fades `fade` tall reach in from the top and the bottom edge
/// of the list at `list`: each ends where `ring`, the focused control's
/// ring, begins, where its band reaches the ring, and is none where the
/// ring reaches the list's edge. Its band keeps its gradient, clear inward,
/// over the height left, so no hard edge lies across the list.
pub(crate) fn fade_reach(
    list: Bounds<Pixels>,
    fade: Pixels,
    ring: Option<Bounds<Pixels>>,
) -> (Pixels, Pixels) {
    let Some(ring) = ring else {
        return (fade, fade);
    };
    let top = if ring.bottom() > list.top() && ring.top() < list.top() + fade {
        (ring.top() - list.top()).max(Pixels::ZERO)
    } else {
        fade
    };
    let bottom = if ring.top() < list.bottom() && ring.bottom() > list.bottom() - fade {
        (list.bottom() - ring.bottom()).max(Pixels::ZERO)
    } else {
        fade
    };
    (top, bottom)
}

/// A list's scroll as its scrollbar reads it: over the column its rows lie
/// in, the list's bounds less the room it keeps around them, which scrolls
/// as far as the list does.
#[derive(Clone)]
struct RowColumn {
    scroll: ScrollHandle,
    room: Pixels,
}

impl ScrollbarHandle for RowColumn {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.scroll.bounds().dilate(-self.room)
    }

    fn offset(&self) -> Point<Pixels> {
        self.scroll.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.scroll.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        let viewport = self.viewport_bounds().size;
        let reach = self.scroll.max_offset();
        size(viewport.width + reach.x, viewport.height + reach.y)
    }
}

/// A cue whose layout keeps cueing otherwise than the frame assumed, frame
/// after frame, fails the test drawing it rather than hanging it: where
/// what overflows follows the gutter (a list that overflows only without
/// it, as File History's panel once did while its toolbar's form followed
/// the gutter), each frame asks for another, and GPUI's test platform
/// draws a dirty window until nothing is left to do, so the draw never
/// returned. In an application the window draws every frame instead.
#[cfg(test)]
pub(crate) mod settling {
    use super::RowColumn;
    use std::{cell::RefCell, collections::HashMap};

    /// The frames running a cue may find its layout cued otherwise.
    const FRAMES: usize = 16;

    thread_local! {
        /// The frames running each list, by where it was laid out, has
        /// found its layout cued otherwise than the frame assumed.
        static RUNS: RefCell<HashMap<String, usize>> = RefCell::default();
    }

    /// Forgets every list's run, as a test's draw begins: a run is the
    /// frames one draw runs, and a list's count, kept under bounds no later
    /// frame settled it at, survived on the thread into the next test, so
    /// `file_history::tests` under `ITERATIONS=20` failed after seeds that
    /// had passed (5 and 15).
    pub(crate) fn reset() {
        RUNS.with(|runs| runs.borrow_mut().clear());
    }

    /// Notes whether the list `rows` reads, as its frame prepainted it, was
    /// cued as that frame assumed.
    pub(super) fn observed(rows: &RowColumn, settled: bool) {
        let at = format!("{:?}", rows.scroll.bounds());
        RUNS.with(|runs| {
            let mut runs = runs.borrow_mut();
            if settled {
                runs.remove(&at);
                return;
            }
            let run = runs.entry(at.clone()).or_default();
            *run += 1;
            assert!(
                *run < FRAMES,
                "the scroll cue of the list at {at} cued otherwise than its frame assumed \
                 {run} frames running: what overflows follows its gutter"
            );
        });
    }
}

/// Whether the list `scroll` tracks overflowed when last laid out, as the
/// kit's scrollbar judges it before painting a thumb.
fn overflows(scroll: &ScrollHandle) -> bool {
    scroll.max_offset().y > Pixels::ZERO
}

/// Whether the list `rows` reads keeps its gutter, where it overflowed when
/// last laid out, and whether it draws its scrollbar there, along a track a
/// thumb moves on ([`SHORTEST_SCROLL_TRACK`]). The kit lays that track over
/// the scrollbar's viewport, the list less the room it keeps around its
/// rows (`RowColumn::viewport_bounds`); judged by the list's whole height,
/// a list keeping room drew a thumb along a track up to twice that room
/// shorter than the shortest.
fn cues(rows: &RowColumn) -> (bool, bool) {
    let overflows = overflows(&rows.scroll);
    (
        overflows,
        overflows && rows.viewport_bounds().size.height >= SHORTEST_SCROLL_TRACK,
    )
}

use crate::native_accessibility::text as static_text;

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    draft: Option<Entity<TagForm>>,
    /// The Tags list or Push to… list opened last, for tests.
    #[cfg(test)]
    list: Option<FocusReveal>,
    /// The filter of the Tags dialog opened last, for tests.
    #[cfg(test)]
    filter: Option<Entity<InputState>>,
}

impl GitTurtle {
    pub(super) fn cancel_tag_action(&mut self) {
        self.tag_actions.generation = self.tag_actions.generation.wrapping_add(1);
        self.tag_actions.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }
    pub(super) fn open_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_tag_action(
            |repo| repo.tags(),
            |result, this, window, cx| match result {
                Ok(list) => {
                    let owner = cx.entity().downgrade();
                    let path = this.path.clone();
                    // An unborn HEAD with no commit in History: nothing for a
                    // tag to name.
                    let committed = !(this.commits.is_empty()
                        && this
                            .work_status
                            .as_ref()
                            .is_some_and(|status| status.head.is_none()));
                    let browser =
                        cx.new(|cx| TagBrowser::new(owner, path, list, committed, window, cx));
                    #[cfg(test)]
                    {
                        this.tag_actions.list = Some(browser.read(cx).rows.clone());
                    }
                    let filter = browser.read(cx).query.read(cx).focus_handle(cx);
                    #[cfg(test)]
                    {
                        this.tag_actions.filter = Some(browser.read(cx).query.clone());
                    }
                    window.open_alert_dialog(cx, move |dialog, window, cx| {
                        // The dialog clips its body to the body's bounds, and
                        // the browser keeps the focus ring's room inside them
                        // above Create tag… and below the last row. The
                        // title's margin and the footer's gap give the room
                        // back, so nothing in the dialog moves.
                        let room = appearance::button_ring_room(cx);
                        dialog
                            .title(
                                native_accessibility::dialog_title("tags-dialog-title", "Tags")
                                    .ml(TEXT_INSET)
                                    .mb(-room),
                            )
                            .width(appearance::dialog_width(px(600.), window))
                            .max_h(appearance::dialog_max_height(window))
                            .gap(DIALOG_FOOTER_GAP - room)
                            .child(browser.clone())
                            .footer(crate::tags::dialog_footer("Done", None))
                    });
                    // Opening the dialog focuses its shell, which draws no
                    // ring: the filter, the first field, takes focus so the
                    // first frame draws its ring, as the Reflog's does.
                    filter.focus(window, cx);
                    window.refresh();
                }
                Err(error) => {
                    this.operation_error = Some(crate::views::OperationFailure::of(&error))
                }
            },
            window,
            cx,
        );
    }
    fn read_tag_action<T: Send + 'static>(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return false;
        }
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let path = repo.path().to_owned();
        self.cancel_tag_action();
        let generation = self.tag_actions.generation;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self.operations.submit_read(move || prepare(repo));
        self.tag_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Tag preparation was interrupted. Open Tags again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.tag_actions.generation != generation {
                    return;
                }
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                this.read_deferred_working(window, cx);
                if this.path.as_ref() == Some(&path) && this.page == AppPage::Repository {
                    receive(result, this, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }
    pub(crate) fn inspect_tag(&mut self, tag: Tag, window: &mut Window, cx: &mut Context<Self>) {
        self.read_tag_action(
            move |repo| Ok((repo.tag_details(&tag)?, repo.remote_configs()?)),
            |result, this, window, cx| match result {
                Ok((details, remotes)) => this.show_tag_details(details, remotes, window, cx),
                Err(error) => {
                    this.operation_error = Some(crate::views::OperationFailure::of(&error))
                }
            },
            window,
            cx,
        );
    }
    fn show_tag_details(
        &mut self,
        details: TagDetails,
        remotes: Vec<RemoteConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let details = Arc::new(details);
        let remotes = Arc::new(remotes);
        let annotation = (details.tag.annotated && details.annotation_unavailable.is_none())
            .then(|| text::editor(&details.message, "text", None, window, cx));
        // The Push to… list's scroll, which reveals the button Tab moves onto.
        let list = FocusReveal::new(Trigger::Keyboard);
        #[cfg(test)]
        {
            self.tag_actions.list = Some(list.clone());
        }
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            // A button focus has moved onto scrolls into view in this frame.
            list.reveal(window, cx);
            let p = palette(cx); let tag = &details.tag;
            let deletion = tag.clone(); let delete_owner = owner.clone(); let delete_path = path.clone();
            let oid = tag.oid.clone();
            let tag_mark = copy_feedback::Mark::new("copy-tag-oid", &tag.oid);
            let tag_copy = tag_mark.clone();
            // The Push to… list scrolls, so it clips its buttons to its bounds
            // on both axes: it keeps the focus ring's room around them and
            // gives it back through its margin. The content keeps the room
            // below its last control inside the dialog's clip of its body,
            // and the footer's gap gives that back, so nothing moves. A
            // button Tab moves onto scrolls into view with its ring.
            let room = appearance::button_ring_room(cx);
            let pushes: Vec<_> = (0..remotes.len()).map(|index| list.focus(("push-tag-remote", index), cx)).collect();
            let reveal = list.items(pushes.clone(), room);
            // Past its height the list cues the rest with its scrollbar.
            let cue = ScrollCue::revealing(&list, room);
            // A write refuses Delete and Push at once, and assistive
            // technology hears them unavailable at once, while they dim only
            // once it looks busy (`workspace::BusyLook`); each refused a
            // press silently, announced and drawn available. This dialog is
            // built in the window's render, so each reads the window's
            // state as it lays out (`workspace::dialog_refusing`).
            let content = div().flex().flex_col().gap_3().pb(room)
                .child(static_text("tag-target-identity", format!("{} · {} {}", if tag.annotated { "Annotated tag" } else { "Lightweight tag" }, tag.target_kind, tag.target_oid)).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .child(static_text("tag-object-identity", format!("Tag object: {}", tag.oid)).text_size(crate::appearance::ui_text(11.)))
                .when(!tag.tagger.is_empty(), |element| element.child(static_text("tag-author", format!("Tagged by {}", tag.tagger)).text_size(crate::appearance::ui_text(12.))))
                .when_some(annotation.as_ref(), |element, annotation| element.child(crate::editor_find::Editor::new(annotation).readonly(true).h(px(160.)).aria_label("Tag annotation and signature, if present")))
                .when_some(details.annotation_unavailable.as_ref(), |element, message| element.child(static_text("tag-annotation-unavailable", message.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
                .child(div().flex().gap_2()
                    .child(copy_feedback::copy_hash_button(tag_mark, "Copy object ID", "", cx).on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(tag_copy.clone()), "tag object ID", oid.clone())))
                    .child(crate::workspace::dialog_refusing(owner.clone(), path.clone(), move |refused, dimmed| button("delete-local-tag", "Delete local tag…", "", false).debug_selector(|| "delete-local-tag".into()).map(|delete| crate::workspace::refusing(delete, || "delete-local-tag".into(), refused, dimmed)).on_click(move |_, window, cx| {
                        let _ = delete_owner.update(cx, |this, cx| {
                            if !this.dialog_refusal(delete_path.as_deref()).0 {
                                window.close_dialog(cx);
                                this.confirm_git_write(format!("Delete local tag '{}'", deletion.name), format!("Remove the local name '{}' pointing to {}. Its annotation and signature will no longer be reachable through this name.\n\nRemote tags, branches, the index and working files stay as they are. This action refuses if the tag has moved since you opened it.", deletion.name, deletion.oid), "Delete local tag", WriteCommand::Tag(Arc::new(TagCommand::Delete(deletion.clone()))), window, cx);
                            }
                        });
                    }).into_any_element())))
                .child(static_text("tag-push-heading", "Push this tag").text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM))
                .child(static_text("tag-push-consequences", "Choose one remote, then review. No other tags or branches are pushed. Existing remote tags are never replaced.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
                .child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("tag-remote-list").debug_selector(|| "tag-remote-list".into()).max_h(px(160.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(list.scroll()).flex().flex_col().gap_1().children(remotes.iter().zip(pushes).enumerate().map(|(index, (remote, focus))| {
                    let remote = remote.clone(); let tag = tag.clone(); let owner = owner.clone(); let path = path.clone();
                    crate::workspace::dialog_refusing(owner.clone(), path.clone(), move |refused, dimmed| button(("push-tag-remote", index), format!("Push to {}…", remote.name), "", false).track_focus(&focus).debug_selector(move || format!("push-tag-remote-{index}")).map(|push| crate::workspace::refusing(push, || format!("push-tag-remote-{index}"), refused, dimmed)).on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if !this.dialog_refusal(path.as_deref()).0 {
                                window.close_dialog(cx);
                                let urls = if remote.push_urls.is_empty() { &remote.urls } else { &remote.push_urls };
                                let destinations = urls.iter().map(|url| crate::workspace::display_remote_url(url)).collect::<Vec<_>>().join("\n");
                                this.confirm_git_write(format!("Push tag '{}'", tag.name), format!("Send only '{}' ({}) to remote '{}':\n{}\n\nThis explicit network action creates the named remote tag. It does not push branches, additional tags, or overwrite an existing remote tag.", tag.name, tag.oid, remote.name, destinations), "Push named tag", WriteCommand::Tag(Arc::new(TagCommand::Push { tag: tag.clone(), remote: Arc::new(remote.clone()) })), window, cx);
                            }
                        });
                    }).into_any_element())
                })).when(remotes.is_empty(), |element| element.child(static_text("tag-remotes-empty", "No remotes configured. Add a remote from the branch menu to push a tag.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))), "tag-remote-scrollbar"));
            dialog.title(native_accessibility::dialog_title("tag-inspector-title", details.tag.name.clone())).width(appearance::dialog_width(px(640.), window)).max_h(appearance::dialog_max_height(window)).gap(DIALOG_FOOTER_GAP - room).child(content).footer(crate::tags::dialog_footer("Done", None))
        });
    }
    pub(super) fn finish_tag_write(
        &mut self,
        path: &std::path::Path,
        command: &TagCommand,
        succeeded: bool,
        cx: &mut Context<Self>,
    ) {
        if !succeeded {
            return;
        }
        let TagCommand::Create(plan) = command else {
            return;
        };
        if self.tag_actions.draft.as_ref().is_some_and(|draft| {
            let draft = draft.read(cx);
            draft.path.as_deref() == Some(path)
                && draft.name.read(cx).value().trim() == plan.name
                && draft
                    .annotated
                    .then(|| draft.message.read(cx).value().to_string())
                    == plan.annotation
        }) {
            self.tag_actions.draft = None;
        }
    }
    fn open_create_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let target = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .map(|commit| commit.oid.clone())
            .unwrap_or_else(|| "HEAD".into());
        let form = self
            .tag_actions
            .draft
            .as_ref()
            .filter(|form| form.read(cx).path == path)
            .cloned()
            .unwrap_or_else(|| cx.new(|cx| TagForm::new(owner, path, target, window, cx)));
        self.tag_actions.draft = Some(form.clone());
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(native_accessibility::dialog_title(
                    "create-tag-dialog-title",
                    "Create a local tag",
                ))
                .width(appearance::dialog_width(px(560.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(form.clone())
                // Cancel and OK on the compact frame, as `dialog_footer`
                // builds them, built here so that Review tag can follow a
                // write (`ReviewTag`).
                .footer(
                    DialogFooter::new()
                        .child({
                            // Cancels its own form and closes the dialog,
                            // wherever focus is: asking the focused element
                            // to cancel missed the dialog once Settings had
                            // shown and taken focus.
                            let form = form.clone();
                            crate::tags::dialog_cancel("cancel", "Cancel")
                                .debug_selector(|| "cancel-create-tag".into())
                                .on_click(move |_, window, cx| {
                                    form.update(cx, |form, cx| form.cancel(window, cx));
                                    window.close_dialog(cx);
                                })
                        })
                        .child(ReviewTag { form: form.clone() }),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |this, cx| this.submit(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |form, cx| form.cancel(window, cx));
                    true
                })
        });
    }
}

struct TagBrowser {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    list: TagList,
    /// Whether the repository has a commit for a tag to name. With none,
    /// Create tag… cannot work: it is disabled with its reason, and the
    /// empty list points at the first commit.
    committed: bool,
    query: Entity<InputState>,
    /// The list's scroll, which reveals the row Tab moves onto.
    rows: FocusReveal,
    /// Whether the dialog's Confirm comes from Return in the filter.
    field_return: FieldReturn,
    _subscription: Subscription,
}
impl TagBrowser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        list: TagList,
        committed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx
            .new(|cx| InputState::new(window, cx).placeholder("Filter tags by name or object ID"));
        // Return in the filter is the dialog's Confirm, routed here
        // ([`Self::confirm_in_field`]), never the field's own `PressEnter`.
        let subscription = cx.subscribe_in(&query, window, |_, _, _: &InputEvent, _, cx| {
            cx.notify();
        });
        Self {
            owner,
            path,
            list,
            committed,
            query,
            rows: FocusReveal::new(Trigger::Keyboard),
            field_return: FieldReturn::default(),
            _subscription: subscription,
        }
    }
    /// Return in the filter, delivered as the dialog's Confirm action:
    /// it inspects the first tag the filter matches, as that tag's row
    /// does, once per press, as the key goes down, never on a held key's
    /// repeats, under X11 too, and never for a modified Enter, as the dialog
    /// confirms. With no match it does nothing, and refused, as the rows
    /// are while a write runs or Tags' repository is no longer shown
    /// ([`Self::activate`]), it says why ([`Self::refuse_choice`]): either
    /// way Tags stays open with its filter. Anywhere else, and for Done's
    /// click, it is the dialog's own, Done ([`FieldReturn`]). The filter
    /// acted on its own `PressEnter` while the dialog's Done took the same
    /// press: with a write running the choice was refused silently and Tags
    /// closed, the filter lost, and with no match Return closed Tags.
    fn confirm_in_field(&mut self, confirm: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let filter = self.query.read(cx).focus_handle(cx);
        if !self.field_return.take() || confirm.secondary || !filter.is_focused(window) {
            cx.propagate();
            return;
        }
        let Some(tag) = self.matches(cx).first().map(|tag| (*tag).clone()) else {
            return;
        };
        if !self.activate(tag, window, cx) {
            self.refuse_choice(window, cx);
        }
    }
    /// Says why Return in the filter chose nothing, where Tags' rows only
    /// stand unavailable: a write's busy look shows at once, as a refused
    /// press shows it (`GitTurtle::show_busy_look`), dimming the rows and
    /// Create tag…, and the window's alert says the reason at once, again
    /// for each press (`copy_feedback::alert`).
    fn refuse_choice(&self, window: &mut Window, cx: &mut Context<Self>) {
        let reason = self
            .owner
            .update(cx, |owner, cx| {
                if owner.path.as_deref() != self.path.as_deref()
                    || owner.page != AppPage::Repository
                {
                    NOT_SHOWN_REASON
                } else {
                    owner.show_busy_look(cx);
                    owner.busy_reason()
                }
            })
            .unwrap_or(NOT_SHOWN_REASON);
        crate::copy_feedback::alert(window, cx, reason);
    }
    fn matches(&self, cx: &App) -> Vec<&Tag> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        self.list
            .tags
            .iter()
            .filter(|tag| {
                tag.name.to_lowercase().contains(&query)
                    || tag.oid.contains(&query)
                    || tag.target_oid.contains(&query)
            })
            .take(101)
            .collect()
    }
    /// Inspects `tag`, as its row or Enter in the filter asks, unless the
    /// window refuses the dialog's actions, as the rows are announced
    /// (`workspace::dialog_refusal`): a write runs, or Tags' repository or
    /// the Repository page is no longer shown. It once checked only the
    /// repository and the write, so a press off the Repository page closed
    /// Tags for a read that then refused.
    ///
    /// Whether it inspects the tag.
    fn activate(&self, tag: Tag, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.owner
            .update(cx, |this, cx| {
                let refused = this.dialog_refusal(self.path.as_deref()).0;
                if !refused {
                    window.close_dialog(cx);
                    this.inspect_tag(tag, window, cx);
                }
                !refused
            })
            .unwrap_or(false)
    }
}
/// A Tags row's ID, and focus key: its tag's name, which no other local tag
/// shares, never its place, which moves as the filter lets other tags in
/// above it, so focus stays on the tag it was on. Keyed by place, the focus
/// a row held passed to whichever tag moved into its place. Hashed, as a
/// stash's row ID is, so the key costs no allocation; its debug selector is
/// its place.
fn tag_row_id(tag: &Tag) -> ElementId {
    use std::hash::{Hash, Hasher};
    let mut name = std::hash::DefaultHasher::new();
    tag.name.hash(&mut name);
    ElementId::NamedInteger("tag-row".into(), name.finish())
}

/// Tags' line in a repository with no commits, where a tag has nothing to
/// name.
const NO_COMMIT_LINE: &str = "No commits to tag yet · Your first commit starts in Changes";
/// Create tag…'s name and tooltip there, which say why it is unavailable.
const NO_COMMIT_NAME: &str = "Create tag…, unavailable until the first commit";
const NO_COMMIT_REASON: &str = "A tag names a commit · Your first commit starts in Changes";

impl Render for TagBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A row focus has moved onto scrolls into view in this frame.
        self.rows.reveal(window, cx);
        let p = palette(cx);
        let matches = self.matches(cx);
        // The browser keeps the focus ring's room above Create tag… and below
        // its last control inside the dialog's clip, which the dialog gives
        // back. The list scrolls, so it clips its rows to its bounds on both
        // axes: it keeps the room around them and gives it back through its
        // margin, so no row moves. A row Tab moves onto scrolls into view
        // with its ring.
        //
        // The dialog's text starts at one inset, the filter's: the title,
        // the summary, the notices, the empty list's line and each row's
        // name, which the kit centred in its row while the summary started
        // at the dialog's edge. With no tag at all the line alone says so;
        // the summary above it offered to inspect, delete or push a tag
        // that was not there.
        let room = appearance::button_ring_room(cx);
        let rows: Vec<_> = matches
            .iter()
            .take(100)
            .map(|tag| self.rows.focus(tag_row_id(tag), cx))
            .collect();
        let reveal = self.rows.items(rows.clone(), room);
        // Past its height the list cues the rest with its scrollbar.
        let cue = ScrollCue::revealing(&self.rows, room);
        // A write refuses Create tag… and the rows, which open a tag, at
        // once, and assistive technology hears them unavailable at once,
        // while they dim only once it looks busy (`workspace::BusyLook`);
        // each refused a press silently, announced and drawn available.
        let (refused, dimmed) =
            crate::workspace::dialog_refusal(&self.owner, self.path.as_deref(), cx);
        let committed = self.committed;
        div().flex().flex_col().gap_3().py(room)
            // Return in the filter is its own ([`Self::confirm_in_field`]).
            .on_action(cx.listener(|this, _: &Enter, _, cx| {
                this.field_return.note(cx);
                cx.propagate();
            }))
            .on_action(cx.listener(Self::confirm_in_field))
            // Painted after the field, the canvas fits its placeholder in
            // whole words, where at 18 pt in a half tile it was cut mid-glyph.
            .child(div().flex().gap_2().child(div().relative().flex_1().child(Input::new(&self.query).aria_label("Filter local tags").cleanable(true)).child(crate::workspace::placeholder_fit(self.query.clone(), &FILTER_FIT, FILTER_PLACEHOLDERS, FIELD_TEXT))).child(button("create-tag", "Create tag…", "plus", false).debug_selector(|| "create-tag".into()).when(!committed, |create| create.accessibility_label(NO_COMMIT_NAME).tooltip(NO_COMMIT_REASON)).map(|create| crate::workspace::refusing(create, || "create-tag".into(), refused || !committed, dimmed || !committed)).on_click(cx.listener(|this, _, window, cx| {
                let _ = this.owner.update(cx, |owner, cx| { if !owner.dialog_refusal(this.path.as_deref()).0 { window.close_dialog(cx); owner.open_create_tag(window, cx); } });
            }))))
            .when(!self.list.tags.is_empty(), |element| element.child(static_text("tag-list-summary", summary(self.list.tags.len())).ml(TEXT_INSET).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("tags-list").debug_selector(|| "tags-list".into()).max_h(px(360.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(self.rows.scroll()).flex().flex_col().gap_1().children(matches.iter().zip(rows).enumerate().map(|(index, (tag, focus))| {
                let tag = (*tag).clone(); let label = format!("{} · {} · {}", tag.name, if tag.annotated { "Annotated" } else { "Lightweight" }, short_oid(&tag.target_oid));
                Button::new(tag_row_id(&tag)).track_focus(&focus).debug_selector(move || format!("tag-row-{index}")).ghost().w_full().h(crate::appearance::ui_size(34.)).pl(TEXT_INSET).child(row_text(format!("tag-row-text-{index}"), label.clone())).accessibility_label(label).map(|row| crate::workspace::refusing(row, || format!("tag-row-{index}"), refused, dimmed)).on_click(cx.listener(move |this, _, window, cx| { this.activate(tag.clone(), window, cx); }))
            })).when(matches.is_empty(), |element| element.child(div().py_3().ml(TEXT_INSET).child(EmptyState::new("tag-list-empty", if !self.committed { NO_COMMIT_LINE } else if self.list.tags.is_empty() { "No local tags yet · Create tag… names a commit" } else { "No tags match this filter" }).block(cx)))), "tags-scrollbar"))
            .when(matches.len() > 100, |element| element.child(static_text("tag-list-match-limit", "Showing 100 matches. Narrow the filter to find another tag.").ml(TEXT_INSET).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .when(self.list.truncated, |element| element.child(static_text("tag-list-load-limit", "Loaded the first 10,000 local tags by name. Additional tags are outside this bounded browser.").ml(TEXT_INSET).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))))
    }
}

/// The summary above a list of `count` tags, the count agreeing with its
/// noun: "1 local tag" read "1 local tags".
fn summary(count: usize) -> String {
    format!(
        "{} · Select to inspect, delete locally, or push one named tag.",
        crate::counted(count, "local tag", "local tags")
    )
}

/// Where the Tags dialog's text starts, in from its content edge: the
/// filter's text inset, where its placeholder starts at every size. The
/// title, the summary, the notices, the empty list's line and each row's
/// name start here, as the Reflog's text does.
const TEXT_INSET: Pixels = appearance::FIELD_TEXT_INSET;

/// The filter's placeholders, each a shorter wording of the one before it in
/// whole words: the first that fits the field shows
/// (`workspace::placeholder_fit`). The field keeps its accessible name,
/// "Filter local tags", whichever shows.
const FILTER_PLACEHOLDERS: &[&str] =
    &["Filter tags by name or object ID", "Filter tags…", "Filter"];

/// The size the kit's medium field draws its text at, at the default
/// interface size: 0.875 rem of a 13 px rem.
const FIELD_TEXT: f32 = 13. * 0.875;

thread_local! {
    /// The filter's placeholder as it fits the width the field last painted at.
    static FILTER_FIT: crate::workspace::PlaceholderFit = const { std::cell::RefCell::new(None) };
}

/// A list row's name, as a list row starts it rather than a centred Button
/// label: from the row's leading inset, on one line ending in an ellipsis
/// where the row is too narrow. The row's accessible name keeps it whole.
pub(crate) fn row_text(selector: String, text: String) -> Div {
    div()
        .debug_selector(move || selector.clone())
        .flex_1()
        .min_w_0()
        .whitespace_nowrap()
        .text_ellipsis()
        .line_height(relative(1.))
        .child(text)
}

struct TagForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    name: Entity<InputState>,
    target: Entity<InputState>,
    message: Entity<TextareaState>,
    annotated: bool,
    error: Option<String>,
    /// The generation of the window's tag read this form started
    /// (`TagForm::pending`).
    preparing: Option<u64>,
}
impl TagForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            owner,
            path,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("v1.0.0")),
            target: cx.new(|cx| InputState::new(window, cx).default_value(target)),
            message: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Describe this tag")
                    .rows(4)
            }),
            annotated: true,
            error: None,
            preparing: None,
        }
    }
    /// Whether the read this form started still runs: the window's tag read
    /// of its generation, which ends when it lands, is dropped, or is
    /// cancelled. A flag the read's reply cleared stayed set when the reply
    /// was dropped, as one landing while Settings showed was, so every
    /// later Review tag was refused while it looked available.
    fn pending(&self, cx: &App) -> bool {
        self.preparing.is_some_and(|generation| {
            self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                owner.tag_actions.generation == generation
                    && owner.operation_busy == Some(PREPARING)
            })
        })
    }
    /// Ends the form's preparation and its read, as Cancel or Escape closes
    /// the dialog, and runs the working-state read asked for meanwhile,
    /// which the cancelled preparation's end would have run.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preparing = None;
        let _ = self.owner.update(cx, |this, cx| {
            this.cancel_tag_action();
            this.read_deferred_working(window, cx);
        });
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused at once while a write runs, as Review tag is announced
        // (`ReviewTag`), before anything changes: a press set the form's
        // error, "The repository changed or another operation is running".
        // The form's own read runs as such a busy spell, so it is refused
        // here too.
        if crate::workspace::dialog_refusal(&self.owner, self.path.as_deref(), cx).0 {
            return;
        }
        let name = self.name.read(cx).value().trim().to_owned();
        let target = self.target.read(cx).value().trim().to_owned();
        let message = self
            .annotated
            .then(|| self.message.read(cx).value().to_string());
        let form = cx.entity().downgrade();
        let preparing = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return None; }
            this.read_tag_action(move |repo| repo.create_tag_plan(&name, &target, message), move |result, this, window, cx| {
                let _ = form.update(cx, |form, cx| { form.preparing = None; cx.notify(); });
                match result {
                    Ok(plan) => {
                        window.close_dialog(cx);
                        let explanation = format!("Create local tag '{}' at commit {}.\n\n{}{}\n\n{}\n\nNo remote is contacted. Your branches, index, and working files stay as they are.", plan.name, plan.target_oid, if plan.annotation.is_some() { "Annotated tag." } else { "Lightweight tag." }, if plan.signing { " Git's configured signing is enabled." } else { " Git's configured signing behavior is preserved." }, plan.annotation.as_deref().unwrap_or("This tag has no annotation."));
                        this.confirm_git_write("Create local tag".into(), explanation, "Create tag", WriteCommand::Tag(Arc::new(TagCommand::Create(plan))), window, cx);
                    }
                    Err(error) => { let _ = form.update(cx, |form, cx| { crate::tags::set_error_line(&mut form.error, format!("{error:#}"), window, cx); cx.notify(); }); }
                }
            }, window, cx).then_some(this.tag_actions.generation)
        }).ok().flatten();
        self.preparing = preparing;
        if preparing.is_none() {
            crate::tags::set_error_line(
                &mut self.error,
                "The repository changed or another operation is running. Reopen Create tag when it finishes.",
                window,
                cx,
            );
        }
        cx.notify();
    }
}
impl Render for TagForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pending = self.pending(cx);
        div().flex().flex_col().gap_3()
            .child(static_text("tag-name-label", "Tag name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.name).aria_label("Tag name"))
            .child(static_text("tag-target-label", "Target commit (selected commit by default)").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.target).aria_label("Target commit for tag"))
            .child(div().flex().gap_2().children([(false, "Lightweight"), (true, "Annotated")].map(|(annotated, label)| button(label, label, "", self.annotated == annotated).toggled(self.annotated == annotated).disabled(pending).on_click(cx.listener(move |this, _, _, cx| { this.annotated = annotated; this.error = None; cx.notify(); })))))
            .when(self.annotated, |element| element.child(shortcuts::tab_leaves(Textarea::new(&self.message).aria_label("Tag annotation"))))
            .child(static_text("tag-signing-explanation", "Signing follows Git configuration. Signing failures are reported without an unsigned fallback.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when(pending, |element| element.child(static_text("tag-preparation-status", "Preparing exact target and signing settings…").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| error_line("tag-preparation-error", error, Some(crate::appearance::ui_text(12.)), window, cx).text_color(rgb(p.warning))))
    }
}

/// Review tag's accessible name while the window's busy spell refuses it:
/// unavailable, and why, in the words of the `reason` its tooltip gives
/// once it dims (`GitTurtle::busy_reason`), as Create tag…'s name and
/// tooltip say why it waits for a first commit. Its name and tooltip once
/// named the current Git operation whatever the spell ran.
fn review_tag_busy_name(reason: &str) -> String {
    format!("Review tag, unavailable. {reason}")
}

/// Create tag's Review tag, the kit's OK as its own footer would draw it.
/// A write refuses it at once, and assistive technology hears it unavailable
/// at once, its name saying why, while it dims, its tooltip saying why, only
/// once the write looks busy (`workspace::BusyLook`), as Tags' own actions
/// do (`workspace::dialog_refusal`). The kit drew it available through any
/// write, and a press set the form's error. The dialog is built in the
/// window's render, so Review tag reads the window's state as it lays out,
/// as `workspace::dialog_refusing` does. The form's own preparation runs as
/// a read the window counts busy; its status line says what it does, so it
/// gives no reason of its own.
#[derive(IntoElement)]
struct ReviewTag {
    form: Entity<TagForm>,
}

impl RenderOnce for ReviewTag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        review_tag(self.form, cx)
    }
}

/// Review tag as `form` and the window leave it now ([`ReviewTag`]).
fn review_tag(form: Entity<TagForm>, cx: &App) -> Button {
    let state = form.read(cx);
    // The form's own read runs as a busy spell, which refuses Review tag and
    // dims it with its busy look; it gives no reason, as its status line
    // says what it does.
    let pending = state.pending(cx);
    // Why the window's busy spell refuses it, while one other than the
    // form's own runs: the spell's own reason, which names a GitHub
    // request's work where one runs (`GitTurtle::busy_reason`).
    let (refused, dimmed, reason) = state.owner.upgrade().map_or((true, true, None), |owner| {
        let owner = owner.read(cx);
        let (refused, dimmed) = owner.dialog_refusal(state.path.as_deref());
        let busy = owner.operation_busy.is_some() && !pending;
        (refused, dimmed, busy.then(|| owner.busy_reason()))
    });
    let tooltip = reason.filter(|_| dimmed);
    #[cfg(test)]
    REVIEW_TAG.with(|review| review.set(tooltip));
    {
        let review = button("ok", "Review tag", "", false)
            .primary()
            .debug_selector(|| "review-tag".into())
            .when_some(reason, |review, reason| {
                review.accessibility_label(review_tag_busy_name(reason))
            })
            .when_some(tooltip, |review, reason| review.tooltip(reason));
        // A press submits its own form, wherever focus is: the kit's OK
        // asked the focused element to confirm, which missed the dialog
        // once Settings had shown and taken focus.
        crate::workspace::refusing(review, || "review-tag".into(), refused, dimmed)
            .on_click(move |_, window, cx| form.update(cx, |form, cx| form.submit(window, cx)))
    }
}

#[cfg(test)]
thread_local! {
    /// The tooltip Review tag was last built with, which no node carries.
    static REVIEW_TAG: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

/// The filter of the Tags dialog opened last, for the focus sweep.
#[cfg(test)]
pub(crate) fn filter(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> FocusHandle {
    app.read_with(cx, |app, cx| {
        app.tag_actions
            .filter
            .as_ref()
            .map(|query| query.read(cx).focus_handle(cx))
    })
    .expect("Tags opened")
}

/// Opens Create tag, annotated as it opens, and returns its annotation
/// field, for the prose fields' Tab regression in `views::tests`.
#[cfg(test)]
pub(crate) fn open_tag_annotation(
    app: &mut GitTurtle,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) -> Entity<TextareaState> {
    app.open_create_tag(window, cx);
    let form = app.tag_actions.draft.clone().expect("Create tag opens");
    let form = form.read(cx);
    assert!(form.annotated, "Create tag opens annotated");
    form.message.clone()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::focus_reveal::tests::{
        assert_every_frame_reveals, assert_steady, assert_tab_reveals, control, focus_filter,
    };
    use ::core::prelude::v1::test;
    use gpui_kit::component::{FocusRing, Root, Theme};
    use std::{cell::RefCell, rc::Rc};

    /// A long error tooltip is cut between whole grapheme clusters, at each
    /// byte inside a letter with its combining marks, a ZWJ emoji sequence
    /// or a flag, still within its byte bound.
    #[test]
    fn an_error_tooltip_is_cut_between_whole_grapheme_clusters() {
        use crate::text_cut::tests::{CLUSTERS, assert_whole_prefix};
        for (case, cluster) in CLUSTERS {
            for inside in 1..cluster.len() {
                let report = format!(
                    "{}{cluster}tail",
                    "a".repeat(super::ERROR_TOOLTIP_BYTES - inside)
                );
                let shown = super::error_tooltip(&report);
                let kept = shown.strip_suffix('…').expect("a cut tooltip");
                assert_whole_prefix(&report, kept, case);
                assert_eq!(kept.len(), super::ERROR_TOOLTIP_BYTES - inside, "{case}");
            }
        }
    }

    /// Runs Git in `path` with no configuration beyond the fixture identity.
    pub(crate) fn git(path: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Three tagged commits: three local tags and three entries in HEAD's
    /// reflog.
    pub(crate) fn tagged_repository(directory: &std::path::Path) -> GitRepository {
        let repo = crate::views::tests::fixture_repository(directory.join("repository"));
        for index in 0..3 {
            let message = format!("Commit {index}");
            git(
                repo.path(),
                &["commit", "--quiet", "--allow-empty", "-m", &message],
            );
            git(repo.path(), &["tag", &format!("v0.{index}")]);
        }
        repo
    }

    /// The application in a window on `repo`, with its palette and focus ring
    /// installed and dialogs opening at rest.
    pub(crate) fn window<'a>(
        cx: &'a mut TestAppContext,
        repo: &GitRepository,
    ) -> (Entity<GitTurtle>, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            // A dialog otherwise slides in on a wall-clock animation.
            cx.set_reduce_motion(true);
        });
        let repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                // Finish startup's preference reads before GPUI first polls
                // their reply, as `commit_message`'s fixture does.
                futures::executor::block_on(app.preferences_writer.submit(|| Ok(())))
                    .expect("startup preferences worker replied")
                    .expect("startup preferences queue drained");
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        draw(cx);
        (app, cx)
    }

    pub(crate) fn draw(cx: &mut VisualTestContext) {
        crate::tags::settling::reset();
        for _ in 0..3 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.executor().run_until_parked();
        }
    }

    /// Returns once the read `open_tags` or `inspect_tag` started has landed.
    /// It runs on the app's operation executor, a thread `run_until_parked`
    /// cannot wait for, and the dialog it prepares opens only with its reply,
    /// so a wait in wall-clock time missed a slow read.
    pub(crate) async fn read_lands(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("a tag read was started").await;
    }

    /// The kit's tooltip delay, after which a hover shows its tooltip.
    pub(crate) const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

    /// The popups the last frame painted, by the kit's popover fill.
    fn painted_popups(cx: &mut VisualTestContext) -> Vec<Bounds<ScaledPixels>> {
        let popover = cx.read(|cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).tokens.popover)
        });
        cx.update(|window, _| {
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.background == popover)
                .map(|quad| quad.bounds)
                .collect()
        })
    }

    /// The tooltips hovering `selector` newly paints, by the kit's popover
    /// fill; the pointer then leaves the window until they have gone.
    pub(crate) fn hovered_tooltips(cx: &mut VisualTestContext, selector: &'static str) -> usize {
        const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
        let popover = cx.read(|cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).tokens.popover)
        });
        let popups = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover)
                    .map(|quad| quad.bounds)
                    .collect::<Vec<_>>()
            })
        };
        let before = popups(cx);
        let control = rendered(cx, selector);
        cx.simulate_mouse_move(control.center(), None, Modifiers::default());
        cx.executor().advance_clock(SHOW_DELAY);
        draw(cx);
        let shown = popups(cx)
            .into_iter()
            .filter(|popup| !before.contains(popup))
            .count();
        cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
        cx.executor().advance_clock(SHOW_DELAY);
        draw(cx);
        shown
    }

    pub(crate) fn rendered(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("rendered {selector}"))
    }

    fn logical(bounds: Bounds<ScaledPixels>, scale: f32) -> Bounds<Pixels> {
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        Bounds::from_corners(
            point(logical(bounds.left()), logical(bounds.top())),
            point(logical(bounds.right()), logical(bounds.bottom())),
        )
    }

    /// Moves the pointer onto the dialog's backdrop, where it hovers nothing.
    fn park_pointer(cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        draw(cx);
    }

    /// The content mask `control` paints in, read from the fill it paints
    /// under the pointer: GPUI keeps a filled quad's mask whole, and that mask
    /// is the intersection of every ancestor's.
    pub(crate) fn content_mask(
        cx: &mut VisualTestContext,
        control: &'static str,
    ) -> Bounds<Pixels> {
        let surface = rendered(cx, control);
        cx.simulate_mouse_move(surface.center(), None, Modifiers::default());
        draw(cx);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = px(1. / scale);
            let masks = window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    let drawn = logical(quad.bounds, scale);
                    !quad.background.is_transparent()
                        && [
                            (drawn.left(), surface.left()),
                            (drawn.top(), surface.top()),
                            (drawn.right(), surface.right()),
                            (drawn.bottom(), surface.bottom()),
                        ]
                        .into_iter()
                        .all(|(drawn, expected)| (drawn - expected).abs() < device)
                })
                .map(|quad| logical(quad.content_mask.bounds, scale))
                .collect::<Vec<_>>();
            match masks.as_slice() {
                [mask, rest @ ..] if rest.iter().all(|other| other == mask) => *mask,
                _ => panic!("one hovered fill of {control} at {surface:?}, masked by {masks:?}"),
            }
        })
    }

    /// `mask` holds `ring`, within float error of the device pixels both lie
    /// on.
    pub(crate) fn holds(mask: Bounds<Pixels>, ring: Bounds<Pixels>) -> bool {
        let slack = px(0.01);
        mask.left() <= ring.left() + slack
            && mask.top() <= ring.top() + slack
            && mask.right() + slack >= ring.right()
            && mask.bottom() + slack >= ring.bottom()
    }

    /// Where the window painted each quad but the outlines drawn as `ring`
    /// draws its strips, transparent and edged in its width and colour, then
    /// where each of `selectors` lies. A focused Button, field or checkbox
    /// paints those strips when `ring` is installed and a ring taking no room
    /// paints none, so the layouts the ring-room checks compare both take
    /// the installed ring and leave out the same outlines: its strips and any
    /// other outline drawn alike, such as the theme editor's focused colour
    /// picker's 2 px accent edge, in both. The scene orders quads by what each
    /// overlaps, which those strips change, so the quads are listed by place
    /// and size.
    fn layout(
        cx: &mut VisualTestContext,
        selectors: &[&'static str],
        ring: FocusRing,
    ) -> (Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>) {
        let mut quads: Vec<Bounds<Pixels>> = cx.update(|window, cx| {
            let scale = window.scale_factor();
            let color = Theme::global(cx).ring.alpha(ring.opacity);
            let width = ring.width;
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    let widths = quad.border_widths;
                    !(width > px(0.)
                        && quad.background.is_transparent()
                        && quad.border_color == color
                        && [widths.top, widths.right, widths.bottom, widths.left]
                            .into_iter()
                            .all(|side| (side.as_f32() / scale - f32::from(width)).abs() < 0.01))
                })
                .map(|quad| logical(quad.bounds, scale))
                .collect()
        });
        let key = |bounds: &Bounds<Pixels>| {
            (
                bounds.left(),
                bounds.top(),
                bounds.size.width,
                bounds.size.height,
            )
        };
        quads.sort_by(|a, b| key(a).partial_cmp(&key(b)).expect("finite bounds"));
        let elements = selectors
            .iter()
            .map(|selector| rendered(cx, selector))
            .collect();
        (quads, elements)
    }

    /// The first painted quad or element of `selectors` that lies elsewhere
    /// in `after` than in `before`.
    fn first_move(
        (before_quads, before_elements): &(Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>),
        (after_quads, after_elements): &(Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>),
        selectors: &[&'static str],
    ) -> Option<String> {
        if before_quads.len() != after_quads.len() {
            return Some(format!(
                "the painted quads: {} instead of {}",
                after_quads.len(),
                before_quads.len()
            ));
        }
        let quad = before_quads
            .iter()
            .zip(after_quads)
            .enumerate()
            .find(|(_, (before, after))| before != after)
            .map(|(index, (before, after))| {
                format!("painted quad {index} from {before:?} to {after:?}")
            });
        quad.or_else(|| {
            selectors
                .iter()
                .zip(before_elements.iter().zip(after_elements))
                .find(|(_, (before, after))| before != after)
                .map(|(selector, (before, after))| {
                    format!("{selector} from {before:?} to {after:?}")
                })
        })
    }

    /// A wheel step down at `position`, which every scrolling container under
    /// it takes as far as it can move.
    fn wheel(cx: &mut VisualTestContext, position: Point<Pixels>) {
        scroll_down(cx, position, ScrollDelta::Lines(point(0., -3.)));
    }

    /// Scrolls every container under `position` down by `delta`, as far as
    /// each can move.
    pub(crate) fn scroll_down(
        cx: &mut VisualTestContext,
        position: Point<Pixels>,
        delta: ScrollDelta,
    ) {
        cx.simulate_mouse_move(position, None, Modifiers::default());
        draw(cx);
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta,
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        draw(cx);
    }

    pub(crate) fn install_ring(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        ring: FocusRing,
    ) {
        cx.update(|window, cx| {
            Theme::global_mut(cx).button_focus_ring = ring;
            app.update(cx, |_, cx| cx.notify());
            window.refresh();
        });
        draw(cx);
    }

    /// Each of `controls` lies in clipping containers. Grown by the installed
    /// Button focus ring's width plus gap, each lies inside the content mask it
    /// paints in. The room that keeps it there moves nothing: the content
    /// fits, and a wheel step over any control scrolls nothing, because the
    /// room's overhang adds nothing a container can scroll to; and laid out
    /// without the room, as a ring taking none lays out, every painted quad,
    /// control and element of `fixed` sits where it does with the room, and
    /// every control's ring is clipped.
    pub(crate) fn assert_room_for_rings(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        controls: &[&'static str],
        fixed: &[&'static str],
    ) {
        assert_rings_whole(cx, controls);
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        let selectors = [controls, fixed].concat();
        park_pointer(cx);
        let with_room = layout(cx, &selectors, installed);
        for &control in controls {
            let center = rendered(cx, control).center();
            wheel(cx, center);
            park_pointer(cx);
            let after = layout(cx, &selectors, installed);
            if let Some(moved) = first_move(&with_room, &after, &selectors) {
                panic!("a wheel step over {control} scrolls content that fits: it moves {moved}");
            }
        }
        assert_room_moves_nothing(cx, app, controls, fixed);
    }

    /// `selector`, a Button holding focus, paints its focus ring's quad at
    /// the installed ring's width and gap around its bounds.
    pub(crate) fn assert_ring_painted(
        cx: &mut VisualTestContext,
        selector: &'static str,
        at: &str,
    ) {
        let control = rendered(cx, selector);
        let ring = cx.read(|cx| Theme::global(cx).button_focus_ring);
        let around = control.dilate(ring.gap + ring.width);
        let painted = cx.update(|window, _| {
            let scale = window.scale_factor();
            window.painted_quads().into_iter().any(|quad| {
                let near = |device: gpui_kit::ScaledPixels, logical: Pixels| {
                    (device.as_f32() - logical.as_f32() * scale).abs() <= 1.
                };
                quad.border_widths.top.as_f32() > 0.
                    && !quad.border_color.is_transparent()
                    && near(quad.bounds.left(), around.left())
                    && near(quad.bounds.top(), around.top())
                    && near(quad.bounds.right(), around.right())
                    && near(quad.bounds.bottom(), around.bottom())
            })
        });
        assert!(
            painted,
            "{at}: {selector} {control:?} draws its ring {around:?}"
        );
    }

    /// Grown by the installed Button focus ring's width plus gap, each of
    /// `controls` lies inside the content mask it paints in, the intersection
    /// of every ancestor's.
    pub(crate) fn assert_rings_whole(cx: &mut VisualTestContext, controls: &[&'static str]) {
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(installed, appearance::BUTTON_FOCUS_RING);
        let footprint = installed.gap + installed.width;
        let mut clipped = Vec::new();
        for &control in controls {
            let mask = content_mask(cx, control);
            let ring = rendered(cx, control).dilate(footprint);
            if !holds(mask, ring) {
                clipped.push(format!("{control}: ring {ring:?}, mask {mask:?}"));
            }
        }
        assert!(
            clipped.is_empty(),
            "rings lie outside their content masks:\n{}",
            clipped.join("\n")
        );
    }

    /// Laid out without the ring's room, as a ring taking none lays out, every
    /// painted quad, control and element of `fixed` sits where it does with
    /// the room, and the ring around every control is clipped. The installed
    /// ring is restored afterwards.
    pub(crate) fn assert_room_moves_nothing(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        controls: &[&'static str],
        fixed: &[&'static str],
    ) {
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        let footprint = installed.gap + installed.width;
        let selectors = [controls, fixed].concat();
        park_pointer(cx);
        let with_room = layout(cx, &selectors, installed);
        install_ring(
            cx,
            app,
            FocusRing {
                width: px(0.),
                gap: px(0.),
                ..installed
            },
        );
        park_pointer(cx);
        let without_room = layout(cx, &selectors, installed);
        if let Some(moved) = first_move(&without_room, &with_room, &selectors) {
            panic!("the room moves {moved}");
        }
        for &control in controls {
            let mask = content_mask(cx, control);
            let ring = rendered(cx, control).dilate(footprint);
            assert!(
                !holds(mask, ring),
                "without room the ring around {control}, {ring:?}, is clipped to {mask:?}"
            );
        }
        install_ring(cx, app, installed);
    }

    /// Applies `interface` as the interface text size, as Settings does.
    pub(crate) fn set_interface_text_size(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        interface: u8,
    ) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        draw(cx);
        park_pointer(cx);
    }

    /// Where the list `list`, which keeps `room` around its rows, paints its
    /// scrollbar's track while it overflows: a strip as wide as the scrollbar
    /// at the right of the rows' column, its full height.
    fn scrollbar_track(
        cx: &mut VisualTestContext,
        list: &'static str,
        room: Pixels,
    ) -> Bounds<Pixels> {
        let column = rendered(cx, list).dilate(-room);
        Bounds::from_corners(
            point(column.right() - Scrollbar::width(), column.top()),
            column.bottom_right(),
        )
    }

    /// The scrollbar thumb the last frame painted in `track`: the one filled
    /// quad inside it shorter than the track, which a track's own fill is
    /// not.
    pub(crate) fn painted_thumb(
        cx: &mut VisualTestContext,
        track: Bounds<Pixels>,
    ) -> Option<Bounds<Pixels>> {
        let thumbs: Vec<_> = cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| logical(quad.bounds, scale))
                .filter(|fill| {
                    holds(track, *fill) && fill.size.height < track.size.height - px(0.5)
                })
                .collect()
        });
        assert!(thumbs.len() <= 1, "one thumb in {track:?}, not {thumbs:?}");
        thumbs.first().copied()
    }

    /// The fill of the scrollbar thumb the last frame painted in `track`
    /// ([`painted_thumb`]).
    pub(crate) fn painted_thumb_fill(
        cx: &mut VisualTestContext,
        track: Bounds<Pixels>,
    ) -> Option<gpui_kit::Background> {
        let thumbs: Vec<_> = cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .filter(|quad| {
                    let fill = logical(quad.bounds, scale);
                    holds(track, fill) && fill.size.height < track.size.height - px(0.5)
                })
                .map(|quad| quad.background)
                .collect()
        });
        assert!(thumbs.len() <= 1, "one thumb in {track:?}, not {thumbs:?}");
        thumbs.first().copied()
    }

    /// Scrolls the list `scroll` tracks to `fraction` of its reach.
    fn scroll_list_to(cx: &mut VisualTestContext, scroll: &ScrollHandle, fraction: f32) {
        let end = scroll.max_offset().y;
        scroll.set_offset(point(px(0.), -end * fraction));
        cx.update(|window, _| window.refresh());
        draw(cx);
        park_pointer(cx);
    }

    /// At 13 and 18 pt, the list `list`, which keeps `room` around its rows
    /// and whose `rows` overflow it, paints its scrollbar's thumb at the
    /// right of the rows' column, where the thumb follows the list's offset:
    /// it starts with the track at the top, lies in proportion between, and
    /// ends with the track at the end. The track overlaps no row and no row's
    /// ring, its bounds grown by the installed Button focus ring's gap plus
    /// width.
    pub(crate) fn assert_scroll_cue(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        scroll: &ScrollHandle,
        list: &'static str,
        room: Pixels,
        rows: &[&'static str],
    ) {
        let ring = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(ring, appearance::BUTTON_FOCUS_RING);
        let footprint = ring.gap + ring.width;
        for size in [13, 18] {
            set_interface_text_size(cx, app, size);
            scroll_list_to(cx, scroll, 0.);
            assert!(
                scroll.max_offset().y > px(0.),
                "at {size} pt the rows of {list} overflow it"
            );
            let track = scrollbar_track(cx, list, room);
            let covered: Vec<_> = rows
                .iter()
                .map(|row| (row, rendered(cx, row).dilate(footprint)))
                .filter(|(_, ring)| {
                    ring.intersects(&track) && ring.right() > track.left() + px(0.01)
                })
                .collect();
            assert!(
                covered.is_empty(),
                "at {size} pt the track {track:?} of {list} overlaps rings {covered:?}"
            );

            let top = painted_thumb(cx, track)
                .unwrap_or_else(|| panic!("at {size} pt {list} paints a thumb at its top"));
            // The kit insets its thumb from the track's ends.
            let inset = top.top() - track.top();
            assert!(
                inset >= px(0.) && inset < px(8.),
                "at its top the thumb {top:?} starts with the track {track:?} of {list}"
            );
            for fraction in [0.5, 1.] {
                scroll_list_to(cx, scroll, fraction);
                let thumb = painted_thumb(cx, track).unwrap_or_else(|| {
                    panic!("at {size} pt {list} paints a thumb scrolled to {fraction}")
                });
                let travel = track.size.height - thumb.size.height - inset * 2.;
                let along = (thumb.top() - track.top() - inset) / travel;
                assert!(
                    (along - fraction).abs() < 0.02,
                    "at {size} pt, scrolled to {fraction} of its reach, the thumb {thumb:?} of \
                     {list} lies {along} along its track {track:?}"
                );
            }
            let end = painted_thumb(cx, track).expect("a thumb at the end");
            assert!(
                (track.bottom() - end.bottom() - inset).abs() < px(0.5),
                "at its end the thumb {end:?} ends with the track {track:?} of {list}"
            );
            scroll_list_to(cx, scroll, 0.);
        }
        set_interface_text_size(cx, app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// At 13 and 18 pt, the list `list`, which keeps `room` around its rows
    /// and whose `rows` fit it, paints no thumb and keeps no gutter: each row
    /// lies `room` inside the list's left and right edges, as it did before
    /// lists drew a scrollbar.
    pub(crate) fn assert_no_scroll_cue(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        list: &'static str,
        room: Pixels,
        rows: &[&'static str],
    ) {
        let inset = room;
        for size in [13, 18] {
            set_interface_text_size(cx, app, size);
            let track = scrollbar_track(cx, list, room);
            assert_eq!(
                painted_thumb(cx, track),
                None,
                "at {size} pt {list} paints a thumb"
            );
            let bounds = rendered(cx, list);
            for row in rows {
                let row_bounds = rendered(cx, row);
                assert!(
                    (row_bounds.left() - bounds.left() - inset).abs() < px(0.01)
                        && (bounds.right() - row_bounds.right() - inset).abs() < px(0.01),
                    "at {size} pt {row} {row_bounds:?} lies {inset:?} inside {list} {bounds:?}"
                );
            }
        }
        set_interface_text_size(cx, app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// The gap the kit's dialog leaves between its body and its footer when
    /// nothing overrides it, measured on a dialog of its own.
    pub(crate) fn kit_dialog_footer_gap(cx: &mut TestAppContext) -> Pixels {
        /// A window that shows nothing but its dialogs.
        struct Blank;
        impl Render for Blank {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .children(Root::render_dialog_layer(window, cx))
            }
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Blank), window, cx));
        cx.update(|window, cx| {
            window.open_alert_dialog(cx, |dialog, _, _| {
                // Both are taller than the dialog's minimum height would
                // stretch its body to.
                dialog
                    .child(div().debug_selector(|| "probe-body".into()).h(px(80.)))
                    .footer(div().debug_selector(|| "probe-footer".into()).h(px(80.)))
            })
        });
        draw(cx);
        rendered(cx, "probe-footer").top() - rendered(cx, "probe-body").bottom()
    }

    /// The Tags dialog gives its room back through the footer gap, taking it
    /// to be the kit's default; any other default would move the footer.
    #[gpui::test]
    fn dialog_footer_gap_is_the_kits(cx: &mut TestAppContext) {
        assert_eq!(kit_dialog_footer_gap(cx), DIALOG_FOOTER_GAP);
    }

    /// The dialogs' own footers stand on the compact frame every other
    /// action in the app does (`button_frame`): each footer button, OK and
    /// Cancel as `dialog_footer` builds them (Add remote's), Create tag's own
    /// Cancel and Review tag, and forced worktree removal's Cancel and Force
    /// remove worktree, is as tall as the frame, 28 px at the interface size,
    /// at 13 and 18 pt. Every Cancel rests on the palette's `subtle` fill with
    /// a 1 px edge in the dialog's edge, in Midnight and Daylight alike. The kit's
    /// own Buttons stood 26 px at 13 pt and 36 px at 18 pt beside the
    /// dialogs' 28 px controls, and Cancel, once moved, rested transparent,
    /// reading as plain text beside the filled primary.
    #[gpui::test]
    async fn dialog_footers_stand_on_the_compact_frame(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep::open_dialog;
        type Open = Box<dyn Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>)>;
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let details = app.read_with(cx, |app, _| {
            let repo = app.repository.clone().expect("a repository");
            let tree = repo.worktrees().unwrap().remove(0);
            repo.worktree_details(&tree).unwrap()
        });
        let dialogs: Vec<(&str, Open, &[&'static str])> = vec![
            (
                "Tags",
                Box::new(|app, window, cx| app.open_tags(window, cx)),
                &["dialog-ok: Done"],
            ),
            (
                "Create tag",
                Box::new(|app, window, cx| app.open_create_tag(window, cx)),
                &["cancel-create-tag", "review-tag"],
            ),
            (
                "Add remote",
                Box::new(|app, window, cx| app.open_remote_form(None, window, cx)),
                &["dialog-cancel: Cancel", "dialog-ok: Review settings"],
            ),
            (
                "Force remove worktree",
                Box::new(move |app, window, cx| {
                    app.confirm_force_worktree_removal(details.clone(), window, cx)
                }),
                &[
                    "cancel-force-worktree-removal",
                    "confirm-force-worktree-removal",
                ],
            ),
            (
                "Worktrees",
                Box::new(|app, window, cx| app.open_worktree_manager(window, cx)),
                &["dialog-ok: Done"],
            ),
            (
                "the Reflog",
                Box::new(|app, window, cx| drop(app.open_reflog_browser(window, cx))),
                &["dialog-ok: Done"],
            ),
            (
                "Git profiles",
                Box::new(|app, window, cx| app.open_profiles(window, cx)),
                &["dialog-ok: Done"],
            ),
            (
                "Activity",
                Box::new(|app, window, cx| app.open_activity(window, cx)),
                &["dialog-ok: Done"],
            ),
        ];
        let cancels = [
            "cancel-create-tag",
            "dialog-cancel: Cancel",
            "cancel-force-worktree-removal",
        ];
        cx.simulate_resize(size(px(1280.), px(800.)));
        let mut failures = Vec::new();
        for theme in [
            appearance::ThemeChoice::Midnight,
            appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|window, cx| theme.apply(Some(window), cx));
            let p = cx.update(|_, cx| palette(cx));
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                let frame = appearance::ui_size(28.);
                for (name, open, buttons) in &dialogs {
                    open_dialog(&app, cx, name, |app, window, cx| open(app, window, cx)).await;
                    draw(cx);
                    let at = format!("{name} in {theme:?} at {text} pt");
                    for button in *buttons {
                        let Some(drawn) = cx.debug_bounds(button) else {
                            failures.push(format!("{at} draws no {button}"));
                            continue;
                        };
                        // Snapped to the test window's 2× device pixels.
                        if (drawn.size.height - frame).abs() > px(0.5) {
                            failures.push(format!(
                                "{at}: {button} is {:?} tall, not the frame's {frame:?}",
                                drawn.size.height
                            ));
                        }
                        if !cancels.contains(button) {
                            continue;
                        }
                        // Its own quad at rest: the fill and the edge.
                        let near = |a: f32, b: Pixels| (a - f32::from(b)).abs() < 0.5;
                        let rest = cx.update(|window, _| {
                            let scale = window.scale_factor();
                            window
                                .painted_quads()
                                .into_iter()
                                .filter(|quad| {
                                    near(quad.bounds.origin.x.as_f32() / scale, drawn.origin.x)
                                        && near(
                                            quad.bounds.origin.y.as_f32() / scale,
                                            drawn.origin.y,
                                        )
                                        && near(
                                            quad.bounds.size.height.as_f32() / scale,
                                            drawn.size.height,
                                        )
                                })
                                .collect::<Vec<_>>()
                        });
                        let fill = gpui_kit::Background::from(Hsla::from(rgb(p.subtle)));
                        let edge = Hsla::from(rgb(p.dialog_edge()));
                        // GPUI paints the fill and the edge as quads of their own.
                        let filled = rest.iter().any(|quad| quad.background == fill);
                        let edged = rest.iter().any(|quad| {
                            quad.border_widths.bottom.as_f32() > 0. && quad.border_color == edge
                        });
                        if !(filled && edged) {
                            failures.push(format!(
                                "{at}: {button} rests as {:?}, not on subtle with the dialog edge",
                                rest.iter()
                                    .map(|quad| (
                                        quad.background,
                                        quad.border_color,
                                        quad.border_widths
                                    ))
                                    .collect::<Vec<_>>()
                            ));
                        }
                    }
                    cx.update(|window, cx| window.close_all_dialogs(cx));
                    draw(cx);
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// The dialog clips its body to the body's bounds, and the tag list
    /// scrolls, so GPUI clips it to the list's bounds on both axes. Create
    /// tag…, at the top of the body, and the first and last rows keep their
    /// rings whole, and nothing moves.
    #[gpui::test]
    async fn tag_browser_keeps_room_for_every_focus_ring(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        assert_room_for_rings(
            cx,
            &app,
            &["create-tag", "tag-row-0", "tag-row-2"],
            &["tags-dialog-title", "tag-list-summary", "tag-row-1"],
        );
    }

    /// The Push to… list scrolls, so GPUI clips it to its bounds on both
    /// axes, and it ends the dialog's body, which the dialog clips. The first
    /// and last Push to… buttons keep their rings whole, and nothing moves.
    #[gpui::test]
    async fn tag_inspector_keeps_room_for_every_push_ring(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        // Local bare remotes: the inspector only lists them, and nothing is
        // fetched or pushed.
        for remote in ["origin", "upstream"] {
            let bare = fixture.path().join(format!("{remote}.git"));
            git(
                fixture.path(),
                &["init", "--quiet", "--bare", bare.to_str().unwrap()],
            );
            git(
                repo.path(),
                &["remote", "add", remote, bare.to_str().unwrap()],
            );
        }
        let tag = repo.tags().unwrap().tags[0].clone();
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        assert_room_for_rings(
            cx,
            &app,
            &["push-tag-remote-0", "push-tag-remote-1"],
            &[
                "tag-inspector-title",
                "tag-target-identity",
                "tag-push-heading",
                "tag-push-consequences",
            ],
        );
    }

    /// Opens Settings, as its shortcut does over an open dialog, and
    /// presses `control` where the last frame drew it before a frame shows
    /// Settings: the press reaches the handler the Repository page's frame
    /// gave the control, which the next frame dims.
    fn press_as_settings_open(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        control: &'static str,
    ) {
        let position = rendered(cx, control).center();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.show_settings(window, cx));
            let button = gpui::MouseButton::Left;
            let modifiers = Modifiers::default();
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button,
                    position,
                    modifiers,
                    click_count: 1,
                }),
                cx,
            );
        });
        draw(cx);
    }

    /// Off the Repository page Tags' actions are refused, as each is
    /// announced (`workspace::dialog_refusal`), and their handlers refuse
    /// with them: a tag's row pressed as Settings opens leaves Tags open and
    /// reads nothing, and Delete local tag… leaves the tag's inspector open
    /// and asks for no review. The handlers checked only the repository and
    /// the write, so the row closed Tags for a read the page then refused,
    /// and Delete closed the inspector and asked to review deleting the tag
    /// over Settings.
    #[gpui::test]
    async fn tag_actions_refuse_off_the_repository_page(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let dialog_open =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        let refused = |control: &str| crate::workspace::refusals::get(control);
        let off_page = Some(crate::workspace::refusals::Refusal {
            announced: true,
            dimmed: true,
        });

        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        press_as_settings_open(&app, cx, "tag-row-0");
        assert!(dialog_open(cx), "the row leaves Tags open");
        assert_eq!(refused("tag-row-0"), off_page, "the row is refused");
        assert!(
            app.read_with(cx, |app, _| app.tag_actions.task.is_none()
                && app.operation_busy.is_none()),
            "the row reads nothing"
        );
        cx.update(|window, cx| {
            window.close_dialog(cx);
            app.update(cx, |app, cx| app.show_history(window, cx));
        });
        draw(cx);

        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        press_as_settings_open(&app, cx, "delete-local-tag");
        assert!(
            cx.debug_bounds("operation-consequences").is_none(),
            "Delete asks for no review"
        );
        assert!(
            cx.debug_bounds("tag-inspector-title").is_some(),
            "Delete leaves the inspector open"
        );
        assert_eq!(
            refused("delete-local-tag"),
            off_page,
            "Delete local tag… is refused"
        );
    }

    /// A long Git report in a form's error line is drawn in at most
    /// [`ERROR_LINES`] lines, the last ending in an ellipsis, with its whole
    /// text as the line's accessible value and its first lines in its
    /// tooltip: at 13 pt in a 1480 × 980 window, a forty-line report under
    /// Create tag keeps three lines, Review tag whole in the window beneath
    /// it, while a one-line report, whole, shows no tooltip. Drawn whole, a
    /// long report grew the form by each of its lines, and a whole line once
    /// repeated itself in a tooltip.
    #[gpui::test]
    async fn a_long_error_line_keeps_three_lines(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        let fixture = tempfile::tempdir().unwrap();
        let (_app, form, cx) = create_lightweight_tag(cx, fixture.path());
        let report = (1..=40)
            .map(|line| format!("remote: hook line {line} declined the tag"))
            .collect::<Vec<_>>()
            .join("\n");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                set_error_line(&mut form.error, report.clone(), window, cx);
                cx.notify();
            })
        });
        written::forget();
        draw(cx);
        let (line, size) = cx.update(|window, _| {
            (
                crate::image_compare::text_line(crate::appearance::ui_text(12.), window),
                window.viewport_size(),
            )
        });
        let drawn = rendered(cx, "tag-preparation-error");
        assert!(
            drawn.size.height <= line * ERROR_LINES as f32 + px(0.5),
            "{drawn:?} keeps {ERROR_LINES} lines of {line:?}"
        );
        let review = rendered(cx, "review-tag");
        assert!(
            review.top() >= drawn.bottom() && review.bottom() <= size.height,
            "Review tag {review:?} lies whole in the window, beneath the line {drawn:?}"
        );
        let value = written::get("tag-preparation-error")
            .and_then(|line| line.node.value().map(str::to_owned));
        assert_eq!(value.as_deref(), Some(report.as_str()), "its whole text");
        assert_eq!(
            hovered_tooltips(cx, "tag-preparation-error"),
            1,
            "its tooltip"
        );
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                set_error_line(&mut form.error, "Tag v9.0 already exists", window, cx);
                cx.notify();
            })
        });
        draw(cx);
        assert_eq!(
            hovered_tooltips(cx, "tag-preparation-error"),
            0,
            "a whole line has no tooltip"
        );
    }

    /// Whether an error line is cut is decided as each frame lays it out,
    /// and a hover shows what the frame it rests on decided: at 13 pt, a
    /// three-line report whole under Create tag in a 1480 × 980 window shows
    /// no tooltip; resized in one step to 461 × 980, below the dialog's
    /// 560 px, where its lines wrap past three, a hover shows its tooltip;
    /// and resized back in one step, a hover shows none. Each resize draws
    /// one frame, as a single configure from the compositor does, and
    /// nothing draws another before the pointer moves onto the line, so the
    /// hover rests on the resize's own frame. Decided while each frame was
    /// built, from the frame before, the tooltip answered for the old width
    /// until something else drew the form again, which a Hyprland re-tile
    /// alone does not.
    #[gpui::test]
    async fn a_one_step_resize_answers_the_error_lines_next_hover(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, form, cx) = create_lightweight_tag(cx, fixture.path());
        let report = (1..=3)
            .map(|line| {
                format!("remote: hook line {line} refused the tag for its annotation policy")
            })
            .collect::<Vec<_>>()
            .join("\n");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                set_error_line(&mut form.error, report.clone(), window, cx);
                cx.notify();
            })
        });
        draw(cx);
        // The frames the window has drawn.
        let frames = |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.draws.len());
        // The tooltips a hover on the line shows once its delay has passed;
        // the pointer then leaves.
        let hovered = |cx: &mut VisualTestContext| {
            let before = painted_popups(cx);
            let line = rendered(cx, "tag-preparation-error");
            cx.simulate_mouse_move(line.center(), None, Modifiers::default());
            cx.executor().advance_clock(TOOLTIP_DELAY);
            cx.run_until_parked();
            let shown = painted_popups(cx)
                .into_iter()
                .filter(|popup| !before.contains(popup))
                .count();
            cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
            cx.executor().advance_clock(TOOLTIP_DELAY);
            cx.run_until_parked();
            shown
        };
        let lines = |cx: &mut VisualTestContext| {
            let width = rendered(cx, "tag-preparation-error").size.width;
            cx.update(|window, _| {
                crate::image_compare::wrapped_lines(
                    &report,
                    crate::appearance::ui_text(12.),
                    width,
                    window,
                )
            })
        };
        // Resized in one step, in one frame.
        let resize = |cx: &mut VisualTestContext, width: f32| {
            let before = frames(cx);
            cx.simulate_resize(size(px(width), px(980.)));
            assert_eq!(
                frames(cx) - before,
                1,
                "the resize to {width} px draws one frame"
            );
        };
        assert_eq!(lines(cx), ERROR_LINES, "whole in the wide window");
        assert_eq!(hovered(cx), 0, "whole, no tooltip");
        resize(cx, 461.);
        assert!(lines(cx) > ERROR_LINES, "cut in the narrow window");
        assert_eq!(
            hovered(cx),
            1,
            "cut in one step, the next hover shows its tooltip"
        );
        resize(cx, 1480.);
        assert_eq!(lines(cx), ERROR_LINES, "whole again");
        assert_eq!(
            hovered(cx),
            0,
            "whole in one step, the next hover shows none"
        );
    }

    /// An error line measures a long report in bounded work through a live
    /// resize: at 13 pt under Create tag, a 16 KiB report in one line, and
    /// one of 550 lines, each stepped one width at a time from 600 to 461
    /// px, below the dialog's 560 px, where each step lays the line out at
    /// a new width. The one-line report lays out at most one line past
    /// [`ERROR_LINES`] for each width, the 550-line one is cut at any width
    /// and shaped at none, and each shows its tooltip at the end. Measured
    /// whole, each step shaped and wrapped all of a report, about 600 lines
    /// of the first in a 400 px line.
    #[gpui::test]
    async fn a_long_report_is_measured_in_bounded_work_through_a_live_resize(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let (_app, form, cx) = create_lightweight_tag(cx, fixture.path());
        let one_line = "remote: declined ".repeat(16 * 1024 / 17);
        let many_lines = (1..=550)
            .map(|line| format!("remote: hook line {line} declined"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(one_line.len() > 16_000 && many_lines.len() > 16_000);
        const WIDTHS: [f32; 7] = [600., 580., 560., 540., 520., 500., 461.];
        for (report, most) in [(&one_line, ERROR_LINES + 1), (&many_lines, 0)] {
            cx.simulate_resize(size(px(1480.), px(980.)));
            cx.update(|window, cx| {
                form.update(cx, |form, cx| {
                    set_error_line(&mut form.error, report.clone(), window, cx);
                    cx.notify();
                })
            });
            draw(cx);
            REPORT_LINES_LAID.set((0, 0));
            for width in WIDTHS {
                cx.simulate_resize(size(px(width), px(980.)));
            }
            let (measured, laid) = REPORT_LINES_LAID.get();
            let lines = report.lines().count();
            assert!(
                laid <= measured * most,
                "{lines} lines: {laid} lines laid out for {measured} widths"
            );
            assert!(
                most == 0 || measured >= WIDTHS.len() - 1,
                "{lines} lines: measured at {measured} of the {} widths",
                WIDTHS.len()
            );
            assert_eq!(
                hovered_tooltips(cx, "tag-preparation-error"),
                1,
                "{lines} lines: cut, its tooltip"
            );
        }
    }

    /// An error line without a size of its own, as the profile form's, is
    /// drawn at the size a dialog's text inherits and prepaints a steady
    /// frame without allocating: under a dialog in a 1480 × 980 window, at
    /// 13 and 18 pt, a one-line report is as tall as a line of the dialog's
    /// own text beside it, and a frame drawn again with nothing changed
    /// prepaints the line once with no allocation. Read from the window's
    /// text style on each prepaint, its size built GPUI's default style,
    /// whose font features are an allocation, on every frame however steady.
    #[gpui::test]
    async fn a_line_without_a_size_prepaints_a_steady_frame_without_allocating(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| {
            window.open_alert_dialog(cx, |dialog, window, cx| {
                dialog
                    .child(
                        div()
                            .debug_selector(|| "probe-dialog-text".into())
                            .child("Profile name"),
                    )
                    .child(error_line(
                        "probe-error-line",
                        "Saving the profile failed",
                        None,
                        window,
                        cx,
                    ))
            })
        });
        for interface in [13, 18] {
            set_interface_text_size(cx, &app, interface);
            draw(cx);
            let inherited = rendered(cx, "probe-dialog-text").size.height;
            let drawn = rendered(cx, "probe-error-line").size.height;
            assert_eq!(
                drawn, inherited,
                "at {interface} pt the line is one line of the dialog's text"
            );
            PREPAINT_ALLOCATIONS.set((0, 0));
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
            assert_eq!(
                PREPAINT_ALLOCATIONS.get(),
                (1, 0),
                "at {interface} pt a steady frame prepaints the line once, allocating nothing"
            );
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// An error line keeps its state key and its text for its id
    /// ([`kept_line`]): under Create tag, frames at rest find both without
    /// an allocation, for a one-line report and for a report past 16 KiB,
    /// which the line carries cut; a new report is copied once, as the next
    /// frame draws it, and the key, formatted as the line is first drawn, is
    /// kept through every report. The line's accessible value is each report
    /// as it is carried. Every frame formatted the line's key and copied its
    /// report.
    #[gpui::test]
    async fn an_error_line_builds_its_key_and_text_only_on_a_change(cx: &mut TestAppContext) {
        use crate::native_accessibility::{bounded_accessible_text, written};
        // Empty, as a GPUI test's seeds share the thread and its cache.
        forget_kept_lines();
        let fixture = tempfile::tempdir().unwrap();
        let (_app, form, cx) = create_lightweight_tag(cx, fixture.path());
        // The allocations finding the line's key and text took while `report`
        // was set and drawn, and the value the line was written with.
        let drawn = |cx: &mut VisualTestContext, report: &str| {
            KEPT_LINE_TEXT.set((0, 0));
            written::forget();
            cx.update(|window, cx| {
                form.update(cx, |form, cx| {
                    set_error_line(&mut form.error, report, window, cx);
                    cx.notify();
                })
            });
            draw(cx);
            let (lines, allocations) = KEPT_LINE_TEXT.get();
            assert!(lines > 0, "the form draws its error line");
            let value = written::get("tag-preparation-error")
                .and_then(|line| line.node.value().map(str::to_owned));
            (allocations, value)
        };
        let at_rest = |at: &str, cx: &mut VisualTestContext| {
            KEPT_LINE_TEXT.set((0, 0));
            for _ in 0..3 {
                form.update(cx, |_, cx| cx.notify());
                draw(cx);
            }
            let (lines, allocations) = KEPT_LINE_TEXT.get();
            assert!(lines >= 3, "{at}: each frame draws the line");
            assert_eq!(
                allocations, 0,
                "{at}: frames at rest build neither the line's key nor its text"
            );
        };
        // Longer than the 23 bytes a shared string holds in place, so its
        // copy is an allocation.
        const EXISTS: &str = "Tag v9.0 already exists in this repository";
        let (first, value) = drawn(cx, EXISTS);
        assert!(
            first > 1,
            "the first frame formats the key and copies the text"
        );
        assert_eq!(value.as_deref(), Some(EXISTS));
        at_rest("a one-line report", cx);

        let long = "remote: hook declined the tag\n".repeat(1_000);
        let carried = bounded_accessible_text(long.as_str());
        assert!(carried.len() < long.len(), "the report is carried cut");
        let (copied, value) = drawn(cx, &long);
        assert!(copied > 0, "a new report is copied");
        assert_eq!(value.as_deref(), Some(&*carried), "carried cut");
        at_rest("a report past 16 KiB", cx);

        let (copied, value) = drawn(cx, EXISTS);
        assert_eq!(
            copied, 1,
            "the one-line report is copied once, into its shared text, and the key is kept"
        );
        assert_eq!(value.as_deref(), Some(EXISTS));
        at_rest("the one-line report again", cx);
    }

    /// A hover's tooltip answers for the frame drawn last, its width, text
    /// size and report together, whatever changed during the hover's delay:
    /// in a 1480 × 980 window, a three-line report whole under Create tag at
    /// 13 pt, the pointer rests on it, and before the delay ends the
    /// interface text grows to 18 pt, where the report needs six lines: its
    /// tooltip shows. Back at 13 pt, a one-line report rests under the
    /// pointer, and before the delay ends a forty-line report replaces it:
    /// its tooltip shows. Decided as the tooltip was asked for, the latest
    /// width was measured at the hovered frame's size and with its report,
    /// and neither tooltip showed.
    #[gpui::test]
    async fn a_change_during_the_hover_delay_is_answered_by_the_frame_drawn_last(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let (_app, form, cx) = create_lightweight_tag(cx, fixture.path());
        let set = |cx: &mut VisualTestContext, report: &str| {
            cx.update(|window, cx| {
                form.update(cx, |form, cx| {
                    set_error_line(&mut form.error, report, window, cx);
                    cx.notify();
                })
            });
            draw(cx);
        };
        let lines = |cx: &mut VisualTestContext, report: &str| {
            let width = rendered(cx, "tag-preparation-error").size.width;
            cx.update(|window, _| {
                crate::image_compare::wrapped_lines(
                    report,
                    crate::appearance::ui_text(12.),
                    width,
                    window,
                )
            })
        };
        // The tooltips a hover shows once its delay has passed, `change`
        // made as the pointer has come to rest on the line: the popups then,
        // past those the window paints once the pointer has left.
        let hovered = |cx: &mut VisualTestContext, change: &dyn Fn(&mut VisualTestContext)| {
            let line = rendered(cx, "tag-preparation-error");
            cx.simulate_mouse_move(line.center(), None, Modifiers::default());
            change(cx);
            cx.executor().advance_clock(TOOLTIP_DELAY);
            cx.run_until_parked();
            let shown = painted_popups(cx).len();
            cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
            cx.executor().advance_clock(TOOLTIP_DELAY);
            cx.run_until_parked();
            shown as isize - painted_popups(cx).len() as isize
        };
        let text = |cx: &mut VisualTestContext, size: u8| {
            cx.update(|window, cx| crate::appearance::apply_text_sizes(size, 12, window, cx));
            draw(cx);
        };

        let three = (1..=3)
            .map(|line| format!("remote: line {line} {}", "x".repeat(50)))
            .collect::<Vec<_>>()
            .join("\n");
        set(cx, &three);
        assert_eq!(lines(cx, &three), ERROR_LINES, "whole at 13 pt");
        assert_eq!(hovered(cx, &|_| ()), 0, "whole at 13 pt, no tooltip");
        text(cx, 18);
        assert!(lines(cx, &three) > ERROR_LINES, "cut at 18 pt");
        text(cx, 13);
        assert_eq!(
            hovered(cx, &|cx| text(cx, 18)),
            1,
            "grown to 18 pt during the delay, the hover shows its tooltip"
        );

        text(cx, 13);
        let forty = (1..=40)
            .map(|line| format!("remote: hook line {line} declined the tag"))
            .collect::<Vec<_>>()
            .join("\n");
        set(cx, "Tag v9.0 already exists");
        assert_eq!(hovered(cx, &|_| ()), 0, "a one-line report, no tooltip");
        assert_eq!(
            hovered(cx, &|cx| set(cx, &forty)),
            1,
            "replaced by a cut report during the delay, the hover shows its tooltip"
        );
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A tooltip shown while the pointer rests on an error line stays as it
    /// was shown through a resize, as macOS keeps a shown tooltip until the
    /// pointer moves off, and the next hover answers for the new cut: at
    /// 13 pt, a three-line report whole under Create tag in a 560 × 980
    /// window, the pointer rests on it past the delay, and nothing shows;
    /// resized in one step to 540 × 980, where the report needs six lines
    /// and the line still lies under the pointer, nothing shows however
    /// long the pointer rests there; moved off and back, its tooltip shows.
    /// Resized back as the pointer rests on it, its tooltip stays; moved off
    /// and back, none shows.
    #[gpui::test]
    async fn a_tooltip_shown_through_a_resize_stays_until_the_pointer_moves_off(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let (_app, form, cx) = create_lightweight_tag(cx, fixture.path());
        let report = (1..=3)
            .map(|line| format!("remote: line {line} {}", "x".repeat(50)))
            .collect::<Vec<_>>()
            .join("\n");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                set_error_line(&mut form.error, report.clone(), window, cx);
                cx.notify();
            })
        });
        // Where the pointer rests through a resize. A resize reads the
        // pointer's place from the platform, which the test platform puts
        // at the window's corner, so before the resize's frame draws, the
        // pointer is put back where it rests, as the desktop reports it.
        let rests = std::rc::Rc::new(std::cell::Cell::new(None::<Point<Pixels>>));
        let _kept = cx.update(|window, cx| {
            let rests = rests.clone();
            let watcher = cx.new(|_| ());
            let observing = watcher.update(cx, |_, cx| {
                cx.observe_window_bounds(window, move |_, window, cx| {
                    if let Some(position) = rests.get() {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position,
                                modifiers: Modifiers::default(),
                                pressed_button: None,
                            }),
                            cx,
                        );
                    }
                })
            });
            (watcher, observing)
        });
        let resize = |cx: &mut VisualTestContext, width: f32| {
            cx.simulate_resize(size(px(width), px(980.)));
            rendered(cx, "tag-preparation-error")
        };
        let cut = |cx: &mut VisualTestContext, line: Bounds<Pixels>| {
            cx.update(|window, _| {
                crate::image_compare::wrapped_lines(
                    &report,
                    crate::appearance::ui_text(12.),
                    line.size.width,
                    window,
                ) > ERROR_LINES
            })
        };
        // The popups the window paints as the pointer rests where it is,
        // past the delay, in frames drawn then.
        let resting = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(TOOLTIP_DELAY * 2);
            cx.run_until_parked();
            draw(cx);
            painted_popups(cx).len()
        };
        let wide = resize(cx, 560.);
        draw(cx);
        let narrow = resize(cx, 540.);
        draw(cx);
        assert!(
            !cut(cx, wide) && cut(cx, narrow),
            "{wide:?} whole, {narrow:?} cut"
        );
        // A point on the line in both layouts.
        let shared = wide.intersect(&narrow);
        let pointer = point(shared.left() + px(40.), shared.center().y);
        assert!(
            shared.contains(&pointer),
            "{wide:?} and {narrow:?} share {pointer:?}"
        );
        let away = point(px(-1.), px(-1.));
        let rest = |cx: &mut VisualTestContext, at: Point<Pixels>| {
            rests.set((at == pointer).then_some(at));
            cx.simulate_mouse_move(at, None, Modifiers::default());
        };

        resize(cx, 560.);
        draw(cx);
        let none = painted_popups(cx).len();
        rest(cx, pointer);
        assert_eq!(resting(cx), none, "whole, nothing shows");
        resize(cx, 540.);
        assert_eq!(
            resting(cx),
            none,
            "cut under the resting pointer, nothing shows however long it rests"
        );
        rest(cx, away);
        resting(cx);
        rest(cx, pointer);
        assert_eq!(
            resting(cx),
            none + 1,
            "moved off and back, its tooltip shows"
        );
        resize(cx, 560.);
        assert_eq!(
            resting(cx),
            none + 1,
            "whole under the resting pointer, its tooltip stays"
        );
        rest(cx, away);
        assert_eq!(resting(cx), none, "moved off, it goes");
        rest(cx, pointer);
        assert_eq!(resting(cx), none, "and back, none shows");
    }

    /// A cut error line's tooltip shows the report's first twelve lines,
    /// within 1 KiB, and an ellipsis where more follows; a short report
    /// whole. The kit's tooltip has no height bound, so a whole report of
    /// many lines would run past the window.
    #[test]
    fn an_error_tooltip_shows_the_reports_first_lines() {
        let report = (1..=40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let shown = error_tooltip(&report);
        assert_eq!(shown.lines().count(), ERROR_TOOLTIP_LINES);
        assert!(
            shown.starts_with("line 1\n") && shown.ends_with("line 12…"),
            "{shown:?}"
        );
        let long = "x".repeat(4 * ERROR_TOOLTIP_BYTES);
        let shown = error_tooltip(&long);
        assert_eq!(shown.len(), ERROR_TOOLTIP_BYTES + '…'.len_utf8());
        assert_eq!(
            error_tooltip("Tag v9.0 already exists"),
            "Tag v9.0 already exists"
        );
    }

    /// Create tag's failure is said once, at once, from the window's alert,
    /// in the frame that draws its line in place: a name already taken
    /// fails the tag's read, and the frame that draws the form's error line
    /// draws the alert saying it. Frames that draw the line again say
    /// nothing more, and the same press refused again is said again, from a
    /// fresh node. The form's line was no live region, and nothing was said.
    #[gpui::test]
    async fn create_tags_failure_is_said_as_its_line_appears(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::alerted;
        let fixture = tempfile::tempdir().unwrap();
        let (app, form, cx) = create_lightweight_tag(cx, fixture.path());
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.name
                    .update(cx, |name, cx| name.set_value("v0.1", window, cx))
            })
        });
        draw(cx);
        assert_eq!(alerted(cx), None, "nothing is said before a press");
        let press = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| form.update(cx, |form, cx| form.submit(window, cx)));
        };
        press(cx);
        read_lands(&app, cx).await;
        draw(cx);
        let error = form
            .read_with(cx, |form, _| form.error.clone())
            .expect("a taken name fails the read");
        assert!(
            cx.debug_bounds("tag-preparation-error").is_some(),
            "the line stands in place"
        );
        let (first, said) = alerted(cx).expect("the frame that draws the line says it");
        assert_eq!(said, error);
        draw(cx);
        draw(cx);
        assert_eq!(
            alerted(cx).map(|(id, _)| id),
            Some(first.clone()),
            "drawing the line again says nothing more"
        );
        press(cx);
        read_lands(&app, cx).await;
        draw(cx);
        let (second, said) = alerted(cx).expect("refused again, it is said again");
        assert!(
            second != first && said == error,
            "a fresh node says the same failure again"
        );
    }

    /// Create tag opened over a tagged repository in a 1480 × 980 window,
    /// naming a lightweight tag, which needs no annotation: the window and
    /// its form.
    fn create_lightweight_tag<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
    ) -> (
        Entity<GitTurtle>,
        Entity<TagForm>,
        &'a mut VisualTestContext,
    ) {
        let repo = tagged_repository(fixture);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_create_tag(window, cx)));
        draw(cx);
        let form = app
            .read_with(cx, |app, _| app.tag_actions.draft.clone())
            .expect("Create tag opens");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.annotated = false;
                form.name
                    .update(cx, |name, cx| name.set_value("v9.0", window, cx))
            })
        });
        draw(cx);
        (app, form, cx)
    }

    /// Review tag's accessibility node as the window would write it now,
    /// built inside a draw as `views::tests::input_node` builds a field's,
    /// and the tooltip the next frame builds it with, which no node carries.
    fn review_tag_frame(
        form: &Entity<TagForm>,
        cx: &mut VisualTestContext,
    ) -> (gpui::accesskit::Node, Option<&'static str>) {
        let form = form.clone();
        let node = crate::views::tests::view_node(cx, move |window, cx| {
            use gpui::Element as _;
            let button = review_tag(form.clone(), cx).base(window, cx);
            let element = RenderOnce::render(button, window, cx).into_element();
            let mut node =
                gpui::accesskit::Node::new(element.a11y_role().expect("Review tag's role"));
            element.write_a11y_info(&mut node);
            node
        });
        // What the window's frame records, as the node's build did too.
        crate::workspace::refusals::forget();
        REVIEW_TAG.with(|review| review.set(None));
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        (node, REVIEW_TAG.with(|review| review.get()))
    }

    /// Create tag's Review tag follows a write as Tags' own actions do. A
    /// write that ends within the busy look's delay leaves it looking
    /// available in every frame, announced unavailable, by a name that says
    /// why, only while the write runs. A slower write refuses a press at
    /// once, before anything changes, and dims it, its tooltip saying why,
    /// once the write has run 200 ms; when the write ends it is available
    /// again and Enter opens the tag's review. Its name is read from the
    /// node it writes. It belonged to the kit's
    /// footer, which drew it available through any write, and a press set
    /// the form's error. Enter, after each press, changes nothing either.
    #[gpui::test]
    async fn review_tag_waits_for_the_busy_look(cx: &mut TestAppContext) {
        use crate::workspace::BUSY_REASON;
        use crate::workspace::refusals::{self, Refusal};
        let fixture = tempfile::tempdir().unwrap();
        let (app, form, cx) = create_lightweight_tag(cx, fixture.path());
        // What the next frame built Review tag with: its announced and
        // drawn availability, whether its node is disabled and named by
        // why, and its tooltip.
        let frame = |cx: &mut VisualTestContext| {
            let (node, tooltip) = review_tag_frame(&form, cx);
            (
                refusals::get("review-tag"),
                node.role(),
                node.is_disabled(),
                node.label() == Some(review_tag_busy_name(BUSY_REASON).as_str()),
                tooltip,
            )
        };
        let state = |announced, dimmed, busy, tooltip| {
            (
                Some(Refusal { announced, dimmed }),
                gpui::accesskit::Role::Button,
                announced,
                busy,
                tooltip,
            )
        };
        let at_rest = state(false, false, false, None);
        let busy = true;
        let start = |cx: &mut VisualTestContext| {
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.write(WriteCommand::StageAll, "Working…", window, cx)
                })
            });
            held
        };
        async fn release(
            app: &Entity<GitTurtle>,
            held: crate::operations::Held,
            cx: &mut VisualTestContext,
        ) {
            drop(held);
            let write = app
                .update(cx, |app, _| app.operation_task.take())
                .expect("the write");
            write.await;
            crate::views::tests::settle(app, cx).await;
        }
        let untouched = |cx: &mut VisualTestContext, at: &str| {
            assert!(
                cx.update(|window, cx| window.has_active_dialog(cx))
                    && cx.debug_bounds("create-tag-dialog-title").is_some(),
                "{at}: Create tag stays open"
            );
            let (error, pending) =
                form.read_with(cx, |form, cx| (form.error.clone(), form.pending(cx)));
            assert_eq!(
                (error, pending),
                (None, false),
                "{at}: the form is unchanged"
            );
            assert!(
                app.read_with(cx, |app, _| app.tag_actions.task.is_none()),
                "{at}: nothing is read"
            );
        };
        assert_eq!(frame(cx), at_rest, "at rest");

        // A quick write: announced unavailable while it runs, never dimmed.
        let held = start(cx);
        assert_eq!(frame(cx), state(true, false, busy, None), "a quick write");
        release(&app, held, cx).await;
        assert_eq!(frame(cx), at_rest, "after a quick write");

        // A slow write: a press is refused at once and changes nothing.
        let held = start(cx);
        assert_eq!(frame(cx), state(true, false, busy, None), "a write starts");
        let review = rendered(cx, "review-tag");
        cx.simulate_click(review.center(), Modifiers::default());
        frame(cx);
        untouched(cx, "pressed as the write starts");
        // Each Enter a press of its own, released before the next: one
        // that never comes up holds the key down, and its next key-down
        // is a repeat, which presses nothing.
        let enter_up = |cx: &mut VisualTestContext| {
            cx.simulate_event(gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse("enter").unwrap(),
            })
        };
        cx.simulate_keystrokes("enter");
        enter_up(cx);
        frame(cx);
        untouched(cx, "Enter as the write starts");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        cx.executor().run_until_parked();
        assert_eq!(
            frame(cx),
            state(true, true, busy, Some(BUSY_REASON)),
            "once the write looks busy"
        );
        cx.simulate_click(review.center(), Modifiers::default());
        frame(cx);
        untouched(cx, "pressed once the write looks busy");
        cx.simulate_keystrokes("enter");
        enter_up(cx);
        frame(cx);
        untouched(cx, "Enter once the write looks busy");
        release(&app, held, cx).await;
        assert_eq!(frame(cx), at_rest, "after the write");

        // Available again: Enter reads the tag's plan and opens its review.
        cx.simulate_keystrokes("enter");
        read_lands(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some(),
            "Review tag opens the tag's review once the write has ended"
        );
    }

    /// Create tag's Review tag says why it waits in the window's busy
    /// spell's own words (`GitTurtle::busy_reason`), in its name at once and
    /// its tooltip once it dims: while a request of the GitHub panel runs,
    /// that request's reason. It named "the current Git operation" whatever
    /// ran. No route opens Create tag while a request runs, nor the panel
    /// over Create tag, so the request is submitted here as the panel
    /// submits one; the reason has one source either way.
    #[gpui::test]
    async fn review_tag_gives_a_github_requests_reason(cx: &mut TestAppContext) {
        use crate::github_view::{RequestKind, submit_test_request};
        let fixture = tempfile::tempdir().unwrap();
        let (app, form, cx) = create_lightweight_tag(cx, fixture.path());
        for kind in RequestKind::ALL {
            // Held as a slow network holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| submit_test_request(app, kind, window, cx))
            });
            let (node, tooltip) = review_tag_frame(&form, cx);
            let named = review_tag_busy_name(kind.reason());
            assert_eq!(
                (node.label(), tooltip),
                (Some(named.as_str()), None),
                "{kind:?}: its name says why at once"
            );
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            cx.executor().run_until_parked();
            let (node, tooltip) = review_tag_frame(&form, cx);
            assert_eq!(
                (node.label(), tooltip),
                (Some(named.as_str()), Some(kind.reason())),
                "{kind:?}: and its tooltip once it dims"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
            assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        }
    }

    /// A plan read Settings cancels as it opens (`end_preparations`) ends
    /// the form's preparation with it: back on the repository, Review tag is
    /// announced and drawn available, and a press reads the plan again and
    /// opens the tag's review. The form kept a flag only the dropped reply
    /// cleared, so Review tag looked available and refused every press.
    #[gpui::test]
    async fn a_plan_read_dropped_off_the_page_leaves_review_tag_working(cx: &mut TestAppContext) {
        use crate::workspace::refusals::{self, Refusal};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_create_tag(window, cx)));
        draw(cx);
        let form = app
            .read_with(cx, |app, _| app.tag_actions.draft.clone())
            .expect("Create tag opens");
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.annotated = false;
                form.name
                    .update(cx, |name, cx| name.set_value("v9.0", window, cx))
            })
        });
        draw(cx);
        let review = rendered(cx, "review-tag");
        // The read waits behind a held executor until Settings shows.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.simulate_click(review.center(), Modifiers::default());
        refusals::forget();
        draw(cx);
        assert!(
            form.read_with(cx, |form, cx| form.pending(cx)),
            "the plan is read"
        );
        assert_eq!(
            refusals::get("review-tag").map(|refusal| refusal.announced),
            Some(true),
            "Review tag is refused while its read runs"
        );
        // Settings opens over the dialog before the read lands, and
        // cancels it.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        assert!(
            app.read_with(cx, |app, _| app.tag_actions.task.is_none()),
            "Settings cancels the plan read"
        );
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        draw(cx);
        assert!(
            !form.read_with(cx, |form, cx| form.pending(cx)),
            "the dropped read leaves the form preparing"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        refusals::forget();
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Create tag stays open"
        );
        assert_eq!(
            refusals::get("review-tag"),
            Some(Refusal {
                announced: false,
                dimmed: false
            }),
            "Review tag is available on the repository again"
        );
        cx.simulate_click(review.center(), Modifiers::default());
        read_lands(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some(),
            "Review tag opens the tag's review"
        );
    }

    /// Cancel closes Create tag wherever focus is: with Settings shown over
    /// the open dialog, which takes focus, Cancel asked the focused element
    /// to cancel, which missed the dialog, so it stayed open.
    #[gpui::test]
    async fn cancel_closes_create_tag_with_settings_shown(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_create_tag(window, cx)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        let open =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        assert!(open(cx), "Create tag stays open over Settings");
        let cancel = rendered(cx, "cancel-create-tag");
        cx.simulate_click(cancel.center(), Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        draw(cx);
        assert!(!open(cx), "Cancel closes Create tag");
    }

    /// The application in a 1000 × 680 window on a repository with three
    /// tagged commits and `tags` more tags on the last.
    fn small_window_with_tags<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
        tags: usize,
    ) -> (GitRepository, Entity<GitTurtle>, &'a mut VisualTestContext) {
        let repo = tagged_repository(fixture);
        for index in 0..tags {
            git(repo.path(), &["tag", &format!("v1.{index:02}")]);
        }
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        (repo, app, cx)
    }

    /// The summary's count agrees with its noun.
    #[test]
    fn the_summary_counts_one_tag_in_the_singular() {
        assert!(summary(1).starts_with("1 local tag · "), "{}", summary(1));
        assert!(summary(3).starts_with("3 local tags · "), "{}", summary(3));
    }

    /// Tags' text starts at one inset, where its filter's text and
    /// placeholder start, at 13 and 18 pt in a 1480 × 980 window and a
    /// 461 × 490 half tile: its title, its summary and each row's name. A
    /// row's name was centred in its row while the summary started at the
    /// dialog's edge. The placeholder fits its field in whole words: at 18
    /// pt in the half tile "Filter tags by name or object ID" was cut
    /// mid-glyph.
    #[gpui::test]
    async fn tags_text_starts_at_the_filters_inset(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (_repo, app, cx) = small_window_with_tags(cx, fixture.path(), 0);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let query = app
            .read_with(cx, |app, _| app.tag_actions.filter.clone())
            .expect("Tags' filter");
        for (width, height) in [(1480., 980.), (461., 490.)] {
            for size in [13, 18] {
                cx.simulate_resize(size_of(width, height));
                set_interface_text_size(cx, &app, size);
                let at = format!("{size} pt in {width} × {height}");
                let filter = cx.read(|cx| query.read(cx).input_bounds().left());
                for selector in ["tags-dialog-title", "tag-list-summary", "tag-row-text-0"] {
                    let start = rendered(cx, selector).left();
                    assert!(
                        (start - filter).abs() < px(0.01),
                        "{at}: {selector} starts at {start:?}, the filter's text at {filter:?}"
                    );
                }
                let row = rendered(cx, "tag-row-text-0");
                assert!(
                    row.size.width > px(100.),
                    "{at}: the row's name {row:?} fills its row from the inset"
                );
                let (shown, room) = cx.read(|cx| {
                    let query = query.read(cx);
                    (
                        query.presentation().placeholder().clone(),
                        query.text_bounds().map(|bounds| bounds.size.width),
                    )
                });
                let expected = if width < 1000. && size == 18 {
                    "Filter tags…"
                } else {
                    FILTER_PLACEHOLDERS[0]
                };
                assert_eq!(shown.as_ref(), expected, "{at}: in a field {room:?} wide");
            }
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    fn size_of(width: f32, height: f32) -> Size<Pixels> {
        size(px(width), px(height))
    }

    /// Tab moves through every row of Tags and Shift+Tab back, and the list
    /// scrolls each row into view with its focus ring. Once a row is shown,
    /// redraws and a click on a partly visible row scroll nothing.
    #[gpui::test]
    async fn tab_reveals_every_tag_row(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (repo, app, cx) = small_window_with_tags(cx, fixture.path(), 27);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");

        let rows = tag_rows(&repo);
        assert_eq!(rows.len(), 30);
        focus_filter(cx, &list, &rows[0]);
        assert_tab_reveals(cx, &list, "tags-list", &rows);
        // A click inspects nothing while an operation runs.
        app.update(cx, |app, _| app.operation_busy = Some("Testing"));
        assert_steady(cx, &list, "tags-list", &rows);
    }

    /// Tab and Shift+Tab through Tags' 30 rows: every frame painted after a
    /// key, the first one that draws the new focus included, shows the
    /// focused row with its whole ring inside the list.
    #[gpui::test]
    async fn tags_reveal_rows_in_the_frame_that_draws_focus(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (repo, app, cx) = small_window_with_tags(cx, fixture.path(), 27);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");

        let rows = tag_rows(&repo);
        assert_eq!(rows.len(), 30);
        focus_filter(cx, &list, &rows[0]);
        assert_every_frame_reveals(cx, &list, "tags-list", &rows);
    }

    /// Tags' rows as `control` names a list's controls, in the order the
    /// dialog lists them: each tracks focus under its tag's ID and is drawn
    /// under its place.
    fn tag_rows(repo: &GitRepository) -> Vec<crate::focus_reveal::tests::Control> {
        repo.tags()
            .expect("the tags")
            .tags
            .iter()
            .enumerate()
            .map(|(index, tag)| (tag_row_id(tag), &*format!("tag-row-{index}").leak()))
            .collect()
    }

    /// Whether the accent focus ring the last frame painted lies around the
    /// control drawn as `selector`.
    fn ring_on(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        let Some(control) = cx.debug_bounds(selector) else {
            return false;
        };
        let ring =
            control.dilate(appearance::BUTTON_FOCUS_RING.gap + appearance::BUTTON_FOCUS_RING.width);
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        crate::focus_ring::painted_outlines_where(cx, |color| {
            color.a > 0.
                && Hsla {
                    a: accent.a,
                    ..color
                } == accent
        })
        .into_iter()
        .any(|outline| {
            outline.drawn
                && [
                    (outline.bounds.left(), ring.left()),
                    (outline.bounds.top(), ring.top()),
                    (outline.bounds.right(), ring.right()),
                    (outline.bounds.bottom(), ring.bottom()),
                ]
                .into_iter()
                .all(|(drawn, expected)| (drawn - expected).abs() <= px(0.5))
        })
    }

    /// Focus belongs to the tag it is on, not to its row's place: with the
    /// filter narrowed to "v1.0", Tab onto its third row, v1.02, then the
    /// filter emptied under it, which lets three tags in above it, the ring
    /// stays on v1.02, now sixth, and Tab from it reaches v1.03. Keyed by
    /// place, focus passed to v0.2, the tag that moved into the third row,
    /// and Tab went on from there.
    #[gpui::test]
    async fn focus_stays_on_its_tag_as_tags_come_in_above_it(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (repo, app, cx) = small_window_with_tags(cx, fixture.path(), 6);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");
        let filter = app
            .read_with(cx, |app, _| app.tag_actions.filter.clone())
            .expect("the Tags filter");
        let set_filter = |text: &'static str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                filter.update(cx, |input, cx| input.set_value(text, window, cx))
            });
            draw(cx);
        };
        let tags = repo.tags().expect("the tags").tags;
        assert_eq!(tags[5].name, "v1.02", "v1.02 is listed sixth");

        set_filter("v1.0", cx);
        for _ in 0..4 {
            if ring_on(cx, "tag-row-0") {
                break;
            }
            cx.simulate_keystrokes("tab");
            draw(cx);
        }
        assert!(ring_on(cx, "tag-row-0"), "Tab reaches the narrowed list");
        cx.simulate_keystrokes("tab tab");
        draw(cx);
        assert!(
            ring_on(cx, "tag-row-2"),
            "the ring is on the third row, v1.02"
        );

        set_filter("", cx);
        assert!(
            ring_on(cx, "tag-row-5"),
            "the ring stays on v1.02, now sixth"
        );
        assert!(!ring_on(cx, "tag-row-2"), "and leaves the third row, v0.2");
        assert_eq!(
            cx.update(|window, _| list.focused(window)),
            Some(tag_row_id(&tags[5])),
            "focus is v1.02's"
        );
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(ring_on(cx, "tag-row-6"), "and Tab goes on to v1.03");
    }

    /// Tab moves through every Push to… button of the tag inspector and
    /// Shift+Tab back, and the list scrolls each into view with its ring.
    #[gpui::test]
    async fn tab_reveals_every_push_destination(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (repo, app, cx) = small_window_with_tags(cx, fixture.path(), 0);
        // Local bare remotes: the inspector only lists them.
        let remotes = 10;
        for index in 0..remotes {
            let bare = fixture.path().join(format!("remote-{index:02}.git"));
            git(
                fixture.path(),
                &["init", "--quiet", "--bare", bare.to_str().unwrap()],
            );
            git(
                repo.path(),
                &[
                    "remote",
                    "add",
                    &format!("remote-{index:02}"),
                    bare.to_str().unwrap(),
                ],
            );
        }
        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Push to… list");

        let pushes: Vec<_> = (0..remotes)
            .map(|index| control("push-tag-remote", index))
            .collect();
        focus_filter(cx, &list, &pushes[0]);
        assert_tab_reveals(cx, &list, "tag-remote-list", &pushes);
        app.update(cx, |app, _| app.operation_busy = Some("Testing"));
        assert_steady(cx, &list, "tag-remote-list", &pushes);
    }

    /// Adds `count` local bare remotes, `remote-00` onwards, to `repo`: the
    /// lists only show them, and nothing is fetched or pushed.
    pub(crate) fn add_remotes(fixture: &std::path::Path, repo: &GitRepository, count: usize) {
        for index in 0..count {
            let bare = fixture.join(format!("remote-{index:02}.git"));
            git(
                fixture,
                &["init", "--quiet", "--bare", bare.to_str().unwrap()],
            );
            git(
                repo.path(),
                &[
                    "remote",
                    "add",
                    &format!("remote-{index:02}"),
                    bare.to_str().unwrap(),
                ],
            );
        }
    }

    /// The selectors `name-0` to `name-{count - 1}`.
    pub(crate) fn selectors(name: &str, count: usize) -> Vec<&'static str> {
        (0..count)
            .map(|index| &*format!("{name}-{index}").leak())
            .collect()
    }

    /// Return in Tags' filter inspects the first tag it matches once per
    /// press, under X11 too, and otherwise keeps Tags open with its filter
    /// and focus in it: while a write runs a press is refused, shows the
    /// busy look and says why at once in the window's alert, the key's
    /// repeats (`REPEATS`) saying nothing more, and a new press after the
    /// key-up says it again; with no match a press does nothing; once the
    /// write has ended a press inspects the first match. A click on Done
    /// with focus in the filter is Done's: Tags closes and nothing is
    /// inspected. The filter acted on its own `PressEnter` while the
    /// dialog's Done took the same press, so a refused choice closed Tags
    /// silently, the filter lost, and Return with no match closed it.
    #[gpui::test]
    async fn return_in_the_filter_chooses_once_per_press_and_keeps_tags_open(
        cx: &mut TestAppContext,
    ) {
        use crate::branch_actions::tests::{REPEATS, enter};
        use crate::copy_feedback::tests::alerted;
        let fixture = tempfile::tempdir().unwrap();
        let (app, _, cx) = open_tags_list(cx, fixture.path(), 3).await;
        let filter = app
            .read_with(cx, |app, _| app.tag_actions.filter.clone())
            .expect("the filter");
        let open = |cx: &mut VisualTestContext| cx.debug_bounds("tags-list").is_some();
        let kept = |cx: &mut VisualTestContext, at: &str, text: &str| {
            assert!(open(cx), "{at}: Tags stays open");
            cx.update(|window, cx| {
                let field = filter.read(cx);
                assert!(
                    field.focus_handle(cx).is_focused(window),
                    "{at}: focus stays in the filter"
                );
                assert_eq!(field.value().as_ref(), text, "{at}: the filter is kept");
            });
            assert!(
                app.read_with(cx, |app, _| app.tag_actions.task.is_none()),
                "{at}: no tag is inspected"
            );
        };
        let busy = |cx: &mut VisualTestContext, busy: bool| {
            app.update(cx, |app, cx| {
                app.operation_busy = busy.then_some("Testing");
                cx.notify();
            });
            draw(cx);
        };
        let filter_by = |cx: &mut VisualTestContext, text: &'static str| {
            cx.update(|window, cx| {
                filter.update(cx, |input, cx| input.set_value(text, window, cx));
                filter.read(cx).focus_handle(cx).focus(window, cx);
            });
            draw(cx);
        };
        filter_by(cx, "");
        busy(cx, true);
        let mut said: Option<(ElementId, String)> = None;
        for press in ["the first press", "a new press after the key-up"] {
            enter(cx, Some(false));
            draw(cx);
            kept(cx, press, "");
            let alert = alerted(cx).expect("the refusal is said");
            assert_eq!(
                alert.1,
                crate::workspace::BUSY_REASON,
                "{press}: it says why"
            );
            assert!(
                said.as_ref().is_none_or(|said| said.0 != alert.0),
                "{press}: said afresh"
            );
            assert!(
                app.read_with(cx, |app, _| app.busy_shown()),
                "{press}: the busy look shows at once"
            );
            for (repeat, events) in REPEATS {
                for down in events {
                    enter(cx, *down);
                }
                draw(cx);
                let at = format!("{press}, {repeat}");
                kept(cx, &at, "");
                assert_eq!(
                    alerted(cx),
                    Some(alert.clone()),
                    "{at}: nothing more is said"
                );
            }
            enter(cx, None);
            draw(cx);
            said = Some(alert);
        }
        filter_by(cx, "no-such-tag");
        enter(cx, Some(false));
        draw(cx);
        kept(cx, "no match", "no-such-tag");
        assert_eq!(alerted(cx), said, "no match: nothing is said");
        enter(cx, None);
        draw(cx);

        filter_by(cx, "");
        busy(cx, false);
        enter(cx, Some(false));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(PREPARING),
            "after the write, a press inspects the first match"
        );
        read_lands(&app, cx).await;
        enter(cx, None);
        draw(cx);
        assert!(!open(cx), "Tags gives way to the tag it inspects");

        // Done's click, with focus in the filter, is Done's.
        cx.update(|window, cx| window.close_all_dialogs(cx));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let filter = app
            .read_with(cx, |app, _| app.tag_actions.filter.clone())
            .expect("the filter");
        cx.update(|window, cx| filter.read(cx).focus_handle(cx).focus(window, cx));
        draw(cx);
        let done = cx.debug_bounds("dialog-ok: Done").expect("Done");
        cx.simulate_click(done.center(), gpui::Modifiers::default());
        draw(cx);
        assert!(!open(cx), "Done closes Tags");
        assert!(
            app.read_with(cx, |app, _| app.tag_actions.task.is_none()
                && app.operation_busy.is_none()),
            "Done inspects nothing"
        );
    }

    /// Tags of `tags` rows open at 1000 × 680, and its list.
    async fn open_tags_list<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
        tags: usize,
    ) -> (Entity<GitTurtle>, FocusReveal, &'a mut VisualTestContext) {
        let (_repo, app, cx) = small_window_with_tags(cx, fixture, tags.saturating_sub(3));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");
        (app, list, cx)
    }

    /// The tag inspector of a repository with `remotes` remotes open at
    /// 1000 × 680, and its Push to… list.
    async fn open_push_list<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
        remotes: usize,
    ) -> (Entity<GitTurtle>, FocusReveal, &'a mut VisualTestContext) {
        let (repo, app, cx) = small_window_with_tags(cx, fixture, 0);
        add_remotes(fixture, &repo, remotes);
        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        read_lands(&app, cx).await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Push to… list");
        (app, list, cx)
    }

    /// A list `height` tall that keeps `room` around its `rows` rows, each
    /// 20 px tall, cued by [`ScrollCue`] as the app's lists are. Tests find
    /// it as `cued-list`, its rows as `cued-row-<n>` and its scrollbar, while
    /// it is drawn, as `cued-list/scrollbar`.
    struct Cued {
        scroll: ScrollHandle,
        height: Pixels,
        room: Pixels,
        rows: usize,
    }

    impl Render for Cued {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let cue = ScrollCue::new(&self.scroll, self.room);
            let observe = cue.observe(|_, _, _| {});
            let list = div()
                .id("cued-list")
                .debug_selector(|| "cued-list".into())
                .h(self.height)
                .p(self.room)
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children((0..self.rows).map(|row| {
                    div()
                        .debug_selector(move || format!("cued-row-{row}"))
                        .h(px(20.))
                        .child(format!("Row {row}"))
                }));
            div().w(px(200.)).child(
                cue.list(list, "cued-list/scrollbar")
                    .on_children_prepainted(observe),
            )
        }
    }

    /// A window drawing a [`Cued`] list `height` tall with `rows` rows and
    /// `room` around them, in the Midnight palette.
    fn cued_window(
        cx: &mut gpui_kit::TestAppContext,
        height: f32,
        room: Pixels,
        rows: usize,
    ) -> (Entity<Cued>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        let (list, cx) = cx.add_window_view(|_, _| Cued {
            scroll: ScrollHandle::new(),
            height: px(height),
            room,
            rows,
        });
        draw(cx);
        (list, cx)
    }

    /// The fades the first frame drawn after `input` paints over the list
    /// drawn at `list` ([`painted_fades`]). `input` runs and the frame is
    /// drawn in one update, and the frame's fades are read before the test
    /// harness, flushing the effects the input left, draws the window again,
    /// which would hide a fade that lagged a frame behind.
    fn first_frame_fades(
        cx: &mut VisualTestContext,
        list: Bounds<Pixels>,
        input: impl FnOnce(&mut Window, &mut App),
    ) -> (Option<Pixels>, Option<Pixels>) {
        cx.update(|window, cx| {
            input(window, cx);
            window.draw(cx).clear(cx);
            fades_in(window, cx, list)
        })
    }

    /// The edge fades painted over the list drawn at `list`: the height of
    /// the one along its top and of the one along its bottom, each as wide
    /// as the list and in the palette's canvas, the theme's background,
    /// opaque at the list's edge and clear inward.
    pub(crate) fn painted_fades(
        cx: &mut VisualTestContext,
        list: Bounds<Pixels>,
    ) -> (Option<Pixels>, Option<Pixels>) {
        cx.update(|window, cx| fades_in(window, cx, list))
    }

    /// [`painted_fades`] for fades in `surface`, a palette colour other than
    /// its canvas.
    pub(crate) fn painted_fades_on(
        cx: &mut VisualTestContext,
        list: Bounds<Pixels>,
        surface: u32,
    ) -> (Option<Pixels>, Option<Pixels>) {
        cx.update(|window, _| fades_over(window, list, surface))
    }

    /// [`painted_fades`] in the frame `window` drew last.
    fn fades_in(
        window: &Window,
        cx: &App,
        list: Bounds<Pixels>,
    ) -> (Option<Pixels>, Option<Pixels>) {
        fades_over(window, list, appearance::palette(cx).canvas)
    }

    /// The fades in `surface` the frame `window` drew last painted over the
    /// list drawn at `list`.
    fn fades_over(
        window: &Window,
        list: Bounds<Pixels>,
        surface: u32,
    ) -> (Option<Pixels>, Option<Pixels>) {
        let surface: Hsla = rgb(surface).into();
        // A gradient toward `angle` (0° the top, 180° the bottom), clear
        // where it starts and the surface where it ends.
        let toward = |angle: f32| {
            linear_gradient(
                angle,
                linear_color_stop(surface.opacity(0.), 0.),
                linear_color_stop(surface, 1.),
            )
        };
        let (up, down) = (toward(0.), toward(180.));
        let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
        let scale = window.scale_factor();
        let (mut top, mut bottom) = (None, None);
        for quad in window.painted_quads() {
            let band = logical(quad.bounds, scale);
            if !near(band.left(), list.left()) || !near(band.right(), list.right()) {
                continue;
            }
            if quad.background == up && near(band.top(), list.top()) {
                assert!(top.is_none(), "one fade along {list:?}'s top");
                top = Some(band.size.height);
            } else if quad.background == down && near(band.bottom(), list.bottom()) {
                assert!(bottom.is_none(), "one fade along {list:?}'s bottom");
                bottom = Some(band.size.height);
            }
        }
        (top, bottom)
    }

    /// A cued list's thumb, the only cue that more rows lie past its edge,
    /// paints the palette's thumb in every palette, 2:1 or more against the
    /// canvas and the panel. In `border` it stood 1.52:1 off Midnight's
    /// canvas, 1.39:1 off its panel and 1.28:1 off Braden's canvas.
    #[gpui::test]
    fn a_cued_lists_thumb_reads_two_to_one_in_every_palette(cx: &mut gpui_kit::TestAppContext) {
        use crate::appearance::custom::contrast;
        const ROOM: Pixels = px(8.);
        let (_list, cx) = cued_window(cx, 120., ROOM, 20);
        let bar = Scrollbar::width();
        let mut failures = Vec::new();
        for (name, palette) in appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            draw(cx);
            let bounds = cx.debug_bounds("cued-list").expect("the list");
            // The scrollbar's viewport, which its track fills.
            let track = Bounds::new(
                point(bounds.right() - ROOM - bar - px(4.), bounds.top() + ROOM),
                size(bar + px(4.), bounds.size.height - ROOM * 2.),
            );
            let thumb = palette.scrollbar_thumb();
            let painted = painted_thumb_fill(cx, track);
            if painted != Some(gpui_kit::Background::from(Hsla::from(rgb(thumb)))) {
                failures.push(format!("{name}: painted {painted:?}, not #{thumb:06x}"));
            }
            for (surface, color) in [("canvas", palette.canvas), ("panel", palette.panel)] {
                let ratio = contrast(thumb, color);
                if ratio < 2. {
                    failures.push(format!(
                        "{name}: #{thumb:06x} {ratio:.3}:1 on the {surface}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// A list that keeps room around its rows is cued where a thumb moves,
    /// which it judges by the track its scrollbar lies along, the list less
    /// that room, which is where the kit lays it: 60 px tall with 8 px
    /// around its rows, a 44 px track shorter than [`SHORTEST_SCROLL_TRACK`],
    /// it draws no scrollbar, and 80 px tall, a 64 px track, it draws one.
    /// Judged by the list's whole height, the 60 px list drew a thumb along a
    /// track shorter than the thumb. Every [`ScrollCue`] follows the rule; it
    /// was once a list's own choice. At either length its overflowing rows
    /// keep the scrollbar's gutter, so they never move as it crosses.
    #[gpui::test]
    fn a_cue_judges_the_track_its_scrollbar_lies_along(cx: &mut gpui_kit::TestAppContext) {
        const ROOM: Pixels = px(8.);
        let (list, cx) = cued_window(cx, 60., ROOM, 20);
        let bar = Scrollbar::width();
        for (height, cued) in [(60., false), (80., true), (60., false)] {
            list.update(cx, |list, cx| {
                list.height = px(height);
                cx.notify();
            });
            draw(cx);
            let bounds = cx.debug_bounds("cued-list").expect("the list");
            let row = cx.debug_bounds("cued-row-0").expect("its first row");
            assert!(
                row.right() < bounds.right() - ROOM - px(0.5),
                "{height} px: the overflowing row {row:?} keeps a gutter in {bounds:?}"
            );
            // The scrollbar's viewport, which its track fills.
            let track = Bounds::new(
                point(bounds.right() - ROOM - bar - px(4.), bounds.top() + ROOM),
                size(bar + px(4.), bounds.size.height - ROOM * 2.),
            );
            let thumb = painted_thumb(cx, track);
            assert_eq!(
                thumb.is_some(),
                cued,
                "{height} px: a scrollbar ({thumb:?}) only along a track a thumb moves on"
            );
            assert_eq!(
                cx.debug_bounds("cued-list/scrollbar").is_some(),
                cued,
                "{height} px: the scrollbar's element is drawn only along a track a thumb moves on"
            );
            assert_eq!(
                painted_fades(cx, bounds).1.is_some(),
                !cued,
                "{height} px: a list at its top fades its bottom edge only where it draws no scrollbar"
            );
        }
    }

    /// A list too short for the kit's thumb to move along its track draws no
    /// scrollbar and cues the rows beyond its edges with a fade at each edge
    /// they continue past: at 13 and 18 pt, 40 px tall with ten 20 px rows,
    /// it leaves no scrollbar element behind, its rows keep the gutter they
    /// keep beside a scrollbar, so they never move as the list grows or
    /// shrinks past the shortest track, and at its top it fades its bottom
    /// edge alone, part way down both,
    /// and scrolled to its end its top alone. Each fade is the scaled 8 px,
    /// at most a quarter of the list, so it grows with the text, and lies
    /// across the list's whole width. A side header's controls region at
    /// its 36–44 px floor once drew the kit's 48 px thumb along a track
    /// shorter than the thumb.
    #[gpui::test]
    fn a_list_too_short_for_a_thumb_fades_the_edges_its_rows_continue_past(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (list, cx) = cued_window(cx, 40., Pixels::ZERO, 10);
        let scroll = list.read_with(cx, |list, _| list.scroll.clone());
        let mut fades = Vec::new();
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            scroll_list_to(cx, &scroll, 0.);
            let at = format!("{text} pt");
            let region = cx.debug_bounds("cued-list").expect("the list");
            assert!(
                (region.size.height - px(40.)).abs() < px(0.01) && scroll.max_offset().y > px(100.),
                "{at}: the 40 px list {region:?} overflows"
            );
            let bar = Scrollbar::width() + px(4.);
            let track = Bounds::new(
                point(region.right() - bar, region.top()),
                size(bar, region.size.height),
            );
            assert!(
                cx.debug_bounds("cued-list/scrollbar").is_none()
                    && painted_thumb(cx, track).is_none(),
                "{at}: the list {region:?} draws no scrollbar"
            );
            let row = cx.debug_bounds("cued-row-0").expect("its first row");
            let gutter = ScrollCue::gutter();
            assert!(
                (row.right() - (region.right() - gutter)).abs() < px(0.5),
                "{at}: the row {row:?} keeps the {gutter:?} gutter it keeps beside a scrollbar in {region:?}"
            );
            let fade = cx
                .update(|_, _| appearance::ui_size(8.))
                .min(region.size.height / 4.);
            let near = |painted: Option<Pixels>| {
                painted.is_some_and(|height| (height - fade).abs() < px(0.01))
            };
            let (top, bottom) = painted_fades(cx, region);
            assert!(
                top.is_none() && near(bottom),
                "{at}: at its top the list fades its bottom edge alone by {fade:?}, not {top:?} and {bottom:?}"
            );
            scroll_list_to(cx, &scroll, 0.5);
            let (top, bottom) = painted_fades(cx, region);
            assert!(
                near(top) && near(bottom),
                "{at}: part way down the list fades both edges, not {top:?} and {bottom:?}"
            );
            scroll_list_to(cx, &scroll, 1.);
            let (top, bottom) = painted_fades(cx, region);
            assert!(
                near(top) && bottom.is_none(),
                "{at}: scrolled to its end the list fades its top edge alone, not {top:?} and {bottom:?}"
            );
            fades.push(fade);
        }
        assert!(
            fades[1] > fades[0],
            "the fade grows with the text: {fades:?}"
        );
    }

    /// A list too short for a thumb leaves no scrollbar behind: 40 px tall
    /// with ten rows that overflow it, it draws no scrollbar element, so no
    /// hidden bar stands at its edge for a pointer or an assistive
    /// technology to meet, and it still reports itself, as it does 80 px
    /// tall with its scrollbar, as a scroll view that clips its rows, at
    /// its own bounds.
    #[gpui::test]
    fn a_list_too_short_for_a_thumb_leaves_no_scrollbar_behind_and_reports_a_scroll_view(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::native_accessibility::written;
        let (list, cx) = cued_window(cx, 80., Pixels::ZERO, 10);
        for (height, bar) in [(80., true), (40., false), (80., true)] {
            list.update(cx, |list, cx| {
                list.height = px(height);
                cx.notify();
            });
            draw(cx);
            written::forget();
            draw(cx);
            assert_eq!(
                cx.debug_bounds("cued-list/scrollbar").is_some(),
                bar,
                "{height} px: a scrollbar element only along a track a thumb moves on"
            );
            let region = cx.debug_bounds("cued-list").expect("the list");
            let written = written::get("cued-list").expect("the list writes its node");
            let (node, layout) = (written.node, written.layout.expect("laid out"));
            let scale = cx.update(|window, _| f64::from(window.scale_factor()));
            assert!(
                node.role() == gpui::accesskit::Role::ScrollView
                    && node.clips_children()
                    && (layout.y1 - layout.y0 - f64::from(f32::from(region.size.height)) * scale)
                        .abs()
                        < 0.5,
                "{height} px: the list {region:?} reports itself as a scroll view that clips its rows, not {node:?} at {layout:?}"
            );
        }
    }

    /// How far each fade reaches with the focused control's ring at each
    /// place along a 100 px list whose fades are 10 px: a ring above the
    /// list, wholly outside it, or below it leaves both fades whole, as one
    /// in the list's middle, between the bands, does; a ring inside a band
    /// ends that band where the ring begins, the other whole; and a ring
    /// that reaches the list's edge, or past it, leaves that edge no fade.
    /// A ring just outside an edge, touching it, is outside the list, so
    /// the fade there stays whole. Without the checks that the ring lies
    /// within the list, a ring above it took the top fade away, and one
    /// below it the bottom's.
    #[test]
    fn a_fade_stops_short_of_a_ring_only_inside_its_band() {
        let list = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(100.)));
        let fade = px(10.);
        let ring = |top: f32, bottom: f32| {
            Some(Bounds::from_corners(
                point(px(10.), px(top)),
                point(px(150.), px(bottom)),
            ))
        };
        for (at, ring, reach) in [
            ("no ring", None, (fade, fade)),
            ("above the list", ring(-30., -20.), (fade, fade)),
            (
                "touching the top edge from outside",
                ring(-14., 0.),
                (fade, fade),
            ),
            ("reaching the top edge", ring(-2., 12.), (px(0.), fade)),
            ("inside the top band", ring(4., 20.), (px(4.), fade)),
            (
                "below the top band, above the bottom one",
                ring(40., 60.),
                (fade, fade),
            ),
            ("inside the bottom band", ring(80., 96.), (fade, px(4.))),
            ("reaching the bottom edge", ring(88., 102.), (fade, px(0.))),
            (
                "touching the bottom edge from outside",
                ring(100., 114.),
                (fade, fade),
            ),
            ("below the list", ring(120., 130.), (fade, fade)),
            ("across the whole list", ring(-5., 105.), (px(0.), px(0.))),
        ] {
            assert_eq!(fade_reach(list, fade, ring), reach, "a ring {at}: {ring:?}");
        }
    }

    /// A list's fades follow its length in the frame that changes it: rows
    /// arriving in a 200 px list, whose track holds a thumb that moves, draw
    /// no fade in the first frame that draws them, while its scrollbar is
    /// still to come, and none once it has come; shrunk to 40 px, the list
    /// fades its bottom edge in the frame that shrinks it, and that frame
    /// draws no scrollbar: no scrollbar element, which a pointer would meet,
    /// and nothing painted in its column. Decided by the overflow alone as
    /// it painted, the tall list drew a bottom fade for a frame before its
    /// scrollbar, on Tags, the branch chooser, the GitHub panel's body and
    /// the Reflog's entries as they opened; and decided as the cue
    /// rendered, the shrunk list's bar painted over the new fade for the
    /// frame that shrank it, its hitbox live.
    #[gpui::test]
    fn a_list_s_fades_follow_its_length_in_the_frame_drawn(cx: &mut gpui_kit::TestAppContext) {
        let (list, cx) = cued_window(cx, 200., Pixels::ZERO, 1);
        let region = cx.debug_bounds("cued-list").expect("the list");
        let grown = list.clone();
        let first = first_frame_fades(cx, region, move |_, cx| {
            grown.update(cx, |list, cx| {
                list.rows = 30;
                cx.notify();
            })
        });
        assert_eq!(
            first,
            (None, None),
            "the first frame of a tall list's arriving rows fades no edge"
        );
        // Every frame the cue draws its scrollbar in, as the frame paints.
        let barred = |frames: &[crate::in_frame::probe::Frame]| {
            frames.iter().any(|frame| {
                frame.region == "cued-list/scrollbar" && frame.part("scrollbar").is_some()
            })
        };
        crate::in_frame::probe::watch();
        draw(cx);
        assert!(
            cx.debug_bounds("cued-list/scrollbar").is_some()
                && barred(&crate::in_frame::probe::take())
                && painted_fades(cx, region) == (None, None),
            "the tall list cues its rows with a scrollbar and no fade"
        );
        let short = Bounds::new(region.origin, size(region.size.width, px(40.)));
        // The scrollbar's column, where the kit paints its track and thumb.
        let column = Bounds::from_corners(
            point(short.right() - Scrollbar::width() - px(4.), short.top()),
            short.bottom_right(),
        );
        let shrunk = list.clone();
        let ((top, bottom), painted) = cx.update(|window, cx| {
            shrunk.update(cx, |list, cx| {
                list.height = px(40.);
                cx.notify();
            });
            window.draw(cx).clear(cx);
            let scale = window.scale_factor();
            let painted: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| logical(quad.bounds, scale))
                .filter(|fill| {
                    fill.left() >= column.left() - px(0.5)
                        && fill.right() <= column.right() + px(0.5)
                        && fill.intersects(&column)
                })
                .collect();
            (fades_in(window, cx, short), painted)
        });
        let frames = crate::in_frame::probe::take();
        crate::in_frame::probe::stop();
        assert!(
            !barred(&frames) && painted.is_empty(),
            "the frame that shrinks the list to 40 px draws no scrollbar: {painted:?} painted in \
             its column {column:?}, {frames:?}"
        );
        assert!(
            top.is_none() && bottom.is_some(),
            "the frame that shrinks the list to 40 px fades its bottom edge, not {top:?} and {bottom:?}"
        );
    }

    /// A short list's fades follow what it shows in the frame that shows it,
    /// and catch no pointer. Rows that arrive in a list that fit fade its
    /// bottom edge in the first frame that draws them: decided as the cue
    /// rendered, from the reach the list kept from the frame before, they
    /// drew none, and nothing drew the list again. A wheel over the bottom
    /// fade scrolls the rows beneath it, and the frame it draws fades both
    /// edges; one to the end leaves the top's alone, and one back to the top
    /// the bottom's alone, each in that one frame.
    #[gpui::test]
    fn a_short_list_s_fades_follow_its_rows_and_a_wheel_in_the_frame_drawn(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (list, cx) = cued_window(cx, 40., Pixels::ZERO, 1);
        let region = cx.debug_bounds("cued-list").expect("the list");
        assert_eq!(
            painted_fades(cx, region),
            (None, None),
            "a list whose rows fit fades no edge"
        );
        let grown = list.clone();
        let (top, bottom) = first_frame_fades(cx, region, move |_, cx| {
            grown.update(cx, |list, cx| {
                list.rows = 10;
                cx.notify();
            })
        });
        assert!(
            top.is_none() && bottom.is_some(),
            "the frame that draws the rows that arrived fades the bottom edge alone, not {top:?} and {bottom:?}"
        );
        let wheel = |by: f32, cx: &mut VisualTestContext| {
            first_frame_fades(cx, region, move |window, cx| {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        // Over the bottom fade.
                        position: point(region.center().x, region.bottom() - px(2.)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(by))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            })
        };
        let first = cx.debug_bounds("cued-row-0").expect("the first row");
        let (top, bottom) = wheel(-15., cx);
        let moved = cx.debug_bounds("cued-row-0").expect("the first row");
        assert!(
            (first.top() - moved.top() - px(15.)).abs() < px(0.5),
            "a wheel over the bottom fade scrolls the rows beneath it: {first:?} to {moved:?}"
        );
        assert!(
            top.is_some() && bottom.is_some(),
            "the frame the wheel draws fades both edges, not {top:?} and {bottom:?}"
        );
        let (top, bottom) = wheel(-10_000., cx);
        let last = cx.debug_bounds("cued-row-9").expect("the last row");
        assert!(
            (last.bottom() - region.bottom()).abs() < px(0.5),
            "the wheel scrolls to the rows' end {last:?} in {region:?}"
        );
        assert!(
            top.is_some() && bottom.is_none(),
            "the frame the wheel to the end draws fades the top edge alone, not {top:?} and {bottom:?}"
        );
        let (top, bottom) = wheel(10_000., cx);
        assert!(
            top.is_none() && bottom.is_some(),
            "the frame the wheel back draws fades the bottom edge alone, not {top:?} and {bottom:?}"
        );
    }

    /// Tags' 30 rows overflow its list at 1000 × 680, and its scrollbar's
    /// thumb follows the list's offset in a gutter clear of every ring.
    #[gpui::test]
    async fn tags_list_cues_rows_beyond_its_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, list, cx) = open_tags_list(cx, fixture.path(), 30).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let rows = selectors("tag-row", 30);
        assert_scroll_cue(cx, &app, list.scroll(), "tags-list", room, &rows);
    }

    /// Tags' three rows fit its list: it paints no thumb, and its rows keep
    /// their width.
    #[gpui::test]
    async fn tags_list_that_fits_paints_no_thumb(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, _list, cx) = open_tags_list(cx, fixture.path(), 3).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        assert_no_scroll_cue(cx, &app, "tags-list", room, &selectors("tag-row", 3));
    }

    /// Ten Push to… buttons overflow the tag inspector's list at
    /// 1000 × 680, and its scrollbar's thumb follows the list's offset in a
    /// gutter clear of every ring.
    #[gpui::test]
    async fn push_list_cues_buttons_beyond_its_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, list, cx) = open_push_list(cx, fixture.path(), 10).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let pushes = selectors("push-tag-remote", 10);
        assert_scroll_cue(cx, &app, list.scroll(), "tag-remote-list", room, &pushes);
    }

    /// Two Push to… buttons fit the tag inspector's list: it paints no
    /// thumb, and the buttons keep their width.
    #[gpui::test]
    async fn push_list_that_fits_paints_no_thumb(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, _list, cx) = open_push_list(cx, fixture.path(), 2).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        assert_no_scroll_cue(
            cx,
            &app,
            "tag-remote-list",
            room,
            &selectors("push-tag-remote", 2),
        );
    }
}
