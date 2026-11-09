use crate::workspace::refusing;
use crate::*;
use gitturtle_core::{IntegrationCommand, OperationKind, OperationState, WriteCommand};
use gpui_kit::component::WindowExt;
use gpui_kit::prelude::FluentBuilder as _;

/// The busy state of the comparison Merge… and Rebase… ask for.
pub(crate) const COMPARING: &str = "Comparing branch tips…";

impl GitTurtle {
    pub(super) fn show_operation_error(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(error) = self.operation_error.as_ref() else {
            return;
        };
        show_error_details(error, window, cx);
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
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let path = path.clone();
            let view = view.clone();
            let command = Arc::clone(&command);
            // Every reviewed write's last step interrupts with its
            // consequences before it runs: an alert.
            dialog
                .kind(DialogKind::Alert)
                .title(title.clone())
                .width(appearance::dialog_width(px(520.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(
                    native_accessibility::static_text(
                        div()
                            .id("operation-consequences")
                            .debug_selector(|| "operation-consequences".to_string()),
                        explanation.clone(),
                    )
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .text_size(crate::appearance::ui_text(13.))
                    .child(explanation.clone()),
                )
                .footer(crate::tags::dialog_footer(action, Some("Cancel")))
                .on_ok(move |_, window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        if this.path == path {
                            // Most reviewed writes report in the window-wide
                            // notice; a tracking branch's creation confirms
                            // where it was asked (`confirmations::reviewed`).
                            let origin = workspace::confirmations::reviewed(&path, &command);
                            this.write_from(command.as_ref().clone(), action, origin, window, cx);
                        }
                    });
                    true
                })
        });
    }

    /// Drops the branch comparison Merge… or Rebase… asked for, as a tab
    /// switch, Settings and Projects do, and ends its busy state with it, as
    /// `cancel_branch_action` ends its own: the dropped comparison
    /// never reaches its end, which ended it, so "Comparing branch tips…"
    /// once stayed for good and every later write and working read, in
    /// every tab, waited for it. Its read still ends on the operation
    /// executor, where a read asked for meanwhile waits behind it.
    pub(super) fn cancel_integration_comparison(&mut self) {
        if self.integration_task.take().is_some() && self.operation_busy == Some(COMPARING) {
            self.operation_busy = None;
        }
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
        self.operation_busy = Some(COMPARING);
        self.operation_error = None;
        let response = self
            .operations
            .submit_read(move || repo.integration_plan(&target));
        self.integration_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Branch comparison was interrupted. Choose the integration target again.")));
            let _ = this.update_in(cx, |this, window, cx| {
                this.operation_busy = None;
                this.read_deferred_working(window, cx);
                // Its confirmation belongs to the repository page that asked:
                // Settings and Projects drop the comparison as they open
                // (`cancel_integration_comparison`), and an answer that
                // reaches another page all the same opens nothing there. It
                // once opened its alert over Settings.
                if this.path.as_ref() != Some(&path) || this.page != AppPage::Repository { cx.notify(); return; }
                match result {
                    Ok(plan) => {
                        let title = if rebase { format!("Rebase {} onto {}", plan.branch, plan.target_label) } else { format!("Merge {} into {}", plan.target_label, plan.branch) };
                        let explanation = format!("{} has {}; {} has {}.\n\n{}\n\n{} between these branch tips.{}",
                            plan.branch, crate::counted(plan.ahead, "unique commit", "unique commits"), plan.target_label, plan.behind,
                            if rebase { "Replay this branch's commits on the target. Replayed commits receive new IDs; use this for local work that others do not depend on. A clean working tree is required." }
                            else { "Bring the target's commits into this branch. Git fast-forwards when possible or creates a merge commit. Conflicts pause the merge for resolution." },
                            crate::counted(plan.affected_paths.len(), "path differs", "paths differ"),
                            if plan.affected_paths.is_empty() { String::new() } else { format!("\n{}{}", plan.affected_paths.iter().take(8).map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n"), if plan.affected_paths.len() > 8 { "\n…" } else { "" }) });
                        this.confirm_git_write(title, explanation, if rebase { "Rebase branch" } else { "Merge branches" }, WriteCommand::Integration(if rebase { IntegrationCommand::Rebase { plan } } else { IntegrationCommand::Merge { plan } }), window, cx);
                    }
                    Err(error) => this.operation_error = Some(crate::views::OperationFailure::of(&error)),
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
        // Refused at once while a write runs, before it looks so.
        if self.operation_busy.is_some() {
            return;
        }
        if operation.kind == OperationKind::Rebase {
            self.prepare_interactive_rebase_continue(window, cx);
            return;
        }
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
            format!("Continue with {}. Git uses its configured hooks and signing.\n\n{}\n\nIf another tool changes the staged contents, refresh and review them again.", crate::counted(operation.staged_paths.len(), "staged file", "staged files"), staged),
            "Continue operation", WriteCommand::Integration(IntegrationCommand::Continue { expected: operation }), window, cx);
    }

    fn confirm_operation_end(
        &mut self,
        operation: OperationState,
        keep_files: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Refused at once while a write runs, before it looks so.
        if self.operation_busy.is_some() {
            return;
        }
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

    /// The banner of an operation in progress, or of diverged branches,
    /// across the repository page `width` wide.
    pub(super) fn render_operation_state(
        &self,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        // A write refuses each action here at once, and they look it only
        // once the write looks busy (`workspace::BusyLook`).
        let busy = self.operation_busy.is_some();
        let looks_busy = self.operation_shown().is_some();
        // A marked tab's operation and sync state are those of the last
        // read, as its header's are: the banner offers nothing from before
        // (`header_status`).
        let operation = (self.integration_state.as_ref()).filter(|_| !self.shown_tab_unavailable());
        let Some(operation) = operation else {
            let diverged =
                (self.header_status()).filter(|status| status.ahead > 0 && status.behind > 0);
            return div().children(diverged.map(|status| {
                let upstream = status.upstream.clone().unwrap_or_default();
                let merge = "@{upstream}".to_owned();
                div().px_3().py_2().flex().items_center().gap_2().bg(rgb(p.subtle)).border_b_1().border_color(rgb(p.border))
                    .child(div().flex_1().text_size(crate::appearance::ui_text(11.)).child(format!("Branches diverged · {} local and {} upstream commits. Choose how to integrate {}.", status.ahead, status.behind, upstream)))
                    // `prepare_integration` refuses while a write runs;
                    // each looks unavailable for it once the write looks busy.
                    .child(refusing(button("merge-upstream", "Merge…", "", false).debug_selector(|| "merge-upstream".into()), || "merge-upstream".into(), busy, looks_busy).on_click(cx.listener(move |this, _, window, cx| this.prepare_integration(merge.clone(), false, window, cx))))
                    .child(refusing(button("rebase-upstream", "Rebase…", "", false).debug_selector(|| "rebase-upstream".into()), || "rebase-upstream".into(), busy, looks_busy).on_click(cx.listener(move |this, _, window, cx| this.prepare_integration("@{upstream}".into(), true, window, cx))))
            })).into_any_element();
        };
        let unresolved = self.header_status().map_or(0, |status| {
            status
                .entries
                .iter()
                .filter(|entry| entry.conflicted)
                .count()
        });
        let continue_state = operation.clone();
        let abort_state = operation.clone();
        let stop_state = operation.clone();
        let continuation = if operation.kind == OperationKind::Merge {
            "Complete merge"
        } else {
            "Continue"
        };
        let kind = operation.kind.label();
        let title = format!("{kind} in progress · {}", operation.branch);
        let detail = format!(
            "{} · {}",
            operation.target_label,
            crate::counted(unresolved, "unresolved file", "unresolved files")
        );
        let fit = BannerFit::new(width, kind, &title, &detail, continuation, window);
        // Labelled, or as its icon named by its label, with the label as its
        // tooltip (DESIGN.md, "One responsive order").
        let action = |id: &'static str, label: &'static str, symbol: &str| {
            let control = if fit.labelled {
                button(id, label, "", false)
            } else {
                let icon = button(id, "", symbol, false);
                crate::views::noted_tooltip(
                    crate::views::compact_control_name(icon, id, label),
                    id,
                    label.into(),
                )
            };
            control.debug_selector(move || id.into())
        };
        div()
            .debug_selector(|| "operation-banner".into())
            .px_3()
            .py_2()
            .flex()
            // Where even the actions' icons have no room beside the
            // title's least, they wrap below it whole, as one group
            // ([`BannerFit::wraps`]).
            .flex_wrap()
            .items_center()
            .gap_2()
            .bg(rgb(p.subtle))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(icon("file-conflict", 16., p.warning))
            .child(
                div()
                    .debug_selector(|| "operation-banner-title".into())
                    .flex_1()
                    .min_w(fit.floor)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::appearance::ui_text(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            .child(detail),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    // Wrapped, on a line of its own whose first icon stands
                    // on the title's edge, the ghost Button's frame its icon
                    // inset before it (DESIGN.md, "One left edge"): at the
                    // banner's padding it stood under the warning icon, an
                    // edge of its own.
                    .when(fit.wraps, |actions| actions.w_full().pl(fit.indent))
                    .child(
                        action("review-conflicts", "Review files", "changes").on_click(
                            cx.listener(|this, _, window, cx| this.show_working(window, cx)),
                        ),
                    )
                    .child(
                        refusing(
                            action("continue-operation", continuation, "check"),
                            || "continue-operation".into(),
                            busy || unresolved > 0,
                            looks_busy || unresolved > 0,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.confirm_continue(continue_state.clone(), window, cx)
                            },
                        )),
                    )
                    .child(
                        refusing(
                            action("abort-operation", "Abort…", "reset"),
                            || "abort-operation".into(),
                            busy,
                            looks_busy,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.confirm_operation_end(abort_state.clone(), false, window, cx)
                            },
                        )),
                    )
                    .child(
                        refusing(
                            // An exit mark: it ends the operation and
                            // leaves the working files as they are. Never
                            // the close mark, which the app keeps for cancel
                            // and clear, nor a page, which Review files and
                            // the Modified status draw.
                            action("stop-operation", "Keep files…", "leave"),
                            || "stop-operation".into(),
                            busy,
                            looks_busy,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.confirm_operation_end(stop_state.clone(), true, window, cx)
                            },
                        )),
                    ),
            )
            .into_any_element()
    }
}

