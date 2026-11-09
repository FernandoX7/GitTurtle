//! Native linear rebase review and interrupted-operation message editing.
use crate::*;
use gitturtle_core::{
    InteractiveRebaseCommand, InteractiveRebasePlan, InteractiveRebaseResume, RebaseAction,
    RebaseStep, WriteCommand,
};
use gpui_kit::component::{
    WindowExt,
    dialog::DialogFooter,
    scroll::{Scrollbar, ScrollbarMode},
};
use gpui_kit::prelude::FluentBuilder;
use std::{cell::Cell, rc::Rc};

pub(crate) const PREPARING: &str = "Reading rebase sequence…";

/// The planned commits' list height where the form has room for it.
const SEQUENCE_HEIGHT: Pixels = px(220.);

/// A planned commit's row height at the default interface size.
const ROW_HEIGHT: f32 = 34.;

/// How tall the planned commits' list is in a body whose content overflows
/// its `view`, the room the dialog's cap leaves it, `room` being the list's
/// focus ring's: [`SEQUENCE_HEIGHT`], or what the view holds with the ring's
/// room above and below, so the list Load commits focuses is shown whole
/// with its ring, though never less than two rows. At 18 pt in a 461 × 490
/// window the view is 220 px, as tall as the list, whose ring's foot it cut.
/// A body whose content fits it is as tall as that content, so the list
/// takes its full height there. Shrinking the list never ends the overflow,
/// since the rest of the form alone is taller than the list's ring room.
fn sequence_height(view: Pixels, room: Pixels) -> Pixels {
    let rows = appearance::ui_size(ROW_HEIGHT) * 2. + px(2.);
    SEQUENCE_HEIGHT.min(view - room * 2.).max(rows)
}

/// What the form's layout decides in the frame that draws it ([`Fitted`]).
#[derive(Clone)]
struct Fit {
    /// The sequence's height as the last frame laid it out, which the next
    /// frame lays the form out with first.
    sequence: Rc<Cell<Pixels>>,
    /// Whether the next frame reveals the sequence where its own layout puts
    /// it: as the plan arrives, and as the form opens again on it.
    reveal: Rc<Cell<bool>>,
    /// The body's content and the sequence, as the layout of the frame being
    /// drawn holds them ([`Marked`]).
    content: Rc<Cell<Option<LayoutId>>>,
    list: Rc<Cell<Option<LayoutId>>>,
    /// The window's viewport size the form was last rendered at: a frame at
    /// another starts from the whole sequence ([`Fit::start`]).
    window: Rc<Cell<Option<Size<Pixels>>>>,
}

impl Default for Fit {
    fn default() -> Self {
        Self {
            sequence: Rc::new(Cell::new(SEQUENCE_HEIGHT)),
            reveal: Rc::default(),
            content: Rc::default(),
            list: Rc::default(),
            window: Rc::default(),
        }
    }
}

impl Fit {
    /// The sequence's height the frame being rendered lays the form out with
    /// first: as the last frame left it, or, in the first frame at a new
    /// window size, [`SEQUENCE_HEIGHT`], which that frame's own
    /// layout shrinks where the form then overflows its body ([`Fitted`]).
    /// Started from the last frame's, the first frame after the window grew
    /// from 461 × 490 to 1,480 × 973 at 18 pt, with the form open and as it
    /// opened again there, drew the sequence at the half tile's 214 px, the
    /// body fitting with that shorter one, and the 220 px came only later.
    fn start(&self, window: &Window) -> Pixels {
        let at = Some(window.viewport_size());
        if self.window.replace(at) != at {
            self.sequence.set(SEQUENCE_HEIGHT);
        }
        self.sequence.get()
    }
}

/// The rebase form laid out for the room its body has in the frame that
/// draws it. The form is laid out first with the sequence as tall as the last
/// frame left it, so the dialog takes the form's height while it fits and its
/// cap's otherwise; where that layout overflows the body's view and the view
/// calls for another height ([`sequence_height`]), the form is built again
/// with it and laid out in the same bounds before anything is prepainted.
/// Sized from the last frame, the frame after the plan arrived, when the form
/// had fitted its body until then, drew the sequence 220 px tall in a 220 px
/// view at 18 pt in a 461 × 490 window, its foot cut, and the 214 px came a
/// frame or two later. As the plan arrives and as the form opens again on
/// it, the body then scrolls the sequence into view whole with its ring's
/// room by the least scroll, from where this frame lays it out: the body's
/// reveal (`focus_reveal`) knows only where the last frame drew a control,
/// and the last frame drew no sequence, so it scrolled to the sequence a
/// frame late. The first frame at a new window size starts from the whole
/// sequence ([`Fit::start`]), so a window grown or shrunk lays the
/// final height out in that frame too, whether the form is open or opens
/// again in it. Only a body whose content shrinks under a shorter sequence,
/// as when an error line goes, is drawn once more, its sequence at full
/// height.
struct Fitted {
    form: WeakEntity<RebaseForm>,
    fit: Fit,
    /// The form, built with its sequence `sequence` tall.
    element: Option<AnyElement>,
    sequence: Pixels,
    /// The sequence's focus ring's room.
    room: Pixels,
}

impl IntoElement for Fitted {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Fitted {
    type RequestLayoutState = ();
    /// The form, laid out for the body's room in this frame.
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let element = self.element.as_mut().expect("the form is built once");
        (element.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let mut element = self.element.take()?;
        // The form is as tall as its body's view, the room the dialog's cap
        // leaves it once the body overflows.
        let view = bounds.size.height;
        let mut again = false;
        if let (Some(content), Some(_)) = (self.fit.content.get(), self.fit.list.get()) {
            let content = window.layout_bounds(content).size.height + self.room;
            if content - view > px(0.005) {
                let height = sequence_height(view, self.room);
                self.fit.sequence.set(height);
                if height != self.sequence
                    && let Some(form) = self.form.upgrade()
                {
                    let form = form.update(cx, |form, cx| form.content(height, window, cx));
                    element = div()
                        .w(bounds.size.width)
                        .h(bounds.size.height)
                        .flex()
                        .flex_col()
                        .child(form)
                        .into_any_element();
                    element.layout_as_root(bounds.size.map(AvailableSpace::Definite), window, cx);
                    again = true;
                }
            } else if self.sequence != SEQUENCE_HEIGHT {
                // The body fits with a shorter sequence, as once an error
                // line goes: whether it still fits with the whole one only
                // that frame's layout can tell.
                self.fit.sequence.set(SEQUENCE_HEIGHT);
                window.defer(cx, |window, _| window.refresh());
            }
        }
        if self.fit.reveal.take() {
            self.reveal(view, window, cx);
        }
        if again {
            element.prepaint_at(bounds.origin, window, cx);
        } else {
            element.prepaint(window, cx);
        }
        Some(element)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        element: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(element) = element {
            element.paint(window, cx);
        }
    }
}

impl Fitted {
    /// Scrolls the body, `view` tall, by the least amount that shows the
    /// sequence holding focus whole with its ring's room, where this frame's
    /// layout puts it, as the body's reveal does a control (`focus_reveal`):
    /// one taller than the view with that room shows its top edge.
    fn reveal(&self, view: Pixels, window: &mut Window, cx: &App) {
        let Some(form) = self.form.upgrade() else {
            return;
        };
        let form = form.read(cx);
        let (Some(content), Some(list)) = (self.fit.content.get(), self.fit.list.get()) else {
            return;
        };
        if !form.focus.is_focused(window) {
            return;
        }
        let (content, list) = (window.layout_bounds(content), window.layout_bounds(list));
        let room = self.room;
        let top = list.top() - content.top() - room;
        let bottom = list.bottom() - content.top() + room;
        let reach = (content.size.height + room - view).max(Pixels::ZERO);
        let scroll = form.body.scroll();
        let offset = scroll.offset();
        let shown = -offset.y.clamp(-reach, Pixels::ZERO);
        let start = if bottom - top > view || top < shown {
            top
        } else if bottom > shown + view {
            bottom - view
        } else {
            shown
        };
        scroll.set_offset(point(offset.x, -start.clamp(Pixels::ZERO, reach)));
    }
}

/// `child`, whose layout node `mark` keeps for the frame being drawn.
struct Marked {
    mark: Rc<Cell<Option<LayoutId>>>,
    child: AnyElement,
}

fn marked(mark: &Rc<Cell<Option<LayoutId>>>, child: impl IntoElement) -> Marked {
    Marked {
        mark: mark.clone(),
        child: child.into_any_element(),
    }
}

impl IntoElement for Marked {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Marked {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout = self.child.request_layout(window, cx);
        self.mark.set(Some(layout));
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// The actions the form offers for the selected commit, in its order.
const ACTIONS: [RebaseAction; 5] = [
    RebaseAction::Pick,
    RebaseAction::Reword,
    RebaseAction::Squash,
    RebaseAction::Fixup,
    RebaseAction::Drop,
];

gpui::actions!(interactive_rebase, [MoveCommitUp, MoveCommitDown]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-up", MoveCommitUp, Some("RebaseSequence")),
        KeyBinding::new("alt-down", MoveCommitDown, Some("RebaseSequence")),
        // Return on a rewritten series' commit row is the list's own, which
        // inspects that commit's files as Return does in History: bound to no
        // action here, the review dialog's default action never takes the
        // key, which closed the review before the list saw it.
        KeyBinding::new(
            "enter",
            gpui_kit::NoAction,
            Some(crate::rewrite_review::SERIES_CONTEXT),
        ),
    ]);
}

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    /// Whether the running preparation's outcome is no longer wanted
    /// ([`GitTurtle::dismiss_interactive_rebase_action`]).
    dismissed: bool,
    /// Whether Changes asked for the running preparation, which History
    /// shown again drops rather than wants again
    /// ([`GitTurtle::interactive_rebase_asked_in_changes`]).
    asked_in_changes: bool,
    draft: Option<Entity<RebaseForm>>,
    message: Option<Entity<MessageForm>>,
}

use crate::native_accessibility::text as label;

/// What the sequence's keys do, in the " · "-separated phrases [`keyboard_help`] draws.
fn keyboard_help_text() -> String {
    format!(
        "↑/↓ select · {} / {} reorder · P/R/S/F/D choose an action. Squash combines messages; \
         Fixup keeps the preceding message. Drop removes the commit's changes.",
        shortcuts::key_label("alt-up"),
        shortcuts::key_label("alt-down")
    )
}

/// The sequence's keyboard help, laid out by phrase as every line of phrases is: drawn as
/// one wrapping text, GPUI broke it before a "+", a "↑", a "/" or a "·" wherever it wrapped.
fn keyboard_help() -> native_accessibility::StaticText<Stateful<Div>> {
    shortcuts::dialog_help("rebase-keyboard-help", keyboard_help_text())
}

fn close_rebase_dialog(
    owner: &WeakEntity<GitTurtle>,
    path: Option<&std::path::Path>,
    window: &mut Window,
    cx: &mut App,
) {
    // Close synchronously so the dialog cannot later restore focus to the
    // branch-menu item that opened it and has since left the rendered tree.
    window.close_dialog(cx);
    // GitTurtle embeds the modal layer, so closing Root's dialog must also
    // invalidate the workspace's cached painted elements.
    window.refresh();
    let _ = owner.update(cx, |this, cx| {
        if this.path.as_deref() == path && this.page == AppPage::Repository {
            this.cancel_interactive_rebase_action();
            // The cancelled preparation's end never runs.
            this.read_deferred_working(window, cx);
            this.app_focus.focus(window, cx);
        }
    });
}

