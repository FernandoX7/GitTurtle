//! Revision and path inspections reuse the workspace preview and retained views.
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
}
enum Target {
    Comparison(RevisionComparison),
    File {
        scope: PathScope,
        entry: TrackedPath,
    },
}
impl State {
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
    pub(super) fn open_revision_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self
            .repository
            .clone()
            .filter(|_| self.operation_busy.is_none())
        else {
            return;
        };
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
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let confirm = form.clone();
            let cancel_action = form.clone();
            dialog
                .title("Compare local revisions")
                .width(px(660.))
                .child(form.clone())
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
    }

    pub(super) fn open_quick_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self
            .repository
            .clone()
            .filter(|_| self.operation_busy.is_none())
        else {
            return;
        };
        let revision = self
            .selected_commit
            .and_then(|i| self.commits.get(i))
            .map_or("HEAD", |c| &c.oid)
            .to_owned();
        let owner = cx.entity().downgrade();
        let return_focus = window.focused(cx);
        let form = cx.new(|cx| QuickForm::new(owner, repo, revision, return_focus, window, cx));
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let activate = form.clone();
            let cancel_action = form.clone();
            dialog
                .title("Quick Open File")
                .width(px(700.))
                .child(form.clone())
                .footer(
                    DialogFooter::new()
                        .child(button("cancel-quick-file", "Cancel", "", false).on_click(
                            move |_, window, cx| {
                                cancel.update(cx, |form, cx| form.cancel(window, cx))
                            },
                        ))
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
        window.refresh();
        window.on_next_frame(move |window, cx| {
            let _ = focus_form.update(cx, |form, cx| {
                if !form.closed.load(Ordering::Acquire) {
                    form.query.read(cx).focus_handle(cx).focus(window, cx);
                }
            });
        });
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
        self.invalidate_read();
        let context = self.take_inspection_context(window, cx);
        let lineage = std::mem::take(&mut self.file_history);
        let previous = std::mem::take(&mut self.revision_inspection);
        self.revision_inspection.active = Some(Box::new(Inspection {
            context,
            lineage,
            previous,
            target,
        }));
        self.file_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.blame.hide();
        self.clear_preview();
        self.mode = WorkspaceMode::Compare;
        self.page = AppPage::Repository;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        true
    }

    fn show_revision_comparison(
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
        self.refresh_file_filter(cx);
        self.selected_file = None;
        self.status = format!(
            "{} changed files · pinned local revisions · open a file to compare",
            self.files.len()
        );
        cx.notify();
    }

    fn show_tracked_file(
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
        self.refresh_file_filter(cx);
        self.selected_file = Some(0);
        self.text_mode = TextMode::After;
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
                    self.select_working(index, area, window, cx);
                }
            } else if !self.reload_tracked_inspection(window, cx)
                && let Some(index) = self.selected_file
            {
                self.load_file(index, window, cx);
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

    pub(super) fn reload_tracked_inspection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
                    this.file_focus.focus(window, cx);
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
            // Quick source omits the changed-file list. Keep its destination
            // focus attached to this visible inspector so app shortcuts work.
            .when(self.is_quick_source(), |element| element.role(Role::Group).aria_label("Tracked source information").track_focus(&self.file_focus).tab_stop(true))
            .child(div().id("revision-metadata").min_h_0().overflow_y_scroll().flex_shrink_0()
                .when(!self.is_quick_source(), |element| element.max_h(px(260.)))
                .p_3().flex().flex_col().gap_2()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().id("revision-identities").role(Role::Label).aria_label(details.clone()).text_size(crate::appearance::ui_text(11.)).child(details))
                .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Targets stay pinned while you inspect files. Refresh reviews current refs. No checkout or download."))
                .child(button("change-revisions","Choose targets…","",false).on_click(cx.listener(|this,_,window,cx|this.open_revision_comparison(window,cx)))))
            .when(!self.is_quick_source(), |element| element.child(self.render_files(cx))).into_any_element()
    }
}

