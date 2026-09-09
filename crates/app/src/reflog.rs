//! A local reflog is an expiring Git record, separate from app activity.
use crate::*;
use gitturtle_core::{ReflogEntry, ReflogPage, ReflogRecoveryPlan, WriteCommand};
use gpui_kit::{
    component::{WindowExt, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

fn label(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

impl GitTurtle {
    pub(super) fn open_reflog_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let browser = cx.new(|cx| ReflogBrowser::new(owner, repo, window, cx));
        browser.update(cx, |this, cx| this.refresh(window, cx));
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let done = browser.clone();
            let cancel = browser.clone();
            let closing = browser.downgrade();
            let closed = browser.read(cx).closed.clone();
            dialog
                .title(label("reflog-title", "Local reflog and recovery"))
                .width(px(740.))
                .child(browser.clone())
                .button_props(DialogButtonProps::default().ok_text("Done"))
                .on_ok(move |_, _, cx| {
                    done.update(cx, |this, _| this.cancel());
                    true
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
                .on_close(move |_, _, cx| {
                    closed.store(true, std::sync::atomic::Ordering::Release);
                    let closing = closing.clone();
                    cx.defer(move |cx| {
                        let _ = closing.update(cx, |this, _| this.cancel());
                    });
                })
        });
    }
}

struct ReflogBrowser {
    closed: Arc<std::sync::atomic::AtomicBool>,
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    reader: operations::SerialExecutor,
    task: Option<Task<()>>,
    cancellation: gitturtle_core::HistoryCancellation,
    scope: Entity<InputState>,
    query: Entity<InputState>,
    name: Entity<InputState>,
    page: Option<ReflogPage>,
    selected: Option<ReflogEntry>,
    commit: Option<Commit>,
    changes: Vec<FileChange>,
    message: Option<Entity<EditorState>>,
    pending: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl Drop for ReflogBrowser {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl ReflogBrowser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("HEAD")
                .placeholder("HEAD or local branch name")
        });
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter log by message, object ID, or selector")
        });
        let scope_subscription =
            cx.subscribe_in(&scope, window, |this: &mut Self, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.refresh(window, cx);
                }
                cx.notify();
            });
        let query_subscription =
            cx.subscribe_in(&query, window, |this: &mut Self, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && let Some(entry) = this.matches(cx).first()
                {
                    this.select(entry.clone(), window, cx);
                }
                cx.notify();
            });
        Self {
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            owner,
            repo,
            reader: operations::SerialExecutor::new("reflog-browser"),
            task: None,
            cancellation: Default::default(),
            scope,
            query,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("recovery/my-commit")),
            page: None,
            selected: None,
            commit: None,
            changes: Vec::new(),
            message: None,
            pending: false,
            error: None,
            _subscriptions: vec![scope_subscription, query_subscription],
        }
    }
    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.task = None;
        self.pending = false;
    }
    fn current(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            let owner = owner.read(cx);
            owner.path.as_deref() == Some(self.repo.path()) && owner.operation_busy.is_none()
        })
    }
    fn read<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(GitRepository) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel();
        self.pending = true;
        self.error = None;
        let repo = self.repo.clone();
        self.cancellation = Default::default();
        let cancellation = self.cancellation.clone();
        let response = self.reader.submit_read(move || {
            gitturtle_core::run_cancellable_inspection(cancellation, || operation(repo))
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Reflog read interrupted. Refresh to try again.")));
            let _ = this.update_in(cx, |this, window, cx| { if this.closed.load(std::sync::atomic::Ordering::Acquire) { return; } this.pending = false; if !this.current(cx) { this.error = Some("The repository changed or an operation started. Reopen the reflog when it finishes.".into()); } else { match result { Ok(value) => receive(this, value, window, cx), Err(error) => this.error = Some(format!("{error:#}")) } } cx.notify(); });
        }));
        cx.notify();
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scope = self.scope.read(cx).value().trim().to_owned();
        let reference = if scope == "HEAD" || scope.starts_with("refs/heads/") {
            scope
        } else {
            format!("refs/heads/{scope}")
        };
        self.selected = None;
        self.commit = None;
        self.message = None;
        self.changes.clear();
        self.page = None;
        self.read(
            move |repo| repo.reflog(&reference),
            |this, page, _, _| this.page = Some(page),
            window,
            cx,
        );
    }
    fn matches(&self, cx: &App) -> Vec<ReflogEntry> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        self.page
            .as_ref()
            .map(|page| {
                page.entries
                    .iter()
                    .filter(|entry| {
                        entry.oid.contains(&query)
                            || entry.selector.to_lowercase().contains(&query)
                            || entry.message.to_lowercase().contains(&query)
                    })
                    .take(101)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    fn select(&mut self, entry: ReflogEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(entry.clone());
        self.commit = None;
        self.message = None;
        self.changes.clear();
        self.name.update(cx, |input, cx| {
            if input.value().is_empty() {
                input.set_value(format!("recovery/{}", short_oid(&entry.oid)), window, cx);
            }
        });
        self.read(
            move |repo| {
                let commit = repo.inspect_reflog_commit(&entry)?;
                let changes = repo.changes(&entry.oid, 0)?;
                Ok((commit, changes))
            },
            |this, (commit, changes), window, cx| {
                let text = format!("{}\n\n{}", commit.subject, commit.body);
                this.message = Some(text::editor(&text, "text", None, window, cx));
                this.commit = Some(commit);
                this.changes = changes;
            },
            window,
            cx,
        );
    }
    fn recover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.commit.is_none() || !self.current(cx) {
            return;
        }
        let Some(entry) = self.selected.clone() else {
            return;
        };
        let name = self.name.read(cx).value().trim().to_owned();
        self.read(move |repo| repo.reflog_recovery_plan(&entry, &name), |this, plan: ReflogRecoveryPlan, window, cx| {
            let _ = this.owner.update(cx, |owner, cx| {
                if owner.path.as_deref() != Some(this.repo.path()) || owner.operation_busy.is_some() { return; }
                window.close_dialog(cx);
                owner.confirm_git_write("Create recovery branch".into(), format!("Repository: {}\nNew branch: {}\nCommit: {}\n{}\n\nSelected from {} at {}.\n\nCreate a new local branch pointing to this available commit. Your current branch, index, working files, and active Git operation stay as they are. Nothing is checked out, reset, fetched, or pushed.\n\nThe reflog and commit are revalidated before writing. If the log expires or changes, refresh and review again.", this.repo.path().display(), plan.branch, plan.entry.oid, plan.commit.subject, plan.entry.selector, full_date(plan.entry.timestamp)), "Create recovery branch", WriteCommand::RecoverReflog(Arc::new(plan)), window, cx);
            });
        }, window, cx);
    }
}

