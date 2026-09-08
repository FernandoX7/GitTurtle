use crate::*;
use gitturtle_core::{IntegrationCommand, OperationKind, OperationState, WriteCommand};
use gpui_kit::component::{WindowExt, dialog::DialogButtonProps};

impl GitTurtle {
    pub(super) fn show_operation_error(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(error) = self.operation_error.as_ref() else {
            return;
        };
        let full: Arc<str> = Arc::from(error.as_str());
        let display = if error.len() > 64 * 1024 {
            format!(
                "{}\n\n[Output shortened for display. Copy details retains the complete report.]\n\n{}",
                &error[..error.floor_char_boundary(56 * 1024)],
                &error[error.ceil_char_boundary(error.len() - 4 * 1024)..]
            )
        } else {
            error.clone()
        };
        let editor = text::editor(&display, "text", None, window, cx);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let full = Arc::clone(&full);
            dialog
                .title("Git operation details")
                .width(px(680.))
                .child(
                    crate::editor_find::Editor::new(&editor)
                        .readonly(true)
                        .h(px(320.))
                        .aria_label("Git operation error details"),
                )
                .child(
                    button("copy-operation-details", "Copy details", "", false).on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(full.to_string()))
                        },
                    ),
                )
                .button_props(DialogButtonProps::default().ok_text("Close"))
        });
    }

    /// The dialog captures both the chosen repository and the prepared command.
    /// Changing repository before confirmation cannot redirect that command.
    pub(super) fn confirm_git_write(
        &mut self,
        title: String,
        explanation: String,
        action: &'static str,
        command: WriteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.path.clone();
        let view = cx.entity().downgrade();
        let command = Arc::new(command);
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let path = path.clone();
            let view = view.clone();
            let command = Arc::clone(&command);
            dialog
                .title(title.clone())
                .width(px(520.))
                .child(
                    div()
                        .id("operation-consequences")
                        .max_h(px(360.))
                        .overflow_y_scroll()
                        .text_size(px(13.))
                        .child(explanation.clone()),
                )
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(action)
                        .cancel_text("Cancel")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        if this.path == path {
                            this.write(command.as_ref().clone(), action, window, cx);
                        }
                    });
                    true
                })
        });
    }

    pub(super) fn prepare_integration(
        &mut self,
        target: String,
        rebase: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || target.trim().is_empty() {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let path = repo.path().to_owned();
        self.operation_busy = Some("Comparing branch tips…");
        self.operation_error = None;
        let response = self
            .operations
            .submit_read(move || repo.integration_plan(&target));
        self.integration_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Branch comparison was interrupted. Choose the integration target again.")));
            let _ = this.update_in(cx, |this, window, cx| {
                this.operation_busy = None;
                if this.path.as_ref() != Some(&path) { return; }
                match result {
                    Ok(plan) => {
                        let title = if rebase { format!("Rebase {} onto {}", plan.branch, plan.target_label) } else { format!("Merge {} into {}", plan.target_label, plan.branch) };
                        let explanation = format!("{} has {} unique commits; {} has {}.\n\n{}\n\n{} paths differ between these branch tips.{}",
                            plan.branch, plan.ahead, plan.target_label, plan.behind,
                            if rebase { "Replay this branch's commits on the target. Replayed commits receive new IDs; use this for local work that others do not depend on. A clean working tree is required." }
                            else { "Bring the target's commits into this branch. Git fast-forwards when possible or creates a merge commit. Conflicts pause the merge for resolution." },
                            plan.affected_paths.len(),
                            if plan.affected_paths.is_empty() { String::new() } else { format!("\n{}{}", plan.affected_paths.iter().take(8).map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n"), if plan.affected_paths.len() > 8 { "\n…" } else { "" }) });
                        this.confirm_git_write(title, explanation, if rebase { "Rebase branch" } else { "Merge branches" }, WriteCommand::Integration(if rebase { IntegrationCommand::Rebase { plan } } else { IntegrationCommand::Merge { plan } }), window, cx);
                    }
                    Err(error) => this.operation_error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn confirm_continue(
        &mut self,
        operation: OperationState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut staged = operation
            .staged_paths
            .iter()
            .take(20)
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        if operation.staged_paths.len() > 20 {
            staged.push_str(&format!(
                "\n… and {} more. Review the full staged list in Changes.",
                operation.staged_paths.len() - 20
            ));
        }
        self.confirm_git_write(format!("Continue {} on {}", operation.kind.label().to_lowercase(), operation.branch),
            format!("Continue with {} staged {}. Git uses its configured hooks and signing.\n\n{}\n\nIf another tool changes the staged contents, refresh and review them again.", operation.staged_paths.len(), if operation.staged_paths.len() == 1 { "file" } else { "files" }, staged),
            "Continue operation", WriteCommand::Integration(IntegrationCommand::Continue { expected: operation }), window, cx);
    }

    fn confirm_operation_end(
        &mut self,
        operation: OperationState,
        keep_files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = if keep_files {
            "Stop and keep files"
        } else {
            "Abort operation"
        };
        let explanation = if keep_files {
            format!(
                "Stop the {} on {} and retain the current HEAD, index and working files. Unresolved index entries remain unresolved, and HEAD may remain detached. You can inspect the files and deliberately choose your next branch.",
                operation.kind.label().to_lowercase(),
                operation.branch
            )
        } else {
            format!(
                "Abort the {} on {} and return to its starting state. Resolution edits for this operation will be discarded. If Git cannot safely preserve other edits, the operation will remain paused with a recovery explanation.",
                operation.kind.label().to_lowercase(),
                operation.branch
            )
        };
        self.confirm_git_write(
            format!(
                "{} {}",
                if keep_files { "Stop" } else { "Abort" },
                operation.kind.label().to_lowercase()
            ),
            explanation,
            action,
            WriteCommand::Integration(if keep_files {
                IntegrationCommand::Quit {
                    expected: operation,
                }
            } else {
                IntegrationCommand::Abort {
                    expected: operation,
                }
            }),
            window,
            cx,
        );
    }

    pub(super) fn render_operation_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(operation) = self.integration_state.as_ref() else {
            let diverged = self
                .work_status
                .as_ref()
                .filter(|status| status.ahead > 0 && status.behind > 0);
            return div().children(diverged.map(|status| {
                let upstream = status.upstream.clone().unwrap_or_default();
                let merge = "@{upstream}".to_owned();
                div().px_3().py_2().flex().items_center().gap_2().bg(rgb(p.subtle)).border_b_1().border_color(rgb(p.border))
                    .child(div().flex_1().text_size(px(11.)).child(format!("Branches diverged · {} local and {} upstream commits. Choose how to integrate {}.", status.ahead, status.behind, upstream)))
                    .child(button("merge-upstream", "Merge…", "", false).disabled(self.operation_busy.is_some()).on_click(cx.listener(move |this, _, window, cx| this.prepare_integration(merge.clone(), false, window, cx))))
                    .child(button("rebase-upstream", "Rebase…", "", false).disabled(self.operation_busy.is_some()).on_click(cx.listener(move |this, _, window, cx| this.prepare_integration("@{upstream}".into(), true, window, cx))))
            })).into_any_element();
        };
        let unresolved = self.work_status.as_ref().map_or(0, |status| {
            status
                .entries
                .iter()
                .filter(|entry| entry.conflicted)
                .count()
        });
        let busy = self.operation_busy.is_some();
        let continue_state = operation.clone();
        let abort_state = operation.clone();
        let stop_state = operation.clone();
        let continuation = if operation.kind == OperationKind::Merge {
            "Complete merge"
        } else {
            "Continue"
        };
        div()
            .px_3()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(rgb(p.subtle))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(icon("file-conflict", 16., p.warning))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!(
                                "{} in progress · {}",
                                operation.kind.label(),
                                operation.branch
                            )),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(rgb(p.muted))
                            .child(format!(
                                "{} · {} unresolved {}",
                                operation.target_label,
                                unresolved,
                                if unresolved == 1 { "file" } else { "files" }
                            )),
                    ),
            )
            .child(
                button("review-conflicts", "Review files", "", false)
                    .on_click(cx.listener(|this, _, window, cx| this.show_working(window, cx))),
            )
            .child(
                button("continue-operation", continuation, "", false)
                    .disabled(busy || unresolved > 0)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_continue(continue_state.clone(), window, cx)
                    })),
            )
            .child(
                button("abort-operation", "Abort…", "", false)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_operation_end(abort_state.clone(), false, window, cx)
                    })),
            )
            .child(
                button("stop-operation", "Keep files…", "", false)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_operation_end(stop_state.clone(), true, window, cx)
                    })),
            )
            .into_any_element()
    }
}
