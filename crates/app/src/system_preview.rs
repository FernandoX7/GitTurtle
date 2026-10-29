//! The explicit action that opens a read-only copy of captured revision bytes
//! in another application: Quick Look's System preview on macOS, and Open in
//! default app on Linux.
//!
//! Repository content controls both the bytes and the stored file name. Only a
//! click opens anything: selection, refresh, tab restore and reopening never
//! do. Whether a side offers the action is decided on the worker when the
//! side is prepared ([`offer`]), never while rendering. On Linux that decision
//! is made from content alone and also fixes the type the copy is handed over
//! as ([`passive`]); the repository's file name is never used. The copy lives
//! in a private per-user cache ([`store`]) and goes to the desktop through
//! the OpenURI portal, or `xdg-open` where no portal answers, only after the
//! desktop's own type sniffers (GIO, and `xdg-mime` for `xdg-open`) agree
//! with that type ([`launch`]).
//!
//! A click answers on its own control, as a copy does ([`copy_feedback`]):
//! once something opened, the control reads Opened for
//! [`copy_feedback::RESTORE_AFTER`] and "Opened a read-only copy of the
//! captured bytes." is announced. When nothing opened, a line beneath the
//! control's row says why ([`header_ends`]): a failure in the warning color,
//! announced at once, or, when the portal reports the request cancelled (a
//! closed application chooser, whether or not it offered an application), a
//! muted "Nothing opened.". Beneath an image side's caption, as in a
//! document's header, the line is short and leads with the reason
//! ([`Answer::short_line`]), the whole answer in its tooltip, name and
//! announcement; the short line is written from the failure's typed
//! [`Category`], never from its sentence. The line stays while that
//! comparison is shown and goes when another is ([`Showing`]); nothing
//! reaches the window-wide notice.

use crate::copy_feedback::{self, Action, Mark};
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use anyhow::Result;
use gpui_kit::prelude::FluentBuilder;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(target_os = "linux")]
mod launch;
#[cfg(target_os = "linux")]
pub(crate) mod passive;
#[cfg(target_os = "linux")]
mod store;

/// The open action one captured side offers. Sides that offer none hold
/// `None`, so no surface shows the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    #[cfg(target_os = "linux")]
    kind: passive::Kind,
}

/// Quick Look previews any captured bytes and never launches an application.
#[cfg(target_os = "macos")]
pub(crate) fn offer(_bytes: &[u8], _check: &impl Fn() -> Result<()>) -> Result<Option<Offer>> {
    Ok(Some(Offer {}))
}

/// The desktop's default application receives only a passive type that the
/// content itself establishes.
#[cfg(target_os = "linux")]
pub(crate) fn offer(bytes: &[u8], check: &impl Fn() -> Result<()>) -> Result<Option<Offer>> {
    Ok(passive::Kind::of(bytes, check)?.map(|kind| Offer { kind }))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn offer(_bytes: &[u8], _check: &impl Fn() -> Result<()>) -> Result<Option<Offer>> {
    Ok(None)
}

impl Offer {
    /// The control's label, naming what happens on this platform.
    pub(crate) fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            "System preview"
        } else {
            "Open in default app"
        }
    }
}

#[cfg(target_os = "linux")]
impl Offer {
    /// What the click hands over, so the tooltip names the type the
    /// application receives rather than the repository's file name.
    pub(crate) fn tooltip(self) -> String {
        format!(
            "Open a read-only copy of these captured bytes as {} in the desktop's default app",
            self.kind.description()
        )
    }

    /// The accessible name of a control labelled `label`: one form for every
    /// open control, which starts with the visible label, so speech input
    /// finds the control by what it shows (WCAG 2.5.3), then names the
    /// `side`, which the label leaves to the heading beside it, and the type
    /// the application receives: "Open in default app: After, a read-only
    /// copy as a PDF document" in a document's header, "Open in default app:
    /// Before, a read-only copy as a PNG image" in an image side's caption.
    pub(crate) fn accessible_name(self, label: &str, side: Option<&str>) -> String {
        let copy = format!("a read-only copy as {}", self.kind.description());
        match side {
            Some(side) => format!("{label}: {side}, {copy}"),
            None => format!("{label}: {copy}"),
        }
    }

    /// The icon form's tooltip, which leads with the label it does not show
    /// and names what the application receives: "Open in default app: a
    /// read-only copy as a PDF document".
    fn icon_tooltip(self) -> String {
        self.accessible_name(self.label(), None)
    }

    /// The sentence that tells a reader where the complete animation is.
    pub(crate) fn complete_animation(self) -> &'static str {
        "Open captured bytes in the default app for the complete animation."
    }
}

#[cfg(not(target_os = "linux"))]
impl Offer {
    pub(crate) fn tooltip(self) -> String {
        "Open an isolated read-only copy of these captured bytes in system Quick Look".into()
    }

    fn icon_tooltip(self) -> String {
        format!("{} · {}", self.label(), self.tooltip())
    }

    pub(crate) fn complete_animation(self) -> &'static str {
        "Open captured bytes in system preview for the complete animation."
    }
}

