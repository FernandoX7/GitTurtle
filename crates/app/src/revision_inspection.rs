//! Revision and path inspections reuse the workspace preview and retained views.
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use gitturtle_core::{ComparisonMode, PathScope, RevisionComparison, TrackedPath, TrackedPaths};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use gpui_kit::prelude::FluentBuilder;
use std::sync::atomic::{AtomicBool, Ordering};

fn claim_modal_result(closed: &AtomicBool, accepted: &AtomicBool) -> bool {
    !closed.load(Ordering::Acquire)
        && accepted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

fn cancel_modal_read(
    closed: &AtomicBool,
    reader: &Worker,
    return_focus: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    if closed.swap(true, Ordering::AcqRel) {
        return;
    }
    reader.cancel();
    window.close_dialog(cx);
    // The workspace embeds Root's dialog layer; invalidating Root alone can
    // leave its previously painted layer cached in the workspace view.
    window.refresh();
    if let Some(focus) = return_focus {
        focus.focus(window, cx);
    }
}

/// The key of the probe recording the ID path of the node that holds Quick
/// source's pane focus while it shows no text, for tests: GPUI derives a
/// node's ID from that path, so a path that stays is a focus that stays.
pub(crate) const QUICK_SOURCE_FOCUS_PATH: &str = "quick-source-focus";

#[derive(Default)]
pub(super) struct State {
    active: Option<Box<Inspection>>,
}
struct Inspection {
    context: file_history::ReturnContext,
    lineage: file_history::State,
    previous: State,
    target: Target,
    /// Whether focus waiting on Quick source's information goes to the text
    /// as the next read lands: set by the open and by a re-read that dropped
    /// the text focus was in ([`GitTurtle::focus_quick_source_text`]). Any
    /// other landing leaves focus where the person put it.
    text_focus_owed: bool,
    /// Whether Quick source's text pane holds the focus waiting on its file
    /// while it shows no text ([`GitTurtle::quick_source_holds_pane`]): set
    /// with `text_focus_owed`, by the open and by a re-read that dropped the
    /// text focus was in, and kept through a landing with no text, so focus
    /// stays on what the pane draws where the reading frame rang it. A
    /// landing that shows the text ends it, as does a re-read begun with the
    /// text shown and focus outside it: the information beside the text then
    /// holds the focus a click gives it there.
    pane_holds_focus: bool,
    /// The name Quick source's pane had as a re-read began that it holds
    /// focus through ([`GitTurtle::hold_quick_source_focus`]): the pane keeps
    /// it while the file is read, so the focused group is renamed only where
    /// the reason that lands differs, and the landing spends it
    /// ([`GitTurtle::land_quick_source`]). Renamed "Refreshing tracked
    /// file…" and back, it was spoken twice for one Refresh; kept past its
    /// landing, it named a later read with nothing shown, as of a file
    /// left with no content, by a reason that read was not.
    held_reason: Option<SharedString>,
    /// Whether the node holding the pane's focus is the image comparison's
    /// root ([`crate::image_compare::IMAGE_ROOT`]): set as a re-read begins
    /// with a decoded image holding it, and kept while the pane holds it
    /// with no text, so the reading placeholder and what lands in its place
    /// are drawn under the root's ID, and focus stays on one node. Drawn
    /// under an ID of its own, the placeholder was another node: focus moved
    /// to it and back to the image as it landed, each move spoken.
    held_on_image: bool,
    /// Quick source's file's mode as its last read found it
    /// ([`GitTurtle::land_quick_source`]), the working file's own for a
    /// working-tree source, kept while the file is read again; `None` until
    /// the first read lands, when the information shows the listing's.
    /// Read from Quick Open's listing alone, it never showed a `chmod +x`.
    landed_mode: Option<String>,
    /// The focus of the information's "Choose targets…", which the
    /// inspection owns, so Back from Settings or Projects, or a tab shown
    /// again, returns focus to it where the person left it: keyed by the
    /// render, the Button's own handle went while another page showed, and
    /// Back landed on the information.
    targets_focus: FocusHandle,
    /// Whether focus was in Quick source's text pane as a press opened the
    /// collapsed modes' menu ([`GitTurtle::note_mode_menu_press`]), which
    /// the menu's choice acts on as the menu is built from it
    /// ([`GitTurtle::take_mode_menu_held`]): the menu holds focus while its
    /// choice acts, so asked then, focus was never in the pane, and the
    /// menu's close gave the pane's focus to the information. A key opens
    /// the menu only from its Button, which holds focus, outside the pane,
    /// and finds nothing noted.
    mode_menu_held: std::cell::Cell<bool>,
}
enum Target {
    Comparison(RevisionComparison),
    File {
        scope: PathScope,
        entry: TrackedPath,
    },
}
impl State {
    pub(super) fn rescale_lists(&self, scales: settings::ListScales) {
        if let Some(active) = &self.active {
            active.lineage.rescale_lists(scales);
            active.previous.rescale_lists(scales);
        }
    }
    pub(super) fn pause_for_tab(&mut self) {
        if let Some(active) = &mut self.active {
            active.context.pause_for_tab();
            active.lineage.pause_for_tab();
            active.previous.pause_for_tab();
        }
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.active.as_ref().map_or(0, |active| {
            (match &active.target {
                Target::Comparison(comparison) => comparison
                    .files
                    .iter()
                    .map(repository_tabs::file_bytes)
                    .sum::<usize>(),
                Target::File { entry, .. } => {
                    entry.path.as_os_str().len() + entry.oid.capacity() + entry.mode.capacity()
                }
            }) + active.context.retained_bytes()
                + active.lineage.retained_bytes()
                + active.previous.retained_bytes()
        })
    }
    pub(super) fn rescale_code(&self, ratio: f32, cx: &mut App) {
        if let Some(active) = &self.active {
            active.context.rescale_code(ratio, cx);
            active.lineage.rescale_code(ratio, cx);
            active.previous.rescale_code(ratio, cx);
        }
    }
    pub(super) fn refresh_theme(&self, cx: &mut App) {
        if let Some(active) = &self.active {
            active.context.refresh_theme(cx);
            active.lineage.refresh_theme(cx);
            active.previous.refresh_theme(cx);
        }
    }
    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }
    fn depth(&self) -> usize {
        self.active
            .as_ref()
            .map_or(0, |active| 1 + active.previous.depth())
    }
}

/// A working file's scope, the last line of Quick source's identities, drawn
/// by phrase (`crate::PhraseLine`, its phrases named `{WORKING_SCOPE_ID}-{index}`),
/// so it breaks between its words and its "·" never starts a line
/// (`DESIGN.md`, one responsive order). Drawn as text, it wrapped "· raw local
/// content" onto a line of its own at 18 pt in a 140 px room.
const WORKING_SCOPE: &str = "Current working file · raw local content";
const WORKING_SCOPE_ID: &str = "revision-scope";

