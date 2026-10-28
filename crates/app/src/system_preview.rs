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
//! control's row says why ([`Target::note`]): a failure in the warning color,
//! announced at once, or, when the person closed the application chooser, a
//! muted line. The line stays while that comparison is shown and goes when
//! another is ([`Showing`]); nothing reaches the window-wide notice.

use crate::copy_feedback::{self, Action, Mark};
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

/// What a muted line beneath the control says when the person closed the
/// application chooser: their choice, not a failure.
pub(crate) const DISMISSED: &str = "Nothing opened: the application chooser was closed.";

/// Why a click opened nothing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotOpened {
    /// The person closed the application chooser.
    Dismissed,
    /// Nothing could be opened, for this reason, a sentence.
    Failed(String),
}

impl From<String> for NotOpened {
    fn from(reason: String) -> Self {
        Self::Failed(reason)
    }
}

impl From<&str> for NotOpened {
    fn from(reason: &str) -> Self {
        Self::Failed(reason.into())
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
    text: SharedString,
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

    /// The line beneath the control's row saying why its last click in this
    /// showing opened nothing, as wide as the row so it starts a line of its
    /// own in a wrapping row: a failure in the warning color fitted to the
    /// page's surfaces, announced at once, or a closed chooser in muted
    /// text, announced politely. Tests find it as `<control id>/note`.
    pub(crate) fn note(&self, cx: &App) -> Option<AnyElement> {
        let note = cx
            .try_global::<Notes>()?
            .notes
            .iter()
            .rev()
            .find(|note| note.mark == self.mark && note.showing == self.opener.showing)?;
        let key = format!("{}/note", self.mark.control());
        let probe = key.clone();
        let color = if note.failed {
            appearance::labels(cx).warning
        } else {
            palette(cx).muted
        };
        Some(
            div()
                .id(("open-note", note.id))
                .debug_selector(move || key.clone())
                .map(|line| {
                    if note.failed {
                        line.role(Role::Alert)
                            .a11y_synthetic_children(native_accessibility::assertive)
                    } else {
                        line.role(Role::Status)
                            .a11y_synthetic_children(native_accessibility::polite)
                    }
                })
                .aria_label(note.text.clone())
                .w_full()
                .min_w_0()
                .max_h(appearance::ui_size(100.))
                .overflow_y_scroll()
                .text_size(appearance::ui_text(11.))
                .text_color(rgb(color))
                .child(note.text.clone())
                .children(appearance::text_probe(move || probe))
                .into_any_element(),
        )
    }

    /// The control's answer to one click: Opened on the control and one
    /// polite announcement, or the line saying why nothing opened.
    fn answer(self, outcome: Result<(), NotOpened>, window: &mut Window, cx: &mut App) {
        let (failed, text) = match outcome {
            Ok(()) => {
                copy_feedback::confirm(window, cx, Some(self.mark), OPENED);
                return;
            }
            Err(NotOpened::Dismissed) => (false, DISMISSED.into()),
            Err(NotOpened::Failed(reason)) => (
                true,
                format!(
                    "Could not open {}. {}.",
                    self.side,
                    reason.trim_end_matches('.')
                ),
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
            Ok(Err(error)) => Err(format!("{error:#}").into()),
            Err(_) => Err("The copy stopped before it was written.".into()),
        })
    }
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
