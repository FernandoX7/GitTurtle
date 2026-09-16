//! The destructive review keeps the exact target and loss warning visible while
//! its bounded, keyboard-scrollable effect list moves independently.
use super::*;
use gitturtle_core::StatusEntry;
use gpui_kit::component::{
    button::ButtonVariant,
    dialog::{Cancel, Confirm},
    scroll::{Scrollbar, ScrollbarMode},
};
use std::{cell::Cell, rc::Rc};

const EFFECT_ROWS: usize = 12;

fn describe_change(entry: &StatusEntry) -> String {
    let kind = if entry.conflicted {
        "Conflict"
    } else if entry.untracked {
        "Untracked"
    } else {
        match (entry.staged.is_some(), entry.unstaged.is_some()) {
            (true, true) => "Staged and unstaged",
            (true, false) => "Staged",
            _ => "Unstaged",
        }
    };
    match &entry.original_path {
        Some(old) => format!("{kind}: {} → {}", old.display(), entry.path.display()),
        None => format!("{kind}: {}", entry.path.display()),
    }
}

pub(super) fn kept_history(details: &WorktreeDetails) -> String {
    match &details.tree.branch {
        Some(branch) => format!(
            "Branch ‘{branch}’ at {} stays available to check out again.",
            short_oid(&details.tree.oid)
        ),
        None => format!(
            "Commit {} has no branch at this detached HEAD. Create a branch or tag first to keep it reachable; the object store alone is not a permanent backup.",
            short_oid(&details.tree.oid)
        ),
    }
}

fn effect_group(
    id: &'static str,
    count: usize,
    singular: &str,
    plural: &str,
    rows: impl Iterator<Item = String>,
    colors: appearance::Palette,
) -> Stateful<Div> {
    let heading = if count == 0 {
        format!("No {plural}")
    } else {
        format!("{count} {}", if count == 1 { singular } else { plural })
    };
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .min_w_0()
        .child(div().font_weight(FontWeight::MEDIUM).child(heading))
        .children(rows.take(EFFECT_ROWS).enumerate().map(|(index, row)| {
            let tooltip = row.clone();
            div()
                .id((id, index))
                .role(Role::Label)
                .aria_label(row.clone())
                .min_w_0()
                .truncate()
                .text_color(rgb(colors.muted))
                .child(row)
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        }))
        .when(count > EFFECT_ROWS, |element| {
            element.child(
                div()
                    .id((id, EFFECT_ROWS))
                    .debug_selector(move || format!("{id}-overflow"))
                    .role(Role::Label)
                    .aria_label(format!(
                        "{} additional files will also be deleted",
                        count - EFFECT_ROWS
                    ))
                    .text_color(rgb(colors.muted))
                    .child(format!("… and {} more files", count - EFFECT_ROWS)),
            )
        })
}

fn effects(details: &WorktreeDetails, colors: appearance::Palette) -> Div {
    let section = || {
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap_3()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(colors.border))
            .bg(rgb(colors.panel))
            .min_w_0()
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .pr_3()
        .text_size(appearance::ui_text(12.))
        .child(
            section()
                .child(label("worktree-will-delete", "Will delete").font_weight(FontWeight::SEMIBOLD).text_color(rgb(colors.removed)))
                .child(effect_group(
                    "worktree-delete-changes",
                    details.changed_files,
                    "changed or untracked file",
                    "changed or untracked files",
                    details.changed_entries.iter().map(describe_change),
                    colors,
                ))
                .child(effect_group(
                    "worktree-delete-ignored",
                    details.ignored_files,
                    "ignored file",
                    "ignored files",
                    details.ignored_paths.iter().map(|path| path.display().to_string()),
                    colors,
                ))
                .children(details.operation_state.iter().enumerate().map(|(index, state)| {
                    div().id(("worktree-delete-operation", index)).child(format!("{state} — unfinished state will be discarded."))
                }))
                .child(label("worktree-delete-metadata", "The entire worktree folder, plus its private HEAD, index, reflog and worktree configuration.")),
        )
        .child(
            section()
                .child(label("worktree-will-keep", "Will keep").font_weight(FontWeight::SEMIBOLD).text_color(rgb(colors.added)))
                .child(label("worktree-kept-history", kept_history(details)))
                .child(label("worktree-kept-shared-data", "Shared objects, refs, stashes and repository configuration. Every other worktree stays intact.")),
        )
        .child(label("worktree-force-revalidation", "If the folder, files or Git state change after this review, removal stops. Reopen Worktrees to review the new state.").text_color(rgb(colors.muted)))
}