/// The open control's action: its external-link icon at rest, and Opened
/// after the check once a click opened something.
pub(crate) const OPEN: Action = Action {
    icon: "external-link",
    done: "Opened",
    state: "opened",
};

/// What assistive technology hears once a click opened something. The portal
/// can let the person choose the application, so it names none.
pub(crate) const OPENED: &str = "Opened a read-only copy of the captured bytes.";

/// What a muted line beneath the control says when the portal reports the
/// request cancelled, which is not a failure. The portal answers the same
/// whether the person closed an application chooser or closed one that
/// offered no application ("No Apps available"), so the line names no
/// cause: "the application chooser was closed" once hid the second.
#[cfg(any(target_os = "linux", test))]
pub(crate) const DISMISSED: &str = "Nothing opened.";

/// Why a click opened nothing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotOpened {
    /// The portal reports the request cancelled: the person closed its
    /// application chooser, with or without an application to offer. Only
    /// the Linux launcher has a portal to cancel, so other builds leave it
    /// out outside tests, as they do the failure categories only Linux
    /// constructs ([`Category`]).
    #[cfg(any(target_os = "linux", test))]
    Dismissed,
    /// Nothing could be opened, for this reason.
    Failed(Failure),
}

impl From<Failure> for NotOpened {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure)
    }
}

impl From<String> for NotOpened {
    fn from(reason: String) -> Self {
        Self::Failed(reason.into())
    }
}

impl From<&str> for NotOpened {
    fn from(reason: &str) -> Self {
        Self::Failed(reason.into())
    }
}

/// What kind of failure stopped a click, as the code that refused it
/// classifies it. The short line beneath the control is written from this
/// alone ([`Category::short_line`]), never from the failure's sentence.
///
/// Only Linux's launcher and copy store refuse with a category of their own,
/// so other builds, tests aside, have `Other` alone rather than variants
/// nothing constructs: macOS's Quick Look reports every failure as `Other`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    /// A desktop type sniffer (GIO for the portal, `xdg-mime` for
    /// `xdg-open`) types the copy as something other than the kind it was
    /// offered as. Holds the type as the launcher names it: "an SVG document"
    /// or "an unknown type", or a media type cleaned to at most 96 of its own
    /// characters ("application/x-php").
    #[cfg(any(target_os = "linux", test))]
    SeenAs(String),
    /// The copy's media type, one its kind may be, has no default
    /// application.
    #[cfg(any(target_os = "linux", test))]
    NoDefaultApplication(String),
    /// The captured bytes exceed the private copy's size limit
    /// ([`gitturtle_preview::MAX_INPUT_BYTES`]), so no copy was written.
    #[cfg(any(target_os = "linux", test))]
    TooLarge,
    /// Anything else, such as a missing program, the portal's answer or the
    /// private copy's store.
    Other,
}

/// Why a click opened nothing: its [`Category`] and the sentence that says
/// so, which the line's tooltip, accessible name and announcement carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) category: Category,
    sentence: String,
}

impl Failure {
    pub(crate) fn new(category: Category, sentence: impl Into<String>) -> Self {
        Self {
            category,
            sentence: sentence.into(),
        }
    }

    pub(crate) fn sentence(&self) -> &str {
        &self.sentence
    }
}

/// A failure of no category of its own.
impl From<String> for Failure {
    fn from(sentence: String) -> Self {
        Self::new(Category::Other, sentence)
    }
}

impl From<&str> for Failure {
    fn from(sentence: &str) -> Self {
        Self::new(Category::Other, sentence)
    }
}

/// The short line for a failure whose category has no line of its own.
pub(crate) const NOT_OPENED: &str = "Could not open this copy";

impl Category {
    /// What a short line shows: the reason first, in a few words that a
    /// side's half of a narrow column holds in two lines ("Seen as an SVG
    /// document, so not opened"). The side is the heading or caption above
    /// the line. A category without a line of its own fails closed to
    /// [`NOT_OPENED`], leaving its words to the whole sentence.
    fn short_line(&self) -> String {
        #[cfg(any(target_os = "linux", test))]
        match self {
            Self::SeenAs(seen) => return format!("Seen as {seen}, so not opened"),
            Self::NoDefaultApplication(media_type) => {
                return format!("No default app for {media_type}");
            }
            Self::TooLarge => {
                return format!(
                    "Over {} MiB, so not opened",
                    gitturtle_preview::MAX_INPUT_BYTES >> 20
                );
            }
            // `Other`, and any category added without a line here.
            _ => {}
        }
        NOT_OPENED.into()
    }
}

/// One showing of a comparison. Its owner takes a new one whenever the
/// preview it shows is replaced or restored (`GitTurtle::clear_preview`, a
/// restored inspection, the rewrite review's next file), so a line beneath an
/// open control stays with the showing it was made in and is gone when the
/// same file, kept by the worker's cache, is shown again. Showings are
/// unique across windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Showing(u64);

impl Showing {
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Who opens a comparison's captured bytes, and the showing its controls'
/// lines belong to.
#[derive(Clone)]
pub(crate) struct Opener {
    owner: WeakEntity<GitTurtle>,
    showing: Showing,
}

impl Opener {
    pub(crate) fn new(owner: WeakEntity<GitTurtle>, showing: Showing) -> Self {
        Self { owner, showing }
    }