impl GitTurtle {
    pub(super) fn inspection_preview_origins(&self) -> Option<markdown_view::Origins> {
        match &self.revision_inspection.active.as_ref()?.target {
            Target::Comparison(comparison) => Some(markdown_view::Origins::revisions(
                Some(comparison.base_oid.clone()),
                Some(comparison.after.oid.clone()),
            )),
            Target::File {
                scope: PathScope::Revision(oid),
                ..
            } => Some(markdown_view::Origins::revisions(None, Some(oid.clone()))),
            Target::File {
                scope: PathScope::Worktree,
                ..
            } => Some(markdown_view::Origins {
                old: None,
                new: Some(gitturtle_core::PreviewAssetScope::Worktree),
            }),
        }
    }
    pub(super) fn open_revision_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_compare_form(window, cx);
    }

    /// Opens Compare local revisions, as [`Self::open_revision_comparison`]
    /// does, and returns its form, which a test waits on for its branch and
    /// tag choices (`focus_ring::sweep::Opened`).
    pub(super) fn open_compare_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<CompareForm>> {
        let repo = self
            .repository
            .clone()
            .filter(|_| self.operation_busy.is_none())?;
        let (before, after, mode) = match self
            .revision_inspection
            .active
            .as_deref()
            .map(|a| &a.target)
        {
            Some(Target::Comparison(c)) => (
                c.before.expression.clone(),
                c.after.expression.clone(),
                c.mode,
            ),
            _ => ("HEAD~1".into(), "HEAD".into(), ComparisonMode::Endpoints),
        };
        let owner = cx.entity().downgrade();
        let return_focus = window.focused(cx);
        let form = cx.new(|cx| {
            CompareForm::new(owner, repo, (before, after, mode), return_focus, window, cx)
        });
        let opened = form.clone();
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let confirm = form.clone();
            let cancel_action = form.clone();
            let shown = form.clone();
            dialog
                .title("Compare local revisions")
                .width(appearance::dialog_width(px(660.), window))
                .max_h(appearance::dialog_max_height(window))
                // In the dialog's content, as the profile form is: the form
                // takes what the cap leaves it and scrolls the rest,
                // revealing the control Tab moves onto, with the ring's room
                // below its last choice, which the footer's gap gives back.
                .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                .footer(
                    DialogFooter::new()
                        .child(
                            button("cancel-revision-comparison", "Cancel", "", false).on_click(
                                move |_, window, cx| {
                                    cancel.update(cx, |form, cx| form.cancel(window, cx))
                                },
                            ),
                        )
                        .child(
                            button("submit-revision-comparison", "Compare", "", true)
                                .disabled(form.read(cx).busy)
                                .on_click(move |_, window, cx| {
                                    confirm.update(cx, |form, cx| form.submit(window, cx))
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel_action.update(cx, |form, cx| form.cancel(window, cx));
                    false // cancel closes directly; do not pop another dialog.
                })
        });
        window.refresh();
        window.on_next_frame(move |window, cx| {
            let _ = focus_form.update(cx, |form, cx| {
                if !form.closed.load(Ordering::Acquire) {
                    form.before.read(cx).focus_handle(cx).focus(window, cx);
                }
            });
        });
        Some(opened)
    }

    pub(super) fn open_quick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_quick_form(window, cx);
    }

    /// Opens Quick Open, as [`Self::open_quick_file`] does, and returns its
    /// form, which a test waits on for its paths (`QuickForm::lands`).
    pub(super) fn open_quick_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<QuickForm>> {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return None;
        }
        let repo = self
            .repository
            .clone()
            .filter(|_| self.operation_busy.is_none())?;
        let revision = self
            .selected_commit
            .and_then(|i| self.commits.get(i))
            .map_or("HEAD", |c| &c.oid)
            .to_owned();
        let owner = cx.entity().downgrade();
        let return_focus = window.focused(cx);
        let form = cx.new(|cx| QuickForm::new(owner, repo, revision, return_focus, window, cx));
        #[cfg(test)]
        OPENED_QUICK_FORM.with(|opened| *opened.borrow_mut() = Some(form.downgrade()));
        let query_focus = form.read(cx).query.read(cx).focus_handle(cx);
        self.dialog_hints
            .register(&query_focus, shortcuts::DialogKeys::QuickOpen);
        let opened = form.clone();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let activate = form.clone();
            let cancel_action = form.clone();
            // A picker of files floats as the menus and the command palette
            // do: on the raised surface with their fitted edge, read on every
            // frame so a theme switch retints it with them.
            let surface = palette(cx);
            // The dialog clips its body to the body's bounds, and the form
            // keeps the focus ring's room inside them around its controls.
            // The title's margin and the footer's gap give the room back, so
            // nothing in the dialog moves.
            let room = appearance::button_ring_room(cx);
            dialog
                .title(
                    native_accessibility::dialog_title("quick-open-title", "Quick Open File")
                        .mb(-room),
                )
                .width(appearance::dialog_width(px(700.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - room)
                .bg(rgb(surface.raised()))
                .border_color(rgb(surface.raised_edge()))
                .child(form.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            shortcuts::DialogKeys::QuickOpen
                                .dismiss_button("cancel-quick-file", false)
                                .on_click(move |_, window, cx| {
                                    cancel.update(cx, |form, cx| form.cancel(window, cx))
                                }),
                        )
                        .child(
                            button("activate-quick-file", "Open file", "", true)
                                .disabled(
                                    form.read(cx).busy
                                        || form
                                            .read(cx)
                                            .page
                                            .as_ref()
                                            .is_none_or(|page| page.entries.is_empty()),
                                )
                                .on_click(move |_, window, cx| {
                                    activate.update(cx, |form, cx| form.activate(window, cx))
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.activate(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel_action.update(cx, |form, cx| form.cancel(window, cx));
                    false
                })
        });
        // Focus after open_alert_dialog installs its shell, in the same action.
        // A key-triggered redraw can run before any on_next_frame callback;
        // waiting for that callback drops the beginning of an immediate query.
        query_focus.focus(window, cx);
        window.refresh();
        Some(opened)
    }

    fn begin_revision_inspection(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.revision_inspection.depth() >= 4 {
            self.error = Some("Four inspections are open. Use Back before opening another.".into());
            cx.notify();
            return false;
        }
        // The comparison stays in the inspection's context for Back, so a
        // review being prepared for it ends here with its read.
        self.release_text_review(cx);
        self.invalidate_read();
        let context = self.take_inspection_context(window, cx);
        let lineage = std::mem::take(&mut self.file_history);
        let previous = std::mem::take(&mut self.revision_inspection);
        self.revision_inspection.active = Some(Box::new(Inspection {
            context,
            lineage,
            previous,
            target,
            text_focus_owed: false,
            pane_holds_focus: false,
            held_reason: None,
            held_on_image: false,
            landed_mode: None,
            targets_focus: cx.focus_handle(),
            mode_menu_held: Default::default(),
        }));
        self.file_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.blame.hide();
        self.clear_preview();
        self.mode = WorkspaceMode::Compare;
        self.page = AppPage::Repository;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(self.files_focus(), cx);
        true
    }

    pub(super) fn show_revision_comparison(
        &mut self,
        comparison: RevisionComparison,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files = comparison.files.clone();
        if !self.begin_revision_inspection(Target::Comparison(comparison), window, cx) {
            return;
        }
        self.files = files;
        self.refresh_file_filter(window, cx);
        self.selected_file = None;
        self.status = format!(
            "{} · pinned local revisions · open a file to compare",
            crate::changed_files(self.files.len())
        );
        cx.notify();
    }

    pub(super) fn show_tracked_file(
        &mut self,
        scope: PathScope,
        entry: TrackedPath,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let mut file = repo.tracked_path_change(&entry);
        if matches!(scope, PathScope::Worktree) {
            file.new_oid = None;
        }
        if !self.begin_revision_inspection(
            Target::File {
                scope: scope.clone(),
                entry: entry.clone(),
            },
            window,
            cx,
        ) {
            return;
        }
        self.files = vec![file];
        self.refresh_file_filter(window, cx);
        self.selected_file = Some(0);
        self.text_mode = TextMode::After;
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.text_focus_owed = true;
            active.pane_holds_focus = true;
        }
        // The frame that opens it rings the focus waiting for the text
        // ([`Self::quick_source_holds_pane`]) however Quick Open was
        // answered, by a click as by a key, as the text's landing does.
        self.focus_ring.mark(window);
        self.status = "Tracked source file · browsing is local and read-only".into();
        self.request(
            Job::TrackedPreview { repo, entry, scope },
            "Reading tracked file…",
            window,
            cx,
        );
    }

    pub(super) fn close_revision_inspection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(active) = self.revision_inspection.active.take() else {
            return false;
        };
        self.invalidate_read();
        self.clear_preview();
        self.revision_inspection = active.previous;
        self.file_history = active.lineage;
        self.restore_inspection_context(active.context, window, cx);
        if self.content.is_none() && self.mode != WorkspaceMode::History {
            if self.mode == WorkspaceMode::Working {
                if let Some((index, area)) = self.working_selected {
                    // Closed under a dialog or sheet, as by a write
                    // confirmed there, the row's list becomes where the
                    // modal returns, in place of this inspection's control
                    // it was opened from.
                    self.select_restored_working(index, area, window, cx);
                }
            } else {
                self.read_selected_preview(window, cx);
            }
        }
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| this.try_automatic_refresh(window, cx));
        });
        cx.notify();
        true
    }

    pub(super) fn is_quick_source(&self) -> bool {
        !self.file_history.is_active()
            && matches!(
                self.revision_inspection
                    .active
                    .as_deref()
                    .map(|active| &active.target),
                Some(Target::File { .. })
            )
    }

    /// Where focus on the changed files lands: their list, or in Quick
    /// source, which draws none, the inspector's tracked source information
    /// beside the file, which takes the window's shortcuts there while the
    /// file is read, and then gives focus to its text
    /// ([`Self::focus_quick_source_text`]).
    pub(super) fn files_focus(&self) -> &FocusHandle {
        if self.is_quick_source() {
            &self.source_focus
        } else {
            &self.file_focus
        }
    }

    /// Whether Quick source's text pane holds the focus that waits on its
    /// file ([`Self::files_focus`]) while it shows no text
    /// ([`Inspection::pane_holds_focus`]): as Quick source opens and through
    /// a re-read that held the text's own focus, the placeholder that stands
    /// where the text will be drawn, and where the file lands with no text,
    /// as an image, a binary, an over-limit file or a failed read, what the
    /// pane draws in its place ([`Self::quick_source_pane`]). The pane rings
    /// its edge for it, so the frame that opens Quick source draws its ring
    /// where the text, as it lands, takes focus and draws the same ring, and
    /// a landing with no text leaves focus and the ring where they were, an
    /// image's as any other's, on the comparison's own root
    /// (`render_image_comparison`). Waiting on the information beside the
    /// file, which drew no ring while the file was read, focus showed nowhere
    /// until the text landed (QA-109); moved there by a landing with no text,
    /// the ring jumped with no key pressed and a second focus was spoken, as
    /// it still was for an image.
    pub(super) fn quick_source_holds_pane(&self) -> bool {
        self.is_quick_source()
            && !matches!(self.shown_text(), Some(views::ShownText::Editor(_)))
            && self
                .revision_inspection
                .active
                .as_deref()
                .is_some_and(|active| active.pane_holds_focus)
    }

    /// Whether focus is in Quick source's text pane now, on any stop drawn
    /// there: on the pane, or on anything drawn inside it, while it holds
    /// the waiting focus ([`Self::quick_source_holds_pane`]); on the text it
    /// shows or its open Find bar, drawn beside the text
    /// ([`crate::editor_find::bar_contains_focused`]); on the rendered
    /// Markdown's own stop; or on a control of the Diagrams view, as Next
    /// diagram ([`crate::rich_preview::holds_focus`]); each of the last two
    /// drawn with no pane around it once focus has left the pane. A change
    /// of mode moves focus found there to what the new mode shows
    /// ([`Self::keep_quick_source_pane_focus`]); Find's field and the
    /// diagrams' buttons were left out, so focus stayed on a control the
    /// new mode no longer draws.
    pub(super) fn quick_source_text_pane_is_focused(&self, window: &Window, cx: &App) -> bool {
        if !self.is_quick_source() {
            return false;
        }
        let pane = self.quick_source_holds_pane() && self.source_focus.contains_focused(window, cx);
        let text = match self.shown_text() {
            Some(views::ShownText::Editor(editor)) => {
                editor
                    .read(cx)
                    .focus_handle(cx)
                    .contains_focused(window, cx)
                    || crate::editor_find::bar_contains_focused(editor, window, cx)
            }
            _ => false,
        };
        let rendered = self.text_mode == TextMode::Markdown
            && self
                .markdown_view
                .as_ref()
                .is_some_and(|view| view.read(cx).focus_handle().contains_focused(window, cx));
        let diagrams =
            self.text_mode == TextMode::Diagrams && crate::rich_preview::holds_focus(window);
        pane || text || rendered || diagrams
    }

    /// After a change of comparison mode, focus that was in Quick source's
    /// text pane as the mode changed (`held`,
    /// [`Self::quick_source_text_pane_is_focused`]) goes where a landing in
    /// the new mode puts it ([`Self::focus_quick_source_text`]): to the text
    /// where it shows the text, and otherwise to the pane, named by the view
    /// it draws ("Rendered Markdown", "Mermaid diagrams"), which holds it
    /// ([`Inspection::pane_holds_focus`]), ringed on the same edge either
    /// way. Focus anywhere else stays where it is and the pane holds none,
    /// as through a re-read ([`Self::hold_quick_source_focus`]). Source
    /// chosen over the rendered view's pane gave focus, left on the pane's
    /// handle, to the information beside the text, ringed there; Rendered or
    /// Diagrams chosen from the text, or Source from the rendered view's own
    /// stop, left focus on a view no longer drawn; and left set from a
    /// rendered view that had held it before Source showed the text, the
    /// pane took the information's focus as Rendered was chosen again.
    pub(super) fn keep_quick_source_pane_focus(
        &mut self,
        held: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_quick_source() {
            return;
        }
        let text = match self.shown_text() {
            Some(views::ShownText::Editor(editor)) => Some(editor.read(cx).focus_handle(cx)),
            _ => None,
        };
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.pane_holds_focus = held && text.is_none();
        }
        if !held {
            return;
        }
        text.as_ref()
            .unwrap_or(&self.source_focus)
            .focus(window, cx);
        self.focus_ring.mark(window);
    }

    /// Notes, as a press opens the collapsed modes' menu, whether focus is
    /// in Quick source's text pane ([`Inspection::mode_menu_held`]).
    pub(super) fn note_mode_menu_press(&self, window: &Window, cx: &App) {
        let held = self.quick_source_text_pane_is_focused(window, cx);
        if let Some(active) = self.revision_inspection.active.as_deref() {
            active.mode_menu_held.set(held);
        }
    }

    /// Whether focus was in Quick source's text pane as the collapsed modes'
    /// menu opened, taken as the menu is built ([`Inspection::mode_menu_held`]).
    pub(super) fn take_mode_menu_held(&self) -> bool {
        self.revision_inspection
            .active
            .as_deref()
            .is_some_and(|active| active.mode_menu_held.take())
    }

    /// Quick source's text pane drawing `content` in place of its text while
    /// it holds the waiting focus ([`Self::quick_source_holds_pane`]): a
    /// group named by why there is no text, the read under way, the notice,
    /// the failure or the picture, so that reason is the one thing spoken as
    /// focus rests there. A line or notice drawn as static text is left out
    /// of the accessibility tree, as the group's name already says it: the
    /// reading line was read twice, "Reading tracked file…" and "Reading
    /// tracked file…. ". The information beside it stays one Tab away.
    pub(super) fn quick_source_pane(&self, content: AnyElement) -> AnyElement {
        if !self.quick_source_holds_pane() {
            return content;
        }
        // A decoded image's comparison root holds it, its sides inside it.
        if self.quick_source_image_holds() {
            return content;
        }
        let (reason, said) = self.quick_source_reason();
        // Where the image held focus as a re-read began, the image's root's
        // ID, at the same place, so focus stays on its node
        // ([`Inspection::held_on_image`]).
        let held_on_image = self
            .revision_inspection
            .active
            .as_deref()
            .is_some_and(|active| active.held_on_image);
        let pane = div()
            .id(if held_on_image {
                crate::image_compare::IMAGE_ROOT
            } else {
                "quick-source-pane"
            })
            .debug_selector(|| "quick-source-pane".into())
            .size_full()
            .role(Role::Group)
            .aria_label(reason)
            // A stop where the text will be one, so Tab and Shift+Tab meet
            // the stops they meet from the text, and come back to it. The
            // handle's stop is shared by its clones and read as it is
            // tracked, so the information, tracking it in other frames,
            // sets it back.
            .track_focus(&self.source_focus.clone().tab_stop(true))
            .children(native_accessibility::node_path_probe(|| {
                QUICK_SOURCE_FOCUS_PATH.into()
            }))
            .child(native_accessibility::hidden_from_assistive_technology(
                div().size_full().child(content),
                said,
            ));
        // An image that decoded into nothing keeps the tree its comparison
        // writes, read as in any comparison, its sides' names and the line
        // that says why: the group above it is the pane's, not the image's,
        // its node written for tests as it is built, outside the wrappers
        // whose nesting that tree keeps. Anything else is wrapped to write
        // its node where a test reads it.
        if matches!(self.content.as_deref(), Some(Content::Images { .. })) {
            #[cfg(test)]
            native_accessibility::written::built(&pane);
            pane.into_any_element()
        } else {
            native_accessibility::clips_children(pane).into_any_element()
        }
    }

    /// Whether the image comparison's own root holds the pane's focus
    /// ([`Self::quick_source_holds_pane`]), named by its format and size
    /// (`render_image_comparison`): only where the source's image decoded.
    /// Where nothing decoded the pane holds it, named by the failure, and
    /// the image's root takes no role between the pane and the area that
    /// says why ("Source details").
    pub(super) fn quick_source_image_holds(&self) -> bool {
        self.quick_source_holds_pane()
            && self.error.is_none()
            && matches!(
                self.content.as_deref(),
                Some(Content::Images { new, .. }) if new.image.is_some()
            )
    }

    /// Why Quick source's pane shows no text, the name of what holds focus
    /// there ([`Self::quick_source_pane`]), and whether what the pane draws
    /// says no more than that, so it is left out of the accessibility tree:
    /// the read under way, named as the landing before it named the pane
    /// where a re-read holds its focus ([`Inspection::held_reason`]), or
    /// "No preview" where nothing is read, as the pane draws it; a
    /// failed read by its first line that says anything, as Stashes says
    /// one; an over-limit file by its notice's own words; a drawn preview, a
    /// PDF's page or a model, as "{format} preview"; a format its metadata
    /// alone names, as WAV or ZIP, as "{format} · no preview", and bytes of
    /// no encoding it knows as "Binary file · no preview"; "File" where no
    /// format is given; an image by its format and size; text by its view,
    /// or as loading only while that view is still being built. The name is
    /// the pane's label alone, never drawn as a line of phrases.
    pub(super) fn quick_source_reason(&self) -> (SharedString, bool) {
        if let Some(error) = &self.error {
            return (
                crate::recovery::first_line(error).map_or(
                    SharedString::new_static("Preview unavailable"),
                    native_accessibility::bounded_accessible_text,
                ),
                true,
            );
        }
        match self.content.as_deref() {
            None => (
                self.revision_inspection
                    .active
                    .as_deref()
                    .and_then(|active| active.held_reason.clone())
                    .unwrap_or_else(|| self.loading.unwrap_or("No preview").into()),
                true,
            ),
            Some(Content::Notice(message)) => (message.clone().into(), true),
            Some(Content::Images { new, .. }) => {
                (crate::image_compare::source_image_name(new), false)
            }
            Some(Content::Rich(preview)) => {
                let side = &preview.new;
                let format = side.metadata.format.trim();
                let reason = if format.is_empty() {
                    "File".into()
                } else if side.pdf.is_some() || side.model.is_some() || !side.pages.is_empty() {
                    format!("{format} preview").into()
                } else if format == gitturtle_preview::metadata::UNKNOWN_ENCODING {
                    "Binary file · no preview".into()
                } else {
                    format!("{format} · no preview").into()
                };
                (reason, false)
            }
            Some(Content::Text { .. } | Content::Conflict(_)) => {
                // Only an editor still to build is hidden behind the
                // loading line: a rendered view drawn in its place is the
                // person's to read.
                let view = match (self.content.as_deref(), self.text_mode) {
                    (Some(Content::Conflict(_)), _) => {
                        self.conflict_view.as_ref().map(|_| "Conflict")
                    }
                    (_, TextMode::Markdown) => {
                        self.markdown_view.as_ref().map(|_| "Rendered Markdown")
                    }
                    (_, TextMode::Diagrams) => Some("Mermaid diagrams"),
                    _ => None,
                };
                view.map_or(("Loading text…".into(), true), |view| {
                    (view.into(), false)
                })
            }
        }
    }

    /// Quick source's file has been read: focus still waiting in its pane
    /// ([`Self::quick_source_holds_pane`]) moves to the source text, which
    /// draws its ring on the pane's edge and takes the window's shortcuts. A
    /// file with no text to show, as an image or a binary, leaves it in the
    /// pane, on what the pane draws, with the same ring. Either way the ring
    /// shows however Quick Open was answered, by a click as by a key, as
    /// Settings opens. Focus the person has moved elsewhere while the file
    /// was read stays there, as does focus on the information after a later
    /// re-read: only the open and a re-read that held the pane's or the
    /// text's own focus are owed the text ([`Inspection::text_focus_owed`]).
    /// Landed while another page shows, the text takes it on the way back
    /// ([`Self::quick_source_return_focus`]).
    pub(super) fn focus_quick_source_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Still reading: the landing answers, with the flag still set.
        if self.content.is_none() {
            return;
        }
        // Taken by every landing, so one that left focus elsewhere owes
        // nothing to a later re-read.
        let owed = self
            .revision_inspection
            .active
            .as_deref_mut()
            .is_some_and(|active| std::mem::take(&mut active.text_focus_owed));
        // The text shown, the pane holds no focus of its own any more.
        let text = match self.shown_text() {
            Some(views::ShownText::Editor(editor)) => Some(editor.read(cx).focus_handle(cx)),
            _ => None,
        };
        if text.is_some()
            && let Some(active) = self.revision_inspection.active.as_deref_mut()
        {
            active.pane_holds_focus = false;
            active.held_on_image = false;
        }
        if !owed || !self.is_quick_source() || !self.source_focus.is_focused(window) {
            return;
        }
        // With no text, focus stays where the pane holds it.
        if let Some(text) = text {
            text.focus(window, cx);
        }
        self.focus_ring.mark(window);
    }

    /// Where the window, back on the repository page, returns `focus`, the
    /// control it held as it left (`resume_repository_page`), or, where that
    /// control's handle went with the page, the information beside the text,
    /// or the pane where no editor is shown ([`Self::quick_source_pane`])
    /// (`return_tab_focus`'s fallback, [`Self::files_focus`]), never the
    /// window's root. A read that landed while another page showed is
    /// settled here, as its landing on this page settles it
    /// ([`Self::focus_quick_source_text`]): whatever it was owed is taken
    /// ([`Inspection::text_focus_owed`]), whatever it shows, and where it
    /// shows the text, the pane holds no focus of its own. Only Quick
    /// source's pane, which held focus while the file was read, gives it to
    /// that text; Rendered Markdown, diagrams, an image or a notice leave it
    /// on the pane. Given back to the pane's handle, which the information
    /// beside the text tracks once the text is shown, focus and its ring
    /// rested on the information; left owed where focus had left the pane,
    /// the text took focus from the information when a later Refresh landed,
    /// with no key pressed; and left owed where the landing showed no
    /// editor, as Rendered Markdown, it still did once Source was chosen.
    pub(super) fn quick_source_return_focus(
        &mut self,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let held = focus == self.source_focus;
        match self.settle_quick_source_landing(cx) {
            Some(text) if held => text,
            _ => focus,
        }
    }

    /// Settles Quick source's read that has landed, as its landing on the
    /// repository page does ([`Self::focus_quick_source_text`]): what it was
    /// owed is taken, whatever it shows, and where it shows the text the
    /// pane holds no focus of its own. The text's focus, where it shows the
    /// text and was owed it; nothing while the file is still read, whose
    /// landing answers. Back settles a read that landed while another page
    /// showed ([`Self::quick_source_return_focus`]), as does a warm tab
    /// shown again after it (`restore_warm_tab`), each giving the text the
    /// focus the pane held as the window left.
    pub(super) fn settle_quick_source_landing(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        if !self.is_quick_source() || self.content.is_none() {
            return None;
        }
        let text = match self.shown_text() {
            Some(views::ShownText::Editor(editor)) => Some(editor.read(cx).focus_handle(cx)),
            _ => None,
        };
        let active = self.revision_inspection.active.as_deref_mut()?;
        let owed = std::mem::take(&mut active.text_focus_owed);
        let text = text?;
        active.pane_holds_focus = false;
        active.held_on_image = false;
        owed.then_some(text)
    }

    /// A read of Quick source's file has landed with `mode`, the file's mode
    /// as read: the information shows it from now on, through the next read
    /// until that one lands ([`Inspection::landed_mode`]), and the name the
    /// pane held through the read is spent ([`Inspection::held_reason`]).
    pub(super) fn land_quick_source(&mut self, mode: String) {
        if !self.is_quick_source() {
            return;
        }
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.landed_mode = Some(mode);
            active.held_reason = None;
        }
    }

    /// A read of Quick source's file ended with nothing to show, failed or
    /// dropped unanswered: what it owed the text is settled, so a later read
    /// that succeeds leaves focus where the person has put it, unless that
    /// read begins with focus still on the pane, which owes it the text again
    /// ([`Self::hold_quick_source_focus`]); and the name and the image's node
    /// the pane held through the read are spent ([`Inspection::held_reason`],
    /// [`Inspection::held_on_image`]), as a landing spends them, so a pane
    /// left with nothing read is named by what it draws, "No preview"
    /// ([`Self::quick_source_reason`]). Kept past a dropped reply, they named
    /// that pane by an image it no longer showed, or "Reading tracked file…"
    /// with nothing reading.
    pub(super) fn settle_quick_source_focus(&mut self) {
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.text_focus_owed = false;
            active.held_reason = None;
            active.held_on_image = false;
        }
    }

    /// Before a re-read of Quick source's file drops its text: focus held
    /// inside that text moves to the information beside it, as it waits there
    /// while the file is first read ([`Self::files_focus`]), and the read's
    /// landing gives it back ([`Self::focus_quick_source_text`]). Left in the
    /// editor, focus rested on nothing once it had gone, after Refresh or a
    /// write's landing with no key pressed, and the window's keys reached no
    /// control until the next click.
    ///
    /// The pane already holding focus over a file with no text keeps it and
    /// its name through the read ([`Inspection::held_reason`]), and is owed
    /// the text should text land, a failed read's pane as any other: focus
    /// never left it, so the text takes it where the pane rang it. Owed
    /// nothing after a failure, the text that landed left focus to the
    /// information beside it, which took it and drew its ring with no key
    /// pressed. Focus moved off the pane after a failure owes the text
    /// nothing ([`Self::settle_quick_source_focus`]).
    fn hold_quick_source_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_quick_source() {
            return;
        }
        let holds = self.quick_source_holds_pane() && self.source_focus.is_focused(window);
        // Only a pane that shows something holds its name: one left with
        // nothing, as each way back to a file left with no content leaves
        // it before reading it again (`read_selected_preview`), is named by
        // the read, so a name no frame drew is never held.
        let held = (holds && (self.content.is_some() || self.error.is_some()))
            .then(|| self.quick_source_reason().0);
        let image = self.quick_source_image_holds();
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.text_focus_owed |= holds;
            // The image's root, or a pane already drawn under its ID.
            active.held_on_image = held.is_some() && (image || active.held_on_image);
            active.held_reason = held;
        }
        // With the text shown the pane holds no focus of its own: through
        // the read it holds the text's, where the text held it, and nothing
        // otherwise. Left set from a rendered view that had held it before
        // Source built the text, the reading pane took the information's
        // focus and ring for the read and gave them back as it landed.
        let Some(views::ShownText::Editor(editor)) = self.shown_text() else {
            return;
        };
        let in_text = editor
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx);
        if in_text {
            self.source_focus.focus(window, cx);
        }
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.text_focus_owed |= in_text;
            active.pane_holds_focus = in_text;
        }
    }

    /// Reads the selected file's preview again without activating it, as
    /// it was first read: a Quick Open source through its own tracked read,
    /// whose working-tree side has no object to read, any other comparison
    /// through `load_file`. Each way back to a comparison left with no
    /// content reads it here: an inspection, File history or attribution
    /// closed over it, an LFS download for it, a warm tab shown again
    /// (`resume_tab_preview`). Read through `load_file`, a working-tree
    /// source asked for a comparison with neither side's object, and showed
    /// an empty file.
    pub(super) fn read_selected_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.reload_tracked_inspection(window, cx)
            && let Some(index) = self.selected_file
        {
            self.load_file(index, window, cx);
        }
    }

    fn reload_tracked_inspection(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.is_quick_source() {
            return false;
        }
        let Some(Target::File { scope, entry }) = self
            .revision_inspection
            .active
            .as_deref()
            .map(|active| &active.target)
        else {
            return false;
        };
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let job = Job::TrackedPreview {
            repo,
            entry: entry.clone(),
            scope: scope.clone(),
        };
        self.hold_quick_source_focus(window, cx);
        self.clear_preview();
        self.request(job, "Reading tracked file…", window, cx);
        true
    }

    fn defer_inspection_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let generation = self.generation;
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == path
                    && this.generation == generation
                    && this.page == AppPage::Repository
                {
                    this.files_focus().focus(window, cx);
                    let failed = this.error.is_some();
                    if let Some(active) = this.revision_inspection.active.as_deref_mut() {
                        active.text_focus_owed = !failed;
                    }
                    // A file read before this frame has already landed, and
                    // this frame's focus would otherwise rest above it.
                    this.focus_quick_source_text(window, cx);
                }
            });
        });
    }

    pub(super) fn inspection_blame_target(
        &self,
        index: usize,
    ) -> Option<gitturtle_core::BlameTarget> {
        use gitturtle_core::BlameTarget;
        let file = self.files.get(index)?;
        match &self.revision_inspection.active.as_ref()?.target {
            Target::Comparison(c) => {
                let before = self.text_mode == TextMode::Before || file.new_path.is_none();
                Some(BlameTarget::Committed {
                    oid: if before {
                        c.base_oid.clone()
                    } else {
                        c.after.oid.clone()
                    },
                    path: if before {
                        file.old_path.clone()?
                    } else {
                        file.new_path.clone()?
                    },
                })
            }
            Target::File {
                scope: PathScope::Worktree,
                entry,
            } => Some(BlameTarget::Working {
                path: entry.path.clone(),
            }),
            Target::File {
                scope: PathScope::Revision(oid),
                entry,
            } => Some(BlameTarget::Committed {
                oid: oid.clone(),
                path: entry.path.clone(),
            }),
        }
    }

    pub(super) fn inspection_history_target(&self, index: usize) -> Option<(String, PathBuf)> {
        match self.inspection_blame_target(index)? {
            gitturtle_core::BlameTarget::Committed { oid, path } => Some((oid, path)),
            gitturtle_core::BlameTarget::Working { path } => {
                Some((self.work_status.as_ref()?.head.clone()?, path))
            }
        }
    }

    pub(super) fn refresh_revision_inspection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(active) = self.revision_inspection.active.as_ref() else {
            return false;
        };
        match &active.target {
            Target::Comparison(_) => self.open_revision_comparison(window, cx),
            Target::File { scope, entry } => {
                if let Some(repo) = self.repository.clone() {
                    let job = Job::TrackedPreview {
                        repo,
                        scope: scope.clone(),
                        entry: entry.clone(),
                    };
                    self.hold_quick_source_focus(window, cx);
                    self.clear_preview();
                    self.request(job, "Refreshing tracked file…", window, cx);
                }
            }
        }
        true
    }

    pub(super) fn render_revision_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(active) = self.revision_inspection.active.as_ref() else {
            return div().into_any_element();
        };
        // The identities as drawn, and a working file's scope beneath them,
        // drawn by phrase ([`WORKING_SCOPE`]).
        let (title, details, scope_line) = match &active.target {
            Target::Comparison(c) => (
                SharedString::new_static("Revision comparison"),
                format!(
                    "Before: {}\n{}\n\nAfter: {}\n{}\n\n{}{}",
                    c.before.expression,
                    c.before.oid,
                    c.after.expression,
                    c.after.oid,
                    if c.mode == ComparisonMode::Endpoints {
                        "Endpoints · Before → After"
                    } else {
                        "Changes since branching · merge base → After"
                    },
                    if c.mode == ComparisonMode::SinceBranching {
                        format!("\nMerge base: {}", c.base_oid)
                    } else {
                        String::new()
                    }
                ),
                None,
            ),
            // The file's mode leads its identities: Quick source draws no
            // footer beneath its pane (`render_preview`), which carried it.
            // Beside the title, "Tracked file · mode 100644" wrapped its "·"
            // at 18 pt in a 461 × 490 window. The blank line closing the
            // mode and path keeps the scope a line apart, as one text drew
            // it.
            Target::File { scope, entry } => {
                let identities = format!(
                    "Mode {}\n{}\n",
                    active.landed_mode.as_deref().unwrap_or(&entry.mode),
                    entry.path.display(),
                );
                match scope {
                    PathScope::Worktree => ("Tracked file".into(), identities, Some(WORKING_SCOPE)),
                    PathScope::Revision(oid) => (
                        "Tracked file".into(),
                        format!("{identities}\nRevision {oid}"),
                        None,
                    ),
                }
            }
        };
        // Read as one text, the scope its last line, as it was drawn.
        let spoken: SharedString = match scope_line {
            Some(scope) => format!("{details}\n{scope}").into(),
            None => details.clone().into(),
        };
        div().id("revision-inspector").debug_selector(|| "revision-inspector".into()).size_full().min_h_0().flex().flex_col().bg(rgb(p.panel)).border_l_1().border_color(rgb(p.border))
            // Quick source omits the changed-file list. Focus waits on this
            // visible inspector while the file is read (`files_focus`), so
            // the app's shortcuts work, and moves to the text as it lands
            // (`focus_quick_source_text`); the inspector holds no selection
            // and its controls are their own stops, so it is no Tab stop.
            .when(self.is_quick_source(), |element| element.relative().role(Role::Group).aria_label("Tracked source information"))
            // No stop here: the handle's stop is set as it is tracked, and
            // the pane makes it one while it holds it.
            .when(self.is_quick_source() && !self.quick_source_holds_pane(), |element| element.track_focus(&self.source_focus.clone().tab_stop(false)))
            .child(crate::native_accessibility::clips_children(div().id("revision-metadata").min_h_0().overflow_y_scroll().flex_shrink_0()
                .when(!self.is_quick_source(), |element| element.max_h(px(260.)))
                .p_3().flex().flex_col().gap_2()
                .child(native_accessibility::text("revision-title", title).font_weight(FontWeight::SEMIBOLD))
                .child(native_accessibility::static_text(div().id("revision-identities").debug_selector(|| "revision-identities".into()).child(details).children(scope_line.map(|scope| crate::PhraseLine::new(WORKING_SCOPE_ID, scope))), spoken).text_size(crate::appearance::ui_text(11.)))
                .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Targets stay pinned while you inspect files. Refresh reviews current refs. No checkout or download."))
                .child(button("change-revisions","Choose targets…","",false).track_focus(&active.targets_focus).on_click(cx.listener(|this,_,window,cx|this.open_revision_comparison(window,cx))))))
            .when(!self.is_quick_source(), |element| element.child(self.render_files(cx)))
            // Where a click has put focus here beside the text, and through a
            // re-read that leaves it here. Not while the text pane holds it,
            // reading the file or showing no text, and draws its ring.
            .when(self.is_quick_source() && !self.quick_source_holds_pane(), |element| {
                element.children(self.focus_ring.edge(&self.source_focus, rgb(p.accent)).map(|edge| {
                    edge.debug_selector(|| "quick-source-information-ring".into())
                }))
            })
            .into_any_element()
    }
}

