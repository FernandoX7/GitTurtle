//! Native linear rebase review and interrupted-operation message editing.
use crate::*;
use gitturtle_core::{
    InteractiveRebaseCommand, InteractiveRebasePlan, InteractiveRebaseResume, RebaseAction,
    RebaseStep, WriteCommand,
};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use gpui_kit::prelude::FluentBuilder;

const PREPARING: &str = "Reading rebase sequence…";

gpui::actions!(interactive_rebase, [MoveCommitUp, MoveCommitDown]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-up", MoveCommitUp, Some("RebaseSequence")),
        KeyBinding::new("alt-down", MoveCommitDown, Some("RebaseSequence")),
    ]);
}

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    draft: Option<Entity<RebaseForm>>,
    message: Option<Entity<MessageForm>>,
}

fn label(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
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
            this.app_focus.focus(window, cx);
        }
    });
}

impl GitTurtle {
    pub(super) fn cancel_interactive_rebase_action(&mut self) {
        self.interactive_rebase.generation = self.interactive_rebase.generation.wrapping_add(1);
        self.interactive_rebase.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
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
        draft.update(cx, |draft, _| {
            draft.pending = false;
            draft.closed = false;
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let close = draft.clone();
            let cancel = draft.clone();
            let button_close = draft.clone();
            dialog
                .title(label("interactive-rebase-title", "Edit local commits"))
                .width(px(720.))
                .child(draft.clone())
                .footer(DialogFooter::new().child(
                    button("close-rebase-plan", "Close", "", false).on_click(
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
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                if this.path.as_ref() == Some(&path) && this.page == AppPage::Repository {
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
            |repo| repo.interactive_rebase_resume(),
            |result, this, window, cx| match result {
                Ok(expected) => {
                    let owner = cx.entity().downgrade();
                    let retained = this
                        .interactive_rebase
                        .message
                        .as_ref()
                        .filter(|form| {
                            let form = form.read(cx);
                            form.expected.root == expected.root
                                && form.expected.operation.same_operation(&expected.operation)
                                && form.expected.message == expected.message
                        })
                        .cloned();
                    let form = if let Some(form) = retained {
                        form.update(cx, |form, _| form.expected = expected);
                        form
                    } else {
                        cx.new(|cx| MessageForm::new(owner, expected, window, cx))
                    };
                    form.update(cx, |form, _| form.closed = false);
                    this.interactive_rebase.message = Some(form.clone());
                    window.open_alert_dialog(cx, move |dialog, _, _| {
                        let close = form.clone();
                        let cancel = form.clone();
                        let button_close = form.clone();
                        dialog
                            .title(label("rebase-message-title", "Continue rebase"))
                            .width(px(640.))
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
                Err(error) => this.operation_error = Some(format!("{error:#}")),
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
    acknowledged: bool,
    pending: bool,
    closed: bool,
    error: Option<String>,
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
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            acknowledged: false,
            pending: false,
            closed: false,
            error: None,
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

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
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
                                    form.focus.focus(window, cx);
                                }
                                Err(error) => form.error = Some(format!("{error:#}")),
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
            self.error = Some("The repository changed or another operation is busy. Reopen this review after it finishes.".into());
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
        if self.closed {
            return;
        }
        let Some(plan) = self.plan.clone() else {
            self.error = Some("Load and review the affected commits first.".into());
            cx.notify();
            return;
        };
        if let Err(error) = plan.validate_steps(&self.steps) {
            self.error = Some(error.to_string());
            cx.notify();
            return;
        }
        if self.base.read(cx).value().trim() != plan.base_revision {
            self.error =
                Some("The base field changed. Load commits again before reviewing.".into());
            cx.notify();
            return;
        }
        if !plan.known_published_refs.is_empty() && !self.acknowledged {
            self.error = Some("Acknowledge the remote-tracking warning before continuing.".into());
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
        div()
            .id(("rebase-commit", index))
            .role(Role::ListBoxOption)
            .aria_selected(self.selected == index)
            .aria_label(text.clone())
            .w_full()
            .min_w_0()
            .h(crate::appearance::ui_size(34.))
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
            }))
            .into_any_element()
    }
}

impl Render for RebaseForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let list = uniform_list(
            "rebase-sequence",
            self.steps.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range.map(|index| this.row(index, cx)).collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.scroll);
        div().id("rebase-plan-scroll")
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)))
            .overflow_y_scroll()
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .child(div().flex().flex_col().gap_3()
            .child(label("rebase-base-explanation", "Base stays unchanged. Review up to 100 commits after it on the current branch. Merge-preserving and root rewrites are unsupported.").text_size(crate::appearance::ui_text(12.)))
            .child(div().flex().gap_2().items_center().child(div().flex_1().child(Input::new(&self.base).aria_label("Exclusive base revision")))
                .child(button("load-rebase-commits", if self.pending { "Loading…" } else { "Load commits" }, "", false).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.load(window, cx)))))
            .when_some(self.plan.as_ref(), |element, plan| element
                .child(label("rebase-resolved-base", format!("{} · {} commits · base {} · branch tip {}", plan.branch, plan.commits.len(), &plan.base[..10], &plan.head[..10])).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .child(div().flex().flex_wrap().gap_1().children([RebaseAction::Pick, RebaseAction::Reword, RebaseAction::Squash, RebaseAction::Fixup, RebaseAction::Drop].into_iter().map(|action| {
                    let active = self.steps.get(self.selected).is_some_and(|step| step.action == action);
                    button(action.label(), action.label(), "", active).toggled(active).disabled(self.pending).on_click(cx.listener(move |this, _, _, cx| this.choose(action, cx)))
                })).child(button("rebase-move-up", "Move up", "", false).disabled(self.pending || self.selected == 0).on_click(cx.listener(|this, _, _, cx| this.move_selected(true, cx))))
                    .child(button("rebase-move-down", "Move down", "", false).disabled(self.pending || self.selected + 1 >= self.steps.len()).on_click(cx.listener(|this, _, _, cx| this.move_selected(false, cx)))))
                .child(div().id("rebase-list-focus").role(Role::ListBox).aria_label("Planned commits, oldest first").tab_stop(true).key_context("RebaseSequence").track_focus(&self.focus).h(px(220.)).border_1().border_color(rgb(p.border)).rounded(px(5.)).overflow_hidden()
                    .on_action(cx.listener(|this, _: &MoveCommitUp, _, cx| this.move_selected(true, cx)))
                    .on_action(cx.listener(|this, _: &MoveCommitDown, _, cx| this.move_selected(false, cx)))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let key = event.keystroke.key.as_str();
                        if matches!(key, "up" | "down" | "home" | "end") && !this.steps.is_empty() {
                            this.selected = match key { "up" => this.selected.saturating_sub(1), "down" => (this.selected + 1).min(this.steps.len() - 1), "home" => 0, _ => this.steps.len() - 1 };
                            this.scroll.scroll_to_item(this.selected, ScrollStrategy::Center); cx.stop_propagation(); cx.notify();
                        } else if let Some(action) = match key { "p" => Some(RebaseAction::Pick), "r" => Some(RebaseAction::Reword), "s" => Some(RebaseAction::Squash), "f" => Some(RebaseAction::Fixup), "d" => Some(RebaseAction::Drop), _ => None } { this.choose(action, cx); cx.stop_propagation(); }
                    })).child(list))
                .child(label("rebase-keyboard-help", "↑/↓ select · Option-↑/↓ reorder · P/R/S/F/D choose an action. Squash combines messages; Fixup keeps the preceding message. Drop removes the commit's changes.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
                .when(!plan.known_published_refs.is_empty(), |element| element
                    .child(label("rebase-published-warning", format!("History rewrite warning: these commits are known to {}. Other people may depend on their IDs.", plan.known_published_refs.join(", "))).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning)))
                    .child(button("acknowledge-rebase-published", if self.acknowledged { "Acknowledged: rewrite known shared commits" } else { "I understand these commits may be shared" }, "", self.acknowledged).toggled(self.acknowledged).on_click(cx.listener(|this, _, _, cx| { this.acknowledged = !this.acknowledged; cx.notify(); }))))
                .child(button("review-interactive-rebase", "Review rebase…", "", true).disabled(self.pending || issue.is_some() || changed_base || (!plan.known_published_refs.is_empty() && !self.acknowledged)).on_click(cx.listener(|this, _, window, cx| this.review(window, cx)))))
            .when(changed_base, |element| element.child(label("rebase-base-changed", "Base edited: load commits again to update the sequence.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .children(issue.map(|issue| label("rebase-invalid-plan", issue).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .children(self.error.as_ref().map(|error| label("rebase-form-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning)))))
    }
}