impl GitTurtle {
    pub(super) fn confirm_force_worktree_removal(
        &mut self,
        details: WorktreeDetails,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repository_path = self.path.clone();
        let owner = cx.entity().downgrade();
        let command = Arc::new(WriteCommand::Worktree(Arc::new(
            WorktreeCommand::ForceRemove(details.clone()),
        )));
        let details = Arc::new(details);
        let scroll = ScrollHandle::new();
        let focus = cx.focus_handle();
        let submitted = Rc::new(Cell::new(false));
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let colors = palette(cx);
            let target = details.tree.path.display().to_string();
            let target_tooltip = target.clone();
            let branch = details.tree.branch.as_deref().unwrap_or("Detached HEAD");
            let revision = format!("{branch} · {}", short_oid(&details.tree.oid));
            let revision_tooltip = format!("{branch} · {}", details.tree.oid);
            let copy_target = target.clone();
            let key_scroll = scroll.clone();
            let click_focus = focus.clone();
            let command = Arc::clone(&command);
            let owner = owner.clone();
            let repository_path = repository_path.clone();
            let submitted = submitted.clone();
            // The toolkit positions dialogs 10% below the window top. Reserve
            // another 10% below, plus scaled title/footer/padding geometry.
            let body_height = (window.viewport_size().height * 0.8 - appearance::ui_size(118.)).max(px(160.));
            dialog
                .title(label("worktree-force-title", "Force remove worktree?"))
                .width(px(720.).min(window.viewport_size().width - px(48.)))
                .child(
                    div()
                        .id("operation-consequences")
                        .debug_selector(|| "operation-consequences".into())
                        .h(body_height)
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .text_color(rgb(colors.text))
                        .child(
                            div()
                                .id("worktree-force-warning")
                                .debug_selector(|| "worktree-force-warning".into())
                                .flex_shrink_0()
                                .rounded_lg()
                                .bg(rgb(colors.removed_background))
                                .border_l_2()
                                .border_color(rgb(colors.removed))
                                .px_3()
                                .py_2()
                                .text_size(appearance::ui_text(12.))
                                .child(label("worktree-force-loss", "This permanently deletes uncommitted work.").font_weight(FontWeight::SEMIBOLD))
                                .child(label("worktree-force-no-undo", "Git cannot recover the deleted files.")),
                        )
                        .child(
                            div()
                                .id("worktree-force-target")
                                .debug_selector(|| "worktree-force-target".into())
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(appearance::ui_text(12.))
                                        .child(label("worktree-force-branch", revision).truncate().font_weight(FontWeight::MEDIUM).tooltip(move |window, cx| Tooltip::new(revision_tooltip.clone()).build(window, cx)))
                                        .child(label("worktree-force-path", target).truncate().text_color(rgb(colors.muted)).tooltip(move |window, cx| Tooltip::new(target_tooltip.clone()).build(window, cx))),
                                )
                                .child(button("copy-worktree-removal-path", "Copy path", "copy", false)
                                    .debug_selector(|| "copy-worktree-removal-path".into())
                                    .accessibility_label("Copy full path of worktree to remove")
                                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_target.clone())))),
                        )
                        .child(
                            div()
                                .id("worktree-force-effects-frame")
                                .flex_1()
                                .min_h_0()
                                .relative()
                                .child(div()
                                    .id("worktree-force-effects")
                                    .debug_selector(|| "worktree-force-effects".into())
                                    .role(Role::Document)
                                    .aria_label("What force removal deletes and keeps. Use arrow keys, Page Up, Page Down, Home or End to scroll.")
                                    .tab_stop(true)
                                    .track_focus(&focus)
                                    .border_1()
                                    .border_color(gpui_kit::transparent_black())
                                    .rounded_lg()
                                    .focus_visible(|style| style.border_color(rgb(colors.accent)))
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
                                    .child(effects(&details, colors)))
                                .child(Scrollbar::vertical(&scroll).id("worktree-force-scrollbar").mode(ScrollbarMode::Always)),
                        ),
                )
                .button_props(DialogButtonProps::default().ok_text("Force remove worktree").ok_variant(ButtonVariant::Danger).show_cancel(true))
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("cancel-force-worktree-removal")
                            .debug_selector(|| "cancel-force-worktree-removal".into())
                            .label("Cancel")
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)))
                        .child(Button::new("confirm-force-worktree-removal")
                            .debug_selector(|| "confirm-force-worktree-removal".into())
                            .danger()
                            .label("Force remove worktree")
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Confirm { secondary: false }), cx))),
                )
                .on_ok(move |_, window, cx| {
                    if submitted.replace(true) { return true; }
                    let _ = owner.update(cx, |owner, cx| {
                        if owner.path == repository_path
                            && owner.repository.as_ref().map(|repo| repo.path()) == repository_path.as_deref()
                            && owner.page == AppPage::Repository
                            && owner.operation_busy.is_none()
                        {
                            owner.write(command.as_ref().clone(), "Force remove worktree", window, cx);
                        }
                    });
                    true
                })
        });
    }
}