thread_local! {
    /// What the operation banner's labelled line takes beside its padding,
    /// icon and gaps, by its title, detail and continuation
    /// ([`BannerFit::new`]).
    static LABELLED_BANNER: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    /// The banner title's least, by the operation's phrase.
    static BANNER_TITLE_FLOOR: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// How the operation banner lays out in the repository page's width, in the
/// one responsive order (DESIGN.md): its title, "Merge in progress · main",
/// and its detail, the target and the unresolved files, are what it says, so
/// its four actions keep their labels only where both lines are whole beside
/// them, and otherwise become their icons, each named by its label and with
/// it as its tooltip. The title then ends in an ellipsis where it must, down
/// to its least, the operation's phrase whole ("Merge in progress…"), and
/// below that the actions wrap beneath it as one group, each whole. Kept
/// labelled at any width, at 18 pt in a 461 × 490 half tile the actions ran
/// past the window's edge, Keep files… drawn as "Ke", and at 13 pt there
/// they left the title "Merge i…" and its detail "other · 1…".
struct BannerFit {
    labelled: bool,
    /// The title's least: the operation's phrase with the ellipsis.
    floor: Pixels,
    /// Whether the actions, as icons, have no room beside the title's
    /// least, and so stand on a line of their own below it.
    wraps: bool,
    /// How far that line's first frame stands past the banner's padding:
    /// past the warning icon and its gap, to the title's edge, less the
    /// frame's icon inset, so its icon stands on the title's edge.
    indent: Pixels,
}

impl BannerFit {
    /// The banner's fit in a page `width` wide, for an operation of `kind`
    /// whose banner reads `title` and `detail`, and whose continuation
    /// reads `continuation`. The text is measured once per font, size and
    /// text ([`crate::views::cached_text_width`]), so a frame at rest lays
    /// none of it out again.
    fn new(
        width: Pixels,
        kind: &str,
        title: &str,
        detail: &str,
        continuation: &str,
        window: &Window,
    ) -> Self {
        let device = |length: Pixels| crate::image_compare::on_device_pixel(length, window);
        let rem = window.rem_size();
        let (padding, gap) = (device(rem * 0.75), device(rem * 0.5));
        let text = |text: &str, size: f32, weight: FontWeight| {
            let shaped =
                crate::views::shaped_width(text, crate::appearance::ui_text(size), weight, window);
            crate::views::laid_out_text_width(shaped.ceil(), window)
        };
        let key = format!("{title}\n{detail}\n{continuation}");
        let line = crate::views::cached_text_width(&LABELLED_BANNER, &key, window, || {
            let regular = window.text_style().font_weight;
            let whole = text(title, 12., FontWeight::MEDIUM).max(text(detail, 11., regular));
            let actions: Pixels = ["Review files", continuation, "Abort…", "Keep files…"]
                .into_iter()
                .map(|label| {
                    let label = crate::views::line_widths(
                        &[(label, crate::appearance::ui_text(12.))],
                        window,
                    );
                    crate::placed_button_width(label, false, window)
                })
                .sum();
            whole + actions
        });
        let phrase = format!("{kind} in progress…");
        let floor = crate::views::cached_text_width(&BANNER_TITLE_FLOOR, &phrase, window, || {
            text(&phrase, 12., FontWeight::MEDIUM)
        });
        // Its padding, the warning's 16 px icon and the gaps between its
        // six parts.
        let chrome = padding * 2. + px(16.) + gap * 5.;
        // The four icons, the shared Button's 28 px squares on device
        // pixels, beside the title's least. A device pixel's slack counts a
        // fit that layout's rounding could break as none, so the actions
        // never wrap of themselves, at the banner's padding.
        let icons = device(crate::appearance::ui_size(28.)) * 4.;
        let slack = px(1. / window.scale_factor());
        let inset = device(crate::appearance::ui_size(crate::ICON_INSET));
        Self {
            labelled: chrome + line <= width + px(0.01),
            floor,
            wraps: chrome + device(floor) + icons + slack > width,
            indent: px(16.) + gap - inset,
        }
    }
}

/// Opens Git's whole report of a failed operation, `error`, in a dialog of
/// its own above whatever is open, with Copy details: the window's error's
/// Details…, and Stashes' beside a Review restore… that Git could not
/// prepare.
pub(crate) fn show_error_details(error: &str, window: &mut Window, cx: &mut App) {
    let full: Arc<str> = Arc::from(error);
    let display = if error.len() > 64 * 1024 {
        format!(
            "{}\n\n[Output shortened for display. Copy details retains the complete report.]\n\n{}",
            &error[..error.floor_char_boundary(56 * 1024)],
            &error[error.ceil_char_boundary(error.len() - 4 * 1024)..]
        )
    } else {
        error.to_owned()
    };
    let editor = text::editor(&display, "text", None, window, cx);
    // Keyed by the details, read once: the dialog shows them unchanged.
    let mark = copy_feedback::Mark::new("copy-operation-details", &*full);
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let full = Arc::clone(&full);
        let copy_mark = mark.clone();
        dialog
            .title("Git operation details")
            .width(appearance::dialog_width(px(680.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(
                crate::editor_find::Editor::new(&editor)
                    .readonly(true)
                    .h(px(320.))
                    .aria_label("Git operation error details"),
            )
            .child(
                copy_feedback::copy_button(mark.clone(), "Copy details", "", cx).on_click(
                    move |_, window, cx| {
                        copy_feedback::copy(
                            window,
                            cx,
                            Some(copy_mark.clone()),
                            "operation details",
                            full.to_string(),
                        )
                    },
                ),
            )
            .footer(crate::tags::dialog_footer("Close", None))
    });
}