impl GitTurtle {
    pub(super) fn refresh_rebase_message(&self, cx: &mut Context<Self>) {
        if let Some(message) = &self.interactive_rebase.message {
            let message = message.downgrade();
            let owner = cx.entity().downgrade();
            cx.defer(move |cx| {
                let (Some(owner), Some(message)) = (owner.upgrade(), message.upgrade()) else {
                    return;
                };
                // Persistence can be requested from this form's input callback.
                // Defer both the key read and update to avoid reentering it.
                let status = owner
                    .read(cx)
                    .recovery_drafts
                    .status_for(&message.read(cx).key);
                message.update(cx, |message, cx| {
                    message.durable_status = status;
                    cx.notify();
                });
            });
        }
    }
    pub(super) fn cancel_interactive_rebase_action(&mut self) {
        self.interactive_rebase.generation = self.interactive_rebase.generation.wrapping_add(1);
        self.interactive_rebase.task = None;
        self.interactive_rebase.dismissed = false;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    /// Drops what the running preparation brings while its read still ends,
    /// as Changes opens over the view that asked for it, or wants it again
    /// (`dismissed` false) as that view shows again
    /// ([`Self::dismiss_recovery_read`]).
    pub(super) fn dismiss_interactive_rebase_action(&mut self, dismissed: bool) {
        self.interactive_rebase.dismissed = dismissed && self.interactive_rebase.task.is_some();
    }

    /// Whether the running preparation is one Changes asked for, as a
    /// paused rebase's Continue there asks: History shown again drops it
    /// rather than want what it brings, which would open over History
    /// ([`Self::recovery_asked_in_changes`]).
    pub(super) fn interactive_rebase_asked_in_changes(&self) -> bool {
        self.interactive_rebase.task.is_some() && self.interactive_rebase.asked_in_changes
    }

    /// A selected commit may be supplied as the exclusive base; otherwise the
    /// editable default reviews the current branch's most recent commit.
    pub(super) fn open_interactive_rebase(
        &mut self,
        base: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository.is_none() || self.operation_busy.is_some() {
            return;
        }
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let draft = self
            .interactive_rebase
            .draft
            .as_ref()
            .filter(|draft| draft.read(cx).path == path && base.is_none())
            .cloned()
            .unwrap_or_else(|| {
                cx.new(|cx| {
                    RebaseForm::new(
                        owner,
                        path,
                        base.unwrap_or_else(|| "HEAD~1".into()),
                        window,
                        cx,
                    )
                })
            });
        self.interactive_rebase.draft = Some(draft.clone());
        let (sequence, first) = draft.update(cx, |draft, cx| {
            draft.pending = false;
            draft.closed = false;
            // Reopened on a sequence it loaded, the form shows it whole: the
            // body's reveal does from where the form was last drawn, and the
            // form's own from where the frame that reopens it lays the
            // sequence out, which a resize while the form was closed has
            // moved. Reopened in a 461 × 490 half tile at
            // 18 pt after the form was last drawn at 1,480 × 973, the body's
            // reveal alone left the sequence below its fold in that frame.
            draft.body.reveal_next_focus();
            draft.fit.reveal.set(true);
            (draft.focus.clone(), draft.first_focus(cx))
        });
        self.dialog_hints
            .register(&sequence, shortcuts::DialogKeys::RebaseSequence);
        let review_owner = cx.entity().downgrade();
        let review_path = self.path.clone();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let close = draft.clone();
            let cancel = draft.clone();
            let button_close = draft.clone();
            let shown = draft.clone();
            // The form keeps the focus ring's room below its last control,
            // inside the dialog's clip of its body; the footer's gap gives
            // it back, so the footer stays put. It lies in the dialog's
            // content, as the LFS download form does: it takes what the cap
            // leaves it and scrolls the rest, revealing the control focus
            // moves onto. In the dialog's own scrolling body, which reveals
            // nothing, at 18 pt in a 461 × 490 window it ran under the
            // footer, where that body's clip cut the foot of what it showed.
            dialog
                .title(
                    native_accessibility::dialog_title(
                        "interactive-rebase-title",
                        "Edit local commits",
                    )
                    .debug_selector(|| "interactive-rebase-title".into()),
                )
                .width(appearance::dialog_width(px(720.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                .footer(
                    // In a narrow window at a large size the two actions
                    // are wider than the footer: they wrap below whole.
                    DialogFooter::new()
                        .flex_wrap()
                        .child(
                            shortcuts::DialogKeys::RebaseSequence
                                .dismiss_button("close-rebase-plan", false)
                                .on_click(move |_, window, cx| {
                                    button_close.update(cx, |form, cx| form.close(window, cx))
                                }),
                        )
                        // A write refuses the rewritten series' review at
                        // once, and assistive technology hears it unavailable
                        // at once, while it dims only once the write looks
                        // busy (`workspace::BusyLook`); it closed this dialog
                        // before its review refused, silently. The dialog is
                        // built in the window's render, so the control reads
                        // the window's state as it lays out.
                        .child(crate::workspace::dialog_refusing(
                            review_owner.clone(),
                            review_path.clone(),
                            {
                                let owner = review_owner.clone();
                                let path = review_path.clone();
                                move |refused, dimmed| {
                                    button(
                                        "review-previous-rewrite",
                                        "Review last rewritten series…",
                                        "",
                                        false,
                                    )
                                    .debug_selector(|| "review-previous-rewrite".into())
                                    .map(|review| {
                                        crate::workspace::refusing(
                                            review,
                                            || "review-previous-rewrite".into(),
                                            refused,
                                            dimmed,
                                        )
                                    })
                                    .on_click(move |_, window, cx| {
                                        let _ = owner.update(cx, |owner, cx| {
                                            // Refused before the dialog closes.
                                            if owner.dialog_refusal(path.as_deref()).0 {
                                                return;
                                            }
                                            window.close_dialog(cx);
                                            owner.open_rewrite_review(window, cx);
                                        });
                                    })
                                    .into_any_element()
                                }
                            },
                        )),
                )
                .on_ok(move |_, window, cx| {
                    close.update(cx, |form, cx| form.close(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |form, cx| form.close(window, cx));
                    false
                })
        });
        // Opening the dialog focuses its shell, which draws no ring, so the
        // form opened with focus nowhere to be seen. Focus its first stop
        // worth a key now, as the palette does its query, so keys typed
        // before the next frame reach it and the first frame draws its ring.
        first.focus(window, cx);
        window.refresh();
    }

    fn read_rebase<T: Send + 'static>(
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
        self.cancel_interactive_rebase_action();
        let generation = self.interactive_rebase.generation;
        self.interactive_rebase.asked_in_changes = self.mode == WorkspaceMode::Working;
        self.operation_busy = Some(PREPARING);
        let response = self.operations.submit_read(move || prepare(repo));
        self.interactive_rebase.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Rebase preparation was interrupted. Review the sequence again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.interactive_rebase.generation != generation {
                    return;
                }
                // Ended, it no longer runs: Changes opened after it drops no
                // plan (`dismiss_interactive_rebase_action`), as a recovery's
                // end does.
                this.interactive_rebase.task = None;
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                this.read_deferred_working(window, cx);
                if this.path.as_ref() == Some(&path)
                    && this.page == AppPage::Repository
                    && !this.interactive_rebase.dismissed
                {
                    receive(result, this, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }

    /// Route the existing rebase Continue control here. Abort and Keep files
    /// continue to use the shared operation-state preservation guards.
    pub(super) fn prepare_interactive_rebase_continue(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_rebase(
            |repo| {
                let expected = repo.interactive_rebase_resume()?;
                let key = recovery_drafts::Key::message(&expected);
                Ok((expected, key))
            },
            |result, this, window, cx| match result {
                Ok((expected, key)) => {
                    let recovered = this.recovery_drafts.latest(&key);
                    let status = this.recovery_drafts.status_for(&key);
                    let owner = cx.entity().downgrade();
                    let retained = this
                        .interactive_rebase
                        .message
                        .as_ref()
                        .filter(|form| {
                            let form = form.read(cx);
                            form.key == key
                        })
                        .cloned();
                    let form = if let Some(form) = retained {
                        form.update(cx, |form, _| form.expected = expected);
                        form
                    } else {
                        cx.new(|cx| MessageForm::new(owner, expected, key, recovered, window, cx))
                    };
                    form.update(cx, |form, _| {
                        form.closed = false;
                        form.durable_status = status;
                    });
                    this.interactive_rebase.message = Some(form.clone());
                    window.open_alert_dialog(cx, move |dialog, window, _| {
                        let close = form.clone();
                        let cancel = form.clone();
                        let button_close = form.clone();
                        dialog
                            .title(native_accessibility::dialog_title(
                                "rebase-message-title",
                                "Continue rebase",
                            ))
                            .width(appearance::dialog_width(px(640.), window))
                            .max_h(appearance::dialog_max_height(window))
                            .child(form.clone())
                            .footer(DialogFooter::new().child(
                                button("close-rebase-message", "Close", "", false).on_click(
                                    move |_, window, cx| {
                                        button_close.update(cx, |form, cx| form.close(window, cx))
                                    },
                                ),
                            ))
                            .on_ok(move |_, window, cx| {
                                close.update(cx, |form, cx| form.close(window, cx));
                                false
                            })
                            .on_cancel(move |_, window, cx| {
                                cancel.update(cx, |form, cx| form.close(window, cx));
                                false
                            })
                    });
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
}

struct RebaseForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    base: Entity<InputState>,
    plan: Option<InteractiveRebasePlan>,
    steps: Vec<RebaseStep>,
    selected: usize,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// Where the sequence's list rests: its offset as it was last
    /// prepainted, and after each wheel turn over it since, which the next
    /// turn is judged from.
    rest: Rc<Cell<Pixels>>,
    /// The form's scrolling body, which shows each control focus moves onto
    /// whole: Tab's, and the sequence Load commits and the form's opening
    /// focus, whatever pressed them (`focus_reveal`).
    body: crate::focus_reveal::FocusReveal,
    /// The sequence's height and reveal, decided as each frame is laid out.
    fit: Fit,
    acknowledged: bool,
    pending: bool,
    closed: bool,
    error: Option<String>,
    /// Every frame the body was prepainted in, and the frame being
    /// prepainted, for tests that read each frame after a change.
    #[cfg(test)]
    painted: std::rc::Rc<std::cell::RefCell<Vec<tests::Frame>>>,
    #[cfg(test)]
    drawing: std::rc::Rc<std::cell::RefCell<tests::Frame>>,
}

impl RebaseForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        base: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            owner,
            path,
            base: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(base)
                    .placeholder("Branch, tag, commit, or HEAD~3")
            }),
            plan: None,
            steps: Vec::new(),
            selected: 0,
            // A Tab stop of its own: the element's `tab_stop` applies only to
            // a handle the element makes itself, so the sequence, given this
            // one, was passed over by Tab and Shift+Tab.
            focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
            rest: Rc::default(),
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
            fit: Fit::default(),
            acknowledged: false,
            pending: false,
            closed: false,
            error: None,
            #[cfg(test)]
            painted: Default::default(),
            #[cfg(test)]
            drawing: Default::default(),
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.pending = false;
        close_rebase_dialog(&self.owner, self.path.as_deref(), window, cx);
    }

    /// Where focus starts as the form opens: the sequence this form already
    /// loaded, on its selected row, or else the base field, which the
    /// sequence needs first.
    fn first_focus(&self, cx: &App) -> FocusHandle {
        if self.steps.is_empty() {
            self.base.read(cx).focus_handle(cx)
        } else {
            self.focus.clone()
        }
    }

    /// Whether a write refuses the form's Load commits and Review rebase…,
    /// and whether they look it ([`crate::workspace::dialog_refusal`]).
    fn refusal(&self, cx: &App) -> (bool, bool) {
        crate::workspace::dialog_refusal(&self.owner, self.path.as_deref(), cx)
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused at once while a write runs, before anything changes: it
        // set the form's error, announced and drawn available.
        if self.pending || self.closed || self.refusal(cx).0 {
            return;
        }
        let base = self.base.read(cx).value().trim().to_owned();
        let form = cx.entity().downgrade();
        let path = self.path.clone();
        self.error = None;
        self.pending = self
            .owner
            .update(cx, |this, cx| {
                if this.path != path {
                    return false;
                }
                this.read_rebase(
                    move |repo| repo.interactive_rebase_plan(&base),
                    move |result, _, window, cx| {
                        let _ = form.update(cx, |form, cx| {
                            if form.closed {
                                return;
                            }
                            form.pending = false;
                            match result {
                                Ok(plan) => {
                                    form.steps = plan.steps();
                                    form.plan = Some(plan);
                                    form.selected = 0;
                                    form.acknowledged = false;
                                    // Focus lands on the sequence, on its
                                    // first commit, which the body shows
                                    // whole, from a click as from a key: in
                                    // a 461 × 490 window at 18 pt it lay
                                    // below the body's fold, focused and
                                    // drawn nowhere.
                                    form.scroll.scroll_to_item(0, ScrollStrategy::Top);
                                    form.body.reveal_next_focus();
                                    form.fit.reveal.set(true);
                                    form.focus.focus(window, cx);
                                }
                                Err(error) => crate::tags::set_error_line(
                                    &mut form.error,
                                    format!("{error:#}"),
                                    window,
                                    cx,
                                ),
                            }
                            cx.notify();
                        });
                    },
                    window,
                    cx,
                )
            })
            .unwrap_or(false);
        if !self.pending {
            crate::tags::set_error_line(
                &mut self.error,
                "The repository changed or another operation is busy. Reopen this review after it finishes.",
                window,
                cx,
            );
        }
        cx.notify();
    }

    fn choose(&mut self, action: RebaseAction, cx: &mut Context<Self>) {
        if !self.pending
            && let Some(step) = self.steps.get_mut(self.selected)
        {
            step.action = action;
            self.error = None;
            cx.notify();
        }
    }

    fn move_selected(&mut self, upwards: bool, cx: &mut Context<Self>) {
        if self.pending || self.steps.is_empty() {
            return;
        }
        let next = if upwards {
            self.selected.saturating_sub(1)
        } else {
            (self.selected + 1).min(self.steps.len() - 1)
        };
        self.steps.swap(self.selected, next);
        self.selected = next;
        self.scroll.scroll_to_item(next, ScrollStrategy::Center);
        self.error = None;
        cx.notify();
    }

    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused at once while a write runs, before anything changes.
        if self.closed || self.refusal(cx).0 {
            return;
        }
        let Some(plan) = self.plan.clone() else {
            crate::tags::set_error_line(
                &mut self.error,
                "Load and review the affected commits first.",
                window,
                cx,
            );
            cx.notify();
            return;
        };
        if let Err(error) = plan.validate_steps(&self.steps) {
            crate::tags::set_error_line(&mut self.error, error.to_string(), window, cx);
            cx.notify();
            return;
        }
        if self.base.read(cx).value().trim() != plan.base_revision {
            crate::tags::set_error_line(
                &mut self.error,
                "The base field changed. Load commits again before reviewing.",
                window,
                cx,
            );
            cx.notify();
            return;
        }
        if !plan.known_published_refs.is_empty() && !self.acknowledged {
            crate::tags::set_error_line(
                &mut self.error,
                "Acknowledge the remote-tracking warning before continuing.",
                window,
                cx,
            );
            cx.notify();
            return;
        }
        let sequence = self
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                let subject = plan
                    .commits
                    .iter()
                    .find(|commit| commit.oid == step.oid)
                    .map_or("", |commit| commit.subject.as_str());
                format!(
                    "{}. {} {} · {}",
                    index + 1,
                    step.action.label(),
                    &step.oid[..10],
                    subject
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let explanation = format!(
            "Rewrite {} from base {} ({}).\n\n{}\n\nThe sequence runs from top to bottom. Reword and Squash pause for native message review. Conflicts pause in Working Changes. Git preserves its configured hooks, identity and signing. A clean tracked working tree is required; no automatic stash is made.\n\n{}",
            plan.branch,
            plan.base_revision,
            &plan.base[..10],
            sequence,
            if plan.known_published_refs.is_empty() {
                "No locally known remote-tracking reference contains these commits. This does not prove they were never published. Nothing is pushed."
            } else {
                "You acknowledged rewriting commits contained in remote-tracking references. Other people may depend on their original IDs. Nothing is pushed; ordinary Push remains non-force."
            }
        );
        let command = WriteCommand::InteractiveRebase(Arc::new(InteractiveRebaseCommand::Start {
            plan,
            steps: self.steps.clone(),
            acknowledge_published: self.acknowledged,
        }));
        let path = self.path.clone();
        let _ = self.owner.update(cx, |this, cx| {
            if this.path == path && this.operation_busy.is_none() {
                self.closed = true;
                window.close_dialog(cx);
                this.app_focus.focus(window, cx);
                this.confirm_git_write(
                    "Start interactive rebase".into(),
                    explanation,
                    "Start rebase",
                    command,
                    window,
                    cx,
                );
                window.refresh();
            }
        });
    }

    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let step = &self.steps[index];
        let subject = self
            .plan
            .as_ref()
            .and_then(|plan| plan.commits.iter().find(|commit| commit.oid == step.oid))
            .map_or("", |commit| commit.subject.as_str());
        let text = format!(
            "{} · {} · {} · {}",
            index + 1,
            step.action.label(),
            &step.oid[..10],
            subject
        );
        let row = div()
            .id(("rebase-commit", index))
            .debug_selector(move || format!("rebase-commit-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(self.selected == index)
            .aria_label(text.clone())
            .w_full()
            .min_w_0()
            .h(crate::appearance::ui_size(ROW_HEIGHT))
            .px_2()
            .flex()
            .items_center()
            .text_size(crate::appearance::ui_text(12.))
            .bg(rgb(if self.selected == index {
                p.selected
            } else {
                p.panel
            }))
            .text_color(rgb(if step.action == RebaseAction::Drop {
                p.muted
            } else {
                p.text
            }))
            .child(div().flex_1().min_w_0().truncate().child(text))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = index;
                this.focus.focus(window, cx);
                cx.notify();
            }));
        // Where each frame draws the first commit, for tests.
        #[cfg(test)]
        let row = {
            use gpui_kit::base::ElementExt as _;
            let drawing = self.drawing.clone();
            row.when(index == 0, |row| {
                row.on_prepaint(move |bounds, _, _| drawing.borrow_mut().first = Some(bounds))
            })
        };
        row.into_any_element()
    }
}

impl Render for RebaseForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Each control Tab moves onto scrolls into the body's view whole with
        // its ring's room, and so does the sequence Load commits and the
        // form's opening focus (`focus_reveal`). In a 461 × 490 window at
        // 18 pt the body revealed nothing: the sequence Load commits focused
        // and I understand these commits may be shared, after it, lay below
        // its fold, focused and drawn nowhere.
        self.body.reveal(window, cx);
        let sequence = self.fit.start(window);
        Fitted {
            form: cx.entity().downgrade(),
            fit: self.fit.clone(),
            element: Some(self.content(sequence, window, cx)),
            sequence,
            room: appearance::button_ring_room(cx),
        }
    }
}