/// The width walk (`width_walk`) reads Compare local revisions once its
/// branch and tag choices have landed. They are read on the form's own
/// worker, which neither `run_until_parked` nor the drain of the app's
/// executor waits for, and a reply that landed mid-walk added the choices
/// matching the field the walk read. A read that answers lists HEAD at least.
#[cfg(test)]
impl crate::focus_ring::sweep::Opened for Option<Entity<CompareForm>> {
    fn finish(self, cx: &mut gpui::VisualTestContext) {
        let Some(compare) = self else {
            return;
        };
        // The worker answers before it idles, and the reply lands on this
        // thread when it next runs.
        compare.read_with(cx, |compare, _| compare.worker.wait_idle());
        cx.executor().run_until_parked();
        if compare.read_with(cx, |compare, _| compare.targets.is_empty()) {
            // A read that answers lists HEAD at least, so this one failed,
            // whose error the form drops, or its reply never landed: the
            // same read again on the form's worker names which.
            let again = compare.read_with(cx, |compare, _| {
                compare.worker.submit(Job::RevisionTargets {
                    repo: compare.repo.clone(),
                })
            });
            let again = futures::executor::block_on(again).map(|read| read.map(|_| ()));
            panic!(
                "the comparison's branch and tag choices never landed; read again, they answer \
                 {again:?}"
            );
        }
    }
}

pub(crate) struct CompareForm {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    before: Entity<InputState>,
    after: Entity<InputState>,
    mode: ComparisonMode,
    targets: Vec<String>,
    selected_side: usize,
    worker: Arc<Worker>,
    task: Option<Task<()>>,
    busy: bool,
    error: Option<String>,
    subscriptions: Vec<Subscription>,
    closed: Arc<AtomicBool>,
    accepted: Arc<AtomicBool>,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    /// The form's scroll, which reveals the control Tab moves onto: its
    /// choices lie below its fold in a half tile.
    body: crate::focus_reveal::FocusReveal,
}
impl CompareForm {
    /// Empties the field the choices follow, so the first branches and tags
    /// the read lists are its choices, for the walk of dialogs' ghost
    /// Buttons (`dialog_sides`).
    #[cfg(test)]
    pub(crate) fn show_every_choice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = if self.selected_side == 0 {
            &self.before
        } else {
            &self.after
        };
        field.update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        initial: (String, String, ComparisonMode),
        return_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (before, after, mode) = initial;
        let before = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(before)
                .placeholder("Branch, tag, or commit revision")
        });
        let after = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(after)
                .placeholder("Branch, tag, or commit revision")
        });
        let mut form = Self {
            owner,
            repo,
            before,
            after,
            mode,
            targets: vec![],
            selected_side: 0,
            worker: Arc::new(Worker::new()),
            task: None,
            busy: false,
            error: None,
            subscriptions: vec![],
            closed: Arc::new(AtomicBool::new(false)),
            accepted: Arc::new(AtomicBool::new(false)),
            focus: cx.focus_handle(),
            return_focus,
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
        };
        // Return compares through the dialog's default action alone, which
        // runs once per press, as the key goes down, never on a held key's
        // repeats, under X11 too. The field's own `PressEnter`, which it
        // sends for every Enter key-down, submitted the form as well, so
        // after a comparison that failed at once each repeat read another.
        for (side, input) in [form.before.clone(), form.after.clone()]
            .into_iter()
            .enumerate()
        {
            form.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, _, event, _, cx| {
                    if let InputEvent::Change | InputEvent::Focus = event {
                        this.selected_side = side;
                        cx.notify();
                    }
                },
            ));
        }
        let response = form.worker.submit(Job::RevisionTargets {
            repo: form.repo.clone(),
        });
        form.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.closed.load(Ordering::Acquire) {
                    return;
                }
                if let Ok(Ok(Output::RevisionTargets(targets))) = result {
                    this.targets = targets;
                }
                cx.notify();
            });
        }));
        form
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.task = None;
        self.busy = false;
        cancel_modal_read(
            &self.closed,
            &self.worker,
            self.return_focus.as_ref(),
            window,
            cx,
        );
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.closed.load(Ordering::Acquire) || self.accepted.load(Ordering::Acquire)
        {
            return;
        }
        self.busy = true;
        self.error = None;
        // Disabled inputs leave the focus tree. Keep Escape in this modal
        // while the worker resolves its captured values.
        self.focus.focus(window, cx);
        let response = self.worker.submit(Job::CompareRevisions {
            repo: self.repo.clone(),
            before: self.before.read(cx).value().to_string(),
            after: self.after.read(cx).value().to_string(),
            mode: self.mode,
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.closed.load(Ordering::Acquire) {
                    return;
                }
                this.busy = false;
                match result {
                    Ok(Ok(Output::RevisionComparison(comparison))) => {
                        let path = this.repo.path().to_owned();
                        let _ = this.owner.update(cx, |owner, cx| {
                            if owner.path.as_ref() == Some(&path) && owner.operation_busy.is_none()
                            {
                                if !claim_modal_result(&this.closed, &this.accepted) {
                                    return;
                                }
                                this.closed.store(true, Ordering::Release);
                                this.worker.cancel();
                                window.close_dialog(cx);
                                window.refresh();
                                owner.show_revision_comparison(comparison, window, cx);
                                owner.defer_inspection_focus(window, cx);
                            }
                        });
                    }
                    Ok(Err(e)) => this.error = Some(format!("{e:#}")),
                    _ => {
                        this.error = Some("Comparison cancelled or unavailable. Try again.".into())
                    }
                }
                if !this.closed.load(Ordering::Acquire) {
                    let form = cx.entity().downgrade();
                    window.on_next_frame(move |window, cx| {
                        let _ = form.update(cx, |form, cx| {
                            if !form.closed.load(Ordering::Acquire) && !form.busy {
                                let input = if form.selected_side == 0 {
                                    &form.before
                                } else {
                                    &form.after
                                };
                                input.read(cx).focus_handle(cx).focus(window, cx);
                            }
                        });
                    });
                }
                window.refresh();
                cx.notify();
            });
        }));
        window.refresh();
        cx.notify();
    }
}
impl Render for CompareForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let query = if self.selected_side == 0 {
            self.before.read(cx).value()
        } else {
            self.after.read(cx).value()
        }
        .to_lowercase();
        let choices: Vec<_> = self
            .targets
            .iter()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .take(8)
            .cloned()
            .collect();
        // The choices, ghost Buttons at the dialog's text edge, stand their
        // labels on that edge, their frames their pull before it, and wrap
        // rather than run past the column (DESIGN.md, "One left edge"); the
        // form keeps the pull with the ring's room on its left, and the
        // ring's room on its right, given back through its margins, so its
        // clip cuts neither them nor the fields' rings. The mode toggles, the
        // chosen one filled, keep their frames at the edge.
        let room = crate::appearance::button_ring_room(cx);
        let lead = crate::ghost_pull(window) + room;
        // A field or Button Tab moves onto scrolls into view with its ring's
        // room (`focus_reveal`), and the room around the controls keeps
        // their rings whole at the form's edges, given back through its
        // margin, its cap growing by it; a wrapper keeps the room below the
        // last choice inside the dialog's clip.
        self.body.reveal(window, cx);
        let [before, after] =
            [&self.before, &self.after].map(|field| field.read(cx).focus_handle(cx));
        let modes = [ComparisonMode::Endpoints, ComparisonMode::SinceBranching]
            .map(|mode| self.body.focus(("comparison-mode", mode as usize), cx));
        let swap = self.body.focus("swap-revisions", cx);
        let picks: Vec<_> = (0..choices.len())
            .map(|i| self.body.focus(("revision-choice", i), cx))
            .collect();
        let mut controls = vec![Some(before), Some(after), None, None];
        controls.extend(picks.iter().cloned().map(Some));
        let toggles = vec![
            Some(modes[0].clone()),
            Some(modes[1].clone()),
            Some(swap.clone()),
        ];
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("comparison-form").track_focus(&self.focus)
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)) + room * 2.).min_h_0()
            .overflow_y_scroll().track_scroll(self.body.scroll()).p(room).m(-room).pl(lead).ml(-lead).text_size(crate::appearance::ui_text(12.))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .child(div().flex().flex_col().gap_3().on_children_prepainted(self.body.controls(controls, room))
            .child(div().child("Before").child(Input::new(&self.before).aria_label("Before: local branch, tag, or commit revision").disabled(self.busy)))
            .child(div().child("After").child(Input::new(&self.after).aria_label("After: local branch, tag, or commit revision").disabled(self.busy)))
            .child(div().flex().flex_wrap().gap_1().on_children_prepainted(self.body.controls(toggles, room)).children([(ComparisonMode::Endpoints,"Endpoints · Before → After"),(ComparisonMode::SinceBranching,"Changes since branching")].into_iter().zip(modes).map(|((mode,label),focus)|button(label,label,"",self.mode==mode).track_focus(&focus).toggled(self.mode==mode).disabled(self.busy).on_click(cx.listener(move |this,_,_,cx|{this.mode=mode;cx.notify();}))))
                .child(button("swap-revisions","Swap","",false).track_focus(&swap).disabled(self.busy).on_click(cx.listener(|this,_,window,cx|{let before=this.before.read(cx).value();let after=this.after.read(cx).value();this.before.update(cx,|input,cx|input.set_value(after,window,cx));this.after.update(cx,|input,cx|input.set_value(before,window,cx));}))))
            .child(div().text_color(rgb(p.muted)).child("Type a local revision, or choose a matching branch/tag for the most recently edited field. Names resolve when you press Compare. A merge-base comparison shows the changes leading to After."))
            .children(choices.into_iter().zip(picks).enumerate().map(|(i,(name,focus))|crate::edge_row(window).child(crate::edge_button(("revision-choice",i),name.clone()).track_focus(&focus).debug_selector(move ||format!("revision-choice-{i}")).disabled(self.busy).on_click(cx.listener(move |this,_,window,cx|{let input=if this.selected_side==0{&this.before}else{&this.after};input.update(cx,|input,cx|input.set_value(name.clone(),window,cx));})))))
            .when(self.busy,|el|el.child(div().child("Resolving local revisions and changed files… Cancel closes this read.")))
            .children(self.error.as_ref().map(|error|failure_line("revision-comparison-error",error.clone()).text_color(rgb(p.removed))))), "comparison-form-scrollbar").min_h_0())
    }
}