    /// The open control `control` for the captured `bytes` of the `side`
    /// ("Before", "After", "Source") stored as `name`, which `offer` offers.
    /// Linux never uses `name`.
    pub(crate) fn target(
        &self,
        control: impl Into<ElementId>,
        side: &'static str,
        bytes: &Arc<[u8]>,
        name: &std::path::Path,
        offer: Offer,
    ) -> Target {
        // The bytes' allocation and length name what was opened, so another
        // file's control in the same place is at rest.
        let subject = (Arc::as_ptr(bytes) as *const u8 as usize, bytes.len());
        #[cfg(target_os = "linux")]
        let _ = name;
        Target {
            opener: self.clone(),
            mark: Mark::new(control, subject),
            side,
            bytes: Arc::clone(bytes),
            offer,
            #[cfg(not(target_os = "linux"))]
            name: name.to_path_buf(),
        }
    }
}

/// How an open control is drawn on its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// Its icon and its label, where the line has room for both.
    Labelled,
    /// Its icon alone in the compact frame's 28 px square, scaled, where the
    /// label does not fit: the content beside it keeps its place and the
    /// action becomes an icon rather than being cut at its column's edge.
    /// Its tooltip and accessible name lead with the label.
    Icon,
}

thread_local! {
    /// The labelled control's label, laid out.
    static LABEL: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

impl Offer {
    /// The form of a control on a line that leaves it `room`: labelled where
    /// its frame, icon and label fit, and otherwise its icon.
    pub(crate) fn form(self, room: Pixels, window: &Window) -> Form {
        if room >= labelled_button_width(self.label(), &LABEL, window) {
            Form::Labelled
        } else {
            Form::Icon
        }
    }
}

/// One open control, with what its click opens: the type the copy is handed
/// over as on Linux, and elsewhere the stored name Quick Look's copy is
/// suffixed from.
#[derive(Clone)]
pub(crate) struct Target {
    opener: Opener,
    mark: Mark,
    side: &'static str,
    bytes: Arc<[u8]>,
    offer: Offer,
    #[cfg(not(target_os = "linux"))]
    name: std::path::PathBuf,
}

/// The lines beneath open controls whose click opened nothing, newest last.
/// Each belongs to its control, bytes and showing; the oldest go once there
/// are more than one showing's two sides and a spare need.
#[derive(Default)]
struct Notes {
    /// Counts notes; each is a fresh node, announced when it appears.
    count: usize,
    notes: VecDeque<Note>,
}
impl Global for Notes {}

const NOTES_KEPT: usize = 4;

struct Note {
    id: usize,
    mark: Mark,
    showing: Showing,
    failed: bool,
    /// The whole answer, which the line's tooltip, accessible name and
    /// announcement carry: "Could not open After. …" or "Nothing opened.".
    text: SharedString,
    /// What a short line shows: the reason first
    /// ([`Category::short_line`]).
    brief: SharedString,
}

/// Why the last click on an open control in its showing opened nothing, as
/// its line beneath the control says it.
pub(crate) struct Answer {
    id: usize,
    key: String,
    failed: bool,
    text: SharedString,
    brief: SharedString,
}

impl Answer {
    /// What the short line shows ([`Category::short_line`]), or "Nothing
    /// opened.".
    pub(crate) fn brief(&self) -> &str {
        &self.brief
    }

    /// The line's frame: a failure in the warning color fitted to the
    /// page's surfaces, announced at once, or a closed chooser in muted text,
    /// announced politely, both by their whole answer. Tests find it as
    /// `<control id>/note`.
    fn frame(&self, cx: &App) -> Stateful<Div> {
        let key = self.key.clone();
        let probe = key.clone();
        let path = key.clone();
        let color = if self.failed {
            appearance::labels(cx).warning
        } else {
            palette(cx).muted
        };
        div()
            .id(("open-note", self.id))
            .debug_selector(move || key.clone())
            .map(|line| {
                if self.failed {
                    line.role(Role::Alert).assertive()
                } else {
                    line.role(Role::Status).polite()
                }
            })
            .aria_label(self.text.clone())
            .w_full()
            .min_w_0()
            .text_size(appearance::ui_text(11.))
            .text_color(rgb(color))
            .children(appearance::text_probe(move || probe))
            .children(native_accessibility::node_path_probe(move || path))
    }

    /// The short line: the reason first ([`Answer::brief`]), whole, or on
    /// `clamp` lines ending in an ellipsis where it needs more, so it ends at
    /// a whole line. Its whole answer is in its tooltip, as in its name,
    /// wherever the line shows less of it.
    pub(crate) fn short_line(self, clamp: Option<usize>, cx: &App) -> Stateful<Div> {
        let whole = self.text.clone();
        let tooltip = (clamp.is_some() || self.text != self.brief).then_some(whole);
        let brief = self.brief.clone();
        self.frame(cx)
            .when_some(tooltip, |line, whole| {
                line.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(whole.clone()).build(window, cx)
                })
            })
            .child(
                div()
                    .min_w_0()
                    .when_some(clamp, |line, lines| line.line_clamp(lines).text_ellipsis())
                    .child(brief),
            )
    }
}

