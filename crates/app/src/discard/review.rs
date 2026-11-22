//! Bounded destructive review with a persistent warning and explicit acceptance.
use super::*;
use gpui_kit::component::{
    dialog::{Cancel, Confirm},
    scroll::{Scrollbar, ScrollbarMode},
};
use gpui_kit::prelude::FluentBuilder;
use std::{cell::Cell, rc::Rc};

use crate::native_accessibility::text as label;

fn path_field(
    id: &'static str,
    heading: &str,
    path: &std::path::Path,
    colors: appearance::Palette,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .min_w_0()
        .child(
            div()
                .text_color(rgb(colors.muted))
                .child(heading.to_owned()),
        )
        .child(label(id, path.display().to_string()).whitespace_normal())
}

impl GitTurtle {
    pub(super) fn confirm_discard(
        &mut self,
        repository: PathBuf,
        plan: DiscardPlan,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let plan = Arc::new(plan);
        let scroll = ScrollHandle::new();
        // The details scroll by key, so Tab reaches them, and they draw the
        // window's keyboard ring on their own edge.
        let focus = cx.focus_handle().tab_stop(true);
        let ring = self.focus_ring.clone();
        let submitted = Rc::new(Cell::new(false));
        // Enter on the dialog container must never accept irreversible loss.
        // Keyboard and accessibility activation still work on the actual button.
        let requested = Rc::new(Cell::new(false));
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let colors = palette(cx);
            let action = if plan.entry.untracked { "Delete file" } else { "Discard changes" };
            let title = if plan.entry.untracked { "Delete untracked file?" } else { "Discard file changes?" };
            let key_scroll = scroll.clone();
            let click_focus = focus.clone();
            let copy_path = repository.join(&plan.entry.path).display().to_string();
            let keyboard_copy_path = copy_path.clone();
            // Keyed by the file the dialog would discard.
            let path_mark = copy_feedback::Mark::new("copy-discard-path", (&repository, &plan.entry.path));
            let (click_mark, keyboard_mark) = (path_mark.clone(), path_mark.clone());
            let request = requested.clone();
            let keyboard_request = requested.clone();
            let requested = requested.clone();
            let submitted = submitted.clone();
            let owner = owner.clone();
            let repository = repository.clone();
            let plan = Arc::clone(&plan);
            let body_height = appearance::ui_size(330.)
                .min(window.viewport_size().height * 0.8 - appearance::ui_size(118.))
                .max(px(160.));
            let details = div()
                .flex()
                .flex_col()
                .gap_3()
                .min_w_0()
                .pr_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(colors.border))
                        .bg(rgb(colors.panel))
                        .min_w_0()
                        .child(label("discard-scope", scope(&plan.entry)).font_weight(FontWeight::SEMIBOLD))
                        .child(path_field("discard-path", if plan.entry.original_path.is_some() { "Renamed path · will be deleted" } else { "File" }, &plan.entry.path, colors))
                        .when_some(plan.entry.original_path.as_ref(), |element, original| {
                            element.child(path_field("discard-original-path", "Original path · will be restored", original, colors))
                        })
                        .child(path_field("discard-repository", "Repository", &repository, colors))
                        .when_some(plan.head.as_ref().filter(|_| !plan.entry.untracked), |element, head| {
                            element.child(label("discard-head", format!("Last commit · {}", short_oid(head))).text_color(rgb(colors.muted)))
                        }),
                )
                .child(label("discard-effect", consequence(&plan)))
                .child(label("discard-preserved", "Other files and the branch stay unchanged.").text_color(rgb(colors.muted)))
                .child(label("discard-revalidation", "If this file or Git state changes, the action stops so you can review it again.").text_color(rgb(colors.muted)));
            dialog
                .kind(DialogKind::Alert)
                .title(native_accessibility::dialog_title("discard-title", title))
                .width(appearance::dialog_width(px(620.), window)).max_h(appearance::dialog_max_height(window))
                .child(
                    div()
                        .id("operation-consequences")
                        .debug_selector(|| "operation-consequences".into())
                        .h(body_height)
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .text_size(appearance::ui_text(12.))
                        .text_color(rgb(colors.text))
                        .child(
                            div()
                                .id("discard-warning")
                                .debug_selector(|| "discard-warning".into())
                                .flex_shrink_0()
                                .rounded_lg()
                                .bg(rgb(colors.removed_background))
                                .px_3()
                                .py_2()
                                .child(label("discard-loss", if plan.entry.untracked { "This permanently deletes the file." } else { "This permanently discards uncommitted work." }).font_weight(FontWeight::SEMIBOLD))
                                .child(label("discard-no-undo", "These changes cannot be recovered from Git.")),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .child(
                                    crate::native_accessibility::clips_children(div()
                                        .id("discard-details")
                                        .debug_selector(|| "discard-details".into())
                                        .role(Role::Document)
                                        .aria_label("File discard review. Use arrow keys, Page Up, Page Down, Home or End to scroll.")
                                        .track_focus(&focus)
                                        .border_1()
                                        .border_color(if ring.shows(&focus) { rgb(colors.accent).into() } else { gpui_kit::transparent_black() })
                                        .rounded_lg()
                                        .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
                                        .on_key_down(move |event, window, cx| {
                                            let modifiers = event.keystroke.modifiers;
                                            if modifiers.platform || modifiers.control || modifiers.alt { return; }
                                            let mut offset = key_scroll.offset();
                                            let page = key_scroll.bounds().size.height * 0.85;
                                            match event.keystroke.key.as_str() {
                                                "up" => offset.y += appearance::ui_size(24.),
                                                "down" => offset.y -= appearance::ui_size(24.),
                                                "pageup" => offset.y += page,
                                                "pagedown" | "space" => offset.y -= page,
                                                "home" => offset.y = px(0.),
                                                "end" => offset.y = -key_scroll.max_offset().y,
                                                _ => return,
                                            }
                                            offset.y = offset.y.clamp(-key_scroll.max_offset().y, px(0.));
                                            key_scroll.set_offset(offset);
                                            window.refresh();
                                            cx.stop_propagation();
                                        })
                                        .size_full()
                                        .overflow_y_scroll()
                                        .track_scroll(&scroll)
                                        .child(details)),
                                )
                                .child(Scrollbar::vertical(&scroll).id("discard-scrollbar").mode(ScrollbarMode::Always)),
                        ),
                )
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(copy_feedback::copy_button(path_mark, "Copy path", "", cx)
                            .accessibility_label("Copy full path of file to discard")
                            .on_action(move |_: &Confirm, window, cx| copy_feedback::copy(window, cx, Some(keyboard_mark.clone()), "file path", keyboard_copy_path.clone()))
                            .on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(click_mark.clone()), "file path", copy_path.clone())))
                        .child(div().flex_1())
                        .child(crate::tags::dialog_cancel("cancel-discard", "Cancel")
                            .debug_selector(|| "cancel-discard".into())
                            .on_action(|_: &Confirm, window, cx| window.dispatch_action(Box::new(Cancel), cx))
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)))
                        .child(button("confirm-discard", action, "", false)
                            .danger()
                            .debug_selector(|| "confirm-discard".into())
                            .on_action(move |_: &Confirm, _, cx| {
                                keyboard_request.set(true);
                                cx.propagate();
                            })
                            .on_click(move |_, window, cx| {
                                request.set(true);
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
                            })),
                )
                .on_ok(move |_, window, cx| {
                    if !requested.replace(false) { return false; }
                    if submitted.replace(true) { return true; }
                    let _ = owner.update(cx, |owner, cx| {
                        if owner.discard_actions.generation == generation
                            && owner.path.as_ref() == Some(&repository)
                            && owner.repository.as_ref().map(|repo| repo.path()) == Some(repository.as_path())
                            && owner.page == AppPage::Repository
                            && owner.operation_busy.is_none()
                        {
                            owner.write(WriteCommand::Discard(Arc::clone(&plan)), action, window, cx);
                        }
                    });
                    true
                })
        });
    }

    pub(super) fn confirm_multi_discard(
        &mut self,
        repository: PathBuf,
        plan: MultiDiscardPlan,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let plan = Arc::new(plan);
        let submitted = Rc::new(Cell::new(false));
        let requested = Rc::new(Cell::new(false));
        let total = plan.items.len();
        let tracked = plan.tracked_count();
        let untracked = plan.untracked_count();
        let title = format!(
            "Discard changes in {}?",
            crate::counted(total, "file", "files")
        );
        let action: &'static str = if tracked == 0 {
            "Delete untracked files"
        } else {
            "Discard selected files"
        };
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let colors = palette(cx);
            let request = requested.clone();
            let keyboard_request = requested.clone();
            let requested = requested.clone();
            let submitted = submitted.clone();
            let owner = owner.clone();
            let repository = repository.clone();
            let plan = Arc::clone(&plan);
            let summary = match (tracked, untracked) {
                (t, 0) => format!(
                    "{} will be restored to HEAD in both the index and the working folder.",
                    crate::counted(t, "tracked file", "tracked files")
                ),
                (0, u) => format!(
                    "{} will be permanently deleted from the working folder.",
                    crate::counted(u, "untracked file", "untracked files")
                ),
                (t, u) => format!(
                    "{} will be restored to HEAD and {} will be permanently deleted.",
                    crate::counted(t, "tracked file", "tracked files"),
                    crate::counted(u, "untracked file", "untracked files")
                ),
            };
            let listed: Vec<String> = plan
                .items
                .iter()
                .take(12)
                .map(|item| {
                    let raw = item.entry.path.display().to_string();
                    let cleaned = gitturtle_core::strip_bidi_controls(&raw);
                    if item.entry.untracked {
                        format!("{cleaned} (delete untracked)")
                    } else {
                        cleaned.into_owned()
                    }
                })
                .collect();
            let overflow = total.saturating_sub(listed.len());
            let content = div()
                .flex()
                .flex_col()
                .gap_3()
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(colors.text))
                .child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(colors.warning))
                        .bg(rgb(colors.panel))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            label("multi-discard-summary", summary)
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .children(listed.into_iter().enumerate().map(|(i, line)| {
                            div()
                                .id(("multi-discard-item", i))
                                .text_size(appearance::ui_text(11.))
                                .text_color(rgb(colors.muted))
                                .child(line)
                        }))
                        .when(overflow > 0, |el| {
                            el.child(
                                div()
                                    .text_size(appearance::ui_text(11.))
                                    .text_color(rgb(colors.muted))
                                    .child(format!("… and {overflow} more")),
                            )
                        }),
                )
                .child(
                    label(
                        "multi-discard-note",
                        "Unselected files and the branch stay unchanged. If any selected file or HEAD changes before execution, the batch stops safely.",
                    )
                    .text_color(rgb(colors.muted)),
                );
            dialog
                .kind(DialogKind::Alert)
                .title(native_accessibility::dialog_title(
                    "multi-discard-title",
                    title.clone(),
                ))
                .width(appearance::dialog_width(px(580.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(content)
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            crate::tags::dialog_cancel("cancel-multi-discard", "Cancel")
                                .debug_selector(|| "cancel-multi-discard".into())
                                .on_action(|_: &Confirm, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                })
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            button("confirm-multi-discard", action, "", false)
                                .danger()
                                .debug_selector(|| "confirm-multi-discard".into())
                                .on_action(move |_: &Confirm, _, cx| {
                                    keyboard_request.set(true);
                                    cx.propagate();
                                })
                                .on_click(move |_, window, cx| {
                                    request.set(true);
                                    window.dispatch_action(
                                        Box::new(Confirm { secondary: false }),
                                        cx,
                                    );
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    if !requested.replace(false) {
                        return false;
                    }
                    if submitted.replace(true) {
                        return true;
                    }
                    let _ = owner.update(cx, |owner, cx| {
                        if owner.discard_actions.generation == generation
                            && owner.path.as_ref() == Some(&repository)
                            && owner.page == AppPage::Repository
                            && owner.operation_busy.is_none()
                        {
                            owner.write(
                                WriteCommand::DiscardMany(Arc::clone(&plan)),
                                "Discarding selected files…",
                                window,
                                cx,
                            );
                        }
                    });
                    true
                })
        });
    }

    pub(crate) fn confirm_partial_discard(
        &mut self,
        diff: Arc<PartialDiff>,
        selection: PartialSelection,
        on_cancel: impl Fn(&mut GitTurtle, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let submitted = Rc::new(Cell::new(false));
        let requested = Rc::new(Cell::new(false));
        let on_cancel = Rc::new(on_cancel);
        let line_count = match &selection {
            PartialSelection::Lines(ids) => ids.len(),
            PartialSelection::Hunks(hunks) => hunks
                .iter()
                .filter_map(|i| diff.hunks.get(*i))
                .flat_map(|h| &h.lines)
                .filter(|l| l.change_id.is_some())
                .count(),
        };
        let title = format!(
            "Discard {} in {}?",
            crate::counted(line_count, "selected line", "selected lines"),
            diff.path.display()
        );
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let colors = palette(cx);
            let request = requested.clone();
            let keyboard_request = requested.clone();
            let requested = requested.clone();
            let submitted_ok = submitted.clone();
            let submitted_cancel = submitted.clone();
            let owner_ok = owner.clone();
            let owner_cancel = owner.clone();
            let on_cancel = on_cancel.clone();
            let repository = repository.clone();
            let diff = Arc::clone(&diff);
            let selection = selection.clone();
            let content = div()
                .flex()
                .flex_col()
                .gap_3()
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(colors.text))
                .child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(colors.warning))
                        .bg(rgb(colors.panel))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            label(
                                "partial-discard-summary",
                                format!(
                                    "Revert {} in {} back to the staged/HEAD snapshot.",
                                    crate::counted(line_count, "unstaged line", "unstaged lines"),
                                    diff.path.display()
                                ),
                            )
                            .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            label(
                                "partial-discard-warning",
                                "Discarded unstaged edits cannot be recovered from Git.",
                            )
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(colors.muted)),
                        ),
                )
                .child(
                    label(
                        "partial-discard-preserved",
                        "Unselected hunks, staged changes in the index, and all other files stay unchanged.",
                    )
                    .text_color(rgb(colors.muted)),
                );
            dialog
                .kind(DialogKind::Alert)
                .title(native_accessibility::dialog_title(
                    "partial-discard-title",
                    title.clone(),
                ))
                .width(appearance::dialog_width(px(540.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(content)
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            crate::tags::dialog_cancel("cancel-partial-discard", "Cancel")
                                .debug_selector(|| "cancel-partial-discard".into())
                                .on_action(|_: &Confirm, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                })
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            button("confirm-partial-discard", "Discard changes", "", false)
                                .danger()
                                .debug_selector(|| "confirm-partial-discard".into())
                                .on_action(move |_: &Confirm, _, cx| {
                                    keyboard_request.set(true);
                                    cx.propagate();
                                })
                                .on_click(move |_, window, cx| {
                                    request.set(true);
                                    window.dispatch_action(
                                        Box::new(Confirm { secondary: false }),
                                        cx,
                                    );
                                }),
                        ),
                )
                .on_cancel({
                    let on_cancel = on_cancel.clone();
                    move |_, _, cx| {
                        if !submitted_cancel.get() {
                            let _ = owner_cancel.update(cx, |this, cx| on_cancel(this, cx));
                        }
                        true
                    }
                })
                .on_ok({
                    let on_cancel = on_cancel;
                    move |_, window, cx| {
                        if !requested.replace(false) {
                            return false;
                        }
                        if submitted_ok.replace(true) {
                            return true;
                        }
                        let _ = owner_ok.update(cx, |owner, cx| {
                            if owner.path == repository
                                && owner.page == AppPage::Repository
                                && owner.operation_busy.is_none()
                            {
                                owner.write(
                                    WriteCommand::DiscardPartial {
                                        diff: Arc::clone(&diff),
                                        selection: selection.clone(),
                                    },
                                    "Discarding selected changes…",
                                    window,
                                    cx,
                                );
                            } else {
                                on_cancel(owner, cx);
                            }
                        });
                        true
                    }
                })
        });
    }

    pub(super) fn confirm_restore_file(
        &mut self,
        repository: PathBuf,
        plan: RestoreFilePlan,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let plan = Arc::new(plan);
        let submitted = Rc::new(Cell::new(false));
        let requested = Rc::new(Cell::new(false));
        let short = short_oid(&plan.revision_oid);
        let title = format!("Restore {} to {short}?", plan.path.display());
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let colors = palette(cx);
            let request = requested.clone();
            let keyboard_request = requested.clone();
            let requested = requested.clone();
            let submitted = submitted.clone();
            let owner = owner.clone();
            let repository = repository.clone();
            let plan = Arc::clone(&plan);
            let content = div()
                .flex()
                .flex_col()
                .gap_3()
                .text_size(appearance::ui_text(12.))
                .text_color(rgb(colors.text))
                .child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(colors.warning))
                        .bg(rgb(colors.panel))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(path_field("restore-file-path", "File", &plan.path, colors))
                        .child(
                            label(
                                "restore-file-revision",
                                format!("Source revision · {}", plan.revision_oid),
                            )
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(colors.muted)),
                        ),
                )
                .child(label(
                    "restore-file-effect",
                    "The working copy of this file will be replaced with its contents from that commit. Uncommitted working edits to this file will be overwritten; the index and all other files stay unchanged.",
                ))
                .child(
                    label(
                        "restore-file-revalidation",
                        "If the working file changes before execution, the restore stops so you can review it again.",
                    )
                    .text_color(rgb(colors.muted)),
                );
            dialog
                .kind(DialogKind::Alert)
                .title(native_accessibility::dialog_title(
                    "restore-file-title",
                    title.clone(),
                ))
                .width(appearance::dialog_width(px(560.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(content)
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            crate::tags::dialog_cancel("cancel-restore-file", "Cancel")
                                .debug_selector(|| "cancel-restore-file".into())
                                .on_action(|_: &Confirm, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                })
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            button("confirm-restore-file", "Restore file", "", false)
                                .danger()
                                .debug_selector(|| "confirm-restore-file".into())
                                .on_action(move |_: &Confirm, _, cx| {
                                    keyboard_request.set(true);
                                    cx.propagate();
                                })
                                .on_click(move |_, window, cx| {
                                    request.set(true);
                                    window.dispatch_action(
                                        Box::new(Confirm { secondary: false }),
                                        cx,
                                    );
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    if !requested.replace(false) {
                        return false;
                    }
                    if submitted.replace(true) {
                        return true;
                    }
                    let _ = owner.update(cx, |owner, cx| {
                        if owner.discard_actions.generation == generation
                            && owner.path.as_ref() == Some(&repository)
                            && owner.page == AppPage::Repository
                            && owner.operation_busy.is_none()
                        {
                            owner.write(
                                WriteCommand::RestoreFile(Arc::clone(&plan)),
                                "Restoring file…",
                                window,
                                cx,
                            );
                        }
                    });
                    true
                })
        });
    }
}