/// A dialog's in-place failure, `text`, as an alert announced once as it
/// appears, as System preview's refusal is: Quick Open's and Compare
/// revisions' were silent, and Quick Open's not even text on the tree.
fn failure_line(
    id: &'static str,
    text: String,
) -> crate::native_accessibility::StaticText<Stateful<Div>> {
    crate::native_accessibility::static_text(
        div()
            .id(id)
            .debug_selector(move || id.into())
            .alert(text.clone())
            .child(text.clone()),
        text,
    )
}

/// A failure of a search the typing started, `text`, as a polite status:
/// heard once speech pauses, never interrupting the next keystroke, where
/// an alert after each 120 ms pause cut in on every partial revision.
/// [`failure_line`] takes its place once Enter asks for the result.
fn typing_failure_line(
    id: &'static str,
    text: String,
) -> crate::native_accessibility::StaticText<Stateful<Div>> {
    crate::native_accessibility::static_text(
        div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::Status)
            .aria_label(text.clone())
            .polite()
            .child(text.clone()),
        text,
    )
}

#[cfg(test)]
thread_local! {
    /// The Quick Open dialog last opened, for a test to await its search.
    static OPENED_QUICK_FORM: std::cell::RefCell<Option<WeakEntity<QuickForm>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs the open Quick Open's search to its answer, for tests: past its
/// typing pause, then the worker's reply, awaited rather than polled.
#[cfg(test)]
pub(crate) async fn settle_quick_open(cx: &mut gpui::VisualTestContext) {
    let form = OPENED_QUICK_FORM
        .with(|form| form.borrow().clone())
        .and_then(|form| form.upgrade())
        .expect("Quick Open is open");
    cx.executor()
        .advance_clock(QUICK_SEARCH_DELAY + std::time::Duration::from_millis(30));
    cx.executor().run_until_parked();
    if let Some(task) = form.update(cx, |form, _| form.task.take()) {
        task.await;
    }
    cx.executor().run_until_parked();
}

/// How long Quick Open waits for typing to pause before it lists paths.
const QUICK_SEARCH_DELAY: std::time::Duration = std::time::Duration::from_millis(120);

pub(crate) struct QuickForm {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    query: Entity<InputState>,
    revision: Entity<InputState>,
    worktree: bool,
    page: Option<TrackedPaths>,
    selected: usize,
    scroll: UniformListScrollHandle,
    worker: Arc<Worker>,
    task: Option<Task<()>>,
    generation: u64,
    busy: bool,
    error: Option<String>,
    /// Enter asked for the failed search's result, so its failure is an
    /// alert; until then it is the polite line the typing started.
    failure_asked: bool,
    subscriptions: Vec<Subscription>,
    closed: Arc<AtomicBool>,
    accepted: Arc<AtomicBool>,
    return_focus: Option<FocusHandle>,
}
/// The focus sweep (`focus_ring::sweep`) and the dialog tests read Quick
/// Open once its first paths have landed ([`QuickForm::lands`]): they are
/// read on the form's own worker, which neither `run_until_parked` nor the
/// drain of the app's executor waits for.
#[cfg(test)]
impl crate::focus_ring::sweep::Opened for Option<Entity<QuickForm>> {
    fn finish(self, cx: &mut gpui::VisualTestContext) {
        if let Some(form) = self {
            QuickForm::lands(&form, cx);
        }
    }
}
impl QuickForm {
    /// Returns once the paths the form's query asks for have landed: past
    /// the typing pause on the test clock, then once the form's reader, a
    /// thread `run_until_parked` cannot wait for, has answered. The test
    /// clock moves only by that pause, and by one more while the form is
    /// still busy once its reader has answered, as a change the form handles
    /// while it settles starts the pause again.
    #[cfg(test)]
    pub(crate) fn lands(form: &Entity<Self>, cx: &mut VisualTestContext) {
        for _ in 0..4 {
            cx.executor().advance_clock(QUICK_SEARCH_DELAY);
            cx.run_until_parked();
            form.read_with(cx, |form, _| form.worker.wait_idle());
            cx.run_until_parked();
            if !form.read_with(cx, |form, _| form.busy) {
                return;
            }
        }
        panic!("Quick Open's paths never landed");
    }

    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        revision: String,
        return_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search tracked paths…"));
        let revision = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(revision)
                .placeholder("Local branch, tag or commit")
        });
        let mut form = Self {
            owner,
            repo,
            query,
            revision,
            worktree: true,
            page: None,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            worker: Arc::new(Worker::new()),
            task: None,
            generation: 0,
            busy: false,
            error: None,
            failure_asked: false,
            subscriptions: vec![],
            closed: Arc::new(AtomicBool::new(false)),
            accepted: Arc::new(AtomicBool::new(false)),
            return_focus,
        };
        // Return opens the selected path through the dialog's default action
        // alone (`on_ok`), once per press, as the key goes down, never on a
        // held key's repeats, under X11 too. Each field's own `PressEnter`,
        // which it sends for every Enter key-down, opened it as well, so a
        // hold that outlasted a pending search opened the first path on a
        // repeat.
        for input in [form.query.clone(), form.revision.clone()] {
            form.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event, window, cx| {
                    if let InputEvent::Change = event {
                        this.search(window, cx);
                    }
                },
            ));
        }
        form.search(window, cx);
        form
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.task = None;
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
        cancel_modal_read(
            &self.closed,
            &self.worker,
            self.return_focus.as_ref(),
            window,
            cx,
        );
    }

    fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.worker.cancel();
        let was_busy = self.busy;
        self.busy = true;
        self.error = None;
        self.failure_asked = false;
        let timer = cx.background_executor().timer(QUICK_SEARCH_DELAY);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            if this
                .read_with(cx, |this, _| this.closed.load(Ordering::Acquire))
                .unwrap_or(true)
            {
                return;
            }
            let Ok(response) = this.update_in(cx, |this, _, cx| {
                this.worker.submit(Job::TrackedPaths {
                    repo: this.repo.clone(),
                    scope: if this.worktree {
                        PathScope::Worktree
                    } else {
                        PathScope::Revision(this.revision.read(cx).value().to_string())
                    },
                    query: this.query.read(cx).value().to_string(),
                })
            }) else {
                return;
            };
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.generation || this.closed.load(Ordering::Acquire) {
                    return;
                }
                this.busy = false;
                this.selected = 0;
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                match result {
                    Ok(Ok(Output::TrackedPaths(page))) => this.page = Some(page),
                    Ok(Err(e)) => {
                        this.page = None;
                        this.error = Some(format!("{e:#}"));
                    }
                    _ => {
                        this.error = Some("File search cancelled. Edit the query to retry.".into())
                    }
                }
                window.refresh();
                cx.notify();
            });
        }));
        if !was_busy {
            window.refresh();
        }
        cx.notify();
    }
    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.closed.load(Ordering::Acquire) || self.accepted.load(Ordering::Acquire)
        {
            return;
        }
        // Enter on a failed search asks for its answer: the failure is
        // announced now, assertively, once.
        if self.error.is_some() && !self.failure_asked {
            self.failure_asked = true;
            cx.notify();
            return;
        }
        let Some(page) = &self.page else {
            return;
        };
        let Some(entry) = page.entries.get(self.selected).cloned() else {
            return;
        };
        let scope = page.scope.clone();
        let path = self.repo.path().to_owned();
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path.as_ref() == Some(&path) && owner.operation_busy.is_none() {
                if !claim_modal_result(&self.closed, &self.accepted) {
                    return;
                }
                self.closed.store(true, Ordering::Release);
                self.worker.cancel();
                window.close_dialog(cx);
                window.refresh();
                owner.show_tracked_file(scope, entry, window, cx);
                owner.defer_inspection_focus(window, cx);
            }
        });
    }
}
impl Render for QuickForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let count = self.page.as_ref().map_or(0, |page| page.entries.len());
        let desired_height = appearance::ui_size(if count == 0 {
            78.
        } else {
            34. * count.min(10) as f32 + 2.
        });
        let available_height = (window.viewport_size().height - appearance::ui_size(300.))
            .max(appearance::ui_size(78.));
        let list = uniform_list(
            "quick-file-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let entry = &this.page.as_ref().unwrap().entries[index];
                        let path = entry.path.display().to_string();
                        // The row's ID is the path it shows, by its bytes,
                        // rather than its place in the results, so the window's
                        // tree keeps one node for each path across searches and
                        // a search that changes the results takes nodes away and
                        // adds others rather than renaming them. Keyed by place,
                        // a query changed after a move renamed the node at each
                        // place, and Orca would speak the path now in it, "not
                        // selected", as it did in Keyboard Shortcuts' list
                        // (QA-92). Its debug selector is still its place, which
                        // tests read.
                        let row = div()
                            .id(ElementId::Path(Arc::from(entry.path.as_path())))
                            .debug_selector(move || format!("quick-file-{index}"))
                            .role(Role::ListBoxOption)
                            .aria_label(path.clone())
                            .aria_selected(index == this.selected)
                            .w_full()
                            .h(crate::appearance::ui_size(34.))
                            .px_2()
                            .flex()
                            .items_center()
                            .relative()
                            // A resting row shows the raised surface beneath
                            // it, as a menu's rows do; the highlight is the
                            // keyboard's cursor and marked as such.
                            .when(index == this.selected, |row| {
                                let colors = palette(cx);
                                row.bg(rgb(colors.selected))
                                    .child(crate::appearance::cursor_marker(colors))
                            })
                            .child(div().truncate().child(path))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.selected = index;
                                this.activate(window, cx);
                            }));
                        // Recorded for tests, which read the rows the window's
                        // tree holds.
                        crate::native_accessibility::recorded(row).into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        // The form scrolls, so it clips its controls to its bounds on both
        // axes. It keeps the focus ring's room around them, giving the sides
        // back through its margin and the top and foot through the dialog's
        // title margin and footer gap, so the scope buttons and the fields
        // draw their rings whole and nothing moves.
        let room = appearance::button_ring_room(cx);
        crate::native_accessibility::clips_children(div().id("quick-file-form-scroll").debug_selector(|| "quick-file-form".into())
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)) + room * 2.)
            .p(room)
            .mx(-room)
            .overflow_y_scroll().text_size(crate::appearance::ui_text(12.))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_key_down(cx.listener(|this,event:&KeyDownEvent,_,cx|{let count=this.page.as_ref().map_or(0,|page|page.entries.len());if count==0{return;}match event.keystroke.key.as_str(){"down"=>this.selected=(this.selected+1).min(count-1),"up"=>this.selected=this.selected.saturating_sub(1),_=>return}cx.stop_propagation();this.scroll.scroll_to_item(this.selected,ScrollStrategy::Center);cx.notify();}))
            .child(div().flex().flex_col().gap_2()
            .child(div().flex().gap_1().children([(true,"Current worktree","quick-open-worktree"),(false,"Chosen revision","quick-open-revision")].into_iter().map(|(worktree,label,selector)|button(label,label,"",self.worktree==worktree).debug_selector(move || selector.into()).toggled(self.worktree==worktree).on_click(cx.listener(move |this,_,window,cx|{this.worktree=worktree;this.search(window,cx);})))) )
            .when(!self.worktree,|el|el.child(Input::new(&self.revision)))
            .child(Input::new(&self.query))
            .child({
                // The count agrees with its noun: "1 match", "2 matches".
                let help: String = if self.busy{"Searching local tracked paths…".into()}else{format!("{} · ↑/↓ select · {} opens · {}{}",crate::counted(count,"match","matches"),shortcuts::ENTER,shortcuts::DialogKeys::QuickOpen.escape_help(),self.page.as_ref().filter(|page|page.truncated).map_or("",|_|" · Limit reached; narrow the query"))};
                let shown = help.clone();
                div().debug_selector(move || format!("quick-open-help: {shown}")).child(crate::native_accessibility::static_text(shortcuts::dialog_help_by_phrase(help.clone()).id("quick-open-help"), help).text_color(rgb(p.muted)))
            })
            .children(self.error.as_ref().map(|error|if self.failure_asked { failure_line("quick-open-error-alert",error.clone()) } else { typing_failure_line("quick-open-error",error.clone()) }.text_color(rgb(p.removed))))
            .child(crate::native_accessibility::clips_children(div().id("quick-open-results").debug_selector(|| "quick-open-results".into()).role(Role::ListBox).aria_label("Matching tracked files").h(desired_height.min(available_height)).border_1().border_color(rgb(p.raised_edge())).overflow_hidden().when(count>0,|el|el.child(list)).when(count==0&&!self.busy,|el|el.child(crate::native_accessibility::text("quick-open-empty","No matching tracked files. Untracked files are available in Working Changes.").p_3()))))
            .child(crate::native_accessibility::text("quick-open-note","File History and Blame are available after opening. Worktree reads show raw current bytes; a chosen revision is pinned to its resolved commit.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))))
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomicBool, Ordering, claim_modal_result};
    use crate::*;
    use core::prelude::v1::test;
    #[test]
    fn enter_and_dialog_confirmation_activate_once_and_closed_results_are_ignored() {
        let closed = AtomicBool::new(false);
        let accepted = AtomicBool::new(false);
        assert!(claim_modal_result(&closed, &accepted));
        assert!(!claim_modal_result(&closed, &accepted));
        let accepted = AtomicBool::new(false);
        closed.store(true, Ordering::Release);
        assert!(!claim_modal_result(&closed, &accepted));
        assert!(!accepted.load(Ordering::Acquire));
    }

    /// A held Return in Compare local revisions reads one comparison per
    /// press, under X11 too: with a revision that does not resolve, Enter
    /// goes down in the field and the comparison fails at once, then the
    /// key's repeats arrive (`REPEATS`), each waited out until the form's
    /// reader has answered and the field has focus again. The form reads
    /// once and says why; after the key-up a new press reads again. The
    /// field's own `PressEnter` submitted the form as well as the dialog's
    /// default action, so after the quick failure each repeat read another
    /// comparison.
    #[gpui::test]
    async fn a_held_return_in_compare_revisions_reads_once_per_press(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::branch_actions::tests::{REPEATS, enter};
        use crate::focus_ring::sweep::Opened;
        use crate::tags::tests::{draw, tagged_repository, window};
        use std::{cell::Cell, rc::Rc};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        let form = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_compare_form(window, cx)))
            .expect("the form opens");
        draw(cx);
        Some(form.clone()).finish(cx);
        cx.update(|window, cx| {
            let before = form.read(cx).before.clone();
            before.update(cx, |input, cx| {
                input.set_value("missing-revision", window, cx)
            });
            before.read(cx).focus_handle(cx).focus(window, cx);
        });
        draw(cx);
        // Comparisons read, counted as the form begins each.
        let reads = Rc::new(Cell::new(0_usize));
        let _counting = cx.update(|_, cx| {
            let reads = reads.clone();
            let mut was = false;
            cx.observe(&form, move |form, cx| {
                let now = form.read(cx).busy;
                reads.set(reads.get() + usize::from(now && !was));
                was = now;
            })
        });
        // The reader's answer lands, and the frame after it gives the field
        // focus back.
        let lands = |cx: &mut VisualTestContext| {
            form.read_with(cx, |form, _| form.worker.wait_idle());
            cx.run_until_parked();
            draw(cx);
        };
        let field_focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                form.read(cx)
                    .before
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            })
        };
        for press in ["the first press", "a new press after the key-up"] {
            let after = reads.get() + 1;
            enter(cx, Some(false));
            lands(cx);
            assert_eq!(reads.get(), after, "{press}: one comparison is read");
            assert_eq!(
                form.read_with(cx, |form, _| (form.busy, form.error.is_some())),
                (false, true),
                "{press}: it fails"
            );
            assert!(field_focused(cx), "{press}: the field has focus again");
            for (repeat, events) in REPEATS {
                for down in events {
                    enter(cx, *down);
                }
                lands(cx);
                assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
                assert!(
                    field_focused(cx),
                    "{press}, {repeat}: focus stays in the field"
                );
                assert_eq!(
                    reads.get(),
                    after,
                    "{press}, {repeat}: no other comparison is read"
                );
            }
            enter(cx, None);
            draw(cx);
        }
    }

    /// A held Return in Quick Open opens the first path once per press,
    /// under X11 too: pressed while the search is still pending, it opens
    /// nothing, and once the search lands during the hold the key's repeats
    /// (`REPEATS`) still open nothing, Quick Open open with focus in its
    /// query; after the key-up a new press opens the path. Each field opened
    /// it on its own `PressEnter`, which it sends for every Enter key-down,
    /// as well as through the dialog's default action, so a hold that
    /// outlasted the search opened the first path on a repeat.
    #[gpui::test]
    async fn a_held_return_in_quick_open_opens_once_per_press(cx: &mut gpui::TestAppContext) {
        use crate::branch_actions::tests::{REPEATS, enter};
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("tracked.txt"), "tracked\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "tracked.txt"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        let form = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_quick_form(window, cx)))
            .expect("Quick Open opens");
        draw(cx);
        let query = form.read_with(cx, |form, _| form.query.clone());
        let opened = |cx: &mut VisualTestContext| {
            form.read_with(cx, |form, _| form.accepted.load(Ordering::Acquire))
        };
        let held = |cx: &mut VisualTestContext, at: &str| {
            assert!(!opened(cx), "{at}: nothing opens");
            cx.update(|window, cx| {
                assert!(window.has_active_dialog(cx), "{at}: Quick Open stays open");
                assert!(
                    query.read(cx).focus_handle(cx).is_focused(window),
                    "{at}: focus stays in its query"
                );
            });
        };
        assert!(
            form.read_with(cx, |form, _| form.busy),
            "the search waits for typing to pause"
        );
        enter(cx, Some(false));
        draw(cx);
        held(cx, "the press while the search is pending");
        // The search lands while the key is held.
        super::QuickForm::lands(&form, cx);
        draw(cx);
        assert!(
            form.read_with(cx, |form, _| form
                .page
                .as_ref()
                .is_some_and(|page| !page.entries.is_empty())),
            "the search lists the tracked path"
        );
        for (repeat, events) in REPEATS {
            for down in events {
                enter(cx, *down);
            }
            draw(cx);
            held(cx, repeat);
        }
        enter(cx, None);
        draw(cx);
        enter(cx, Some(false));
        draw(cx);
        enter(cx, None);
        draw(cx);
        assert!(opened(cx), "a new press opens the first path");
    }

    /// Quick Open's failure, here a chosen revision that does not resolve,
    /// has a node of its own, named by its whole text. A search the typing
    /// started reports it as a polite status, heard once speech pauses, so a
    /// partial revision never interrupts the next keystroke; Enter, which asks
    /// for the result, makes it an alert, a fresh node announced once, as
    /// System preview's refusal is; and typing again makes it the polite line
    /// again. It was a bare line with no node of its own, and then an alert
    /// after every 120 ms pause.
    #[gpui::test]
    async fn quick_open_announces_its_failure_as_an_alert(cx: &mut gpui::TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::{draw, tagged_repository, window};
        use gpui::accesskit::Role;
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_quick_file(window, cx)));
        draw(cx);
        let revision = cx
            .debug_bounds("quick-open-revision")
            .expect("Chosen revision");
        cx.simulate_click(revision.center(), Modifiers::default());
        draw(cx);
        super::settle_quick_open(cx).await;
        // The failure's node, by the line's id, after the next frame.
        let line = |cx: &mut VisualTestContext, id: &str| {
            written::forget();
            draw(cx);
            written::get(id).map(|written| written.node)
        };
        // The revision field leads the query, which has focus. Each pause in
        // the typing searches, and each partial revision fails.
        cx.simulate_keystrokes("shift-tab");
        for typed in ["-un", "known"] {
            cx.simulate_input(typed);
            super::settle_quick_open(cx).await;
            let status = line(cx, "quick-open-error")
                .unwrap_or_else(|| panic!("after {typed:?}: the failure writes its node"));
            assert_eq!(status.role(), Role::Status, "after {typed:?}");
            assert!(
                line(cx, "quick-open-error-alert").is_none(),
                "after {typed:?}: no alert while typing"
            );
        }
        let error = line(cx, "quick-open-error")
            .and_then(|node| node.label().map(str::to_owned))
            .expect("the failure is named");
        assert!(
            error.contains("unknown"),
            "named by the whole failure: {error:?}"
        );
        cx.simulate_keystrokes("enter");
        let alert = line(cx, "quick-open-error-alert").expect("Enter makes the failure an alert");
        assert_eq!(alert.role(), Role::Alert);
        assert_eq!(alert.label(), Some(error.as_str()));
        assert!(line(cx, "quick-open-error").is_none(), "one line at a time");
        cx.simulate_input("x");
        super::settle_quick_open(cx).await;
        assert_eq!(
            line(cx, "quick-open-error").map(|node| node.role()),
            Some(Role::Status),
            "typing again"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
    }

    /// QA-109: Quick source opened by Quick Open's Enter draws its focus
    /// ring in the frame that opens it, while its file is read: the read's
    /// placeholder, where the text will stand, holds focus, ringed on the
    /// text pane's edge, and as the text lands it takes focus with the same
    /// ring on the same edge, whole in both. The reading frame drew no ring,
    /// with focus on the information beside the file, ringless while it
    /// waited.
    #[gpui::test]
    async fn quick_source_rings_its_reading_frame_where_the_text_lands(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, history_window, settle};
        let (_fixture, app, cx) = history_window(cx).await;
        // The fixture's file checked out, for Quick Open to find.
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::tags::tests::git(&path, &["reset", "--quiet", "--hard"]);
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        cx.simulate_resize(size(px(1200.), px(800.)));
        settle(&app, cx).await;
        let form = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_quick_form(window, cx)))
            .expect("Quick Open opens");
        draw(cx);
        cx.simulate_input("notes");
        super::QuickForm::lands(&form, cx);
        draw(cx);
        // The file's read waits, so the frame Enter opens is drawn before it.
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        let (source, loading, quick) = cx.update(|window, cx| {
            (
                app.read(cx).source_focus.is_focused(window),
                app.read(cx).loading,
                app.read(cx).is_quick_source(),
            )
        });
        assert!(
            quick && loading.is_some(),
            "Quick source is open, its file read"
        );
        assert!(source, "the waiting focus holds");
        let reading = cx
            .debug_bounds("quick-source-pane")
            .expect("the placeholder stands where the text will");
        let pane = cx.debug_bounds("preview-content").expect("the text pane");
        let ring = cx
            .debug_bounds("compare-text-ring")
            .expect("the reading frame draws the ring");
        assert_eq!(ring, pane, "on the text pane's whole edge");
        assert!(
            pane.contains(&reading.center()),
            "around the placeholder {reading:?} in {pane:?}"
        );
        assert!(
            cx.debug_bounds("quick-source-information-ring").is_none(),
            "and nowhere else"
        );

        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        let text = app
            .read_with(cx, |app, cx| {
                app.after_editor
                    .as_ref()
                    .map(|editor| editor.read(cx).focus_handle(cx))
            })
            .expect("the text lands");
        assert!(
            cx.update(|window, _| text.is_focused(window)),
            "the text takes focus"
        );
        assert_eq!(
            cx.debug_bounds("compare-text-ring"),
            Some(ring),
            "with the same ring, whole, on the same edge"
        );
        assert!(cx.debug_bounds("quick-source-pane").is_none());
    }

    /// A 1200 × 800 History window for Quick Open's routes, the app's keys
    /// bound, with the fixture's file checked out, a binary committed beside
    /// it at `assets/blob.bin`, a 1 × 1 PNG at `assets/image.png` and a WAV
    /// with no samples at `assets/sound.wav`.
    async fn quick_open_window(
        cx: &mut gpui::TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
    ) {
        use crate::views::tests::{history_window, settle};
        let (fixture, app, cx) = history_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::tags::tests::git(&path, &["reset", "--quiet", "--hard"]);
        std::fs::create_dir_all(path.join("assets")).unwrap();
        std::fs::write(path.join("assets/blob.bin"), [0_u8, 1, 2, 0, 255, 0]).unwrap();
        std::fs::write(
            path.join("assets/image.png"),
            crate::system_preview::tests::png(None),
        )
        .unwrap();
        std::fs::write(
            path.join("assets/sound.wav"),
            b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\0\0\0\0",
        )
        .unwrap();
        crate::tags::tests::git(&path, &["add", "assets"]);
        crate::tags::tests::git(&path, &["commit", "--quiet", "-m", "A binary"]);
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        cx.simulate_resize(size(px(1200.), px(800.)));
        settle(&app, cx).await;
        (fixture, app, cx)
    }

    /// Quick Open over the window with its paths for `query` landed.
    fn quick_open(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        query: &str,
    ) -> Entity<super::QuickForm> {
        use crate::views::tests::draw;
        let form = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_quick_form(window, cx)))
            .expect("Quick Open opens");
        draw(cx);
        cx.simulate_input(query);
        super::QuickForm::lands(&form, cx);
        draw(cx);
        form
    }

    /// The name Quick source's pane wrote for the group that holds focus
    /// while it shows no text, in the frame drawn now: its placeholder's or
    /// notice's group, or an image comparison's root.
    fn pane_name(cx: &mut gpui::VisualTestContext) -> Option<String> {
        crate::native_accessibility::written::forget();
        crate::views::tests::draw(cx);
        written_pane_name()
    }

    /// The name [`pane_name`] reads, as the last frame drawn wrote it.
    fn written_pane_name() -> Option<String> {
        use crate::native_accessibility::written;
        ["quick-source-pane", crate::image_compare::IMAGE_ROOT]
            .into_iter()
            .find_map(written::get)
            .and_then(|written| written.node.label().map(str::to_owned))
    }

    /// The live regions the last frame drew that say something as they
    /// change: a second focus or a landing said aloud would be among them.
    fn speaking_regions() -> Vec<String> {
        crate::native_accessibility::written::announced_from("")
            .into_iter()
            .filter(|(_, announced)| announced.live != gpui::accesskit::Live::Off)
            .map(|(id, _)| id)
            .collect()
    }

    /// The window's AT-SPI adapter fed each frame drawn, on Linux.
    struct Renames {
        #[cfg(target_os = "linux")]
        bus: crate::native_accessibility::atspi_state_tests::Bus,
    }

    impl Renames {
        fn start(cx: &mut gpui::VisualTestContext) -> Self {
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
                Self {
                    bus: Bus::new(next_frame(cx)),
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                crate::views::tests::draw(cx);
                Self {}
            }
        }

        /// Draws the next frame and returns the names of the groups, as the
        /// pane's that holds focus, the adapter reported renamed in it, which
        /// Orca speaks for the one it presents, on Linux; `None` elsewhere.
        /// The window's status line, static text Orca does not present, is
        /// renamed by every read.
        fn next(&mut self, cx: &mut gpui::VisualTestContext) -> Option<Vec<String>> {
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::next_frame;
                use accesskit_atspi_common::Role;
                self.bus.update(next_frame(cx));
                Some(
                    self.bus
                        .renamed()
                        .into_iter()
                        .filter(|(role, _)| *role == Role::Panel)
                        .map(|(_, name)| name)
                        .collect(),
                )
            }
            #[cfg(not(target_os = "linux"))]
            {
                crate::native_accessibility::written::forget();
                crate::views::tests::draw(cx);
                None
            }
        }
    }

    /// A Quick source file that lands with no text, a binary, an image or a
    /// read that fails, keeps focus and the ring where the reading frame put
    /// them, in the text pane: the reading placeholder's group holds focus,
    /// named by the read, and the landing names what holds it by why there
    /// is no text, the one thing spoken, its ring the same on the pane's
    /// edge: a binary's metadata as having no preview, an image, on the
    /// comparison's own root with its side's name inside it, by its format
    /// and size, and a failure by its first line. The landing moved focus to
    /// "Tracked source information", the ring jumping with no key pressed and
    /// a second focus spoken, and an image's landing still did.
    #[gpui::test]
    async fn quick_source_with_no_text_keeps_focus_and_its_ring_in_the_pane(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        for (query, gone, reason) in [
            ("blob", false, "Binary file · no preview"),
            ("sound", false, "WAV audio · no preview"),
            ("image", false, "PNG image · 1 × 1"),
            ("notes", true, ""),
        ] {
            quick_open(&app, cx, query);
            // Found, then gone before its read: the read fails.
            if gone {
                std::fs::remove_file(path.join("notes.txt")).unwrap();
            }
            let held = app.read_with(cx, |app, _| app.worker.hold());
            cx.simulate_keystrokes("enter");
            // Released, as a person does: a press with no key-up before the
            // next is an X11 repeat, which Quick Open answers with nothing.
            crate::branch_actions::tests::enter(cx, None);
            draw(cx);
            assert!(
                cx.update(|window, _| source.is_focused(window)),
                "{query}: the reading frame holds the waiting focus"
            );
            let pane = cx.debug_bounds("preview-content").expect("the text pane");
            let ring = cx.debug_bounds("compare-text-ring");
            assert_eq!(ring, Some(pane), "{query}: ringed on the pane's edge");
            assert_eq!(
                pane_name(cx).as_deref(),
                Some("Reading tracked file…"),
                "{query}: named by the read"
            );

            drop(held);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            settle(&app, cx).await;
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.loading.is_none()
                    && app.after_editor.is_none()),
                "{query}: the file landed with no text"
            );
            assert!(
                cx.update(|window, _| source.is_focused(window)),
                "{query}: focus stays where the reading frame put it"
            );
            assert_eq!(
                cx.debug_bounds("compare-text-ring"),
                ring,
                "{query}: the same ring on the same edge"
            );
            assert!(
                cx.debug_bounds("quick-source-information-ring").is_none(),
                "{query}: and none on the information"
            );
            let named = pane_name(cx)
                .unwrap_or_else(|| panic!("{query}: what the pane draws holds focus and names it"));
            let failure = app.read_with(cx, |app, _| {
                app.error
                    .as_deref()
                    .and_then(crate::recovery::first_line)
                    .map(str::to_owned)
            });
            assert_eq!(
                Some(named.as_str()),
                if gone {
                    failure.as_deref()
                } else {
                    Some(reason)
                },
                "{query}: named by why there is no text"
            );
            if query == "image" {
                assert!(
                    crate::native_accessibility::written::lies_in(
                        "image-captions",
                        "image-comparison-root"
                    ),
                    "{query}: its side's name inside it"
                );
            }
            assert_eq!(
                speaking_regions(),
                Vec::<String>::new(),
                "{query}: and nothing else is said"
            );
        }
    }

    /// Ctrl+R with Quick source's pane holding focus on a file with no text
    /// keeps the focused group's name while the file is read again, so the
    /// reason is not said again: a binary or an image read again as itself
    /// is never renamed, and one deleted meanwhile is renamed once, by the
    /// failure that lands. The group was renamed "Refreshing tracked file…"
    /// and back, which Orca speaks for the object it presents. Over an image
    /// focus also stays on one node, the image comparison's root, whose ID
    /// the reading placeholder takes: the placeholder was another node, so
    /// focus moved to it and back to the image, two focus events, each
    /// spoken.
    #[gpui::test]
    async fn quick_source_refresh_keeps_its_pane_name_while_it_reads(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        for (query, file, name, node) in [
            (
                "blob",
                "assets/blob.bin",
                "Binary file · no preview",
                "quick-source-pane",
            ),
            (
                "image",
                "assets/image.png",
                "PNG image · 1 × 1",
                crate::image_compare::IMAGE_ROOT,
            ),
        ] {
            quick_open(&app, cx, query);
            cx.simulate_keystrokes("enter");
            // Released, as a person does: a press with no key-up before the
            // next is an X11 repeat, which Quick Open answers with nothing.
            crate::branch_actions::tests::enter(cx, None);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            settle(&app, cx).await;
            draw(cx);
            assert_eq!(pane_name(cx).as_deref(), Some(name), "{query}");
            let ring = cx.debug_bounds("compare-text-ring");
            assert!(ring.is_some(), "{query}: the pane holds focus, ringed");
            // The node that holds focus in the frame just drawn: one ID and
            // one path, from which GPUI derives the node's, so the adapter
            // reports no focus moved.
            let holder = |at: &str, node: &str| {
                use crate::native_accessibility::written;
                let drawn = ["quick-source-pane", crate::image_compare::IMAGE_ROOT]
                    .into_iter()
                    .filter(|id| written::get(id).is_some())
                    .collect::<Vec<_>>();
                assert_eq!(drawn, vec![node], "{query}, {at}: one node holds focus");
                #[cfg(target_os = "linux")]
                {
                    written::node_path(super::QUICK_SOURCE_FOCUS_PATH)
                        .unwrap_or_else(|| panic!("{query}, {at}: the holder's path"))
                }
                #[cfg(not(target_os = "linux"))]
                {
                    String::new()
                }
            };
            let held = holder("landed", node);
            let mut renames = Renames::start(cx);
            for gone in [false, true] {
                if gone {
                    std::fs::remove_file(path.join(file)).unwrap();
                }
                let waiting = app.read_with(cx, |app, _| app.worker.hold());
                cx.simulate_keystrokes(refresh);
                let reading = renames.next(cx);
                assert!(
                    app.read_with(cx, |app, _| app.loading.is_some()),
                    "{query}, gone {gone}: the file is read again"
                );
                assert_eq!(
                    written_pane_name().as_deref(),
                    Some(name),
                    "{query}, gone {gone}: the group keeps its name while it reads"
                );
                assert_eq!(
                    holder("reading", node),
                    held,
                    "{query}, gone {gone}: the same node holds focus while it reads"
                );
                if let Some(reading) = reading {
                    assert_eq!(
                        reading,
                        Vec::<String>::new(),
                        "{query}, gone {gone}: nothing renamed"
                    );
                }
                assert!(cx.update(|window, _| source.is_focused(window)));
                assert_eq!(cx.debug_bounds("compare-text-ring"), ring);
                drop(waiting);
                app.read_with(cx, |app, _| app.worker.wait_idle());
                settle(&app, cx).await;
                let landing = renames.next(cx);
                let named = written_pane_name().expect("the group holds focus");
                let failure = app.read_with(cx, |app, _| {
                    app.error
                        .as_deref()
                        .and_then(crate::recovery::first_line)
                        .map(str::to_owned)
                });
                assert_eq!(
                    named,
                    failure.as_deref().unwrap_or(name),
                    "{query}, gone {gone}: the reason that landed names the group"
                );
                assert_eq!(
                    failure.is_some(),
                    gone,
                    "{query}: the read fails once the file is gone"
                );
                // A failure lands as the pane's own node, named by it, the
                // image's node spent with the image (decided 2026-11-10).
                let landed = if gone { "quick-source-pane" } else { node };
                let path = holder("landed again", landed);
                if landed == node {
                    assert_eq!(
                        path, held,
                        "{query}, gone {gone}: and the same node holds it as it lands"
                    );
                }
                if let Some(landing) = landing
                    && landed == node
                {
                    assert_eq!(
                        landing,
                        if gone {
                            vec![named.clone()]
                        } else {
                            Vec::new()
                        },
                        "{query}, gone {gone}: renamed only where the reason differs"
                    );
                }
                assert!(
                    cx.update(|window, _| source.is_focused(window)),
                    "{query}, gone {gone}: focus stays"
                );
                assert_eq!(cx.debug_bounds("compare-text-ring"), ring);
                assert_eq!(speaking_regions(), Vec::<String>::new());
            }
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.close_revision_inspection(window, cx))
            });
            settle(&app, cx).await;
        }
    }

    /// The name Quick source's pane holds through a re-read is spent as the
    /// read lands: a later read the pane holds focus through with nothing
    /// shown to name it, as each way back to a file left with no content
    /// reads it (`read_selected_preview`), is named by that read. Kept past
    /// its landing, the name a Refresh held named the next such read, here
    /// "WAV audio · no preview" for a file the Refresh had found was no
    /// longer one.
    #[gpui::test]
    async fn quick_source_spends_its_held_name_as_the_read_lands(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::settle;
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        quick_open(&app, cx, "sound");
        cx.simulate_keystrokes("enter");
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        assert_eq!(pane_name(cx).as_deref(), Some("WAV audio · no preview"));

        // Refresh holds that name while it reads the file, now bytes of no
        // known encoding, and the landing renames the pane.
        std::fs::write(path.join("assets/sound.wav"), [0_u8, 1, 2, 0, 255, 0]).unwrap();
        cx.simulate_keystrokes(refresh);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        assert_eq!(pane_name(cx).as_deref(), Some("Binary file · no preview"));
        assert!(cx.update(|window, _| source.is_focused(window)));

        // Left with no content and read again, its focus still in the pane.
        let waiting = app.read_with(cx, |app, _| app.worker.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.clear_preview();
                app.read_selected_preview(window, cx);
            })
        });
        assert!(
            app.read_with(cx, |app, _| app.loading.is_some()),
            "the file is read again"
        );
        assert_eq!(
            pane_name(cx).as_deref(),
            Some("Reading tracked file…"),
            "named by the read, not by a name an earlier read held"
        );
        drop(waiting);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        assert_eq!(pane_name(cx).as_deref(), Some("Binary file · no preview"));
        assert!(cx.update(|window, _| source.is_focused(window)));
    }

    /// A re-read of Quick source's image whose reply the worker drops ends
    /// with nothing reading and nothing shown: the pane, still holding
    /// focus, is named "No preview", as it draws, under its own ID, the
    /// image's name and node it held through the read spent. Kept, they
    /// named the pane by the image it no longer showed.
    #[gpui::test]
    async fn quick_source_names_a_dropped_read_no_preview(cx: &mut gpui::TestAppContext) {
        use crate::native_accessibility::written;
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        quick_open(&app, cx, "image");
        cx.simulate_keystrokes("enter");
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        let image = pane_name(cx).expect("the image's root holds focus");
        assert!(written::get(crate::image_compare::IMAGE_ROOT).is_some());

        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes(refresh);
        draw(cx);
        assert_eq!(
            pane_name(cx),
            Some(image),
            "the read holds the image's name"
        );
        // Dropped unanswered, as a cancel that leaves the read's generation
        // as it was drops it.
        app.read_with(cx, |app, _| app.worker.cancel());
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        cx.executor().run_until_parked();
        assert!(
            app.read_with(cx, |app, _| app.loading.is_none()
                && app.content.is_none()
                && app.error.is_none()),
            "the read ends with nothing read"
        );
        written::forget();
        crate::width_walk::frame(cx);
        assert_eq!(
            written::get("quick-source-pane")
                .and_then(|written| written.node.label().map(str::to_owned))
                .as_deref(),
            Some("No preview"),
            "the pane is named by what it draws"
        );
        assert!(
            written::get(crate::image_compare::IMAGE_ROOT).is_none(),
            "under its own ID"
        );
        assert!(cx.update(|window, _| source.is_focused(window)));
    }

    /// The first frame after Quick Open's accept, by Enter and by a click on
    /// its row, opened over History and over Changes, draws one ring: the
    /// placeholder's, on the text pane's edge, while the file is read. The
    /// dialog's close gives focus back to its opener and the inspection takes
    /// it in the same action, so no frame draws the opener's ring or none.
    #[gpui::test]
    async fn quick_source_rings_only_its_pane_in_the_first_frame_after_accept(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        draw(cx);
        let accent: gpui::Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        let rings = |cx: &mut gpui::VisualTestContext| {
            crate::focus_ring::painted_outlines(cx, accent)
                .into_iter()
                .filter(|outline| outline.drawn)
                .map(|outline| outline.bounds)
                .collect::<Vec<_>>()
        };
        for changes in [false, true] {
            if changes {
                cx.dispatch_action(ShowChanges);
                settle(&app, cx).await;
            }
            for click in [false, true] {
                let opener = if changes { "Changes" } else { "History" };
                let press = if click { "a click" } else { "Enter" };
                quick_open(&app, cx, "notes");
                let held = app.read_with(cx, |app, _| app.worker.hold());
                if click {
                    crate::copy_feedback::tests::click(cx, "quick-file-0");
                } else {
                    cx.simulate_keystrokes("enter");
                    // Released, as a person does: a press with no key-up before the
                    // next is an X11 repeat, which Quick Open answers with nothing.
                    crate::branch_actions::tests::enter(cx, None);
                }
                // The frame the close draws before the next input, and the
                // next frame the platform asks for: one ring in each.
                for frame in ["the close's", "the next"] {
                    if frame == "the next" {
                        cx.update(|window, cx| {
                            window.simulate_next_frame(cx);
                            window.draw(cx).clear(cx);
                        });
                    }
                    assert!(
                        app.read_with(cx, |app, _| app.is_quick_source() && app.loading.is_some()),
                        "{opener}, {press}: Quick source is open, its file read"
                    );
                    let pane = cx.debug_bounds("preview-content").expect("the text pane");
                    assert_eq!(
                        cx.debug_bounds("compare-text-ring"),
                        Some(pane),
                        "{opener}, {press}, {frame} frame: the ring on the pane's edge"
                    );
                    assert_eq!(
                        rings(cx),
                        vec![pane],
                        "{opener}, {press}, {frame} frame: and no ring elsewhere"
                    );
                }
                drop(held);
                app.read_with(cx, |app, _| app.worker.wait_idle());
                settle(&app, cx).await;
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.close_revision_inspection(window, cx))
                });
                settle(&app, cx).await;
            }
        }
    }

    /// Quick source opened on a slow read and Settings shown before the
    /// text lands: Back returns focus to the text, ringed on the pane's
    /// edge, as its landing gives it on the repository page. Back gave focus
    /// to the pane's handle it saved, which the information beside the text
    /// tracks once the text is shown, so focus and the ring rested there.
    #[gpui::test]
    async fn quick_source_text_landed_under_settings_takes_focus_on_back(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        quick_open(&app, cx, "notes");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the pane holds focus while the file is read"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.loading.is_none()
                && app.content.is_some()),
            "the text lands while Settings shows"
        );

        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        settle(&app, cx).await;
        draw(cx);
        draw(cx);
        let text = app
            .read_with(cx, |app, cx| {
                app.after_editor
                    .as_ref()
                    .map(|editor| editor.read(cx).focus_handle(cx))
            })
            .expect("the text is shown");
        assert!(
            cx.update(|window, _| text.is_focused(window)),
            "Back gives focus to the text"
        );
        assert_eq!(
            cx.debug_bounds("compare-text-ring"),
            cx.debug_bounds("preview-content"),
            "ringed on the pane's edge"
        );
        assert!(
            cx.debug_bounds("quick-source-information-ring").is_none(),
            "and not on the information"
        );
    }

    /// Text that landed while Settings showed owes nothing once Back has
    /// returned: Tab moved focus off the pane while the file was read, so
    /// Back gives the text no focus and settles the landing, and a later
    /// Refresh with focus clicked onto the information leaves it there.
    /// Back returns focus to the stop Tab reached, "Choose targets…", ringed
    /// in Back's first frame, never to the window's root. Left owed, that
    /// Refresh's landing moved focus and the ring from the information to
    /// the text with no key pressed; and keyed by the render, the Button's
    /// handle went while Settings showed, so Back gave focus to the
    /// information.
    #[gpui::test]
    async fn quick_source_text_landed_under_settings_owes_nothing_after_back(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let root = app.read_with(cx, |app, _| app.app_focus.clone());
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        quick_open(&app, cx, "notes");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(cx.update(|window, _| source.is_focused(window)));
        cx.simulate_keystrokes("tab");
        let moved = cx
            .update(|window, cx| window.focused(cx))
            .expect("Tab finds a stop");
        assert_ne!(moved, source, "Tab leaves the pane");
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.content.is_some()),
            "the text lands while Settings shows"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        // Back's first frame.
        crate::width_walk::frame(cx);
        let back = cx
            .update(|window, cx| window.focused(cx))
            .expect("Back gives focus to a control");
        assert_ne!(back, root, "never to the window's root");
        assert_eq!(back, moved, "Back returns focus to the stop Tab reached");
        assert!(
            app.read_with(cx, |app, _| app.focus_ring.shows(&moved)),
            "ringed in Back's first frame"
        );
        settle(&app, cx).await;
        draw(cx);
        let text = app
            .read_with(cx, |app, cx| {
                app.after_editor
                    .as_ref()
                    .map(|editor| editor.read(cx).focus_handle(cx))
            })
            .expect("the text is shown");
        assert!(
            !cx.update(|window, _| text.is_focused(window)),
            "Back leaves the text unfocused"
        );
        assert!(
            cx.update(|window, _| moved.is_focused(window)),
            "and focus where Back put it"
        );

        // A click on the information, below its controls.
        let inspector = cx
            .debug_bounds("revision-inspector")
            .expect("the information is drawn");
        cx.simulate_click(
            gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
            gpui::Modifiers::none(),
        );
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the click puts focus on the information"
        );
        cx.simulate_keystrokes(refresh);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.loading.is_none() && app.content.is_some()),
            "Refresh's read lands"
        );
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "focus stays on the information"
        );
        assert!(!cx.update(|window, _| text.is_focused(window)));
        assert!(
            cx.debug_bounds("quick-source-information-ring").is_some(),
            "ringed there"
        );
    }

    /// The text's focus handle Quick source shows now, if it shows its text.
    fn quick_source_text(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> Option<FocusHandle> {
        app.read_with(cx, |app, cx| {
            app.after_editor
                .as_ref()
                .map(|editor| editor.read(cx).focus_handle(cx))
        })
    }

    /// Rendered Markdown that landed while Settings showed is settled as
    /// Back returns, as text is: focus returns to the pane that held it,
    /// named by the rendered view it shows, and once Source is chosen and
    /// the information clicked, a later Refresh leaves focus there. Back
    /// settled only a landing that showed an editor, so the rendered one
    /// stayed owed, and that Refresh moved focus from the information to the
    /// text with no key pressed.
    #[gpui::test]
    async fn quick_source_rendered_markdown_landed_under_settings_owes_nothing_after_back(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        std::fs::write(path.join("README.md"), "# Notes\n\nSome text\n").unwrap();
        crate::tags::tests::git(&path, &["add", "README.md"]);
        crate::tags::tests::git(&path, &["commit", "--quiet", "-m", "Markdown"]);
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        quick_open(&app, cx, "README");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the pane holds focus while the file is read"
        );
        // The file lands Rendered, as the tab-order tests choose it.
        cx.update(|_, cx| app.update(cx, |app, _| app.text_mode = TextMode::Markdown));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.content.is_some()
                && app.text_mode == TextMode::Markdown),
            "Rendered Markdown lands while Settings shows"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        settle(&app, cx).await;
        draw(cx);
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "Back returns focus to the pane"
        );
        assert_eq!(
            pane_name(cx).as_deref(),
            Some("Rendered Markdown"),
            "named by the rendered view it shows"
        );

        source_then_refresh_keeps_the_information(&app, cx).await;
    }

    /// Rendered Markdown that landed on the repository page holds focus on
    /// the pane; once Source is chosen and the information clicked, a
    /// Refresh leaves focus and its ring on the information through its
    /// read and its landing. Still marked as the rendered view's holder, the
    /// reading pane took focus and the ring from the information, spoken as
    /// "Refreshing tracked file…", and gave them back as the text landed:
    /// two moves with no key pressed.
    #[gpui::test]
    async fn quick_source_rendered_markdown_then_source_refreshes_on_the_information(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        std::fs::write(path.join("README.md"), "# Notes\n\nSome text\n").unwrap();
        crate::tags::tests::git(&path, &["add", "README.md"]);
        crate::tags::tests::git(&path, &["commit", "--quiet", "-m", "Markdown"]);
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        quick_open(&app, cx, "README");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        // The file lands Rendered, as the tab-order tests choose it.
        cx.update(|_, cx| app.update(cx, |app, _| app.text_mode = TextMode::Markdown));
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the rendered view's pane holds focus"
        );
        assert_eq!(pane_name(cx).as_deref(), Some("Rendered Markdown"));
        source_then_refresh_keeps_the_information(&app, cx).await;
    }

    /// Source, by its button, a click on the information below its
    /// controls, and a Refresh whose read is held: from the first frame
    /// after the key, the reading pane draws no group of its own and the
    /// information keeps focus and its ring, through the read and as the
    /// text lands.
    async fn source_then_refresh_keeps_the_information(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) {
        use crate::views::tests::{draw, settle};
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        let button = cx.debug_bounds("Source").expect("Source is offered");
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        draw(cx);
        assert!(
            quick_source_text(app, cx).is_some(),
            "Source shows the text"
        );
        let inspector = cx
            .debug_bounds("revision-inspector")
            .expect("the information is drawn");
        cx.simulate_click(
            gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
            gpui::Modifiers::none(),
        );
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the click puts focus on the information"
        );
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes(refresh);
        crate::width_walk::frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.content.is_none()),
            "the file is read again"
        );
        assert!(
            cx.debug_bounds("quick-source-pane").is_none(),
            "the reading pane holds no focus"
        );
        assert!(
            cx.update(|window, _| source.is_focused(window))
                && cx.debug_bounds("quick-source-information-ring").is_some(),
            "the information keeps focus and its ring from the read's first frame"
        );
        draw(cx);
        assert!(cx.debug_bounds("quick-source-pane").is_none());
        assert!(cx.debug_bounds("quick-source-information-ring").is_some());
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(app, cx).await;
        crate::width_walk::frame(cx);
        let text = quick_source_text(app, cx).expect("Refresh's read lands its text");
        assert!(
            cx.update(|window, _| source.is_focused(window))
                && cx.debug_bounds("quick-source-information-ring").is_some(),
            "focus and its ring stay on the information as the text lands"
        );
        assert!(!cx.update(|window, _| text.is_focused(window)));
    }

    /// Rendered Markdown that landed holds focus on the pane; once Source is
    /// chosen and the information clicked, choosing Rendered again leaves
    /// focus on the information, its ring as it was, from the click's first
    /// frame,
    /// the rendered view drawn with no pane over it. Still marked as the
    /// holder the rendered view had left it, the pane was drawn over the
    /// rendered view again and took the information's focus and ring on
    /// that click.
    #[gpui::test]
    async fn quick_source_rendered_again_after_source_leaves_focus_on_the_information(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        std::fs::write(path.join("README.md"), "# Notes\n\nSome text\n").unwrap();
        crate::tags::tests::git(&path, &["add", "README.md"]);
        crate::tags::tests::git(&path, &["commit", "--quiet", "-m", "Markdown"]);
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        quick_open(&app, cx, "README");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        // The file lands Rendered, as the tab-order tests choose it.
        cx.update(|_, cx| app.update(cx, |app, _| app.text_mode = TextMode::Markdown));
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert_eq!(pane_name(cx).as_deref(), Some("Rendered Markdown"));
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the rendered view's pane holds focus"
        );

        let button = cx.debug_bounds("Source").expect("Source is offered");
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        draw(cx);
        assert!(
            quick_source_text(&app, cx).is_some(),
            "Source shows the text"
        );
        let inspector = cx
            .debug_bounds("revision-inspector")
            .expect("the information is drawn");
        cx.simulate_click(
            gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
            gpui::Modifiers::none(),
        );
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the click puts focus on the information"
        );
        let ring = cx.debug_bounds("quick-source-information-ring");

        let button = cx.debug_bounds("Rendered").expect("Rendered is offered");
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        // The click's first frame, and the frames after it.
        for frame in 0..2 {
            if frame == 0 {
                crate::width_walk::frame(cx);
            } else {
                draw(cx);
            }
            assert!(
                app.read_with(cx, |app, _| app.text_mode == TextMode::Markdown),
                "frame {frame}: Rendered is shown"
            );
            assert!(
                cx.debug_bounds("quick-source-pane").is_none(),
                "frame {frame}: no pane is drawn over the rendered view"
            );
            assert!(
                cx.update(|window, _| source.is_focused(window))
                    && cx.debug_bounds("quick-source-information-ring") == ring
                    && cx.debug_bounds("compare-text-ring").is_none(),
                "frame {frame}: the information keeps focus, its ring as it was"
            );
        }
    }

    /// Quick source of `README.md`, committed with `diagrams` Mermaid
    /// diagrams, landed in Rendered with focus on the pane, named by the
    /// rendered view it draws and ringed on its edge, as a landing gives it.
    async fn rendered_landing(
        cx: &mut gpui::TestAppContext,
        diagrams: usize,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut gpui::VisualTestContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let mut markdown = String::from("# Notes\n\nSome text\n");
        for _ in 0..diagrams {
            markdown.push_str("\n```mermaid\nflowchart LR; A-->B\n```\n");
        }
        std::fs::write(path.join("README.md"), markdown).unwrap();
        crate::tags::tests::git(&path, &["add", "README.md"]);
        crate::tags::tests::git(&path, &["commit", "--quiet", "-m", "Markdown"]);
        quick_open(&app, cx, "README");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        // The file lands Rendered, as the tab-order tests choose it.
        cx.update(|_, cx| app.update(cx, |app, _| app.text_mode = TextMode::Markdown));
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        assert_eq!(pane_name(cx).as_deref(), Some("Rendered Markdown"));
        assert_pane_rings(cx, &source, "the rendered landing");
        (fixture, app, cx)
    }

    /// In the frame drawn last, `focus` holds the window's focus, ringed on
    /// the text pane's whole edge, and nothing rings the information.
    #[track_caller]
    fn assert_pane_rings(cx: &mut gpui::VisualTestContext, focus: &FocusHandle, at: &str) {
        assert!(
            cx.update(|window, _| focus.is_focused(window)),
            "{at}: focus is in the text pane, on what it shows"
        );
        let pane = cx.debug_bounds("preview-content").expect("the text pane");
        assert_eq!(
            cx.debug_bounds("compare-text-ring"),
            Some(pane),
            "{at}: ringed on the text pane's whole edge"
        );
        assert!(
            cx.debug_bounds("quick-source-information-ring").is_none(),
            "{at}: and not on the information"
        );
    }

    /// Clicks the mode button `name` and draws the click's first frame.
    fn click_mode(cx: &mut gpui::VisualTestContext, name: &'static str) {
        let button = cx
            .debug_bounds(name)
            .unwrap_or_else(|| panic!("{name} is offered"));
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        crate::width_walk::frame(cx);
    }

    /// Source chosen while the rendered view's pane holds focus gives focus
    /// to the text it shows, ringed on the same edge from the click's first
    /// frame, as a landing in Source gives it, and the information takes
    /// none. The pane gave up its hold as the text showed and focus stayed
    /// on its handle, which the information beside the text tracks: focus
    /// moved there.
    #[gpui::test]
    async fn quick_source_source_from_the_rendered_pane_focuses_its_text(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = rendered_landing(cx, 0).await;
        let ring = cx.debug_bounds("compare-text-ring");
        click_mode(cx, "Source");
        let text = quick_source_text(&app, cx).expect("Source shows the text");
        assert_pane_rings(cx, &text, "Source's first frame");
        assert_eq!(
            cx.debug_bounds("compare-text-ring"),
            ring,
            "on the edge the pane rang"
        );
        assert!(cx.debug_bounds("quick-source-pane").is_none());
        draw(cx);
        assert_pane_rings(cx, &text, "the frames after it");
    }

    /// With focus in the text, Rendered or Diagrams gives it to the pane,
    /// named by the view it draws, "Rendered Markdown" or "Mermaid
    /// diagrams", ringed on the same edge from the click's first frame; and
    /// Source chosen from the rendered view's own stop gives it back to the
    /// text. Focus stayed on the editor or the rendered view, which the new
    /// mode no longer draws: nothing showed focus and no key reached a
    /// control.
    #[gpui::test]
    async fn quick_source_mode_changes_keep_focus_in_the_text_pane(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = rendered_landing(cx, 1).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        click_mode(cx, "Source");
        let text = quick_source_text(&app, cx).expect("Source shows the text");
        // Focus moved to the text by a click on it.
        let content = cx.debug_bounds("preview-content").expect("the text pane");
        cx.simulate_click(content.center(), gpui::Modifiers::none());
        draw(cx);
        assert!(cx.update(|window, _| text.is_focused(window)));

        for (mode, name) in [
            ("Rendered", "Rendered Markdown"),
            ("Diagrams", "Mermaid diagrams"),
        ] {
            crate::native_accessibility::written::forget();
            click_mode(cx, mode);
            assert_eq!(
                written_pane_name().as_deref(),
                Some(name),
                "{mode}'s first frame: the pane is named by its view"
            );
            assert_pane_rings(cx, &source, &format!("{mode}'s first frame"));
            click_mode(cx, "Source");
            assert_pane_rings(cx, &text, &format!("Source after {mode}"));
        }

        // From the rendered view's own stop, a Tab from the pane.
        click_mode(cx, "Rendered");
        let rendered = app
            .read_with(cx, |app, cx| {
                app.markdown_view
                    .as_ref()
                    .map(|view| view.read(cx).focus_handle())
            })
            .expect("the Markdown is rendered");
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            cx.update(|window, _| rendered.is_focused(window)),
            "Tab reaches the rendered view's own stop"
        );
        click_mode(cx, "Source");
        assert_pane_rings(cx, &text, "Source from the rendered view's stop");

        // The rendered view's stop with no pane around it: Rendered chosen
        // with focus clicked onto the information beside the text, then
        // focus moved to the rendered view, as a Tab moves it.
        let inspector = cx
            .debug_bounds("revision-inspector")
            .expect("the information is drawn");
        cx.simulate_click(
            gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
            gpui::Modifiers::none(),
        );
        draw(cx);
        assert!(cx.update(|window, _| source.is_focused(window)));
        click_mode(cx, "Rendered");
        assert!(
            cx.debug_bounds("quick-source-pane").is_none(),
            "the pane holds no focus"
        );
        cx.update(|window, cx| window.focus(&rendered, cx));
        draw(cx);
        click_mode(cx, "Source");
        assert_pane_rings(cx, &text, "Source from the rendered view's bare stop");
    }

    /// With the Source text's Find bar open and its field focused, Rendered
    /// or Diagrams gives focus to the pane, named by the view it draws and
    /// ringed on its edge from the click's first frame, as from the text.
    /// The bar is drawn beside the text it searches, not inside it, so
    /// focus stayed on a field the new mode no longer draws.
    #[gpui::test]
    async fn quick_source_mode_changes_from_the_find_bar_keep_focus_in_the_text_pane(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = rendered_landing(cx, 1).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        for (mode, name) in [
            ("Rendered", "Rendered Markdown"),
            ("Diagrams", "Mermaid diagrams"),
        ] {
            click_mode(cx, "Source");
            let text = quick_source_text(&app, cx).expect("Source shows the text");
            assert!(cx.update(|window, _| text.is_focused(window)));
            // Find, as the platform's Find key opens it from the text.
            cx.dispatch_action(gpui_kit::component::input::Search);
            draw(cx);
            assert!(
                cx.debug_bounds("file-find-bar").is_some(),
                "{mode}: Find opens over the text"
            );
            let field = cx.update(|window, cx| window.focused(cx));
            assert!(
                field.is_some() && field != Some(text.clone()) && field != Some(source.clone()),
                "{mode}: Find's field holds focus"
            );
            crate::native_accessibility::written::forget();
            click_mode(cx, mode);
            assert_eq!(
                written_pane_name().as_deref(),
                Some(name),
                "{mode} from Find's field: the pane is named by its view"
            );
            assert_pane_rings(cx, &source, &format!("{mode} from Find's field"));
            draw(cx);
            assert_pane_rings(cx, &source, &format!("{mode} from Find's field, later"));
        }
    }

    /// With focus on a control the Diagrams view draws in the pane, Copy
    /// source, its last stop, or Next diagram above it, reached by Shift+Tab
    /// from the information while the pane holds no focus, Source gives
    /// focus to the text, ringed on the pane's edge from the click's first
    /// frame. Only the rendered Markdown's stop was counted, so focus stayed
    /// on a button the new mode no longer draws.
    #[gpui::test]
    async fn quick_source_source_from_a_diagrams_control_focuses_its_text(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = rendered_landing(cx, 2).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        for (presses, control) in [(1, "Copy source"), (2, "Next diagram")] {
            click_mode(cx, "Source");
            // A click on the information, below its controls.
            let inspector = cx
                .debug_bounds("revision-inspector")
                .expect("the information is drawn");
            cx.simulate_click(
                gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
                gpui::Modifiers::none(),
            );
            draw(cx);
            assert!(cx.update(|window, _| source.is_focused(window)));
            click_mode(cx, "Diagrams");
            assert!(
                cx.debug_bounds("quick-source-pane").is_none(),
                "{control}: the pane holds no focus"
            );
            // Each stop on the way is a control of the view's own.
            let mut reached = Vec::new();
            for _ in 0..presses {
                cx.simulate_keystrokes("shift-tab");
                draw(cx);
                let on = cx.update(|window, cx| window.focused(cx));
                assert!(
                    on.is_some()
                        && on != Some(source.clone())
                        && !reached.contains(&on)
                        && cx.update(|window, _| crate::rich_preview::holds_focus(window)),
                    "{control}: Shift+Tab reaches a control of the Diagrams view's own"
                );
                reached.push(on);
            }
            click_mode(cx, "Source");
            let text = quick_source_text(&app, cx).expect("Source shows the text");
            assert_pane_rings(cx, &text, &format!("Source from {control}"));
        }
    }

    /// What a mode chosen from the collapsed menu left: the mode, whether
    /// the pane's handle holds focus, and whether the pane holding it is
    /// drawn, whether the text holds focus, and the text pane's ring, the
    /// pane's bounds and the information's ring.
    type MenuChoice = (
        TextMode,
        (bool, bool),
        bool,
        Option<Bounds<Pixels>>,
        Option<Bounds<Pixels>>,
        Option<Bounds<Pixels>>,
    );

    fn menu_choice(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) -> MenuChoice {
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let text = quick_source_text(app, cx);
        (
            app.read_with(cx, |app, _| app.text_mode),
            (
                cx.update(|window, _| source.is_focused(window)),
                cx.debug_bounds("quick-source-pane").is_some(),
            ),
            text.is_some_and(|text| cx.update(|window, _| text.is_focused(window))),
            cx.debug_bounds("compare-text-ring"),
            cx.debug_bounds("preview-content"),
            cx.debug_bounds("quick-source-information-ring"),
        )
    }

    /// Opens the collapsed modes' menu by a click, presses `keys` and draws
    /// the choice's first frame.
    fn choose_from_menu(cx: &mut gpui::VisualTestContext, keys: &str) {
        let menu = cx.debug_bounds("text-mode-menu").expect("the modes' menu");
        cx.simulate_click(menu.center(), gpui::Modifiers::none());
        crate::views::tests::draw(cx);
        cx.simulate_keystrokes(keys);
        crate::width_walk::frame(cx);
    }

    /// The collapsed "Comparison mode" menu, at 18 pt in a 461 × 490 window,
    /// too narrow for Source, Diagrams and Rendered as buttons, acts on where
    /// focus was as it opened: Rendered chosen again with the pane holding
    /// focus keeps focus and its ring on the pane from the choice's first
    /// frame, Source then gives it to the text, ringed on the same edge, and
    /// Rendered chosen with focus clicked onto the information beside the
    /// text leaves it there. The menu held focus as its item acted, so the pane
    /// gave up its hold, and the menu's close returned focus to the pane's
    /// handle, which the information tracked: focus and its ring moved there.
    #[gpui::test]
    async fn quick_source_mode_menu_acts_on_the_focus_it_opened_from(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = rendered_landing(cx, 1).await;
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let collapsed = cx.debug_bounds("text-mode-menu").is_some();
        // Read first, the text size restored before any assertion.
        let mut seen = Vec::new();
        if collapsed {
            // Source, Diagrams, Rendered: Rendered, the last, chosen again.
            choose_from_menu(cx, "end enter");
            seen.push(menu_choice(&app, cx));
            // Source, the first, from the pane.
            choose_from_menu(cx, "home enter");
            seen.push(menu_choice(&app, cx));
            // A click on the information, below its controls.
            let inspector = cx
                .debug_bounds("revision-inspector")
                .expect("the information is drawn");
            cx.simulate_click(
                gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
                gpui::Modifiers::none(),
            );
            draw(cx);
            seen.push(menu_choice(&app, cx));
            choose_from_menu(cx, "end enter");
            seen.push(menu_choice(&app, cx));
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
        assert!(collapsed, "a narrow window collapses the modes");
        let (mode, (focused, pane), _, ring, content, information) = seen[0];
        assert!(mode == TextMode::Markdown, "Rendered is chosen again");
        assert!(
            focused && pane,
            "the pane keeps focus from the choice's first frame"
        );
        assert!(ring.is_some() && ring == content, "ringed on its edge");
        assert!(information.is_none(), "and not on the information");
        let (mode, _, text, ring, content, information) = seen[1];
        assert!(mode == TextMode::After, "Source is chosen");
        assert!(text, "the text takes the pane's focus");
        assert!(ring.is_some() && ring == content, "ringed on the same edge");
        assert!(information.is_none(), "and not on the information");
        let (_, (clicked, pane), ..) = seen[2];
        assert!(clicked && !pane, "the click puts focus on the information");
        let (mode, (kept, pane), _, ring, _, information) = seen[3];
        assert!(mode == TextMode::Markdown, "Rendered is chosen");
        assert!(kept && !pane, "focus stays on the information");
        assert!(information.is_some(), "ringed there, as the keys left it");
        assert!(ring.is_none(), "and not on the text pane");
    }

    /// Text that landed while Settings showed, its tab then left from
    /// Settings and shown again, is drawn as the tab returns and takes the
    /// focus its pane held as the window left, as Back gives it, ringed on
    /// the pane's edge in the frame that draws the tab; it owes nothing
    /// after that, so focus clicked onto the information stays there through
    /// a later Refresh. Restored with its text unbuilt, the pane read
    /// "Loading text…" until a Refresh, and still owed, that Refresh's
    /// landing moved focus from the information to the text with no key
    /// pressed; then the tab gave focus to the information where Back gave
    /// it to the text.
    #[gpui::test]
    async fn quick_source_text_landed_under_settings_draws_as_its_tab_returns(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx) = quick_open_window(cx).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        quick_open(&app, cx, "notes");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Settings
                && app.content.is_some()),
            "the text lands while Settings shows"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(other.path().to_owned(), window, cx)
            })
        });
        settle(&app, cx).await;
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        // The frame that draws the tab.
        crate::width_walk::frame(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Repository
                && app.is_quick_source()),
            "the tab returns to its Quick source"
        );
        let text = quick_source_text(&app, cx).expect("the text is built as its tab returns");
        assert!(
            cx.update(|window, _| text.is_focused(window)),
            "the text takes the focus its pane held, as Back gives it"
        );
        assert_eq!(
            cx.debug_bounds("compare-text-ring"),
            cx.debug_bounds("preview-content"),
            "ringed on the pane's edge"
        );
        assert!(cx.debug_bounds("quick-source-information-ring").is_none());
        settle(&app, cx).await;
        assert_eq!(
            pane_name(cx),
            None,
            "the pane shows the text, not a line in its place"
        );
        assert!(cx.update(|window, _| text.is_focused(window)));

        // A click on the information, below its controls.
        let inspector = cx
            .debug_bounds("revision-inspector")
            .expect("the information is drawn");
        cx.simulate_click(
            gpui::point(inspector.center().x, inspector.bottom() - px(8.)),
            gpui::Modifiers::none(),
        );
        draw(cx);
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "the click puts focus on the information"
        );
        cx.simulate_keystrokes(refresh);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        let text = quick_source_text(&app, cx).expect("Refresh's read lands its text");
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "focus stays on the information"
        );
        assert!(!cx.update(|window, _| text.is_focused(window)));
    }

    /// Quick source draws no footer beneath its pane: its file's mode leads
    /// the information's identities, so in a short window its
    /// image gives way only to its own labels. At 18 pt in a 461 × 490
    /// window the image keeps its usable minimum, a scaled 96 px (133), its
    /// notes given way and its captions whole in the pane: 142 px, the whole
    /// room beneath the captions. Drawn beneath the pane, the footer's
    /// "Tracked source · mode 100644" outlasted the image, which gave up its
    /// minimum first and kept 109 px.
    #[gpui::test]
    async fn quick_source_image_gives_its_room_to_no_footer(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        quick_open(&app, cx, "image");
        cx.simulate_keystrokes("enter");
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        assert_eq!(
            information_lines(cx).1.as_deref(),
            Some("Mode 100644"),
            "the file's mode leads the information's identities"
        );
        assert!(
            cx.debug_bounds("compare-footer").is_none(),
            "Quick source draws no footer"
        );

        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        settle(&app, cx).await;
        draw(cx);
        draw(cx);
        let bounds = |selector: &'static str, cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let slack = px(0.5);
        assert!(
            cx.debug_bounds("compare-footer").is_none(),
            "18 pt in 461 × 490: no footer"
        );
        let pane = bounds("preview-content", cx);
        let captions = ["image-side-caption-0", "image-side-caption-1"]
            .into_iter()
            .filter_map(|caption| cx.debug_bounds(caption))
            .collect::<Vec<_>>();
        assert!(!captions.is_empty(), "the image's caption is drawn");
        for caption in &captions {
            assert!(
                caption.top() >= pane.top() - slack && caption.bottom() <= pane.bottom() + slack,
                "the caption {caption:?} is whole in the pane {pane:?}"
            );
        }
        let room = bounds("image-stage-room", cx);
        let image = bounds("image-canvas-area", cx);
        let minimum = crate::appearance::ui_size(crate::image_compare::STAGE_MIN);
        assert!(
            image.size.height >= minimum - slack,
            "the image {image:?} keeps its minimum {minimum:?}"
        );
        // Where notes beneath it would take it under its minimum, they give
        // way, and the image takes their room too.
        let notes = cx.debug_bounds("image-source-scale").is_some()
            || cx.debug_bounds("image-interaction-guidance").is_some();
        assert!(
            notes || (room.size.height - image.size.height).abs() <= slack,
            "the image {image:?} takes the room {room:?} its notes left"
        );
        assert!(
            room.bottom() <= pane.bottom() + slack,
            "the room {room:?} ends in the pane {pane:?}"
        );
    }

    /// Quick source's working-file scope, "Current working file · raw local
    /// content", is drawn by phrase beneath the mode and path: at 18 pt, in
    /// each room from 170 down to 100 px a narrowing window gives the
    /// identities, its lines read as the scope does, lie inside the room,
    /// and none starts with its "·", and the identities still read as one
    /// text. Drawn as one text with the mode and path, it put the "·" at a
    /// line's start: "· raw local content" in a 140 px room.
    #[gpui::test]
    async fn quick_source_scope_line_never_starts_with_its_separator(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::settle;
        let (_fixture, app, cx) = quick_open_window(cx).await;
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        quick_open(&app, cx, "notes");
        cx.simulate_keystrokes("enter");
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        let (_, first) = information_lines(cx);
        assert!(
            first.is_some_and(|line| line.starts_with("Mode ")),
            "the identities read as one text, the mode first"
        );
        let spoken = crate::native_accessibility::written::get("revision-identities")
            .and_then(|text| text.node.value().map(str::to_owned))
            .unwrap_or_default();
        assert!(
            spoken.ends_with(&format!("\n\n{}", super::WORKING_SCOPE)),
            "the scope is its last line, a line apart: {spoken:?}"
        );
        let mut rooms = Vec::new();
        let mut failures = Vec::new();
        for width in (400..=620).rev() {
            cx.simulate_resize(size(px(width as f32), px(490.)));
            crate::width_walk::frame(cx);
            let Some(room) = cx.debug_bounds("revision-identities") else {
                continue;
            };
            if room.size.width < px(100.) || room.size.width > px(170.) {
                continue;
            }
            rooms.push(room.size.width);
            let lines =
                crate::phrase_tests::drawn_lines(cx, super::WORKING_SCOPE_ID, super::WORKING_SCOPE);
            let read: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
            let at = format!("{width} px window, a {:?} room", room.size.width);
            if read.join(" ") != super::WORKING_SCOPE {
                failures.push(format!("{at}: the lines {read:?}"));
            }
            for line in &lines {
                if line.text.starts_with('·') {
                    failures.push(format!("{at}: a line starts with \"·\": {read:?}"));
                }
                if line.left < room.left() - px(0.5) || line.right > room.right() + px(0.5) {
                    failures.push(format!("{at}: {line:?} is cut by the room {room:?}"));
                }
            }
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
        let least = rooms.iter().copied().fold(px(f32::MAX), Pixels::min);
        let most = rooms.iter().copied().fold(px(0.), Pixels::max);
        assert!(
            least < px(102.) && most > px(168.),
            "the window's widths gave rooms of {least:?} to {most:?} only"
        );
        assert!(
            failures.is_empty(),
            "{} of {} rooms fail:\n{}",
            failures.len(),
            rooms.len(),
            failures.join("\n")
        );
    }

    /// The information's title and the first line of its identities, as
    /// the frame drawn now writes them.
    fn information_lines(cx: &mut gpui::VisualTestContext) -> (Option<String>, Option<String>) {
        use crate::native_accessibility::written;
        written::forget();
        crate::views::tests::draw(cx);
        let value =
            |id: &str| written::get(id).and_then(|text| text.node.value().map(str::to_owned));
        (
            value("revision-title"),
            value("revision-identities").and_then(|lines| lines.lines().next().map(str::to_owned)),
        )
    }

    /// Quick source's information is titled "Tracked file" alone, its file's
    /// mode the first line of its identities as the file's read found it: a
    /// working file made executable reads "Mode 100755" once Refresh's read
    /// lands, the mode read last standing while the file is read again, the
    /// listing's before the first read lands and that read's own after it. At
    /// 18 pt the title keeps one line in a 461 × 490 window. Taken from
    /// Quick Open's listing, the mode never showed the `chmod +x`; and on the
    /// title's line, "Tracked file · mode 100644" wrapped its "·" at 18 pt in
    /// a 461 × 490 window.
    #[cfg(unix)]
    #[gpui::test]
    async fn quick_source_information_reads_the_mode_its_read_found(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::{draw, settle};
        use std::os::unix::fs::PermissionsExt as _;
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let refresh = if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        };
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        quick_open(&app, cx, "notes");
        cx.simulate_keystrokes("enter");
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        assert_eq!(
            information_lines(cx),
            (Some("Tracked file".into()), Some("Mode 100644".into())),
            "titled alone, the mode leading the identities"
        );
        let identities = crate::native_accessibility::written::get("revision-identities")
            .and_then(|text| text.node.value().map(str::to_owned))
            .unwrap_or_default();
        assert!(
            identities.contains("Current working file"),
            "the working file is the source: {identities}"
        );

        let file = path.join("notes.txt");
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&file, permissions).unwrap();
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes(refresh);
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.loading.is_some()),
            "the file is read again"
        );
        assert_eq!(
            information_lines(cx).1.as_deref(),
            Some("Mode 100644"),
            "the mode read last stands while it reads"
        );
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        assert_eq!(
            information_lines(cx).1.as_deref(),
            Some("Mode 100755"),
            "the working file's own mode once the read lands"
        );
        // Read again, the mode that read found stands, not the listing's.
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes(refresh);
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.loading.is_some()),
            "the file is read a third time"
        );
        assert_eq!(
            information_lines(cx).1.as_deref(),
            Some("Mode 100755"),
            "the mode the last read found stands while it reads again"
        );
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;

        // The title's room as the window lays it out, and its lines there in
        // the bundled interface font a Linux session draws it in; the test
        // window's own text system gives every glyph one width.
        cx.simulate_resize(size(px(461.), px(490.)));
        settle(&app, cx).await;
        draw(cx);
        #[cfg(target_os = "linux")]
        {
            let room = cx
                .debug_bounds("revision-title")
                .expect("the title")
                .size
                .width;
            let size =
                cx.update(|window, _| window.text_style().font_size.to_pixels(window.rem_size()));
            let title = information_lines(cx).0.expect("the title");
            assert_eq!(
                crate::views::tests::interface_lines(&title, size, FontWeight::SEMIBOLD, room),
                vec![title.clone()],
                "the title keeps one line at 18 pt in 461 × 490, {room:?} wide"
            );
        }
    }

    /// An image in Quick source that decodes into nothing keeps focus and
    /// the ring on the pane, a group named by why it shows nothing, the
    /// decode's first line, while the comparison's root takes no role
    /// between the pane and the area that says why. No test read that
    /// group's name, which is written outside the wrappers a test reads.
    #[gpui::test]
    async fn quick_source_names_an_undecoded_image_by_its_failure(cx: &mut gpui::TestAppContext) {
        use crate::views::tests::{draw, edit_image_sides, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        quick_open(&app, cx, "image");
        cx.simulate_keystrokes("enter");
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        assert_eq!(pane_name(cx).as_deref(), Some("PNG image · 1 × 1"));
        edit_image_sides(&app, cx, |side, image| {
            if side == 1 {
                image.image = None;
                image.render = None;
                image.message = Some("\nThe PNG image could not be decoded\nat byte 33".into());
            }
        });
        assert_eq!(
            pane_name(cx).as_deref(),
            Some("The PNG image could not be decoded"),
            "the pane's group is named by the failure's first line that says anything"
        );
        assert_eq!(
            crate::native_accessibility::written::get("quick-source-pane")
                .map(|pane| pane.node.role()),
            Some(Role::Group),
            "the pane holds it, a group"
        );
        assert!(
            crate::native_accessibility::written::get(crate::image_compare::IMAGE_ROOT).is_none(),
            "the image's root takes no role between"
        );
        assert!(
            cx.update(|window, _| source.is_focused(window)),
            "focus stays on the pane"
        );
        assert_eq!(
            cx.debug_bounds("compare-text-ring"),
            cx.debug_bounds("preview-content"),
            "ringed on its edge"
        );
    }

    /// While Quick source's file is read, Tab and Shift+Tab from the pane
    /// reach the stops they reach from the text once it lands, and come back
    /// to the pane as to the text; text that lands once focus has moved
    /// leaves it where it is.
    #[gpui::test]
    async fn quick_source_tabs_from_its_reading_pane_as_from_its_text(
        cx: &mut gpui::TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = quick_open_window(cx).await;
        let source = app.read_with(cx, |app, _| app.source_focus.clone());
        let focused = |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| window.focused(cx));
        quick_open(&app, cx, "notes");
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(focused(cx).as_ref(), Some(&source), "the pane holds focus");
        cx.simulate_keystrokes("shift-tab");
        let before = focused(cx).expect("Shift+Tab finds a stop");
        cx.simulate_keystrokes("tab");
        assert_eq!(
            focused(cx).as_ref(),
            Some(&source),
            "Tab comes back to the pane"
        );
        cx.simulate_keystrokes("tab");
        let after = focused(cx).expect("Tab finds a stop");
        assert_ne!(after, source);
        assert_ne!(before, source);

        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        let text = app
            .read_with(cx, |app, cx| {
                app.after_editor
                    .as_ref()
                    .map(|editor| editor.read(cx).focus_handle(cx))
            })
            .expect("the text lands");
        assert_eq!(
            focused(cx).as_ref(),
            Some(&after),
            "the landing leaves focus where Tab moved it"
        );
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(
            focused(cx).as_ref(),
            Some(&text),
            "Shift+Tab reaches the text"
        );
        // The text's own toolbar controls, drawn once it lands, come between
        // the text and the stop before the pane.
        let mut walked = 0;
        while focused(cx).as_ref() != Some(&before) {
            walked += 1;
            assert!(
                walked <= 8,
                "Shift+Tab from the text reaches the stop before the pane"
            );
            cx.simulate_keystrokes("shift-tab");
        }
        cx.update(|window, cx| window.focus(&text, cx));
        cx.simulate_keystrokes("tab");
        assert_eq!(
            focused(cx).as_ref(),
            Some(&after),
            "and Tab the stop after it, as from the pane"
        );
    }
}