impl Target {
    /// The control in `form`, with the shared button frame and the action's
    /// icon, reading Opened in its own bounds while a click's confirmation
    /// lasts (in its tooltip as an icon). Labelled, it shows the action alone
    /// ("Open in default app"), under the heading or caption that names its
    /// side. In either form its name leads with that label: on Linux
    /// [`Offer::accessible_name`], which adds the side and the type the
    /// application receives, as the tooltip names the type; macOS callers
    /// may name it more fully.
    pub(crate) fn button(&self, form: Form, cx: &App) -> Button {
        let label = self.offer.label();
        let (shown, tooltip) = match form {
            Form::Labelled => (label, self.offer.tooltip()),
            Form::Icon => ("", self.offer.icon_tooltip()),
        };
        let target = self.clone();
        let button = copy_feedback::feedback_button(self.mark.clone(), OPEN, shown, tooltip, cx)
            .on_click(move |_, window, cx| {
                let opener = target.opener.owner.clone();
                let _ = opener.update(cx, |this, cx| {
                    this.open_captured_bytes(target.clone(), window, cx)
                });
            });
        #[cfg(target_os = "linux")]
        let button = button.accessibility_label(self.offer.accessible_name(label, Some(self.side)));
        #[cfg(not(target_os = "linux"))]
        let button = button.accessibility_label(label);
        button
    }

    /// Why its last click in this showing opened nothing, for a caller that
    /// draws the short line itself ([`Answer::short_line`]), as an image
    /// side's band does beneath its caption and a document's header does
    /// through [`header_ends`].
    pub(crate) fn answered(&self, cx: &App) -> Option<Answer> {
        let note = cx
            .try_global::<Notes>()?
            .notes
            .iter()
            .rev()
            .find(|note| note.mark == self.mark && note.showing == self.opener.showing)?;
        Some(Answer {
            id: note.id,
            key: format!("{}/note", self.mark.control()),
            failed: note.failed,
            text: note.text.clone(),
            brief: note.brief.clone(),
        })
    }

    /// The control's answer to one click: Opened on the control and one
    /// polite announcement, or the line saying why nothing opened.
    fn answer(self, outcome: Result<(), NotOpened>, window: &mut Window, cx: &mut App) {
        let (failed, text, brief) = match outcome {
            Ok(()) => {
                copy_feedback::confirm(window, cx, Some(self.mark), OPENED);
                return;
            }
            #[cfg(any(target_os = "linux", test))]
            Err(NotOpened::Dismissed) => (false, DISMISSED.into(), DISMISSED.into()),
            Err(NotOpened::Failed(failure)) => (
                true,
                format!(
                    "Could not open {}. {}.",
                    self.side,
                    failure.sentence().trim_end_matches('.')
                ),
                failure.category.short_line(),
            ),
        };
        let notes = cx.default_global::<Notes>();
        notes.count = notes.count.wrapping_add(1);
        let id = notes.count;
        notes.notes.push_back(Note {
            id,
            mark: self.mark,
            showing: self.opener.showing,
            failed,
            text: text.into(),
            brief: brief.into(),
        });
        while notes.notes.len() > NOTES_KEPT {
            notes.notes.pop_front();
        }
        window.refresh();
    }

    /// Drops this control's line before another click on it.
    fn forget(&self, cx: &mut App) {
        if let Some(notes) = cx.try_global::<Notes>()
            && notes
                .notes
                .iter()
                .any(|note| note.mark == self.mark && note.showing == self.opener.showing)
        {
            cx.global_mut::<Notes>()
                .notes
                .retain(|note| note.mark != self.mark || note.showing != self.opener.showing);
        }
    }
}

/// How a document's side header ends beneath its controls, which
/// [`header_ends`] draws beneath the header's own padding, which is the gap
/// above a line of its own: the header's inset either side, its padding beneath
/// a drawn line and its rule beneath, with a rule on its right where it has one.
#[derive(Clone, Copy)]
pub(crate) struct HeaderEnd {
    pub(crate) inset: Pixels,
    pub(crate) bottom: Pixels,
    pub(crate) right_rule: bool,
    pub(crate) border: u32,
}

