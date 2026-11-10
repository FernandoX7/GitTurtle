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
        }
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

    /// Quick source's file has been read: focus still waiting on its
    /// information moves to the source text, which draws its ring on the
    /// pane's edge and takes the window's shortcuts. A file with no text to
    /// show, as an image or a binary, leaves it there, where the information
    /// draws the ring. Either way the ring shows however Quick Open was
    /// answered, by a click as by a key, as Settings opens. Focus the person
    /// has moved elsewhere while the file was read stays there, as does
    /// focus on the information itself after a later re-read: only the open
    /// and a re-read that held the text's own focus are owed the text
    /// ([`Inspection::text_focus_owed`]).
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
        if !owed || !self.is_quick_source() || !self.source_focus.is_focused(window) {
            return;
        }
        if let Some(views::ShownText::Editor(editor)) = self.shown_text() {
            editor.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.focus_ring.mark(window);
    }

    /// A read of Quick source's file ended with nothing to show: what it owed
    /// the text is settled, so a later read that succeeds leaves focus where
    /// the person has put it.
    pub(super) fn settle_quick_source_focus(&mut self) {
        if let Some(active) = self.revision_inspection.active.as_deref_mut() {
            active.text_focus_owed = false;
        }
    }

    /// Before a re-read of Quick source's file drops its text: focus held
    /// inside that text moves to the information beside it, as it waits there
    /// while the file is first read ([`Self::files_focus`]), and the read's
    /// landing gives it back ([`Self::focus_quick_source_text`]). Left in the
    /// editor, focus rested on nothing once it had gone, after Refresh or a
    /// write's landing with no key pressed, and the window's keys reached no
    /// control until the next click.
    fn hold_quick_source_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_quick_source() {
            return;
        }
        if let Some(views::ShownText::Editor(editor)) = self.shown_text()
            && editor
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        {
            self.source_focus.focus(window, cx);
            if let Some(active) = self.revision_inspection.active.as_deref_mut() {
                active.text_focus_owed = true;
            }
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
        let (title, details) = match &active.target {
            Target::Comparison(c) => (
                "Revision comparison",
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
            ),
            Target::File { scope, entry } => (
                "Tracked file",
                format!(
                    "{}\n\n{}",
                    entry.path.display(),
                    match scope {
                        PathScope::Worktree => "Current working file · raw local content".into(),
                        PathScope::Revision(oid) => format!("Revision {oid}"),
                    }
                ),
            ),
        };
        div().id("revision-inspector").size_full().min_h_0().flex().flex_col().bg(rgb(p.panel)).border_l_1().border_color(rgb(p.border))
            // Quick source omits the changed-file list. Focus waits on this
            // visible inspector while the file is read (`files_focus`), so
            // the app's shortcuts work, and moves to the text as it lands
            // (`focus_quick_source_text`); the inspector holds no selection
            // and its controls are their own stops, so it is no Tab stop.
            .when(self.is_quick_source(), |element| element.relative().role(Role::Group).aria_label("Tracked source information").track_focus(&self.source_focus))
            .child(crate::native_accessibility::clips_children(div().id("revision-metadata").min_h_0().overflow_y_scroll().flex_shrink_0()
                .when(!self.is_quick_source(), |element| element.max_h(px(260.)))
                .p_3().flex().flex_col().gap_2()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(native_accessibility::text("revision-identities", details).text_size(crate::appearance::ui_text(11.)))
                .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Targets stay pinned while you inspect files. Refresh reviews current refs. No checkout or download."))
                .child(button("change-revisions","Choose targets…","",false).on_click(cx.listener(|this,_,window,cx|this.open_revision_comparison(window,cx))))))
            .when(!self.is_quick_source(), |element| element.child(self.render_files(cx)))
            // Where a file with no text to take focus has left it here. Not
            // while the file is read, when the ring would flash and move on.
            .when(self.is_quick_source() && self.loading.is_none(), |element| {
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
        for input in [form.query.clone(), form.revision.clone()] {
            form.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                |this, _, event, window, cx| match event {
                    InputEvent::Change => this.search(window, cx),
                    InputEvent::PressEnter { .. } => this.activate(window, cx),
                    _ => {}
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
}