struct MessageForm {
    owner: WeakEntity<GitTurtle>,
    expected: InteractiveRebaseResume,
    editor: Option<Entity<TextareaState>>,
    error: Option<String>,
    closed: bool,
}
impl MessageForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        expected: InteractiveRebaseResume,
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
        Self {
            owner,
            expected,
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

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        let message = self
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).value().to_string());
        if message.as_ref().is_some_and(|message| {
            message.trim().is_empty() || message.len() > 1024 * 1024 || message.contains('\0')
        }) {
            self.error =
                Some("Enter a nonempty message of at most 1 MiB without NUL bytes.".into());
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
            "Continue rebasing {} with {} staged paths.\n\n{}\n\n{}\n\nGit applies its normal message cleanup, hooks and signing. Later message edits and conflicts pause again. Closing this review keeps the operation paused.",
            expected.operation.branch,
            expected.operation.staged_paths.len(),
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
        div().id("rebase-message-scroll")
            .max_h((window.viewport_size().height - px(240.)).max(px(160.)))
            .overflow_y_scroll()
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| { this.close(window, cx); cx.stop_propagation(); }))
            .child(div().flex().flex_col().gap_3()
            .child(label("rebase-resume-state", format!("{} · Base: {} · {} staged paths", self.expected.operation.branch, self.expected.operation.target_label, self.expected.operation.staged_paths.len())).text_size(crate::appearance::ui_text(12.)))
            .when_some(self.expected.operation.commit.as_ref(), |element, commit| element.child(label("rebase-resume-commit", format!("Replaying original commit {commit}")).text_size(crate::appearance::ui_text(12.))))
            .child(label("rebase-message-explanation", "Review Git's pending commit message. Git uses its configured cleanup rules for comments and whitespace, hooks, author identity, and signing. Your edited draft is retained when this dialog closes.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
            .when_some(self.editor.as_ref(), |element, editor| element.child(Textarea::new(editor).h(px(240.)).aria_label("Rebase commit message")))
            .when(self.editor.is_none(), |element| element.child(label("rebase-no-message", "No message is pending. Continue will replay the next planned step and pause if a message or conflict needs attention.").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| label("rebase-message-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
            .child(button("review-rebase-continue", "Review Continue…", "", true).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))))
    }
}