/// The ends of a document's side headers, side by side, each with the line
/// saying why its last click in this showing opened nothing (`answers`, one
/// per header) as its header's last line, beneath all of its controls, and
/// the sides' `body` beneath them: the short line an image side's band shows
/// ([`Answer::short_line`]), with its whole answer in its tooltip, its
/// accessible name and its announcement. Content stays whole before labels
/// do: the page, diagram or model in `body` keeps its usable minimum
/// ([`crate::image_compare::STAGE_MIN`]) first, and the line shows every line
/// it wraps to where the room beneath the headers' controls holds them above
/// that minimum, otherwise the whole lines that room holds above it, the last
/// ending in an ellipsis, so it always ends at a whole line. It keeps one
/// whole line even where that room holds none, so a click always answers on
/// screen: the content then gives way below its minimum, as an image does
/// beneath its band ([`crate::image_compare`]), and where the side is
/// shorter than that line with its padding and rule, the padding goes first.
/// The ends and `body` are laid out for the room the side leaves them in the
/// frame they are drawn ([`crate::in_frame`]), so neither the first frame
/// after an answer nor one after a resize draws the line for another room.
/// The line once showed the whole answer, scrolling inside a scaled 100 px
/// that cut a line at 13 pt in a 640 × 680 window, and then took every line
/// the clip left it, which left a PDF page no height at 18 pt in a 461 × 490
/// window; then it drew none where the room held none above the minimum, so
/// a failed open in the rewrite review's 100 px box, the Stashes dialog's
/// 120 px floor or Compare at 18 pt in a 461 × 490 window changed nothing on
/// screen. Tests find the line as `<control id>/note`, the body as
/// `<name>/body` and the region's frames under `name`.
pub(crate) fn header_ends(
    name: &'static str,
    answers: Vec<Option<Answer>>,
    end: HeaderEnd,
    body: AnyElement,
) -> crate::in_frame::InFrame {
    crate::in_frame::in_frame(move |bounds, window, cx| {
        let size = appearance::ui_text(11.);
        let line = crate::image_compare::text_line(size, window);
        let sides = answers.len().max(1) as f32;
        let right = if end.right_rule { px(1.) } else { px(0.) };
        let width = bounds.size.width / sides - end.inset * 2. - right;
        // The lines the room beneath the controls holds above the content's
        // minimum, above the padding beneath a drawn line and the rule.
        let room = bounds.size.height
            - appearance::ui_size(crate::image_compare::STAGE_MIN)
            - end.bottom
            - px(1.);
        let fit = (room / line).floor().max(0.) as usize;
        let ends = answers.into_iter().map(|answer| {
            let header = div()
                .flex_1()
                .min_w_0()
                .px(end.inset)
                .border_b_1()
                .when(end.right_rule, |header| header.border_r_1())
                .border_color(rgb(end.border));
            let Some(answer) = answer else {
                return header;
            };
            let natural = crate::image_compare::wrapped_lines(&answer.brief, size, width, window);
            // Always one whole line: the click answers on screen, and the
            // content gives way below its minimum for it, as an image does
            // beneath its band.
            let shown = natural.min(fit).max(1);
            let clamp = (natural > shown).then_some(shown);
            // Where the side is shorter than the line with its padding and
            // rule, the padding goes first.
            let padded = bounds.size.height >= line * shown as f32 + end.bottom + px(1.);
            header
                .when(padded, |header| header.pb(end.bottom))
                .child(crate::in_frame::probed(
                    answer.short_line(clamp, cx),
                    "header-note",
                ))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex().min_w_0().children(ends))
            .child(crate::in_frame::probed(
                div()
                    .debug_selector(move || format!("{name}/body"))
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(body),
                "body",
            ))
            .into_any_element()
    })
    .named(name)
}

impl GitTurtle {
    /// Opens `target`'s captured bytes off the UI thread and answers on its
    /// control. Nothing here retries or opens anything else.
    fn open_captured_bytes(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        target.forget(cx);
        window.refresh();
        let opening = match queued_outcome() {
            Some(outcome) => Task::ready(outcome),
            None => self.launch(&target, cx),
        };
        cx.spawn_in(window, async move |this, cx| {
            let outcome = opening.await;
            let _ = this.update_in(cx, |_, window, cx| target.answer(outcome, window, cx));
        })
        .detach();
    }

    #[cfg(target_os = "linux")]
    fn launch(&mut self, target: &Target, cx: &mut Context<Self>) -> Task<Result<(), NotOpened>> {
        self.open_in_default_app(Arc::clone(&target.bytes), target.offer, cx)
    }

    #[cfg(not(target_os = "linux"))]
    fn launch(&mut self, target: &Target, cx: &mut Context<Self>) -> Task<Result<(), NotOpened>> {
        self.quick_look(Arc::clone(&target.bytes), target.name.clone(), cx)
    }
}

#[cfg(test)]
thread_local! {
    /// The outcomes tests queued for their next clicks, oldest first.
    static QUEUED: std::cell::RefCell<VecDeque<Result<(), NotOpened>>> = Default::default();
}

/// In tests, the outcome a test queued for this click ([`tests::queue`]), so
/// no test launches anything and one that clicks without queueing fails; in
/// other builds, none.
fn queued_outcome() -> Option<Result<(), NotOpened>> {
    #[cfg(test)]
    {
        Some(
            QUEUED
                .with(|queued| queued.borrow_mut().pop_front())
                .expect("a test that clicks an open control queues its outcome"),
        )
    }
    #[cfg(not(test))]
    {
        None
    }
}

/// Removes copies that sessions which have ended left behind, off the UI
/// thread, and this session's own copies when the app quits.
#[cfg(target_os = "linux")]
pub(crate) fn init(cx: &mut gpui_kit::App) {
    cx.background_executor()
        .spawn(async {
            if let Some(store) = store::shared() {
                let _ = store.remove_stale();
            }
        })
        .detach();
    cx.on_app_quit(|cx| {
        cx.background_executor().spawn(async {
            if let Some(store) = store::shared() {
                store.close();
            }
        })
    })
    .detach();
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn init(_cx: &mut gpui_kit::App) {}

#[cfg(target_os = "linux")]
impl crate::GitTurtle {
    /// Writes the private copy on the serialized background writer and hands
    /// it to the desktop off the UI thread; the task ends with what opened or
    /// why nothing did.
    fn open_in_default_app(
        &mut self,
        bytes: std::sync::Arc<[u8]>,
        offer: Offer,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Task<Result<(), NotOpened>> {
        let kind = offer.kind;
        let slot = match crate::rich_preview::ExternalPreviewSlot::reserve() {
            Ok(slot) => slot,
            Err(error) => return Task::ready(Err(format!("{error:#}").into())),
        };
        let copy = self.preferences_writer.submit_read(move || {
            store::shared()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "The user cache folder is unavailable: neither XDG_CACHE_HOME nor HOME names an absolute path."
                    )
                })?
                .write(&bytes, kind)
        });
        cx.spawn(async move |_, cx| match copy.await {
            Ok(Ok(copy)) => {
                let executor = cx.background_executor().clone();
                executor
                    .clone()
                    .spawn(launch::open(copy, kind, slot, executor))
                    .await
            }
            Ok(Err(error)) => Err(copy_refused(&error).into()),
            Err(_) => Err("The copy stopped before it was written.".into()),
        })
    }
}