struct CompareForm {
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
}
impl CompareForm {
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
        };
        for (side, input) in [form.before.clone(), form.after.clone()]
            .into_iter()
            .enumerate()
        {
            form.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, _, event, window, cx| match event {
                    InputEvent::Change | InputEvent::Focus => {
                        this.selected_side = side;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    _ => {}
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
        div().id("comparison-form").track_focus(&self.focus)
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)))
            .overflow_y_scroll().text_size(crate::appearance::ui_text(12.))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .child(div().flex().flex_col().gap_3()
            .child(div().child("Before").child(Input::new(&self.before).aria_label("Before: local branch, tag, or commit revision").disabled(self.busy)))
            .child(div().child("After").child(Input::new(&self.after).aria_label("After: local branch, tag, or commit revision").disabled(self.busy)))
            .child(div().flex().flex_wrap().gap_1().children([(ComparisonMode::Endpoints,"Endpoints · Before → After"),(ComparisonMode::SinceBranching,"Changes since branching")].into_iter().map(|(mode,label)|button(label,label,"",self.mode==mode).toggled(self.mode==mode).disabled(self.busy).on_click(cx.listener(move |this,_,_,cx|{this.mode=mode;cx.notify();}))))
                .child(button("swap-revisions","Swap","",false).disabled(self.busy).on_click(cx.listener(|this,_,window,cx|{let before=this.before.read(cx).value();let after=this.after.read(cx).value();this.before.update(cx,|input,cx|input.set_value(after,window,cx));this.after.update(cx,|input,cx|input.set_value(before,window,cx));}))))
            .child(div().text_color(rgb(p.muted)).child("Type a local revision, or choose a matching branch/tag for the most recently edited field. Names resolve when you press Compare. A merge-base comparison shows the changes leading to After."))
            .children(choices.into_iter().enumerate().map(|(i,name)|button(("revision-choice",i),name.clone(),"",false).disabled(self.busy).on_click(cx.listener(move |this,_,window,cx|{let input=if this.selected_side==0{&this.before}else{&this.after};input.update(cx,|input,cx|input.set_value(name.clone(),window,cx));}))))
            .when(self.busy,|el|el.child(div().child("Resolving local revisions and changed files… Cancel closes this read.")))
            .children(self.error.as_ref().map(|error|div().text_color(rgb(p.removed)).child(error.clone()))))
    }
}

struct QuickForm {
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
    subscriptions: Vec<Subscription>,
    closed: Arc<AtomicBool>,
    accepted: Arc<AtomicBool>,
    return_focus: Option<FocusHandle>,
}
impl QuickForm {
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
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(120));
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
        let list = uniform_list(
            "quick-file-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let entry = &this.page.as_ref().unwrap().entries[index];
                        let path = entry.path.display().to_string();
                        div()
                            .id(("quick-file", index))
                            .role(Role::ListBoxOption)
                            .aria_label(path.clone())
                            .aria_selected(index == this.selected)
                            .w_full()
                            .h(crate::appearance::ui_size(34.))
                            .px_2()
                            .flex()
                            .items_center()
                            .bg(rgb(if index == this.selected {
                                palette(cx).selected
                            } else {
                                palette(cx).panel
                            }))
                            .child(div().truncate().child(path))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.selected = index;
                                this.activate(window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div().id("quick-file-form-scroll")
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)))
            .overflow_y_scroll().text_size(crate::appearance::ui_text(12.))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.cancel(window, cx); cx.stop_propagation(); }))
            .on_key_down(cx.listener(|this,event:&KeyDownEvent,_,cx|{let count=this.page.as_ref().map_or(0,|page|page.entries.len());if count==0{return;}match event.keystroke.key.as_str(){"down"=>this.selected=(this.selected+1).min(count-1),"up"=>this.selected=this.selected.saturating_sub(1),_=>return}cx.stop_propagation();this.scroll.scroll_to_item(this.selected,ScrollStrategy::Center);cx.notify();}))
            .child(div().flex().flex_col().gap_2()
            .child(div().flex().gap_1().children([(true,"Current worktree"),(false,"Chosen revision")].into_iter().map(|(worktree,label)|button(label,label,"",self.worktree==worktree).toggled(self.worktree==worktree).on_click(cx.listener(move |this,_,window,cx|{this.worktree=worktree;this.search(window,cx);})))) )
            .when(!self.worktree,|el|el.child(Input::new(&self.revision)))
            .child(Input::new(&self.query))
            .child(div().text_color(rgb(p.muted)).child(if self.busy{"Searching local tracked paths…".into()}else{format!("{count} matches · ↑/↓ select · Return opens · Escape cancels{}",self.page.as_ref().filter(|page|page.truncated).map_or("",|_|" · Limit reached; narrow the query"))}))
            .children(self.error.as_ref().map(|error|div().text_color(rgb(p.removed)).child(error.clone())))
            .child(div().id("quick-open-results").role(Role::ListBox).aria_label("Matching tracked files").h(px(320.)).border_1().border_color(rgb(p.border)).overflow_hidden().when(count>0,|el|el.child(list)).when(count==0&&!self.busy,|el|el.child(div().p_3().child("No matching tracked files. Untracked files are available in Working Changes."))))
            .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("File History and Blame are available after opening. Worktree reads show raw current bytes; a chosen revision is pinned to its resolved commit.")))
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomicBool, Ordering, claim_modal_result};
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
}
