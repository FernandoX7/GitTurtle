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

/// The open control's label on macOS, where Quick Look shows the copy.
pub(crate) const QUICK_LOOK: &str = "System preview";

impl Offer {
    /// The control's label, naming what happens on this platform.
    pub(crate) fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            QUICK_LOOK
        } else {
            "Open in default app"
        }
    }
}

/// The accessible name of an open control labelled `label`, in one form on
/// every platform and surface: the visible label first, so speech input finds
/// the control by what it shows (WCAG 2.5.3), then the `side`, which the label
/// leaves to the heading or caption beside it, and then the read-only `copy`
/// the click hands over. On Linux that names the type the default application
/// receives (`Offer::copy`): "Open in default app: After, a read-only copy
/// as a PDF document" in a document's header, "Open in default app: Before, a
/// read-only copy as a PNG image" in an image side's caption. On macOS it
/// names what the surface shows (`quick_look_copy`): "System preview:
/// Source, a read-only copy of the captured image".
pub(crate) fn accessible_name(label: &str, side: Option<&str>, copy: &str) -> String {
    match side {
        Some(side) => format!("{label}: {side}, {copy}"),
        None => format!("{label}: {copy}"),
    }
}

/// What a click hands Quick Look, for a control on a surface that shows
/// `what` ("image", "PDF", "model", or "file" for any other document):
/// "a read-only copy of the captured image". Linux builds it only for the
/// tests that pin macOS's names.
#[cfg(any(not(target_os = "linux"), test))]
pub(crate) fn quick_look_copy(what: &str) -> String {
    format!("a read-only copy of the captured {what}")
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

    /// What a click hands the default application, as an open control's
    /// [`accessible_name`] ends: "a read-only copy as a PDF document".
    fn copy(self) -> String {
        format!("a read-only copy as {}", self.kind.description())
    }

    /// The icon form's tooltip, which leads with the label it does not show
    /// and names what the application receives: "Open in default app: a
    /// read-only copy as a PDF document".
    fn icon_tooltip(self) -> String {
        accessible_name(self.label(), None, &self.copy())
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
    /// The scrolling container the comparison is drawn in, if it reveals
    /// its controls, and the margin it reveals them with
    /// ([`Self::revealed_by`]).
    reveal: Option<(crate::focus_reveal::FocusReveal, Pixels)>,
}

impl Opener {
    pub(crate) fn new(owner: WeakEntity<GitTurtle>, showing: Showing) -> Self {
        Self {
            owner,
            showing,
            reveal: None,
        }
    }

    /// The opener of a comparison drawn inside `reveal`'s scrolling
    /// container, whose open controls are among the container's controls
    /// ([`Target::control`]): the keyboard reveals each with its ring,
    /// `margin`, and one holding focus counts as the container's
    /// ([`crate::focus_reveal::FocusReveal::holds_focus`]). Unknown to the
    /// Stashes detail, a focused open control was not revealed, and the note
    /// that a pause had ended scrolled it out of view.
    pub(crate) fn revealed_by(
        mut self,
        reveal: crate::focus_reveal::FocusReveal,
        margin: Pixels,
    ) -> Self {
        self.reveal = Some((reveal, margin));
        self
    }

    /// The showing its controls belong to.
    pub(crate) fn showing(&self) -> Showing {
        self.showing
    }

    /// The open control `control` for the captured `bytes` of the `side`
    /// ("Before", "After", "Source") stored as `name`, which `offer` offers,
    /// on a surface that shows `what` (`quick_look_copy`). Linux never uses
    /// `name` or `what`: its name says the type the bytes are handed over as.
    pub(crate) fn target(
        &self,
        control: impl Into<ElementId>,
        side: &'static str,
        bytes: &Arc<[u8]>,
        name: &std::path::Path,
        what: &'static str,
        offer: Offer,
    ) -> Target {
        // The bytes' allocation and length name what was opened, so another
        // file's control in the same place is at rest.
        let subject = (Arc::as_ptr(bytes) as *const u8 as usize, bytes.len());
        #[cfg(target_os = "linux")]
        let _ = (name, what);
        let control = control.into();
        Target {
            opener: self.clone(),
            mark: Mark::new(control.clone(), subject),
            control,
            side,
            bytes: Arc::clone(bytes),
            offer,
            #[cfg(not(target_os = "linux"))]
            name: name.to_path_buf(),
            #[cfg(not(target_os = "linux"))]
            what,
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
    /// The control's id, which its button and its mark carry.
    control: ElementId,
    side: &'static str,
    bytes: Arc<[u8]>,
    offer: Offer,
    #[cfg(not(target_os = "linux"))]
    name: std::path::PathBuf,
    /// What the surface shows, which macOS's name says Quick Look receives.
    #[cfg(not(target_os = "linux"))]
    what: &'static str,
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
                    line.alert(self.text.clone())
                } else {
                    line.role(Role::Status)
                        .polite()
                        .aria_label(self.text.clone())
                }
            })
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
    /// side. In either form it is named in one form on every platform
    /// ([`accessible_name`]): that label, the side, and what the click hands
    /// over, the type the application receives on Linux, as the tooltip
    /// names it, and on macOS what the surface shows.
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
        // Tests read each frame's form where the control is drawn
        // (`in_frame::probe`).
        crate::in_frame::probed(button, "open-control").accessibility_label(accessible_name(
            label,
            Some(self.side),
            &self.copy(),
        ))
    }

    /// [`Self::button`] as a comparison places it. Where the opener's
    /// comparison lies in a scrolling container that reveals its controls
    /// ([`Opener::revealed_by`]), the control is one of them: it tracks the
    /// container's handle for its id and reports where it lies as each frame
    /// prepaints it, so the keyboard reveals it with its ring and the
    /// container knows it holds focus. Its wrapper keeps the button's own
    /// size and does not shrink, as the button does not.
    pub(crate) fn control(&self, form: Form, cx: &App) -> AnyElement {
        let button = self.button(form, cx);
        let Some((reveal, margin)) = &self.opener.reveal else {
            return button.into_any_element();
        };
        let focus = reveal.focus(self.control.clone(), cx);
        div()
            .flex_shrink_0()
            .on_children_prepainted(reveal.control(focus.clone(), *margin))
            .child(button.track_focus(&focus))
            .into_any_element()
    }

    /// [`Self::control`] in a side's own header, whose region also reveals
    /// it ([`SideHeader::track`]): the button keeps the header's focus
    /// handle, and where the opener's container reveals its controls too
    /// (a narrow model or PDF page side in Stashes), the wrapper tracks the
    /// container's handle around it, so focus on the button lies in both
    /// and each reveals it in its own scroll.
    pub(crate) fn held_control(&self, form: Form, held: &SideHeader, cx: &mut App) -> AnyElement {
        let button = held.track(self.control.clone(), self.button(form, cx), cx);
        let Some((reveal, margin)) = &self.opener.reveal else {
            return button.into_any_element();
        };
        let focus = reveal.focus(self.control.clone(), cx);
        div()
            .flex_shrink_0()
            .track_focus(&focus)
            .on_children_prepainted(reveal.control(focus, *margin))
            .child(button)
            .into_any_element()
    }

    /// What its click hands over, as its accessible name ends.
    fn copy(&self) -> String {
        #[cfg(target_os = "linux")]
        {
            self.offer.copy()
        }
        #[cfg(not(target_os = "linux"))]
        {
            quick_look_copy(self.what)
        }
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
///
/// A side whose header is its own, a 3D model's or the PDF page view's,
/// passes it as `side` ([`OwnHeader`]), and it is laid out above its line in
/// the room the side has in the frame drawn, in one order, content first.
/// The rule beneath the header always keeps its pixel, and the line its one
/// whole line with its padding where a click answered; the content keeps its
/// usable minimum ([`crate::image_compare::STAGE_MIN`]) wherever that leaves
/// the header its floor, room for one control whole with its focus ring's
/// room or the header's own padding above and below, whichever is larger,
/// and otherwise what is left beyond that floor; the header takes the rest,
/// whole where that holds its controls and otherwise in a region that tall
/// which scrolls, cued by a scrollbar and revealing the control Tab moves
/// onto, so its controls stay inside the preview and the line whole beneath
/// them. A side's `foot`, the model's
/// details, gives way before the header does. In a narrow side, at 13 and
/// 18 pt in a 461 × 490 window and at 18 pt in 640 × 680, the controls once
/// wrapped past the preview's foot and took the line out of view; then the
/// floor was a control's height alone, which cut the ring of the control
/// Tab moved onto in a side as short as the rewrite review's scaled 100 px
/// file box or the Stashes dialog's 120 px floor. Tests find that region as
/// `<name>/header`.
pub(crate) fn header_ends(
    name: &'static str,
    side: Option<OwnHeader>,
    answers: Vec<Option<Answer>>,
    end: HeaderEnd,
    body: AnyElement,
) -> crate::in_frame::InFrame {
    let Some(OwnHeader {
        header,
        foot,
        padding,
        held,
    }) = side
    else {
        return ends(name, answers, end, body);
    };
    let answered = answers.iter().any(Option::is_some);
    crate::in_frame::in_frame(move |bounds, window, cx| {
        let height = bounds.size.height;
        // The rule's pixel, and the line's one whole line with its padding
        // where a click answered.
        let line = crate::image_compare::text_line(appearance::ui_text(11.), window);
        let reserve = (px(1.) + if answered { line + end.bottom } else { px(0.) }).min(height);
        // The content keeps its minimum, and the header the rest, where
        // that is at least its floor; otherwise the header keeps its floor,
        // so each control Tab moves onto shows whole with its ring, and the
        // content the rest. The floor holds the scaled 28 px control and the
        // larger of what lies above and below it: in a header that scrolls,
        // its ring's room, which the reveal keeps clear of the region's
        // edges, and in a header of one row, the header's own padding, so
        // that header shows whole there, unscrolled. Both are counted as
        // GPUI lays them out, snapped to device pixels: unsnapped, the
        // model's `py_1` at 18 pt on a 1× display, 4.5 px drawn as 5, would
        // leave a header of one row a 1 px scroll range. Counting both, the
        // floor once took a PDF page about 6 px more than it needed at 18 pt
        // in a 461 × 490 window.
        let around = (window.pixel_snap(padding.to_pixels(window.rem_size())) * 2.)
            .max(appearance::button_ring_room(cx) * 2.);
        let row = (window.pixel_snap(appearance::ui_size(28.)) + around).min(height - reserve);
        let most =
            (height - reserve - appearance::ui_size(crate::image_compare::STAGE_MIN)).max(row);
        let content = height - reserve - most;
        let cue = crate::tags::ScrollCue::new(held.reveal.scroll(), px(0.));
        let region = div()
            .on_children_prepainted(cue.observe(held.reveal.observe()))
            .id(SharedString::from(format!("{name}/header")))
            .debug_selector(move || format!("{name}/header"))
            .max_h(most)
            .overflow_y_scroll()
            .track_scroll(held.reveal.scroll())
            .child(header);
        div()
            .size_full()
            .flex()
            .flex_col()
            // A region that scrolls and clips what it holds for assistive
            // technology, its scrollbar shown while it overflows. It never
            // shrinks, since `most` bounds it and the foot always takes the
            // shortfall: shrinking, it took about a millionth of the foot's
            // share, which GPUI's snapping to device pixels and its scroll
            // range's rounding absorb, so no test reproduces a range from
            // it, but a header that fits must never get one.
            .child(cue.list(region, held.scrollbar).flex_shrink_0())
            .child(
                div()
                    .flex_1()
                    .min_h(reserve + content)
                    .flex()
                    .flex_col()
                    .child(ends(name, answers, end, body)),
            )
            // The foot gives way first, and what it holds gives way with it
            // as a column's child: a scroll area of its own with no minimum
            // height (`OwnHeader`) shrinks with it, so its lines stay
            // within its reach. Clipped by the foot instead, the model's
            // details were cut mid-line where they partly gave way.
            .children(foot.map(|foot| {
                div()
                    .debug_selector(move || format!("{name}/foot"))
                    .flex_shrink(1_000_000.)
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(foot)
            }))
            .into_any_element()
    })
}

/// The width a control in a side's own header has on a line to itself, in a
/// side `side` wide whose header keeps `inset` either side: the side less its
/// right rule, those insets and the gutter the header's region keeps for its
/// scrollbar while it scrolls ([`crate::tags::ScrollCue`]), counted whether
/// or not it scrolls, so a control's form never changes with the scroll it
/// causes. Its focus ring lies in the insets. A labelled control wider than
/// this draws a shorter form, its icon or a shorter reading, its whole label
/// in its tooltip and its accessible name the one it has labelled: in an
/// 88 px side at 13 pt in a 461 × 490 window the model's Copy source, views
/// menu and Linked, and the page view's Read page text, Retry page, page
/// number and Linked pages were cut at the side's edge.
pub(crate) fn header_line_room(side: Pixels, inset: Pixels) -> Pixels {
    let ring = appearance::BUTTON_FOCUS_RING;
    side - px(1.)
        - inset * 2.
        - (gpui_kit::component::scroll::Scrollbar::width() + ring.gap + ring.width)
}

/// A side's own header for [`header_ends`]: the header, what lies beneath
/// the side's content (the model's details), the header's own padding at its
/// top and at its bottom, each, as the header sets it (the page view's
/// `p_2`, the model's `py_1`), which its floor counts as GPUI snaps it, and
/// the header's scrolling state. The foot gives way before the header,
/// shrinking as a column's child, so a foot that can hold more than it
/// shows is a scroll area of its own ([`SideHeader::foot`]), which keeps its
/// lines within its reach as it shrinks to nothing.
pub(crate) struct OwnHeader {
    pub(crate) header: AnyElement,
    pub(crate) foot: Option<AnyElement>,
    pub(crate) padding: Rems,
    pub(crate) held: SideHeader,
}

/// The scrolling state of a side's own header, and of its foot: the header
/// reveals the control Tab moves onto ([`crate::focus_reveal`]), and both
/// are made again for each showing, so the next file's header and foot
/// start at their top.
#[derive(Clone)]
pub(crate) struct SideHeader {
    reveal: crate::focus_reveal::FocusReveal,
    foot: ScrollHandle,
    scrollbar: &'static str,
}

/// One side header's state, with the showing it was made for.
struct HeldSide {
    showing: Showing,
    reveal: crate::focus_reveal::FocusReveal,
    foot: ScrollHandle,
}

/// Each side header's state, by the view drawing it and the side's region.
/// A view's entries go when the view does ([`SideHeader::new`]): they once
/// stayed for the application's life, each closed Stashes dialog or rewrite
/// review leaving its sides' scroll and focus handles behind.
#[derive(Default)]
struct SideHeaders(std::collections::HashMap<(EntityId, &'static str), HeldSide>);
impl Global for SideHeaders {}

impl SideHeader {
    /// The header of side `name` that the view rendering with `cx` draws in
    /// `showing`, revealing in this frame the control focus has moved onto;
    /// its scrollbar is `scrollbar`. The view's headers go when it is
    /// released, as a dialog's view is when the dialog closes.
    pub(crate) fn new<T: 'static>(
        name: &'static str,
        scrollbar: &'static str,
        showing: Showing,
        window: &Window,
        cx: &mut Context<T>,
    ) -> Self {
        let view = cx.entity_id();
        let fresh = || HeldSide {
            showing,
            reveal: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
            foot: ScrollHandle::new(),
        };
        let headers = &mut cx.default_global::<SideHeaders>().0;
        let first = !headers.keys().any(|(held, _)| *held == view);
        let held = headers.entry((view, name)).or_insert_with(fresh);
        if held.showing != showing {
            *held = fresh();
        }
        let (reveal, foot) = (held.reveal.clone(), held.foot.clone());
        if first {
            cx.on_release(move |_, cx| {
                if cx.has_global::<SideHeaders>() {
                    cx.global_mut::<SideHeaders>()
                        .0
                        .retain(|(held, _), _| *held != view);
                }
            })
            .detach();
        }
        reveal.reveal(window, cx);
        Self {
            reveal,
            foot,
            scrollbar,
        }
    }

    /// `area`, what lies beneath the side's content and gives way before
    /// its header (the model's details), as the scroll area of its own that
    /// [`OwnHeader`]'s foot is: with no minimum height, so it shrinks with
    /// the foot to nothing and its lines, with the padding `area` keeps
    /// inside it, stay within its reach, and cued while they overflow by
    /// the always-visible scrollbar `scrollbar`, as a comparable bounded
    /// area's is ([`crate::tags::ScrollCue`]). Shrunk to a sliver, the
    /// model's details once showed a part of their lines with no sign that
    /// the rest scrolled.
    pub(crate) fn foot(&self, area: Stateful<Div>, scrollbar: &'static str) -> Div {
        let cue = crate::tags::ScrollCue::new(&self.foot, px(0.));
        let observe = cue.observe(|_, _, _| {});
        let area = area.min_h_0().overflow_y_scroll().track_scroll(&self.foot);
        cue.list(area, scrollbar)
            .on_children_prepainted(observe)
            .min_h_0()
    }

    /// `button`, which Tab reveals in the header's region, its focus tracked
    /// as `id`.
    pub(crate) fn track(&self, id: impl Into<ElementId>, button: Button, cx: &mut App) -> Button {
        let focus = self.reveal.focus(id, cx);
        let report = self
            .reveal
            .control(focus.clone(), appearance::button_ring_room(cx));
        gpui_kit::base::ElementExt::on_prepaint(
            button.track_focus(&focus),
            move |bounds, window, cx| report(vec![bounds], window, cx),
        )
    }
}

/// The state of the side header `name` that `view` draws, for a test.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn side_header(
    view: EntityId,
    name: &'static str,
    cx: &App,
) -> Option<crate::focus_reveal::FocusReveal> {
    cx.try_global::<SideHeaders>()
        .and_then(|headers| headers.0.get(&(view, name)))
        .map(|held| held.reveal.clone())
}

/// The ends [`header_ends`] draws beneath the headers' controls, with the
/// sides' `body`, laid out for the room beneath those controls.
fn ends(
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
    use ::core::prelude::v1::test;

    /// A narrow side, 88 px wide and a scaled `height` tall, whose own header
    /// holds `controls` icon controls padded `padding` rems above and below
    /// (the PDF page view's `p_2` or the model's `py_1`), above a failed
    /// open's line and content that keeps no room of its own.
    #[cfg(target_os = "linux")]
    struct FloorSide {
        height: f32,
        padding: f32,
        controls: usize,
        showing: Showing,
    }

    #[cfg(target_os = "linux")]
    impl Render for FloorSide {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let held = SideHeader::new(
                "floor-side",
                "floor-side/scrollbar",
                self.showing,
                window,
                cx,
            );
            let mut header = div()
                .debug_selector(|| "floor-side-header".into())
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .px_2()
                .py(rems(self.padding));
            for index in 0..self.controls {
                let control = crate::button(("floor-control", index), "", "plus", false)
                    .debug_selector(move || format!("floor-control-{index}"));
                header = header.child(held.track(("floor-control", index), control, cx));
            }
            let answer = Answer {
                id: 0,
                key: "floor-control-0/note".into(),
                failed: true,
                text: "Could not open After.".into(),
                brief: NOT_OPENED.into(),
            };
            let end = HeaderEnd {
                inset: window.rem_size() * 0.5,
                bottom: window.rem_size() * 0.5,
                right_rule: false,
                border: palette(cx).border,
            };
            div()
                .w(px(88.))
                .h(appearance::ui_size(self.height))
                .flex()
                .flex_col()
                .child(header_ends(
                    "floor-side",
                    Some(OwnHeader {
                        header: header.into_any_element(),
                        foot: None,
                        padding: rems(self.padding),
                        held,
                    }),
                    vec![Some(answer)],
                    end,
                    div().size_full().into_any_element(),
                ))
        }
    }

    /// The probe side with `controls` controls in each side its header keeps
    /// only its floor in, at 13 and 18 pt: the rewrite review's scaled
    /// 100 px file box and the Stashes dialog's scaled 120 px floor, padded
    /// as the model's header (`py_1`) and the PDF page view's (`p_2`). Each
    /// is passed to `check` with its name and the side's header's state.
    #[cfg(target_os = "linux")]
    fn walk_floor_sides(
        cx: &mut gpui_kit::TestAppContext,
        controls: usize,
        check: impl Fn(&mut gpui_kit::VisualTestContext, &str, &crate::focus_reveal::FocusReveal),
    ) {
        cx.update(gpui_kit::init);
        cx.update(|cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        let (side, cx) = cx.add_window_view(|_, _| FloorSide {
            height: 100.,
            padding: 0.25,
            controls,
            showing: Showing::next(),
        });
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for (height, padding) in [(100., 0.25), (120., 0.25), (100., 0.5), (120., 0.5)] {
                side.update(cx, |side, cx| {
                    side.height = height;
                    side.padding = padding;
                    side.showing = Showing::next();
                    cx.notify();
                });
                crate::tags::tests::draw(cx);
                let at = format!("{text} pt in a scaled {height} px box, padded {padding} rem");
                let list = cx
                    .update(|_, cx| side_header(side.entity_id(), "floor-side", cx))
                    .expect("the side's header");
                check(cx, &at, &list);
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A narrow side's own header at the floor [`header_ends`] keeps it, in
    /// the rewrite review's scaled 100 px file box and at the Stashes
    /// dialog's scaled 120 px floor, at 13 and 18 pt beneath a failed open's
    /// line, shows each control focus moves onto whole with its focus ring,
    /// under the PDF page view's padding and the 3D model's, where its
    /// controls wrap and it scrolls. The floor was once a control's height
    /// alone, which held no control's ring.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn a_side_header_s_floor_shows_a_whole_control(cx: &mut gpui_kit::TestAppContext) {
        let ring = appearance::BUTTON_FOCUS_RING.width + appearance::BUTTON_FOCUS_RING.gap;
        walk_floor_sides(cx, 4, |cx, at, list| {
            for index in 0..4usize {
                list.reveal_next_focus();
                cx.update(|window, cx| list.focus(("floor-control", index), cx).focus(window, cx));
                crate::tags::tests::draw(cx);
                let selector = format!("floor-control-{index}").leak();
                let shown = cx.debug_bounds(selector).expect("the control").dilate(ring);
                let held = cx.debug_bounds("floor-side/header").expect("the region");
                assert!(
                    shown.top() >= held.top() - px(0.01)
                        && shown.bottom() <= held.bottom() + px(0.01),
                    "{at}: {selector} and its ring {shown:?} lie whole in the header's region {held:?}"
                );
            }
        });
    }

    /// A header of one row, at the floor [`header_ends`] keeps it in the
    /// same sides, draws whole and unscrolled in a region its own height,
    /// with no scroll range: the floor counts the header's own padding where
    /// that is larger than the focus ring's room, as the page view's `p_2`
    /// is at 13 and 18 pt and the model's `py_1` at 18 pt. The ring's room
    /// alone would leave such a header a range of the padding's excess over
    /// it, 7 px under the page view's padding at 13 pt and 12 px at 18 pt.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn a_one_row_header_shows_whole_at_its_floor(cx: &mut gpui_kit::TestAppContext) {
        walk_floor_sides(cx, 1, |cx, at, list| {
            let held = cx.debug_bounds("floor-side/header").expect("the region");
            let header = cx.debug_bounds("floor-side-header").expect("the header");
            let control = cx.debug_bounds("floor-control-0").expect("the control");
            let range = list.scroll().max_offset().y;
            assert!(
                range == Pixels::ZERO && (header.top() - held.top()).abs() < px(0.01),
                "{at}: the header {header:?} draws unscrolled in its region {held:?}, with no range ({range:?})"
            );
            assert!(
                (header.size.height - held.size.height).abs() < px(0.01),
                "{at}: the region {held:?} is the one row's height {header:?}"
            );
            assert!(
                control.top() >= held.top() && control.bottom() <= held.bottom(),
                "{at}: the control {control:?} lies whole in the region {held:?}"
            );
        });
    }

    /// A side 240 px wide and 400 px tall whose own header holds one
    /// control, above content that keeps no room of its own and a foot of
    /// `lines` lines in a scroll area at most 48 px tall, as a model's
    /// details are.
    #[cfg(target_os = "linux")]
    struct FootSide {
        lines: usize,
        showing: Showing,
    }

    #[cfg(target_os = "linux")]
    impl Render for FootSide {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let held =
                SideHeader::new("foot-side", "foot-side/scrollbar", self.showing, window, cx);
            let control = crate::button("foot-control", "", "plus", false);
            let header = div()
                .px_2()
                .py(rems(0.25))
                .child(held.track("foot-control", control, cx));
            let area = div()
                .id("foot-area")
                .debug_selector(|| "foot-area".into())
                .max_h(px(48.))
                .child(
                    div()
                        .debug_selector(|| "foot-lines".into())
                        .p_2()
                        .children((0..self.lines).map(|line| div().child(format!("Line {line}")))),
                );
            let end = HeaderEnd {
                inset: window.rem_size() * 0.5,
                bottom: window.rem_size() * 0.5,
                right_rule: false,
                border: palette(cx).border,
            };
            div()
                .w(px(240.))
                .h(px(400.))
                .flex()
                .flex_col()
                .child(header_ends(
                    "foot-side",
                    Some(OwnHeader {
                        header: header.into_any_element(),
                        foot: Some(
                            held.foot(area, "foot-side/foot-scrollbar")
                                .into_any_element(),
                        ),
                        padding: rems(0.25),
                        held,
                    }),
                    vec![None],
                    end,
                    div().size_full().into_any_element(),
                ))
        }
    }

    /// A side's foot, the model's details, draws an always-visible
    /// scrollbar at its edge only while its lines overflow its room, and
    /// none where they fit, as they come and go: a foot that showed a part
    /// of its lines once gave no sign that the rest scrolled.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn a_side_s_foot_cues_its_scroll_only_while_it_overflows(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        let (side, cx) = cx.add_window_view(|_, _| FootSide {
            lines: 1,
            showing: Showing::next(),
        });
        let bar = gpui_kit::component::scroll::Scrollbar::width();
        for (lines, overflows) in [(1, false), (12, true), (1, false)] {
            side.update(cx, |side, cx| {
                side.lines = lines;
                cx.notify();
            });
            crate::tags::tests::draw(cx);
            let area = cx.debug_bounds("foot-area").expect("the foot's area");
            let shown = cx.debug_bounds("foot-lines").expect("its lines");
            assert_eq!(
                shown.size.height > area.size.height + px(0.005),
                overflows,
                "{lines} lines, {shown:?}, in the foot {area:?}"
            );
            let track = Bounds::new(
                point(area.right() - bar - px(4.), area.top()),
                size(bar + px(4.), area.size.height),
            );
            let thumb = crate::tags::tests::painted_thumb(cx, track);
            assert_eq!(
                thumb.is_some(),
                overflows,
                "{lines} lines in the foot {area:?}: a scrollbar ({thumb:?}) only while they overflow"
            );
        }
    }

    /// Side `name`'s own header, which `view` draws in a box its content
    /// gives way in, keeps its `floor` exactly: the scaled 28 px control and
    /// the larger of its ring's room and the header's padding above and
    /// below, as GPUI snaps them to the test window's 2× device pixels: at
    /// 13 pt 34 px under the model's `py_1` (3.25 px drawn as 3, so 34.5
    /// unsnapped) and 41 px under the page view's `p_2`, at 18 pt 48 and
    /// 57 px (on a 1× display 34 and 40, 49 and 57). From
    /// `first`, revealed as a page that opens on it does, Tab moves through
    /// `controls` and Shift+Tab back, each revealed whole with its ring in
    /// every frame painted after the key. `at` names the case.
    #[cfg(target_os = "linux")]
    pub(crate) fn assert_a_header_keeps_its_floor(
        cx: &mut gpui_kit::VisualTestContext,
        at: &str,
        (view, name): (EntityId, &'static str),
        floor: Pixels,
        first: &crate::focus_reveal::tests::Control,
        controls: &[crate::focus_reveal::tests::Control],
    ) {
        use crate::tags::tests::{draw, rendered};
        let slack = px(0.5);
        let region: &'static str = format!("{name}/header").leak();
        let body = rendered(cx, format!("{name}/body").leak());
        let minimum = appearance::ui_size(crate::image_compare::STAGE_MIN);
        assert!(
            body.size.height < minimum - slack,
            "{at}: the content {body:?} gives way below its minimum {minimum:?}"
        );
        let held = rendered(cx, region);
        assert!(
            (held.size.height - floor).abs() < px(0.01),
            "{at}: the header's region {held:?} keeps its floor, {floor:?}"
        );
        let list = cx
            .update(|_, cx| side_header(view, name, cx))
            .expect("the side's header");
        list.reveal_next_focus();
        cx.update(|window, cx| list.focus(first.0.clone(), cx).focus(window, cx));
        draw(cx);
        crate::focus_reveal::tests::assert_every_frame_reveals(cx, &list, region, controls);
    }

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

    /// Walks a window showing a document's comparison in the `in_frame`
    /// region `region` from 1,481 to 461 px wide and back, in 60 px steps,
    /// 980 px tall, at 13 and 18 pt, and checks, `at` naming the surface,
    /// that every frame drawn after each resize draws the open controls
    /// (`open-control`) in the form of the frame it settles in. Returns the
    /// forms of After's control the walk met, "labelled" or "icon". Chosen
    /// from the region's width as last laid out, the first frame after a
    /// resize drew the form of the width before.
    #[cfg(target_os = "linux")]
    pub(crate) fn open_controls_take_their_form_in_the_frame(
        cx: &mut gpui_kit::VisualTestContext,
        at: &str,
        region: &'static str,
    ) -> std::collections::BTreeSet<&'static str> {
        use crate::in_frame::probe;
        use crate::views::tests::draw;
        let icon = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.))) + px(1.)
        };
        let forms = |frame: &probe::Frame, icon: Pixels| -> Vec<&'static str> {
            frame
                .parts
                .iter()
                .filter(|(part, _)| *part == "open-control")
                .map(|(_, bounds)| {
                    if bounds.size.width > icon {
                        "labelled"
                    } else {
                        "icon"
                    }
                })
                .collect()
        };
        probe::watch();
        let mut met = std::collections::BTreeSet::new();
        for interface in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(1481.), px(980.)));
            draw(cx);
            let icon = icon(cx);
            let widths = (461..=1481)
                .rev()
                .step_by(60)
                .chain((461..=1481).step_by(60));
            for width in widths {
                let case = format!("{at} at {interface} pt, resized to {width} × 980");
                probe::take();
                cx.simulate_resize(size(px(width as f32), px(980.)));
                draw(cx);
                let frames: Vec<_> = probe::take()
                    .into_iter()
                    .filter(|frame| frame.region == region)
                    .collect();
                draw(cx);
                let settled = probe::take()
                    .into_iter()
                    .rfind(|frame| frame.region == region)
                    .unwrap_or_else(|| panic!("{case}: {region} is drawn"));
                let settled = forms(&settled, icon);
                assert!(
                    !frames.is_empty(),
                    "{case}: {region} was drawn after the resize"
                );
                for (index, frame) in frames.iter().enumerate() {
                    assert_eq!(
                        forms(frame, icon),
                        settled,
                        "{case}: frame {index} of {} draws the open controls as they settle, in {:?}",
                        frames.len(),
                        frame.bounds
                    );
                }
                met.insert(
                    *settled
                        .last()
                        .unwrap_or_else(|| panic!("{case}: After's open control is drawn")),
                );
            }
        }
        probe::stop();
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        met
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
            control: ("image-system-preview", 1usize).into(),
            side: "After",
            bytes: Arc::from(&b"captured"[..]),
            offer: Offer {
                #[cfg(target_os = "linux")]
                kind: passive::Kind::Png,
            },
            #[cfg(not(target_os = "linux"))]
            name: "captured".into(),
            #[cfg(not(target_os = "linux"))]
            what: "image",
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
                "image",
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

    /// macOS names every open control in the form Linux does, label first
    /// ([`accessible_name`]), its copy named for what the surface shows:
    /// it once led with the side on an image's caption ("Source: open
    /// captured image in System preview") and a PDF's header ("After: open
    /// captured PDF in System preview"), and a model's header read "Open
    /// captured After model in system preview", none starting with the label
    /// the control shows; a document's header named the label alone. Linux
    /// builds never draw these names, so this pins them; macOS's own build
    /// draws them through [`Target::button`].
    #[test]
    fn macos_names_an_open_control_label_first() {
        let name = |side, what| accessible_name(QUICK_LOOK, Some(side), &quick_look_copy(what));
        for (side, what, expected) in [
            (
                "Source",
                "image",
                "System preview: Source, a read-only copy of the captured image",
            ),
            (
                "Before",
                "image",
                "System preview: Before, a read-only copy of the captured image",
            ),
            (
                "After",
                "PDF",
                "System preview: After, a read-only copy of the captured PDF",
            ),
            (
                "After",
                "model",
                "System preview: After, a read-only copy of the captured model",
            ),
            (
                "Before",
                "file",
                "System preview: Before, a read-only copy of the captured file",
            ),
        ] {
            assert_eq!(name(side, what), expected);
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
                        .target(*control, side, bytes, name, "file", offer)
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