/// Why the private copy was not written, in its category: the store's size
/// limit is `TooLarge` and anything else `Other`. The sentence is the
/// store's own.
#[cfg(target_os = "linux")]
fn copy_refused(error: &anyhow::Error) -> Failure {
    let category = if error.is::<store::TooLarge>() {
        Category::TooLarge
    } else {
        Category::Other
    };
    Failure::new(category, format!("{error:#}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use ::core::prelude::v1::test;

    /// Queues what the next click on an open control reports, in place of
    /// launching anything.
    pub(crate) fn queue(outcome: Result<(), NotOpened>) {
        QUEUED.with(|queued| queued.borrow_mut().push_back(outcome));
    }

    /// The text of the newest line beneath an open control, and whether it
    /// is a failure.
    pub(crate) fn latest_note(cx: &App) -> Option<(String, bool)> {
        let note = cx.try_global::<Notes>()?.notes.back()?;
        Some((note.text.to_string(), note.failed))
    }

    #[cfg(target_os = "linux")]
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
            }
        }
        !crc
    }

    /// A 1 × 1 PNG, with `text` in a tEXt chunk when given.
    #[cfg(target_os = "linux")]
    pub(crate) fn png(text: Option<&[u8]>) -> Vec<u8> {
        let chunk = |kind: &[u8], data: &[u8]| {
            let body = [kind, data].concat();
            [
                &(data.len() as u32).to_be_bytes()[..],
                &body,
                &crc32(&body).to_be_bytes(),
            ]
            .concat()
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", b"\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0"));
        if let Some(text) = text {
            png.extend(chunk(b"tEXt", &[b"Comment\0".as_slice(), text].concat()));
        }
        let pixels = b"\x78\x01\x01\x05\0\xfa\xff\0\0\0\0\0\0\x05\0\x01";
        png.extend(chunk(b"IDAT", pixels));
        png.extend(chunk(b"IEND", b""));
        png
    }

    /// The refusal the launcher reports when GIO types a copy offered as
    /// `kind` ("a PNG image") as an SVG document, with its category.
    pub(crate) fn seen_as_svg(kind: &str) -> NotOpened {
        NotOpened::Failed(Failure::new(
            Category::SeenAs("an SVG document".into()),
            format!(
                "The desktop would open this copy as an SVG document (image/svg+xml) rather than as {kind}, so nothing was opened."
            ),
        ))
    }

    /// Clicks After's open control `control` in a document's header, whose
    /// open fails, and checks the answer on screen, `at` naming the case: the
    /// line `note` beneath the header's controls shows one whole line inside
    /// each of `clips`, the box the comparison is drawn in and what scrolls
    /// it, its whole answer in its tooltip, though the room beneath the
    /// controls holds no line above the content `body`'s minimum, which gives
    /// way for it. Where the room held none, the line once drew none and the
    /// click changed nothing there.
    #[cfg(target_os = "linux")]
    pub(crate) fn a_failed_open_answers_on_one_line(
        cx: &mut gpui_kit::VisualTestContext,
        at: &str,
        [control, note, body]: [&'static str; 3],
        clips: &[&'static str],
    ) {
        use crate::tags::tests::{draw, hovered_tooltips, rendered};
        let slack = px(0.5);
        queue(Err(seen_as_svg("a PDF document")));
        let center = rendered(cx, control).center();
        cx.simulate_click(center, gpui::Modifiers::none());
        cx.run_until_parked();
        draw(cx);
        let (whole, failed) = cx.read(latest_note).expect("a line says why");
        assert!(
            failed && whole.starts_with("Could not open After. The desktop would open"),
            "{at}: the answer {whole:?}"
        );
        let (line, body) = (rendered(cx, note), rendered(cx, body));
        let (line_height, minimum) = cx.update(|window, _| {
            (
                crate::image_compare::text_line(appearance::ui_text(11.), window),
                appearance::ui_size(crate::image_compare::STAGE_MIN),
            )
        });
        assert!(
            (line.size.height - line_height).abs() <= slack,
            "{at}: the line {line:?} shows one whole line of {line_height:?}"
        );
        assert!(
            body.size.height < minimum - slack,
            "{at}: the content {body:?} keeps its minimum {minimum:?}, so the room held a line"
        );
        for &selector in clips {
            let clip = rendered(cx, selector);
            assert!(
                line.top() >= clip.top() - slack
                    && line.bottom() <= clip.bottom() + slack
                    && line.left() >= clip.left() - slack
                    && line.right() <= clip.right() + slack,
                "{at}: the line {line:?} lies whole in {selector} {clip:?}"
            );
        }
        assert_eq!(
            hovered_tooltips(cx, note),
            1,
            "{at}: the line's tooltip holds the whole answer"
        );
    }

    /// After's open control, whose clicks a test answers itself.
    fn after_control() -> Target {
        Target {
            opener: Opener::new(WeakEntity::new_invalid(), Showing::next()),
            mark: Mark::new(("image-system-preview", 1usize), "captured"),
            side: "After",
            bytes: Arc::from(&b"captured"[..]),
            offer: Offer {
                #[cfg(target_os = "linux")]
                kind: passive::Kind::Png,
            },
            #[cfg(not(target_os = "linux"))]
            name: "captured".into(),
        }
    }

    /// The short line `outcome` draws beneath `target`, and its whole answer.
    fn answered_lines(
        target: &Target,
        outcome: Result<(), NotOpened>,
        cx: &mut gpui_kit::VisualTestContext,
    ) -> (String, String) {
        cx.update(|window, cx| target.clone().answer(outcome, window, cx));
        cx.update(|_, cx| {
            let answer = target.answered(cx).expect("the click's answer");
            (answer.brief.to_string(), answer.text.to_string())
        })
    }

    /// A failure's short line is written from its category and leads with
    /// its reason, never from words found in its sentence: the line once
    /// found "Seen as …" after "would open this copy as" in the launcher's
    /// sentence, which a band in a 461 px window's half never reached
    /// ("Could not / open After." at 18 pt). A copy the desktop would type
    /// as something else reads "Seen as …, so not opened" whatever its
    /// sentence says, a type without a default application names that type,
    /// and any other failure, the launcher's refusal sentence without its
    /// category among them, fails closed to the generic line. The whole
    /// answer keeps the sentence, after the side.
    #[gpui::test]
    fn a_failure_line_is_written_from_its_category(cx: &mut gpui_kit::TestAppContext) {
        let (_app, cx) = crate::copy_feedback::tests::history(cx, Vec::new());
        let target = after_control();
        let refusal = "The desktop would open this copy as an SVG document (image/svg+xml) rather than as a PNG image, so nothing was opened.";
        let cases = [
            (
                Failure::new(Category::SeenAs("an SVG document".into()), "Refused."),
                "Seen as an SVG document, so not opened",
            ),
            (
                Failure::new(Category::SeenAs("application/x-php".into()), refusal),
                "Seen as application/x-php, so not opened",
            ),
            (
                Failure::new(
                    Category::NoDefaultApplication("image/png".into()),
                    "No default application is set for image/png, so nothing was opened.",
                ),
                "No default app for image/png",
            ),
            (
                Failure::new(
                    Category::TooLarge,
                    "The captured file exceeds the 32 MiB limit for opening it in another app.",
                ),
                "Over 32 MiB, so not opened",
            ),
            (Failure::from(refusal), NOT_OPENED),
            (
                Failure::from("The desktop portal refused it: denied."),
                NOT_OPENED,
            ),
        ];
        for (failure, short) in cases {
            let sentence = failure.sentence().trim_end_matches('.').to_owned();
            let (brief, whole) = answered_lines(&target, Err(NotOpened::Failed(failure)), cx);
            assert_eq!(brief, short);
            assert_eq!(whole, format!("Could not open After. {sentence}."));
        }
        let (brief, whole) = answered_lines(&target, Err(NotOpened::Dismissed), cx);
        assert_eq!((brief.as_str(), whole.as_str()), (DISMISSED, DISMISSED));
    }

    /// The private copy's size limit is the one refusal before the launcher
    /// with a category of its own; every other way the copy can fail stays
    /// `Other`. Each keeps the store's sentence for the whole answer.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_copy_size_limit_has_its_own_category() {
        let folder = tempfile::tempdir().unwrap();
        let store = store::Store::new(folder.path().join("captured-copies"));
        let large = vec![b'a'; gitturtle_preview::MAX_INPUT_BYTES + 1];
        let Err(error) = store.write(&large, passive::Kind::Text) else {
            panic!("a copy over the limit is refused");
        };
        let refused = copy_refused(&error);
        assert_eq!(refused.category, Category::TooLarge);
        assert_eq!(
            refused.sentence(),
            "The captured file exceeds the 32 MiB limit for opening it in another app."
        );
        let other = copy_refused(&anyhow::anyhow!("The user cache folder is unavailable."));
        assert_eq!(other.category, Category::Other);
        assert_eq!(other.sentence(), "The user cache folder is unavailable.");
        store.close();
    }

    /// The short line beneath an image side's caption, and the line in a
    /// document's header ([`header_ends`]) in each form it takes there
    /// (every line, the lines that fit, one), shows its reason first and is
    /// named, as it is announced, by the whole answer, with the side it
    /// could not open: "Could not open After. The desktop would open …".
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn a_short_line_is_named_by_the_whole_answer(cx: &mut gpui_kit::TestAppContext) {
        use gpui::Element as _;
        let (_app, cx) = crate::copy_feedback::tests::history(cx, Vec::new());
        let opener = Opener::new(WeakEntity::new_invalid(), Showing::next());
        let png: Arc<[u8]> = Arc::from(png(None));
        let target = cx.update(|_, _| {
            let offer = offer(&png, &|| Ok(())).unwrap().expect("offered");
            opener.target(
                ("image-system-preview", 1usize),
                "After",
                &png,
                std::path::Path::new("captured"),
                offer,
            )
        });
        let reason = "The desktop would open this copy as an SVG document (image/svg+xml) rather than as a PNG image, so nothing was opened.";
        cx.update(|window, cx| {
            target
                .clone()
                .answer(Err(seen_as_svg("a PNG image")), window, cx)
        });
        let mut written = Vec::new();
        let viewport = cx.update(|window, _| window.viewport_size());
        cx.draw(gpui::point(px(0.), px(0.)), viewport, |_, cx| {
            let answer = target.answered(cx).expect("the click's answer");
            assert_eq!(answer.brief(), "Seen as an SVG document, so not opened");
            let lines = [
                target.answered(cx).unwrap().short_line(None, cx),
                target.answered(cx).unwrap().short_line(Some(2), cx),
                answer.short_line(Some(1), cx),
            ];
            for line in lines {
                let element = line.into_element();
                let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("a role"));
                element.write_a11y_info(&mut node);
                // Read as it is built: the window's next draw starts its live
                // regions afresh, and these lines are drawn outside it.
                let id = gpui::Element::id(&element).expect("the line's ID");
                let announced = crate::native_accessibility::written::announced(&id.to_string());
                written.push((announced, node));
            }
            div()
        });
        assert_eq!(written.len(), 3, "every form was built");
        for (announced, node) in written {
            assert_eq!(node.role(), gpui::accesskit::Role::Alert);
            assert_eq!(
                node.label(),
                Some(format!("Could not open After. {reason}").as_str())
            );
            // A failure interrupts: it is announced at once, as a whole.
            assert_eq!(
                announced,
                Some(crate::native_accessibility::written::Announced::ASSERTIVE)
            );
        }
    }

    /// Every open control is named in one form that starts with what it
    /// shows (WCAG 2.5.3, label in name) and then names the side its label
    /// leaves to the heading beside it and what the application receives:
    /// an image side's control, which once read "Open Before in default app"
    /// in a row of its own, as much as a document's, and the icon a narrow
    /// column draws in its place, whose label is in its name and tooltip.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn every_open_control_is_named_by_its_label_then_what_it_hands_over(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (_app, cx) = crate::copy_feedback::tests::history(cx, Vec::new());
        let opener = Opener::new(WeakEntity::new_invalid(), Showing::next());
        let png: Arc<[u8]> = Arc::from(&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..]);
        let pdf: Arc<[u8]> = Arc::from(&b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n"[..]);
        let cases = [
            (
                ("image-system-preview", 0usize),
                &png,
                "Before",
                Form::Labelled,
                "Open in default app: Before, a read-only copy as a PNG image",
            ),
            (
                ("system-preview", 1usize),
                &pdf,
                "After",
                Form::Labelled,
                "Open in default app: After, a read-only copy as a PDF document",
            ),
            (
                ("model-external", 1usize),
                &pdf,
                "After",
                Form::Icon,
                "Open in default app: After, a read-only copy as a PDF document",
            ),
        ];
        let buttons = cx.update(|_, cx| {
            cases
                .iter()
                .map(|(control, bytes, side, form, _)| {
                    let offer = offer(bytes, &|| Ok(())).unwrap().expect("offered");
                    let name = std::path::Path::new("captured");
                    opener
                        .target(*control, side, bytes, name, offer)
                        .button(*form, cx)
                })
                .collect::<Vec<_>>()
        });
        let names = crate::worktrees::ring_room_tests::row_nodes(cx, buttons)
            .into_iter()
            .map(|node| node.label().map(str::to_owned));
        for ((.., expected), name) in cases.iter().zip(names) {
            let name = name.expect("the control is named");
            assert!(
                name.starts_with("Open in default app"),
                "{name:?} starts with what it shows"
            );
            assert_eq!(name, *expected);
        }
    }
}