impl Render for ReflogBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let matches = self.matches(cx);
        let unavailable = self.pending || !self.current(cx);
        div().id("reflog-browser-content").flex().flex_col().gap_3().max_h(px(590.)).overflow_y_scroll()
            .child(label("reflog-explanation", "Git records local reference movements here, including actions by other tools. HEAD belongs to this worktree; branch logs are shared. Entries expire, and unreachable objects may be pruned. This is not a permanent backup or a complete activity history.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
            .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.scope).aria_label("Reflog scope: HEAD or local branch name"))).child(button("refresh-reflog", "Read log", "refresh-cw", false).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx)))))
            .when_some(self.page.as_ref(), |element, page| element.child(label("reflog-scope-summary", format!("{} · {} retained entries · newest first", page.reference, page.entries.len())).text_size(crate::appearance::ui_text(12.))).when(page.truncated, |element| element.child(label("reflog-limit", "Showing the newest 1,000 records within a 4 MiB tail. Older records are outside this bounded view.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning)))))
            .child(Input::new(&self.query).aria_label("Filter loaded reflog entries").cleanable(true))
            .when(self.pending, |element| element.child(div().flex().gap_2().child(label("reflog-loading", "Reading local reflog or selected commit…").text_size(crate::appearance::ui_text(12.))).child(button("cancel-reflog-read", "Cancel", "", false).on_click(cx.listener(|this, _, _, cx| { this.cancel(); cx.notify(); })))))
            .children(self.error.as_ref().map(|error| label("reflog-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .child(div().id("reflog-list").max_h(px(230.)).overflow_y_scroll().flex().flex_col().gap_1().children(matches.iter().take(100).enumerate().map(|(index, entry)| {
                let entry = entry.clone(); let text = format!("{} · {} · {} · {}", entry.selector, short_oid(&entry.oid), full_date(entry.timestamp), if entry.message.is_empty() { "Reference updated" } else { &entry.message });
                Button::new(("reflog-entry", index)).ghost().w_full().h_auto().min_h(crate::appearance::ui_size(36.)).label(text.clone()).accessibility_label(text).toggled(self.selected.as_ref() == Some(&entry)).on_click(cx.listener(move |this, _, window, cx| this.select(entry.clone(), window, cx)))
            })).when(matches.is_empty() && self.page.is_some() && !self.pending, |element| element.child(label("reflog-empty", if self.page.as_ref().is_some_and(|page| page.entries.is_empty()) { "No local reflog records for this scope. Reflog recording may be disabled, the branch may not exist, or older entries may have expired." } else { "No loaded entries match this filter." }).text_size(crate::appearance::ui_text(12.)))))
            .when(matches.len() > 100, |element| element.child(label("reflog-match-limit", "Showing 100 matches. Narrow the filter to find another retained entry.").text_size(crate::appearance::ui_text(12.))))
            .when_some(self.selected.as_ref(), |element, entry| {
                let oid = entry.oid.clone();
                element.child(label("reflog-selected-object", format!("{}\nCommit: {}\nPrevious: {}", entry.selector, entry.oid, entry.previous_oid)).text_size(crate::appearance::ui_text(12.)))
                    .child(button("copy-reflog-oid", "Copy commit ID", "", false).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(oid.clone()))))
            })
            .when_some(self.commit.as_ref(), |element, commit| element
                .child(label("reflog-commit-metadata", format!("{} · {}\n{} parent(s) · {} changed files against {}", commit.author, full_date(commit.timestamp), commit.parents.len(), self.changes.len(), if commit.parents.len() > 1 { "first parent" } else { "parent or empty tree" })).text_size(crate::appearance::ui_text(12.)))
                .when_some(self.message.as_ref(), |element, message| element.child(crate::editor_find::Editor::new(message).readonly(true).h(px(110.)).aria_label("Reflog commit message")))
                .child(div().id("reflog-commit-files").max_h(px(110.)).overflow_y_scroll().flex().flex_col().children(self.changes.iter().take(100).enumerate().map(|(index, file)| { let text = format!("{} · {}", file.status.label(), file.path().display()); div().id(("reflog-commit-file", index)).role(Role::Label).aria_label(text.clone()).text_size(crate::appearance::ui_text(12.)).child(text) })))
                .when(self.changes.len() > 100, |element| element.child(label("reflog-file-limit", "Showing the first 100 changed paths.").text_size(crate::appearance::ui_text(12.))))
                .child(label("reflog-recovery-name-label", "Create a new branch at this commit to keep it reachable").text_size(crate::appearance::ui_text(12.)))
                .child(Input::new(&self.name).aria_label("New recovery branch name"))
                .child(button("review-reflog-recovery", "Review recovery branch…", "", false).disabled(unavailable).on_click(cx.listener(|this, _, window, cx| this.recover(window, cx)))))
    }
}