impl RebaseForm {
    /// The form, its sequence `sequence` tall ([`Fitted`]).
    fn content(
        &mut self,
        sequence: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.fit.content.set(None);
        self.fit.list.set(None);
        let p = palette(cx);
        let issue = self
            .plan
            .as_ref()
            .and_then(|plan| plan.validate_steps(&self.steps).err())
            .map(|error| error.to_string());
        let changed_base = self
            .plan
            .as_ref()
            .is_some_and(|plan| plan.base_revision != self.base.read(cx).value().trim());
        // A write refuses Load commits and Review rebase… at once, and
        // assistive technology hears them unavailable at once, while they dim
        // only once it looks busy (`workspace::BusyLook`); each refused a
        // press silently, announced and drawn available.
        let (refused, dimmed) = self.refusal(cx);
        let refusing = |control: Button, key: &'static str, unavailable: bool| {
            crate::workspace::refusing(
                control,
                || key.into(),
                unavailable || refused,
                unavailable || dimmed,
            )
        };
        let list = uniform_list(
            "rebase-sequence",
            self.steps.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range.map(|index| this.row(index, cx)).collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.scroll)
        // The sequence lies inside the body's own scroll, and GPUI's scroll
        // listeners never stop a wheel event, so a turn over a sequence
        // longer than its view scrolled it and the body together. Nested
        // scrolling chains natively, as Your themes' list in Settings does:
        // the turn is the sequence's while it moves the sequence, and the
        // body's only once it cannot. The list's own scroll listener has
        // already applied the turn when this one runs (bubble order, the
        // last registered first), unclamped, by the line height it kept as
        // it was drawn and from either axis, as GPUI maps a sideways turn
        // onto a vertical scroller; both judge the pointer by the list's
        // hitbox. So the turn moved the sequence where the offset it left,
        // clamped to the list's reach, differs from where the list rested.
        // Judged by a step recomputed from the event's vertical part, a
        // sideways turn scrolled both, and a turn by lines was measured by
        // the window's line height rather than the list's.
        .on_scroll_wheel({
            let (scroll, rest) = (self.scroll.clone(), self.rest.clone());
            move |_, _, cx| {
                let state = scroll.0.borrow();
                let Some(size) = state.last_item_size else {
                    return;
                };
                let reach = (size.contents.height - size.item.height).max(Pixels::ZERO);
                let turned = state.base_handle.offset().y.clamp(-reach, Pixels::ZERO);
                // Within a rounding error of where it rested, as GPUI
                // rounds the reach it clamps to.
                if (rest.replace(turned) - turned).abs() > px(0.01) {
                    cx.stop_propagation();
                }
            }
        });
        // Where the list rests once prepainted, its offset clamped there.
        let rested = {
            let (scroll, rest) = (self.scroll.clone(), self.rest.clone());
            move |_: Vec<Bounds<Pixels>>, _: &mut Window, _: &mut App| {
                rest.set(scroll.0.borrow().base_handle.offset().y);
            }
        };
        // The form scrolls, so it clips its controls to its bounds on both
        // axes. It keeps the focus ring's room beside and below them, giving
        // the sides back through its margin and the foot through the
        // dialog's footer gap, so the base field, Load commits and the
        // action buttons draw their rings whole and nothing moves.
        let room = appearance::button_ring_room(cx);
        let field = self.base.read(cx).focus_handle(cx);
        let load = self.body.focus("load-rebase-commits", cx);
        // The sequence draws its keyboard ring on its own 1 px edge, as
        // History's lists do: after a key, until a pointer press.
        let ringed = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).focus_ring.shows(&self.focus));
        let observe = self.body.observe();
        // Each frame the body is prepainted in, for tests.
        #[cfg(test)]
        let observe = {
            let (painted, drawing) = (self.painted.clone(), self.drawing.clone());
            let (focus, scroll) = (self.focus.clone(), self.body.scroll().clone());
            move |bounds, window: &mut Window, cx: &mut App| {
                observe(bounds, window, cx);
                let mut frame = std::mem::take(&mut *drawing.borrow_mut());
                frame.viewport = scroll.bounds();
                frame.offset = scroll.offset().y;
                frame.on_sequence = focus.is_focused(window);
                painted.borrow_mut().push(frame);
            }
        };
        let field_row = self
            .body
            .controls(vec![Some(field), Some(load.clone())], room);
        #[cfg(test)]
        let field_row = {
            let drawing = self.drawing.clone();
            move |bounds: Vec<Bounds<Pixels>>, window: &mut Window, cx: &mut App| {
                drawing.borrow_mut().load = bounds.get(1).copied();
                field_row(bounds, window, cx)
            }
        };
        let sequence_control = self.body.control(self.focus.clone(), room);
        #[cfg(test)]
        let sequence_control = {
            let drawing = self.drawing.clone();
            move |bounds: Vec<Bounds<Pixels>>, window: &mut Window, cx: &mut App| {
                drawing.borrow_mut().list = bounds.iter().copied().reduce(|a, b| a.union(&b));
                sequence_control(bounds, window, cx)
            }
        };
        // Cued by its scrollbar while it overflows, as the dialogs' other
        // scrolling bodies are. The window less 240 px is a loose bound only:
        // the `min_h_0` chain from the dialog's content down to the body
        // holds it to the room the dialog's cap leaves it. In the dialog's
        // own scrolling body the form asked for that bound, 33 px more than
        // the cap left it at 18 pt in a 461 × 490 window, so that body's clip
        // cut the foot of a control the form showed whole.
        let body = crate::native_accessibility::clips_children(div().on_children_prepainted(observe).id("rebase-plan-scroll").debug_selector(|| "rebase-plan-scroll".into())
            .min_h_0()
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)) + room)
            .px(room)
            .mx(-room)
            .pb(room)
            .overflow_y_scroll()
            .track_scroll(self.body.scroll())
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .child(marked(&self.fit.content, div().flex().flex_col().gap_3()
            .child(label("rebase-base-explanation", "Base stays unchanged. Review up to 100 commits after it on the current branch. Merge-preserving and root rewrites are unsupported.").text_size(crate::appearance::ui_text(12.)))
            .child(div().flex().gap_2().items_center().on_children_prepainted(field_row)
                .child(div().flex_1().debug_selector(|| "rebase-base-field".into()).child(Input::new(&self.base).aria_label("Exclusive base revision")))
                .child(refusing(button("load-rebase-commits", if self.pending { "Loading…" } else { "Load commits" }, "", false).track_focus(&load).debug_selector(|| "load-rebase-commits".into()), "load-rebase-commits", self.pending).on_click(cx.listener(|this, _, window, cx| this.load(window, cx)))))
            .when_some(self.plan.as_ref(), |element, plan| {
                // Each control reports where it is drawn from a wrapper or
                // row of its own: a body that tracks its scroll hands its own
                // children's bounds to the scroll handle.
                let actions: Vec<_> = (0..ACTIONS.len()).map(|index| self.body.focus(("rebase-action", index), cx)).collect();
                let move_up = self.body.focus("rebase-move-up", cx);
                let move_down = self.body.focus("rebase-move-down", cx);
                let acknowledge = (!plan.known_published_refs.is_empty()).then(|| self.body.focus("acknowledge-rebase-published", cx));
                let review = self.body.focus("review-interactive-rebase", cx);
                let row: Vec<_> = actions.iter().chain([&move_up, &move_down]).cloned().map(Some).collect();
                element
                .child(label("rebase-resolved-base", format!("{} · {} · base {} · branch tip {}", plan.branch, crate::counted(plan.commits.len(), "commit", "commits"), &plan.base[..10], &plan.head[..10])).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .child(div().flex().flex_wrap().gap_1().on_children_prepainted(self.body.controls(row, room)).children(ACTIONS.into_iter().zip(&actions).map(|(action, focus)| {
                    let active = self.steps.get(self.selected).is_some_and(|step| step.action == action);
                    button(action.label(), action.label(), "", active).track_focus(focus).debug_selector(move || format!("rebase-action-{}", action.label().to_lowercase())).toggled(active).disabled(self.pending).on_click(cx.listener(move |this, _, _, cx| this.choose(action, cx)))
                })).child(button("rebase-move-up", "Move up", "", false).track_focus(&move_up).debug_selector(|| "rebase-move-up".into()).disabled(self.pending || self.selected == 0).on_click(cx.listener(|this, _, _, cx| this.move_selected(true, cx))))
                    .child(button("rebase-move-down", "Move down", "", false).track_focus(&move_down).debug_selector(|| "rebase-move-down".into()).disabled(self.pending || self.selected + 1 >= self.steps.len()).on_click(cx.listener(|this, _, _, cx| this.move_selected(false, cx)))))
                .child(marked(&self.fit.list, div().flex().flex_col().on_children_prepainted(sequence_control).child(crate::native_accessibility::clips_children(div().on_children_prepainted(rested).id("rebase-list-focus").debug_selector(|| "rebase-list-focus".into()).role(Role::ListBox).aria_label("Planned commits, oldest first").tab_stop(true).key_context("RebaseSequence").track_focus(&self.focus).h(sequence).border_1().border_color(rgb(if ringed { p.accent } else { p.border })).rounded(px(5.)).overflow_hidden()
                    .on_action(cx.listener(|this, _: &MoveCommitUp, _, cx| this.move_selected(true, cx)))
                    .on_action(cx.listener(|this, _: &MoveCommitDown, _, cx| this.move_selected(false, cx)))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let key = event.keystroke.key.as_str();
                        if matches!(key, "up" | "down" | "home" | "end") && !this.steps.is_empty() {
                            this.selected = match key { "up" => this.selected.saturating_sub(1), "down" => (this.selected + 1).min(this.steps.len() - 1), "home" => 0, _ => this.steps.len() - 1 };
                            this.scroll.scroll_to_item(this.selected, ScrollStrategy::Center); cx.stop_propagation(); cx.notify();
                        } else if let Some(action) = match key { "p" => Some(RebaseAction::Pick), "r" => Some(RebaseAction::Reword), "s" => Some(RebaseAction::Squash), "f" => Some(RebaseAction::Fixup), "d" => Some(RebaseAction::Drop), _ => None } { this.choose(action, cx); cx.stop_propagation(); }
                    })).child(list)))))
                .child(keyboard_help().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
                .when_some(acknowledge, |element, acknowledge| element
                    .child(label("rebase-published-warning", format!("History rewrite warning: these commits are known to {}. Other people may depend on their IDs.", plan.known_published_refs.join(", "))).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning)))
                    .child(div().flex().flex_col().on_children_prepainted(self.body.control(acknowledge.clone(), room)).child(button("acknowledge-rebase-published", if self.acknowledged { "Acknowledged: rewrite known shared commits" } else { "I understand these commits may be shared" }, "", self.acknowledged).track_focus(&acknowledge).debug_selector(|| "acknowledge-rebase-published".into()).toggled(self.acknowledged).on_click(cx.listener(|this, _, _, cx| { this.acknowledged = !this.acknowledged; cx.notify(); })))))
                .child(div().flex().flex_col().on_children_prepainted(self.body.control(review.clone(), room)).child(refusing(button("review-interactive-rebase", "Review rebase…", "", true).track_focus(&review).debug_selector(|| "review-interactive-rebase".into()), "review-interactive-rebase", self.pending || issue.is_some() || changed_base || (!plan.known_published_refs.is_empty() && !self.acknowledged)).on_click(cx.listener(|this, _, window, cx| this.review(window, cx)))))
            })
            .when(changed_base, |element| element.child(label("rebase-base-changed", "Base edited: load commits again to update the sequence.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .children(issue.map(|issue| label("rebase-invalid-plan", issue).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .children(self.error.as_ref().map(|error| crate::tags::error_line("rebase-form-error", error, Some(crate::appearance::ui_text(12.)), window, cx).text_color(rgb(p.warning)))))));
        // The body's scrollbar stands in the dialog's side padding, beside
        // the body and its ring room, as the worktree manager's and the
        // Reflog's do, and paints only while the body overflows. The body
        // keeps no gutter for it, so it keeps its full width and nothing in
        // it moves sideways as the loaded commits make it overflow: with a
        // gutter kept only while the body overflowed, Load commits moved
        // 19 px to the left as the commits it loaded arrived, at 18 pt in a
        // 461 × 490 window, and the text above the sequence wrapped anew;
        // kept always, the gutter left the one empty column of any dialog.
        div()
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .child(body)
            .child(
                div()
                    .debug_selector(|| "rebase-plan-scrollbar".into())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(-Scrollbar::width() - room)
                    .w(Scrollbar::width())
                    .child(
                        Scrollbar::vertical(self.body.scroll())
                            .id("rebase-plan-scrollbar")
                            .mode(ScrollbarMode::Always)
                            .viewport_from_layout(),
                    ),
            )
            .into_any_element()
    }
}

struct MessageForm {
    owner: WeakEntity<GitTurtle>,
    expected: InteractiveRebaseResume,
    editor: Option<Entity<TextareaState>>,
    error: Option<String>,
    closed: bool,
    key: recovery_drafts::Key,
    recovered: Option<recovery_drafts::Draft>,
    accepted: String,
    durable_status: String,
    _subscription: Option<Subscription>,
}
impl MessageForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        expected: InteractiveRebaseResume,
        key: recovery_drafts::Key,
        recovered: Option<recovery_drafts::Draft>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = expected.message.as_ref().map(|message| {
            cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(10)
                    .default_value(message.clone())
            })
        });
        let subscription = editor.as_ref().map(|editor| cx.subscribe_in(editor, window, |this, editor, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::Change) { return; }
            if editor.read(cx).text().len() > 1024 * 1024 || editor.read(cx).value().contains('\0') {
                editor.update(cx, |editor, cx| editor.set_value(this.accepted.clone(), window, cx));
                crate::tags::set_error_line(&mut this.error, "Message edits are limited to 1 MiB without NUL bytes. The previous draft is preserved.", window, cx);
                cx.notify(); return;
            }
            let value = editor.read(cx).value().to_string();
            this.accepted = value.clone();
            let key = this.key.clone();
            let _ = this.owner.update(cx, |owner, cx| owner.persist_recovery_draft(key, Some(value), window, cx));
            cx.notify();
        }));
        Self {
            owner,
            accepted: expected.message.clone().unwrap_or_default(),
            durable_status: "Edits save automatically outside the repository".into(),
            expected,
            key,
            recovered,
            _subscription: subscription,
            editor,
            error: None,
            closed: false,
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        close_rebase_dialog(&self.owner, Some(&self.expected.root), window, cx);
    }

    /// Whether a write refuses Review Continue…, and whether it looks it
    /// ([`crate::workspace::dialog_refusal`]).
    fn refusal(&self, cx: &App) -> (bool, bool) {
        crate::workspace::dialog_refusal(&self.owner, Some(self.expected.root.as_path()), cx)
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Refused at once while a write runs, before anything changes: an
        // invalid message set its error first.
        if self.closed || self.refusal(cx).0 {
            return;
        }
        let message = self
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).value().to_string());
        if message.as_ref().is_some_and(|message| {
            message.trim().is_empty() || message.len() > 1024 * 1024 || message.contains('\0')
        }) {
            crate::tags::set_error_line(
                &mut self.error,
                "Enter a nonempty message of at most 1 MiB without NUL bytes.",
                window,
                cx,
            );
            cx.notify();
            return;
        }
        let expected = self.expected.clone();
        let path = expected.root.clone();
        let staged = expected
            .operation
            .staged_paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        let explanation = format!(
            "Continue rebasing {} with {}.\n\n{}\n\n{}\n\nGit applies its normal message cleanup, hooks and signing. Later message edits and conflicts pause again. Closing this review keeps the operation paused.",
            expected.operation.branch,
            crate::counted(
                expected.operation.staged_paths.len(),
                "staged path",
                "staged paths"
            ),
            staged,
            message
                .as_deref()
                .unwrap_or("No current message edit is required; continue the pending sequence.")
        );
        let _ = self.owner.update(cx, |this, cx| {
            if this.path.as_ref() == Some(&path) && this.operation_busy.is_none() {
                self.closed = true;
                window.close_dialog(cx);
                this.app_focus.focus(window, cx);
                this.confirm_git_write(
                    "Continue interactive rebase".into(),
                    explanation,
                    "Continue rebase",
                    WriteCommand::InteractiveRebase(Arc::new(InteractiveRebaseCommand::Continue {
                        expected,
                        message,
                    })),
                    window,
                    cx,
                );
                window.refresh();
            }
        });
    }
}
impl Render for MessageForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let durable_status = self.durable_status.clone();
        crate::native_accessibility::clips_children(div().id("rebase-message-scroll")
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)))
            .overflow_y_scroll()
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .child(div().flex().flex_col().gap_3()
            .child(label("rebase-resume-state", format!("{} · Base: {} · {}", self.expected.operation.branch, self.expected.operation.target_label, crate::counted(self.expected.operation.staged_paths.len(), "staged path", "staged paths"))).text_size(crate::appearance::ui_text(12.)))
            .when_some(self.expected.operation.commit.as_ref(), |element, commit| element.child(label("rebase-resume-commit", format!("Replaying original commit {commit}")).text_size(crate::appearance::ui_text(12.))))
            .child(label("rebase-message-explanation", "Review Git's pending commit message. Git uses its configured cleanup rules for comments and whitespace, hooks, author identity, and signing. Your edited draft is retained when this dialog closes.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
            .when_some(self.recovered.clone(), |element, recovered| {
                let valid = recovered.key == self.key; let copy = recovered.text.clone(); let mark = copy_feedback::Mark::new("copy-rebase-draft", &recovered.key); let copy_mark = mark.clone();
                element.child(div().flex().flex_col().gap_2()
                    .child(label("rebase-recovered-context", if valid { "A saved message matches this rebase, staged index and Git message. Restore it to review before Continue." } else { "Saved message source changed. Copy the saved text to recover it; it cannot replace this message automatically." }).text_color(rgb(if valid { p.muted } else { p.warning })))
                    .child(div().flex().gap_2()
                        .child(copy_feedback::copy_button(mark, "Copy saved text", "", cx).on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(copy_mark.clone()), "saved message", copy.clone())))
                        .child(button("restore-rebase-draft", "Restore saved message", "", false).disabled(!valid || self.editor.is_none()).on_click(cx.listener(move |this, _, window, cx| { if valid { if let Some(editor) = &this.editor { editor.update(cx, |editor, cx| editor.set_value(recovered.text.clone(), window, cx)); } this.recovered = None; cx.notify(); } })))))
            })
            .when_some(self.editor.as_ref(), |element, editor| element.child(shortcuts::tab_leaves(Textarea::new(editor).h(px(240.)).aria_label("Rebase commit message"))))
            .child(label("rebase-draft-save-status", durable_status).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when(self.editor.is_none(), |element| element.child(label("rebase-no-message", "No message is pending. Continue will replay the next planned step and pause if a message or conflict needs attention.").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| crate::tags::error_line("rebase-message-error", error, Some(crate::appearance::ui_text(12.)), window, cx).text_color(rgb(p.warning))))
            // A write refuses it at once, announced unavailable at once, and
            // it dims only once the write looks busy (`workspace::BusyLook`).
            .child({ let (refused, dimmed) = self.refusal(cx); crate::workspace::refusing(button("review-rebase-continue", "Review Continue…", "", true).debug_selector(|| "review-rebase-continue".into()), || "review-rebase-continue".into(), refused, dimmed) }.on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))))))
    }
}

/// The open Continue rebase dialog's message field, for the prose fields'
/// Tab regression in `views::tests`.
#[cfg(test)]
pub(crate) fn rebase_message(app: &GitTurtle, cx: &App) -> Option<Entity<TextareaState>> {
    let form = app.interactive_rebase.message.as_ref()?;
    form.read(cx).editor.clone()
}

/// Opens `app`'s rebase form and loads its commits, for tests in other
/// modules: the plan is read on the app's operation executor, a thread
/// `run_until_parked` cannot wait for, so its read is awaited.
#[cfg(test)]
pub(crate) async fn open_loaded(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx)));
    let form = app.read_with(cx, |app, _| {
        app.interactive_rebase.draft.clone().expect("the form")
    });
    cx.update(|window, cx| form.update(cx, |form, cx| form.load(window, cx)));
    let read = app.update(cx, |app, _| app.interactive_rebase.task.take());
    read.expect("the plan is read").await;
    cx.executor().run_until_parked();
    form.read_with(cx, |form, _| {
        assert!(form.plan.is_some(), "the plan loaded: {:?}", form.error)
    });
}

/// Whether `app`'s rebase form shows an error or waits for a read, as a
/// refused press must not leave it.
#[cfg(test)]
pub(crate) fn form_changed(app: &GitTurtle, cx: &App) -> bool {
    app.interactive_rebase.draft.as_ref().is_some_and(|form| {
        let form = form.read(cx);
        form.error.is_some() || form.pending || form.closed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::tests::{
        assert_room_for_rings, draw, git, holds, painted_thumb, rendered, set_interface_text_size,
        tagged_repository, window,
    };
    use core::prelude::v1::test;

    /// The keyboard help is a line of phrases like every other: swept from 461 px down at 13
    /// and 18 pt, no key name breaks and no line starts with a "/" or a "·".
    #[gpui::test]
    fn the_keyboard_help_keeps_its_key_names_and_separators_whole(cx: &mut TestAppContext) {
        let keys = [
            "↑/↓".to_owned(),
            shortcuts::key_label("alt-up"),
            shortcuts::key_label("alt-down"),
            "P/R/S/F/D".to_owned(),
        ];
        let runs: Vec<SharedString> = crate::phrase_runs(&keyboard_help_text())
            .into_iter()
            .flat_map(|phrase| phrase.runs)
            .collect();
        crate::phrase_tests::sweep(
            cx,
            ("rebase-keyboard-help", "rebase-keyboard-help"),
            &keyboard_help_text(),
            &keys,
            move |window| {
                crate::phrase_tests::widest(&runs, crate::appearance::ui_text(11.), window)
            },
            || {
                keyboard_help()
                    .text_size(crate::appearance::ui_text(11.))
                    .into_any_element()
            },
        );
    }

    /// One frame the form's body was prepainted in: where it drew Load
    /// commits, the sequence and its first commit, where it drew them, its
    /// view and offset, and whether the sequence held focus.
    #[derive(Clone, Debug, Default)]
    pub(super) struct Frame {
        pub(super) load: Option<Bounds<Pixels>>,
        pub(super) list: Option<Bounds<Pixels>>,
        pub(super) first: Option<Bounds<Pixels>>,
        pub(super) viewport: Bounds<Pixels>,
        pub(super) offset: Pixels,
        pub(super) on_sequence: bool,
    }

    impl Frame {
        /// Where Load commits lies in the body's content, whatever its scroll.
        fn load_in_content(&self) -> Option<Bounds<Pixels>> {
            self.load.map(|load| load - point(px(0.), self.offset))
        }
    }

    /// Every frame `form`'s body was prepainted in since the last call.
    fn frames(cx: &mut VisualTestContext, form: &Entity<RebaseForm>) -> Vec<Frame> {
        form.read_with(cx, |form, _| {
            std::mem::take(&mut *form.painted.borrow_mut())
        })
    }

    /// How Load commits is pressed.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Press {
        Click,
        /// Enter, Tab having moved focus onto it from the base field.
        Enter,
    }

    /// What pressing Load commits painted: every frame of the body from the
    /// press on, and where Load commits lay in the body's content as it was
    /// pressed.
    struct Loaded {
        frames: Vec<Frame>,
        load: Option<Bounds<Pixels>>,
    }

    /// From the frame the plan arrives in, the first that draws the
    /// sequence, every frame in `frames` draws it holding focus, `height`
    /// tall and whole inside the body with its ring's room, its first commit
    /// whole inside it, and Load commits where it lay in the body's content
    /// as it was pressed, where `load` gives that.
    fn assert_arrives_whole(
        frames: &[Frame],
        height: Pixels,
        room: Pixels,
        load: Option<Bounds<Pixels>>,
        at: &str,
    ) {
        let start = frames
            .iter()
            .position(|frame| frame.list.is_some())
            .unwrap_or_else(|| panic!("{at}: a frame draws the sequence: {frames:?}"));
        let count = frames.len() - start;
        for (index, frame) in frames[start..].iter().enumerate() {
            let list = frame
                .list
                .unwrap_or_else(|| panic!("{at}: frame {index} of the plan draws the sequence"));
            assert!(
                frame.on_sequence,
                "{at}: frame {index}: the sequence holds focus"
            );
            assert_eq!(
                list.size.height, height,
                "{at}: frame {index} of {count} draws the sequence {height:?} tall, {list:?}"
            );
            assert!(
                lies_inside(list, room, frame.viewport),
                "{at}: frame {index} of {count} draws the sequence with its ring's room, \
                 {:?}, outside the body, {:?}, at offset {:?}",
                list.dilate(room),
                frame.viewport,
                frame.offset
            );
            let first = frame
                .first
                .unwrap_or_else(|| panic!("{at}: frame {index} draws the first commit"));
            assert!(
                first.top() >= list.top()
                    && first.bottom() <= list.bottom()
                    && lies_inside(first, px(0.), frame.viewport),
                "{at}: frame {index} draws the first commit, {first:?}, whole inside the \
                 sequence, {list:?}, and the body, {:?}",
                frame.viewport
            );
            if load.is_some() {
                assert_eq!(
                    frame.load_in_content(),
                    load,
                    "{at}: frame {index} of {count}: Load commits lies where it was pressed"
                );
            }
        }
    }

    /// The body's Tab stops in their order, by the selector each is drawn
    /// under.
    const STOPS: [&str; 12] = [
        "rebase-base-field",
        "load-rebase-commits",
        "rebase-action-pick",
        "rebase-action-reword",
        "rebase-action-squash",
        "rebase-action-fixup",
        "rebase-action-drop",
        "rebase-move-up",
        "rebase-move-down",
        "rebase-list-focus",
        "acknowledge-rebase-published",
        "review-interactive-rebase",
    ];

    /// The stops Tab reaches in the loaded form of [`published_repository`]
    /// before its acknowledgment: Move up waits for a later commit to be
    /// selected and Review rebase… for the acknowledgment.
    const LOADED_STOPS: [&str; 10] = [
        "rebase-base-field",
        "load-rebase-commits",
        "rebase-action-pick",
        "rebase-action-reword",
        "rebase-action-squash",
        "rebase-action-fixup",
        "rebase-action-drop",
        "rebase-move-down",
        "rebase-list-focus",
        "acknowledge-rebase-published",
    ];

    /// A base, tagged `base`, and four commits after it, which a
    /// remote-tracking branch contains, so Edit local commits asks for its
    /// acknowledgment, as it did on the native fixture.
    fn published_repository(directory: &std::path::Path) -> GitRepository {
        published_series(directory, 4)
    }

    /// [`published_repository`] with `commits` commits after its base, its
    /// four first, then as many more as `commits` asks: twelve make a
    /// sequence longer than its view.
    fn published_series(directory: &std::path::Path, commits: usize) -> GitRepository {
        let repo = crate::views::tests::fixture_repository(directory.join("repository"));
        let named = [
            "Touch every module",
            "Add the wide flag line",
            "Add two large noise images",
            "Rework the blend notes",
        ];
        let subjects = std::iter::once("Base".to_owned()).chain((0..commits).map(|index| {
            named.get(index).map_or_else(
                || format!("Tune step {index}"),
                |subject| (*subject).to_owned(),
            )
        }));
        for subject in subjects {
            git(
                repo.path(),
                &["commit", "--quiet", "--allow-empty", "-m", &subject],
            );
            if subject == "Base" {
                git(repo.path(), &["tag", "base"]);
            }
        }
        git(
            repo.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        repo
    }

    /// Opens Edit local commits on `repo` as a person does, in a window of
    /// `size` at `interface` pt: from the branch picker's menu, its base
    /// entered, the window resized to `load_in` where given, and Load commits
    /// pressed as `press_load` says, the plan read and drawn.
    async fn loaded_from_the_branch_picker<'a>(
        cx: &'a mut TestAppContext,
        repo: &GitRepository,
        size: Size<Pixels>,
        interface: u8,
        load_in: Option<Size<Pixels>>,
        press_load: Press,
    ) -> (
        Entity<GitTurtle>,
        Entity<RebaseForm>,
        &'a mut VisualTestContext,
        Loaded,
    ) {
        let (app, cx) = window(cx, repo);
        cx.update(|_, cx| {
            shortcuts::bind_keys(cx);
            native_accessibility::bind_keys(cx);
        });
        cx.simulate_resize(size);
        set_interface_text_size(cx, &app, interface);
        // The picker offers Edit local commits on a branch, which the status
        // names.
        app.update(cx, |app, cx| {
            app.work_status = Some(Arc::new(gitturtle_core::RepositoryStatus {
                branch: Some("main".into()),
                ..Default::default()
            }));
            cx.notify();
        });
        draw(cx);
        let press = |cx: &mut VisualTestContext, selector: &'static str| {
            let bounds = rendered(cx, selector);
            cx.simulate_click(bounds.center(), Modifiers::default());
            draw(cx);
        };
        press(cx, "current-branch");
        // In a short window the menu, capped, holds the item below its
        // fold: the wheel brings it into view.
        let first = rendered(cx, "menu-item: Find or create a branch… · MenuItem");
        cx.simulate_event(ScrollWheelEvent {
            position: first.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        draw(cx);
        press(cx, "menu-item: Edit local commits… · MenuItem");
        let form = app.read_with(cx, |app, _| {
            app.interactive_rebase.draft.clone().expect("the form")
        });
        let base = form.read_with(cx, |form, _| form.base.clone());
        assert!(
            cx.update(|window, cx| base.read(cx).focus_handle(cx).is_focused(window)),
            "the form opens in its base field"
        );
        cx.update(|window, cx| base.update(cx, |input, cx| input.set_value("base", window, cx)));
        draw(cx);
        if let Some(size) = load_in {
            cx.simulate_resize(size);
            draw(cx);
        }
        // The plan is read on the app's operation executor, a thread
        // `run_until_parked` cannot wait for: its read is awaited unless it
        // has already landed, which ends the task.
        if press_load == Press::Enter {
            cx.simulate_keystrokes("tab");
            draw(cx);
            assert_eq!(
                focused_stop(cx, &form),
                Some("load-rebase-commits"),
                "Tab from the base field"
            );
        }
        let load = frames(cx, &form).last().and_then(Frame::load_in_content);
        assert!(load.is_some(), "the body draws Load commits");
        match press_load {
            Press::Click => {
                let load = rendered(cx, "load-rebase-commits");
                cx.simulate_click(load.center(), Modifiers::default());
            }
            Press::Enter => cx.simulate_keystrokes("enter"),
        }
        let read = app.update(cx, |app, _| app.interactive_rebase.task.take());
        if let Some(read) = read {
            read.await;
        }
        draw(cx);
        form.read_with(cx, |form, _| {
            assert!(
                form.error.is_none() && !form.steps.is_empty(),
                "the plan loaded: {:?}",
                form.error
            );
            assert_eq!(form.selected, 0, "on its first commit");
        });
        let frames = frames(cx, &form);
        (app, form, cx, Loaded { frames, load })
    }

    /// The body's stop holding focus, if one does.
    fn focused_stop(cx: &mut VisualTestContext, form: &Entity<RebaseForm>) -> Option<&'static str> {
        cx.update(|window, cx| {
            let form = form.read(cx);
            STOPS.into_iter().find(|stop| {
                let focus = match *stop {
                    "rebase-base-field" => form.base.read(cx).focus_handle(cx),
                    "rebase-list-focus" => form.focus.clone(),
                    stop => match stop.strip_prefix("rebase-action-") {
                        Some(action) => {
                            let index = ACTIONS
                                .iter()
                                .position(|each| each.label().to_lowercase() == action)
                                .expect("an action");
                            form.body.focus(("rebase-action", index), cx)
                        }
                        None => form.body.focus(stop, cx),
                    },
                };
                focus.is_focused(window)
            })
        })
    }

    /// A fill the window painted and the clip it was painted in.
    type Clipped = (Bounds<Pixels>, Bounds<Pixels>);

    /// Where the body's scrollbar runs, beside the body in the dialog's side
    /// padding, and each fill the last frame painted inside that track, with
    /// the clip it was painted in.
    fn side_scrollbar(cx: &mut VisualTestContext) -> (Bounds<Pixels>, Vec<Clipped>) {
        let track = rendered(cx, "rebase-plan-scrollbar");
        let fills = cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |bounds: Bounds<ScaledPixels>| {
                Bounds::from_corners(
                    point(
                        px(bounds.left().as_f32() / scale),
                        px(bounds.top().as_f32() / scale),
                    ),
                    point(
                        px(bounds.right().as_f32() / scale),
                        px(bounds.bottom().as_f32() / scale),
                    ),
                )
            };
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| (logical(quad.bounds), logical(quad.content_mask.bounds)))
                .filter(|(fill, _)| holds(track, *fill))
                .collect()
        });
        (track, fills)
    }

    /// The body, `viewport`, keeps no gutter: the sequence spans its whole
    /// width less its ring's `room`, and every stop the form draws lies
    /// inside it with its ring.
    fn assert_full_width(
        cx: &mut VisualTestContext,
        viewport: Bounds<Pixels>,
        room: Pixels,
        at: &str,
    ) {
        let list = rendered(cx, "rebase-list-focus");
        assert!(
            (list.right() - (viewport.right() - room)).abs() < px(0.01),
            "{at}: the sequence {list:?} spans the body's width, {viewport:?}, less its ring's room"
        );
        for stop in STOPS {
            let Some(drawn) = cx.debug_bounds(stop) else {
                continue;
            };
            let ring = drawn.dilate(room);
            assert!(
                ring.right() <= viewport.right() + px(0.01),
                "{at}: {stop} with its ring, {ring:?}, lies past the body's right, {viewport:?}"
            );
        }
    }

    /// `drawn`, grown by `room`, lies inside `viewport`, within float error.
    fn lies_inside(drawn: Bounds<Pixels>, room: Pixels, viewport: Bounds<Pixels>) -> bool {
        let ring = drawn.dilate(room);
        let slack = px(0.01);
        ring.top() + slack >= viewport.top() && ring.bottom() <= viewport.bottom() + slack
    }

    /// With the body at its top and focus in the base field, presses `key`,
    /// Tab from that field and Shift+Tab through the footer, through the
    /// body's stops until focus leaves the body, or comes back to the base
    /// field, and returns the stops focus lay on, in order. After each press
    /// the focused stop, grown by its ring's room, lies inside the body as
    /// drawn, and so it does in every frame painted after the press, the
    /// first that draws the focus included. Where `still` gives where the
    /// sequence and Load commits rest, every one of those frames draws them
    /// there, the body never scrolled.
    fn walk(
        cx: &mut VisualTestContext,
        form: &Entity<RebaseForm>,
        key: &str,
        at: &str,
        still: Option<(Bounds<Pixels>, Bounds<Pixels>)>,
    ) -> Vec<&'static str> {
        let body = form.read_with(cx, |form, _| form.body.clone());
        let room = cx.read(appearance::button_ring_room);
        cx.update(|window, cx| {
            body.scroll().set_offset(Point::default());
            form.read(cx)
                .base
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        });
        draw(cx);
        assert_eq!(focused_stop(cx, form), Some("rebase-base-field"), "{at}");
        let mut visited = Vec::new();
        if key == "tab" {
            visited.push("rebase-base-field");
        }
        for _ in 0..STOPS.len() + 4 {
            body.take_painted();
            frames(cx, form);
            cx.simulate_keystrokes(key);
            draw(cx);
            if let Some((list, load)) = still {
                let painted = frames(cx, form);
                assert!(!painted.is_empty(), "{at}: {key} paints the body");
                for (index, frame) in painted.iter().enumerate() {
                    assert_eq!(
                        (frame.offset, frame.list, frame.load_in_content()),
                        (px(0.), Some(list), Some(load)),
                        "{at}: frame {index} after {key} scrolls the body or moves the \
                         sequence or Load commits"
                    );
                }
            }
            let Some(stop) = focused_stop(cx, form) else {
                // Tab leaves the body for the footer after its last stop;
                // Shift+Tab passes the footer before it.
                if key == "tab" {
                    break;
                }
                continue;
            };
            if visited.contains(&stop) {
                break;
            }
            let viewport = rendered(cx, "rebase-plan-scroll");
            let drawn = rendered(cx, stop);
            assert!(
                lies_inside(drawn, room, viewport),
                "{at}: after {key}, {stop} with its ring, {:?}, lies outside the body, \
                 {viewport:?}",
                drawn.dilate(room)
            );
            let frames = body.take_painted();
            assert!(
                !frames.is_empty(),
                "{at}: {key} onto {stop} paints the body"
            );
            for (index, frame) in frames.iter().enumerate() {
                let control = frame
                    .control
                    .unwrap_or_else(|| panic!("{at}: frame {index} after {key} draws {stop}"));
                assert!(
                    lies_inside(control, room, frame.viewport),
                    "{at}: frame {index} after {key} paints {stop} with its ring, {:?}, \
                     outside the body, {:?}",
                    control.dilate(room),
                    frame.viewport
                );
            }
            visited.push(stop);
            if stop == "rebase-base-field" {
                break;
            }
        }
        visited
    }

    /// Edit local commits in a 461 × 490 half tile at 18 pt, opened from the
    /// branch picker, its base entered and Load commits clicked, as native QA
    /// opened it (QA-106 part c, D3): focus lands on the planned commits,
    /// which the body scrolls to show whole with their ring's room, their
    /// first commit selected and drawn inside them, the list fitted to the
    /// body's 220 px view. The body revealed nothing: the sequence lay at
    /// y 430.5 below a body ending at y 346, focused and drawn nowhere, Tab
    /// passed over it and moved on to I understand these commits may be
    /// shared below it, and from the base field to Pick through Move down,
    /// out of sight too. With the body at its top the sequence and the
    /// acknowledgment start below its fold; Tab and Shift+Tab reach every
    /// stop of the body, the sequence among them, whole inside it in every
    /// frame after the key, Review rebase… too once Enter acknowledges the
    /// warning; the body draws its scrollbar at its right, clear of every
    /// stop's ring; and every stop of the dialog draws its ring whole, no
    /// clip around the body cutting one (`focus_ring::sweep`).
    #[gpui::test]
    async fn load_commits_and_tab_show_each_stop_whole_in_a_half_tile_at_18_pt(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_repository(directory.path());
        let (app, form, cx, loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            None,
            Press::Click,
        )
        .await;
        let body = form.read_with(cx, |form, _| form.body.clone());
        let room = cx.read(appearance::button_ring_room);
        let at = "461 × 490 at 18 pt";

        assert_eq!(
            focused_stop(cx, &form),
            Some("rebase-list-focus"),
            "{at}: Load commits focuses the sequence"
        );
        // From the frame the plan arrives in, every frame lays the sequence
        // out at its settled height, whole with its first commit, and keeps
        // Load commits where it was clicked.
        let height = rendered(cx, "rebase-list-focus").size.height;
        assert_arrives_whole(&loaded.frames, height, room, loaded.load, at);
        let viewport = rendered(cx, "rebase-plan-scroll");
        let list = rendered(cx, "rebase-list-focus");
        let first = rendered(cx, "rebase-commit-0");
        for (name, drawn) in [("the sequence", list), ("its first commit", first)] {
            assert!(
                drawn.size.width > px(0.) && drawn.size.height > px(0.),
                "{at}: {name} is drawn, {drawn:?}"
            );
            assert!(
                lies_inside(drawn, room, viewport),
                "{at}: {name} with its ring's room, {:?}, lies inside the body, {viewport:?}",
                drawn.dilate(room)
            );
        }
        assert!(
            first.top() >= list.top() && first.bottom() <= list.bottom(),
            "{at}: the first commit {first:?} lies inside the sequence {list:?}"
        );
        assert!(
            body.scroll().offset().y < px(0.),
            "{at}: Load commits scrolled the body to the sequence"
        );

        // The body overflows and draws its scrollbar beside it, in the
        // dialog's side padding, as the worktree manager's content does: the
        // track starts at the body's right edge, and the thumb lies in the
        // padding, no clip cutting it (the panel's edge cuts only the outer
        // 2 px of the track's own fill, as it does the worktree manager's
        // and the Reflog's). The body keeps no gutter for it, so
        // the sequence spans the body's whole width and every stop lies
        // inside the body with its ring.
        assert!(
            body.scroll().max_offset().y > px(0.),
            "{at}: the loaded form is taller than its body"
        );
        let (track, fills) = side_scrollbar(cx);
        assert!(
            (track.left() - viewport.right()).abs() < px(0.01),
            "{at}: the track {track:?} starts at the body's right edge, {viewport:?}"
        );
        let thumb = painted_thumb(cx, track)
            .unwrap_or_else(|| panic!("{at}: the scrollbar paints its thumb in {track:?}"));
        let side =
            cx.update(|window, cx| gpui_kit::component::dialog::dialog_side_padding(window, cx));
        assert!(
            thumb.right() <= viewport.right() - room + side + px(0.01),
            "{at}: the thumb {thumb:?} lies in the dialog's {side:?} side padding, beside the \
             body {viewport:?}"
        );
        let (_, clip) = fills
            .into_iter()
            .find(|(fill, _)| *fill == thumb)
            .unwrap_or_else(|| panic!("{at}: the thumb {thumb:?} is a fill in {track:?}"));
        assert!(
            holds(clip, thumb),
            "{at}: a clip, {clip:?}, cuts the thumb {thumb:?}"
        );
        assert_full_width(cx, viewport, room, at);

        // At the body's top the sequence and the acknowledgment lie below its
        // fold.
        cx.update(|window, _| {
            body.scroll().set_offset(Point::default());
            window.refresh();
        });
        draw(cx);
        let viewport = rendered(cx, "rebase-plan-scroll");
        for stop in ["rebase-list-focus", "acknowledge-rebase-published"] {
            let drawn = rendered(cx, stop);
            assert!(
                drawn.top() >= viewport.bottom(),
                "{at}: with the body at its top, {stop} {drawn:?} starts below its fold, \
                 {viewport:?}"
            );
        }

        assert_eq!(
            walk(cx, &form, "tab", at, None),
            LOADED_STOPS,
            "{at}: Tab's stops"
        );
        let mut backward = LOADED_STOPS.to_vec();
        backward.reverse();
        assert_eq!(
            walk(cx, &form, "shift-tab", at, None),
            backward,
            "{at}: Shift+Tab's stops"
        );

        // Enter on the acknowledgment, reached from the footer, makes Review
        // rebase… a stop too.
        for _ in 0..4 {
            cx.simulate_keystrokes("shift-tab");
            draw(cx);
            if focused_stop(cx, &form) == Some("acknowledge-rebase-published") {
                break;
            }
        }
        assert_eq!(
            focused_stop(cx, &form),
            Some("acknowledge-rebase-published"),
            "{at}: Shift+Tab from the footer"
        );
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            form.read_with(cx, |form, _| form.acknowledged),
            "{at}: Enter acknowledges"
        );
        let mut acknowledged = LOADED_STOPS.to_vec();
        acknowledged.push("review-interactive-rebase");
        assert_eq!(
            walk(cx, &form, "tab", at, None),
            acknowledged,
            "{at}: Tab's stops, acknowledged"
        );
        acknowledged.reverse();
        assert_eq!(
            walk(cx, &form, "shift-tab", at, None),
            acknowledged,
            "{at}: Shift+Tab's stops, acknowledged"
        );

        let mut failures = Vec::new();
        crate::focus_ring::sweep::walk(&app, cx, "Edit local commits", 18, &mut failures);
        crate::focus_ring::sweep::walk_back(&app, cx, "Edit local commits", 18, &mut failures);
        crate::focus_ring::sweep::assert_whole(failures);
    }

    /// The same route in a 1,480 × 973 window at 13 pt, where the loaded form
    /// fits its body: focus lands on the sequence, drawn whole with its first
    /// commit; the body neither scrolls nor draws a scrollbar, and Tab and
    /// Shift+Tab reach every stop whole without moving any.
    #[gpui::test]
    async fn load_commits_in_a_wide_window_at_13_pt_moves_nothing(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_repository(directory.path());
        let (_app, form, cx, loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(1480.), px(973.)),
            13,
            None,
            Press::Click,
        )
        .await;
        let body = form.read_with(cx, |form, _| form.body.clone());
        let room = cx.read(appearance::button_ring_room);
        let at = "1,480 × 973 at 13 pt";

        assert_eq!(
            focused_stop(cx, &form),
            Some("rebase-list-focus"),
            "{at}: Load commits focuses the sequence"
        );
        // Every frame from the one the plan arrives in lays the sequence out
        // at its full height where it rests, the body unscrolled, and keeps
        // Load commits where it was clicked.
        assert_arrives_whole(&loaded.frames, SEQUENCE_HEIGHT, room, loaded.load, at);
        let resting = loaded.frames.last().expect("the plan paints the body");
        let still = (
            resting.list.expect("the sequence"),
            resting.load_in_content().expect("Load commits"),
        );
        for (index, frame) in loaded.frames.iter().enumerate() {
            assert_eq!(frame.offset, px(0.), "{at}: frame {index} scrolls the body");
            if frame.list.is_some() {
                assert_eq!(
                    frame.list,
                    Some(still.0),
                    "{at}: frame {index} moves the sequence"
                );
            }
        }
        let viewport = rendered(cx, "rebase-plan-scroll");
        for drawn in [
            "rebase-list-focus",
            "rebase-commit-0",
            "rebase-keyboard-help-0",
        ] {
            let bounds = rendered(cx, drawn);
            assert!(
                bounds.size.height > px(0.) && lies_inside(bounds, room, viewport),
                "{at}: {drawn} {bounds:?} is drawn whole inside the body, {viewport:?}"
            );
        }
        assert_eq!(
            rendered(cx, "rebase-list-focus").size.height,
            SEQUENCE_HEIGHT,
            "{at}: the sequence keeps its height"
        );
        assert_eq!(
            body.scroll().max_offset().y,
            px(0.),
            "{at}: the loaded form fits its body"
        );
        assert_eq!(body.scroll().offset().y, px(0.), "{at}");
        let (track, fills) = side_scrollbar(cx);
        assert!(
            fills.is_empty(),
            "{at}: the body paints no scrollbar, yet {track:?} holds {fills:?}"
        );
        assert_full_width(cx, viewport, room, at);
        let resting: Vec<_> = STOPS.iter().map(|stop| rendered(cx, stop)).collect();

        assert_eq!(
            walk(cx, &form, "tab", at, Some(still)),
            LOADED_STOPS,
            "{at}: Tab's stops"
        );
        let mut backward = LOADED_STOPS.to_vec();
        backward.reverse();
        assert_eq!(
            walk(cx, &form, "shift-tab", at, Some(still)),
            backward,
            "{at}: Shift+Tab's stops"
        );
        assert_eq!(body.scroll().offset().y, px(0.), "{at}: nothing scrolled");
        let walked: Vec<_> = STOPS.iter().map(|stop| rendered(cx, stop)).collect();
        assert_eq!(walked, resting, "{at}: no stop moved");
    }

    /// Load commits pressed by Enter in a 461 × 490 half tile at 18 pt, and
    /// the form reopened on the sequence it loaded: every frame from the one
    /// the plan arrives in, and every frame of the reopened form, lays the
    /// sequence out at its final height, the 214 px the body's 220 px view
    /// holds with its ring's room, revealed whole with its first commit, and
    /// Load commits where it was pressed. Sized from the frame before, when
    /// the form still fitted its body, the first frame after the plan
    /// arrived drew the sequence 220 px tall in a 220 px view, its foot cut,
    /// and the 214 px came a frame or two later.
    #[gpui::test]
    async fn load_commits_by_enter_and_reopening_lay_out_the_final_sequence_at_18_pt(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_repository(directory.path());
        let (app, form, cx, loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            None,
            Press::Enter,
        )
        .await;
        let room = cx.read(appearance::button_ring_room);
        let at = "461 × 490 at 18 pt, by Enter";
        let viewport = rendered(cx, "rebase-plan-scroll");
        let height = rendered(cx, "rebase-list-focus").size.height;
        assert_eq!(
            height,
            viewport.size.height - room * 2.,
            "{at}: the sequence takes what the body's view holds with its ring's room"
        );
        assert!(
            height < SEQUENCE_HEIGHT,
            "{at}: the view is short of 220 px"
        );
        assert_arrives_whole(&loaded.frames, height, room, loaded.load, at);

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        frames(cx, &form);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx))
        });
        draw(cx);
        assert_eq!(
            focused_stop(cx, &form),
            Some("rebase-list-focus"),
            "{at}: the form reopens on its sequence"
        );
        let reopened = frames(cx, &form);
        assert!(
            reopened.first().is_some_and(|frame| frame.list.is_some()),
            "{at}: the reopened form's first frame draws the sequence: {reopened:?}"
        );
        assert_arrives_whole(&reopened, height, room, loaded.load, "reopened");
    }

    /// In a 461 × 330 window at 18 pt, shortened to it from 461 × 490 once the
    /// form is open, the body's view holds less than two of the sequence's
    /// rows with its ring's room: the sequence keeps two whole
    /// rows, taller than the view, and Load commits reveals it by its top
    /// edge, its ring's room above it, in every frame from the one the plan
    /// arrives in.
    #[gpui::test]
    async fn a_short_window_keeps_two_rows_of_the_sequence_revealed_by_its_top(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_repository(directory.path());
        let (_app, _form, cx, loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            Some(size(px(461.), px(330.))),
            Press::Click,
        )
        .await;
        let room = cx.read(appearance::button_ring_room);
        let at = "461 × 330 at 18 pt";
        let rows = appearance::ui_size(ROW_HEIGHT) * 2. + px(2.);
        let snapped = cx.update(|window, _| window.pixel_snap(rows));
        let viewport = rendered(cx, "rebase-plan-scroll");
        assert!(
            viewport.size.height < rows + room * 2.,
            "{at}: the body's view, {viewport:?}, holds less than two rows with their ring's room"
        );
        let start = loaded
            .frames
            .iter()
            .position(|frame| frame.list.is_some())
            .expect("the plan is drawn");
        for (index, frame) in loaded.frames[start..].iter().enumerate() {
            let list = frame.list.expect("the sequence");
            assert!(
                frame.on_sequence,
                "{at}: frame {index}: the sequence holds focus"
            );
            assert_eq!(
                list.size.height, snapped,
                "{at}: frame {index} draws the sequence two rows tall, on whole device pixels"
            );
            assert!(
                (list.top() - room - frame.viewport.top()).abs() < px(0.01),
                "{at}: frame {index} reveals the sequence, {list:?}, by its top edge, its \
                 ring's room below the body's top, {:?}",
                frame.viewport
            );
            let first = frame.first.expect("the first commit");
            assert!(
                lies_inside(first, px(0.), frame.viewport),
                "{at}: frame {index} draws the first commit, {first:?}, whole inside the \
                 body, {:?}",
                frame.viewport
            );
        }
    }

    /// Every frame in `painted` draws the sequence `height` tall, holding
    /// focus, whole inside the body with its ring's room and its first
    /// commit whole inside it: none draws it at another height first.
    fn assert_every_frame_final(painted: &[Frame], height: Pixels, room: Pixels, at: &str) {
        assert!(!painted.is_empty(), "{at}: the change paints the body");
        assert!(
            painted.iter().all(|frame| frame.list.is_some()),
            "{at}: every frame draws the sequence: {painted:?}"
        );
        assert_arrives_whole(painted, height, room, None, at);
    }

    /// Edit local commits loaded in a 461 × 490 half tile at 18 pt, where the
    /// sequence takes the 214 px its body's view holds, then, with the form
    /// open, the window grown to 1,480 × 973, where the form fits its body
    /// with the whole 220 px, and the text set to 13 pt: every frame painted
    /// after each change lays the sequence out at its final height, whole
    /// inside the body with its first commit. Started from the last frame's
    /// height, the first frame after the grow drew it at the 214 px, since
    /// the body fitted with that shorter sequence ([`Fit::start`]).
    #[gpui::test]
    async fn a_resize_or_text_size_with_the_form_open_lays_out_the_final_sequence_at_once(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_repository(directory.path());
        let (app, form, cx, _loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            None,
            Press::Click,
        )
        .await;
        let at = "461 × 490 at 18 pt";
        let room = cx.read(appearance::button_ring_room);
        assert!(
            rendered(cx, "rebase-list-focus").size.height < SEQUENCE_HEIGHT,
            "{at}: the half tile's body holds less than the whole sequence"
        );

        frames(cx, &form);
        cx.simulate_resize(size(px(1480.), px(973.)));
        draw(cx);
        let at = "grown to 1,480 × 973 at 18 pt";
        assert_eq!(
            rendered(cx, "rebase-list-focus").size.height,
            SEQUENCE_HEIGHT,
            "{at}: the form fits with the whole sequence"
        );
        assert_every_frame_final(&frames(cx, &form), SEQUENCE_HEIGHT, room, at);

        set_interface_text_size(cx, &app, 13);
        draw(cx);
        let at = "1,480 × 973 at 13 pt";
        assert_eq!(
            rendered(cx, "rebase-list-focus").size.height,
            SEQUENCE_HEIGHT,
            "{at}: the form fits with the whole sequence"
        );
        assert_every_frame_final(&frames(cx, &form), SEQUENCE_HEIGHT, room, at);
    }

    /// Edit local commits loaded in a 461 × 490 half tile at 18 pt and
    /// closed, the window grown to 1,480 × 973 and the form reopened, then
    /// closed again, the window shrunk back to the half tile and the form
    /// reopened: every frame of each reopened form lays the sequence out at
    /// its final height, 220 px in the grown window and the 214 px the half
    /// tile's body holds, revealed whole inside the body with its first
    /// commit. Reopened in the grown window, the first frame once drew the
    /// sequence at the half tile's 214 px ([`Fit::start`]). Reopened in the
    /// half tile, the form was last drawn in the
    /// grown window, its sequence near the top of a body that fitted, so only
    /// the reveal the form's own layout makes in the frame that reopens it
    /// shows the sequence there: the body's reveal, from where that frame
    /// drew it, left the sequence below the fold.
    #[gpui::test]
    async fn reopening_after_a_resize_lays_out_and_reveals_the_final_sequence(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_repository(directory.path());
        let (app, form, cx, _loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            None,
            Press::Click,
        )
        .await;
        let room = cx.read(appearance::button_ring_room);
        let half = rendered(cx, "rebase-list-focus").size.height;
        assert!(half < SEQUENCE_HEIGHT, "the half tile's body holds less");
        let reopen = |cx: &mut VisualTestContext, window: Size<Pixels>| {
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
            cx.simulate_resize(window);
            draw(cx);
            frames(cx, &form);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx))
            });
            draw(cx);
            assert_eq!(
                focused_stop(cx, &form),
                Some("rebase-list-focus"),
                "the form reopens on its sequence"
            );
            frames(cx, &form)
        };
        let grown = reopen(cx, size(px(1480.), px(973.)));
        assert_every_frame_final(&grown, SEQUENCE_HEIGHT, room, "reopened at 1,480 × 973");
        let shrunk = reopen(cx, size(px(461.), px(490.)));
        assert_every_frame_final(&shrunk, half, room, "reopened at 461 × 490");
        assert!(
            shrunk.iter().all(|frame| frame.offset < px(0.)),
            "the reopened half tile's body is scrolled to the sequence in every frame"
        );
    }

    /// Turns the wheel by `dy` over the middle of what the body shows of the
    /// element `selector` names.
    fn wheel_over(cx: &mut VisualTestContext, selector: &'static str, dy: Pixels) {
        turn_over(cx, selector, ScrollDelta::Pixels(point(px(0.), dy)));
    }

    /// Turns the wheel by `delta` over the middle of what the body shows of
    /// the element `selector` names.
    fn turn_over(cx: &mut VisualTestContext, selector: &'static str, delta: ScrollDelta) {
        let viewport = rendered(cx, "rebase-plan-scroll");
        let shown = rendered(cx, selector).intersect(&viewport);
        assert!(
            shown.size.height > px(1.),
            "the body, {viewport:?}, shows {selector}"
        );
        cx.simulate_event(ScrollWheelEvent {
            position: shown.center(),
            delta,
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        draw(cx);
    }

    /// Twelve commits loaded in a 461 × 490 half tile at 18 pt, a sequence
    /// longer than its 214 px view: a wheel turned down over the sequence
    /// scrolls only the sequence until its last commit is at its foot, and
    /// then the body; turned up, only the sequence until its first commit is
    /// at its top, and then the body; one long turn up brings the sequence
    /// to its top, and a wheel over the explanation scrolls only the body,
    /// as nested scrolling chains natively. The sequence's scroll never
    /// stopped the wheel, so every turn over it scrolled the body too.
    #[gpui::test]
    async fn a_wheel_over_a_long_sequence_scrolls_it_before_the_body_at_18_pt(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_series(directory.path(), 12);
        let (_app, form, cx, _loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            None,
            Press::Click,
        )
        .await;
        let at = "461 × 490 at 18 pt, twelve commits";
        let (list, body) = form.read_with(cx, |form, _| {
            assert_eq!(form.steps.len(), 12, "{at}: the plan loaded");
            (form.scroll.clone(), form.body.scroll().clone())
        });
        let offsets =
            |_: &mut VisualTestContext| (list.0.borrow().base_handle.offset().y, body.offset().y);
        let reach = list
            .0
            .borrow()
            .last_item_size
            .map(|size| size.item.height - size.contents.height)
            .expect("the sequence is laid out");
        assert!(reach < px(0.), "{at}: the sequence is longer than its view");
        let end = -body.max_offset().y;
        let (top, shown) = offsets(cx);
        assert_eq!(top, px(0.), "{at}: Load commits shows the first commit");
        assert!(
            shown < px(0.) && shown > end,
            "{at}: the body, scrolled to the sequence at {shown:?}, can move both ways, to {end:?}"
        );
        let step = px(60.);

        // Down: the sequence alone, until its end.
        let mut last = offsets(cx);
        let mut turns = 0;
        while last.0 > reach {
            wheel_over(cx, "rebase-list-focus", -step);
            let now = offsets(cx);
            assert_eq!(
                now,
                ((last.0 - step).max(reach), last.1),
                "{at}: a turn down over the sequence at {last:?} scrolls it alone"
            );
            last = now;
            turns += 1;
            assert!(turns < 20, "{at}: the sequence reaches its end");
        }
        assert!(turns > 1, "{at}: the sequence took several turns");
        // Then the body.
        wheel_over(cx, "rebase-list-focus", -step);
        let now = offsets(cx);
        assert_eq!(
            now,
            (reach, (last.1 - step).max(end)),
            "{at}: at its end, a turn down over the sequence scrolls the body"
        );
        assert!(now.1 < last.1, "{at}: the body moved");
        last = now;

        // Up: the sequence alone, until its top.
        let mut turns = 0;
        while last.0 < px(0.) {
            wheel_over(cx, "rebase-list-focus", step);
            let now = offsets(cx);
            assert_eq!(
                now,
                ((last.0 + step).min(px(0.)), last.1),
                "{at}: a turn up over the sequence at {last:?} scrolls it alone"
            );
            last = now;
            turns += 1;
            assert!(turns < 20, "{at}: the sequence reaches its top");
        }
        // Then the body.
        wheel_over(cx, "rebase-list-focus", step);
        let now = offsets(cx);
        assert_eq!(
            now,
            (px(0.), (last.1 + step).min(px(0.))),
            "{at}: at its top, a turn up over the sequence scrolls the body"
        );
        assert!(now.1 > last.1, "{at}: the body moved");

        // One long turn up, from the sequence's end, brings it to its top,
        // the body still.
        cx.update(|_, _| {
            list.scroll_to_item(11, ScrollStrategy::Bottom);
        });
        cx.update(|window, _| window.refresh());
        draw(cx);
        let (bottom, shown) = offsets(cx);
        assert_eq!(bottom, reach, "{at}: the sequence shows its last commit");
        wheel_over(cx, "rebase-list-focus", px(2000.));
        assert_eq!(
            offsets(cx),
            (px(0.), shown),
            "{at}: a long turn up over the sequence brings it to its top alone"
        );

        // Over the explanation, with the body at its top: only the body.
        cx.update(|window, _| {
            body.set_offset(Point::default());
            window.refresh();
        });
        draw(cx);
        assert_eq!(offsets(cx), (px(0.), px(0.)), "{at}");
        wheel_over(cx, "rebase-base-explanation", -step);
        assert_eq!(
            offsets(cx),
            (px(0.), (-step).max(end)),
            "{at}: a turn down over the explanation scrolls the body alone"
        );
        wheel_over(cx, "rebase-base-explanation", step);
        assert_eq!(
            offsets(cx),
            (px(0.), px(0.)),
            "{at}: a turn up over the explanation scrolls the body alone"
        );
    }

    /// The twelve commits of
    /// [`a_wheel_over_a_long_sequence_scrolls_it_before_the_body_at_18_pt`],
    /// turned by lines, as a mouse wheel turns them, and sideways, which GPUI
    /// maps onto the vertical sequence and body: a sideways turn moves only
    /// the sequence while it can move, and the body once the sequence is at
    /// its end; a turn by lines down at its end reaches the body, and one
    /// back up moves only the sequence. Judged by a step recomputed from the
    /// event's vertical part, a sideways turn over the sequence moved the
    /// body with it.
    #[gpui::test]
    async fn a_turn_by_lines_or_sideways_over_a_long_sequence_chains_at_its_end(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = published_series(directory.path(), 12);
        let (_app, form, cx, _loaded) = loaded_from_the_branch_picker(
            cx,
            &repo,
            size(px(461.), px(490.)),
            18,
            None,
            Press::Click,
        )
        .await;
        let at = "461 × 490 at 18 pt, twelve commits";
        let (list, body) = form.read_with(cx, |form, _| {
            assert_eq!(form.steps.len(), 12, "{at}: the plan loaded");
            (form.scroll.clone(), form.body.scroll().clone())
        });
        let offsets =
            |_: &mut VisualTestContext| (list.0.borrow().base_handle.offset().y, body.offset().y);
        let reach = list
            .0
            .borrow()
            .last_item_size
            .map(|size| size.item.height - size.contents.height)
            .expect("the sequence is laid out");
        let end = -body.max_offset().y;
        let sideways = || ScrollDelta::Pixels(point(px(-60.), px(0.)));

        // Sideways at the sequence's top: the sequence alone.
        let start = offsets(cx);
        assert_eq!(start.0, px(0.), "{at}: Load commits shows the first commit");
        turn_over(cx, "rebase-list-focus", sideways());
        assert_eq!(
            offsets(cx),
            (px(-60.), start.1),
            "{at}: a sideways turn over the sequence scrolls it alone"
        );

        // Sideways at its end: the body.
        list.scroll_to_item(11, ScrollStrategy::Bottom);
        cx.update(|window, _| window.refresh());
        draw(cx);
        let last = offsets(cx);
        assert_eq!(last.0, reach, "{at}: the sequence shows its last commit");
        turn_over(cx, "rebase-list-focus", sideways());
        let now = offsets(cx);
        assert_eq!(
            now,
            (reach, (last.1 - px(60.)).max(end)),
            "{at}: at its end, a sideways turn over the sequence scrolls the body"
        );
        assert!(now.1 < last.1, "{at}: the body moved");

        // By lines at its end: the body; back up: the sequence alone.
        let last = now;
        turn_over(cx, "rebase-list-focus", ScrollDelta::Lines(point(0., -1.)));
        let now = offsets(cx);
        assert!(
            now.0 == reach && now.1 < last.1,
            "{at}: at its end, a turn by lines down over the sequence at {last:?} scrolls the \
             body alone, to {now:?}"
        );
        let last = now;
        turn_over(cx, "rebase-list-focus", ScrollDelta::Lines(point(0., 1.)));
        let now = offsets(cx);
        assert!(
            now.0 > reach && now.1 == last.1,
            "{at}: a turn by lines up over the sequence at {last:?} scrolls it alone, to {now:?}"
        );
    }

    /// The rebase form opens with focus in its base field, which draws its
    /// focus whole: its 1 px accent edge and the Button's ring outside it,
    /// whose left side and foot the form's clip cut. It opened with focus on
    /// the dialog's shell, which draws nothing. Once commits load, the sequence
    /// holds focus and, after a key, draws its 1 px edge in the accent on all
    /// four sides; reopened on that sequence, the form starts there, ringed.
    #[gpui::test]
    async fn the_rebase_form_opens_with_its_first_stop_focused_and_ringed(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let repo = tagged_repository(directory.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let open = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx))
            });
            draw(cx);
        };
        let accent = cx.read(|cx| gpui_kit::Hsla::from(rgb(palette(cx).accent)));
        let sequence_ringed = |cx: &mut VisualTestContext| {
            let list = cx
                .debug_bounds("rebase-list-focus")
                .expect("the sequence is drawn");
            focus_ring::painted_outlines(cx, accent)
                .into_iter()
                .find(|edge| {
                    (edge.bounds.left() - list.left()).abs() <= px(1.)
                        && (edge.bounds.top() - list.top()).abs() <= px(1.)
                        && (edge.bounds.right() - list.right()).abs() <= px(1.)
                        && (edge.bounds.bottom() - list.bottom()).abs() <= px(1.)
                })
                .is_some_and(|edge| edge.width == px(1.) && edge.whole)
        };

        open(cx);
        let form = app.read_with(cx, |app, _| app.interactive_rebase.draft.clone().unwrap());
        let (base, text) = cx.read(|cx| {
            let base = form.read(cx).base.read(cx);
            (base.focus_handle(cx), base.input_bounds())
        });
        assert!(
            cx.update(|window, _| base.is_focused(window)),
            "the base field holds focus as the form opens"
        );
        focus_ring::assert_input_ringed(cx, text, "the rebase base field");
        // Load commits, at the form's other side, keeps its ring whole too,
        // and the room for both moves neither the form nor its footer.
        assert_room_for_rings(
            cx,
            &app,
            &["load-rebase-commits"],
            &["interactive-rebase-title", "dialog-dismiss: Close"],
        );

        cx.update(|window, cx| form.update(cx, |form, cx| form.load(window, cx)));
        let read = app.update(cx, |app, _| app.interactive_rebase.task.take());
        read.expect("the plan is read").await;
        draw(cx);
        let sequence = form.read_with(cx, |form, _| form.focus.clone());
        assert!(cx.update(|window, _| sequence.is_focused(window)));
        cx.simulate_keystrokes("down");
        draw(cx);
        assert!(sequence_ringed(cx), "a key rings the focused sequence");

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert!(!cx.update(|window, _| sequence.is_focused(window)));
        open(cx);
        assert!(
            cx.update(|window, _| sequence.is_focused(window)),
            "the form reopens on the sequence it loaded"
        );
        assert!(sequence_ringed(cx), "the reopened sequence is ringed");
    }

    /// Under a modal dialog the status bar names the keys of the dialog
    /// control holding focus, or nothing: never the page's hints, whose keys
    /// do not reach the page beneath. The rebase sequence names selection
    /// and reordering in the platform's key labels; the Reflog, and the
    /// rebase dialog's base field before the sequence loads, name none.
    /// Escape is named by the dialog's own button it stands for, which the
    /// dialog draws, and a picker's inline help says Escape with the same
    /// verb: the palette said "Esc Close" beside its Cancel button and
    /// "Escape returns to your previous focus" below its list, while Quick
    /// Open said "Esc Cancel", and Keyboard Shortcuts "Esc Close" beside Done.
    #[gpui::test]
    async fn the_status_bar_names_the_dialogs_keys_and_never_the_pages(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let repo = tagged_repository(directory.path());
        let (app, cx) = window(cx, &repo);
        let hints = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.status_hints(window, cx)))
        };
        let drawn = |cx: &mut VisualTestContext| cx.debug_bounds("status-hints").is_some();
        // The verb the hints name Escape by, and whether the open dialog
        // draws its own button with that label.
        let escape = |cx: &mut VisualTestContext, hints: &str| {
            // Up to the next hint: Keyboard Shortcuts' spare hints follow
            // Escape.
            let verb = hints
                .rsplit_once("Esc ")
                .and_then(|(_, rest)| rest.split(" · ").next())
                .unwrap_or_default()
                .to_owned();
            let button = cx
                .debug_bounds(format!("dialog-dismiss: {verb}").leak())
                .is_some();
            (verb, button)
        };
        let page = hints(cx).expect("History names its keys");
        assert!(
            page.contains(&format!("{}F Search", primary_label())),
            "{page}"
        );
        assert!(drawn(cx));

        cx.update(|window, cx| app.update(cx, |app, cx| app.open_reflog_browser(window, cx)));
        draw(cx);
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(hints(cx), None, "the Reflog");
        assert!(!drawn(cx), "the Reflog");
        cx.update(|window, cx| window.close_dialog(cx));
        draw(cx);
        assert_eq!(hints(cx).as_ref(), Some(&page), "after the Reflog");

        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx))
        });
        draw(cx);
        assert_eq!(hints(cx), None, "the base field");
        assert!(!drawn(cx), "the base field");

        let form = app.read_with(cx, |app, _| app.interactive_rebase.draft.clone().unwrap());
        cx.update(|window, cx| form.update(cx, |form, cx| form.load(window, cx)));
        let read = app.update(cx, |app, _| app.interactive_rebase.task.take());
        read.expect("the plan is read").await;
        draw(cx);
        assert!(cx.update(|window, cx| form.read(cx).focus.is_focused(window)));
        assert_eq!(
            hints(cx),
            Some(format!(
                "↑↓ Select · {} / {} Reorder · Esc Close",
                shortcuts::key_label("alt-up"),
                shortcuts::key_label("alt-down")
            )),
            "the rebase sequence"
        );
        assert!(drawn(cx), "the rebase sequence");
        let sequence = hints(cx).unwrap();
        assert_eq!(
            escape(cx, &sequence),
            ("Close".to_owned(), true),
            "the rebase sequence's Escape and its button"
        );

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(hints(cx).as_ref(), Some(&page), "after the rebase dialog");

        // Each other dialog with keys of its own names them from its search.
        for (keys, open, verb, help) in [
            (
                shortcuts::DialogKeys::Palette,
                GitTurtle::open_command_palette
                    as fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>),
                "Cancel",
                "Escape cancels",
            ),
            (
                shortcuts::DialogKeys::QuickOpen,
                GitTurtle::open_quick_file,
                "Cancel",
                "Escape cancels",
            ),
            (
                shortcuts::DialogKeys::Shortcuts,
                GitTurtle::shortcut_help,
                "Close",
                "Escape closes",
            ),
        ] {
            cx.update(|window, cx| app.update(cx, |app, cx| open(app, window, cx)));
            // The inline help, which Quick Open shows once it has read its
            // paths on the worker after a 120 ms pause for typing.
            let help = format!("dialog-help-escape: {help}").leak();
            for _ in 0..20 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(150));
                std::thread::sleep(std::time::Duration::from_millis(5));
                draw(cx);
                if cx.debug_bounds(help).is_some() {
                    break;
                }
            }
            assert!(cx.debug_bounds(help).is_some(), "{keys:?}: {help}");
            let named = hints(cx);
            assert_eq!(named, Some(keys.key_hints().whole()), "{keys:?}");
            let named = named.unwrap();
            assert!(!named.contains("Search"), "{keys:?}");
            assert!(drawn(cx), "{keys:?}");
            assert_eq!(
                escape(cx, &named),
                (verb.to_owned(), true),
                "{keys:?}: {named}"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{keys:?}"
            );
            assert_eq!(hints(cx).as_ref(), Some(&page), "after {keys:?}");
        }
        assert!(
            shortcuts::DialogKeys::Palette
                .key_hints()
                .whole()
                .contains(shortcuts::ENTER)
        );
    }

    /// Edit local commits' Load commits that has ended no longer runs, so
    /// Changes opened after it drops no plan: a plan is dropped only while
    /// its preparation runs (`dismiss_interactive_rebase_action`), as a
    /// recovery's is. The ended task stayed, and each later Changes marked a
    /// plan dropped that nothing was preparing.
    #[gpui::test]
    async fn changes_after_load_commits_ends_drops_no_plan(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let repo = tagged_repository(directory.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx))
        });
        let form = app.read_with(cx, |app, _| {
            app.interactive_rebase.draft.clone().expect("the form")
        });
        cx.update(|window, cx| form.update(cx, |form, cx| form.load(window, cx)));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(PREPARING)
        );
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None, "the preparation ended");
            assert!(app.interactive_rebase.task.is_none(), "it no longer runs");
        });
        form.read_with(cx, |form, _| {
            assert!(form.plan.is_some(), "the plan loaded")
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        assert!(
            !app.read_with(cx, |app, _| app.interactive_rebase.dismissed),
            "Changes drops no plan"
        );
    }
}
