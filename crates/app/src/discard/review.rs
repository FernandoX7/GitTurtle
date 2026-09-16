//! Bounded destructive review with a persistent warning and explicit acceptance.
use super::*;
use gpui_kit::component::{
    button::ButtonVariant,
    dialog::{Cancel, Confirm, DialogButtonProps},
    scroll::{Scrollbar, ScrollbarMode},
};
use gpui_kit::prelude::FluentBuilder;
use std::{cell::Cell, rc::Rc};

fn label(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

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
        let focus = cx.focus_handle();
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
                .title(label("discard-title", title))
                .width(px(620.).min(window.viewport_size().width - px(48.)))
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
                                    div()
                                        .id("discard-details")
                                        .debug_selector(|| "discard-details".into())
                                        .role(Role::Document)
                                        .aria_label("File discard review. Use arrow keys, Page Up, Page Down, Home or End to scroll.")
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
                                        .child(details),
                                )
                                .child(Scrollbar::vertical(&scroll).id("discard-scrollbar").mode(ScrollbarMode::Always)),
                        ),
                )
                .button_props(DialogButtonProps::default().ok_text(action).ok_variant(ButtonVariant::Danger).show_cancel(true))
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(button("copy-discard-path", "Copy path", "copy", false)
                            .debug_selector(|| "copy-discard-path".into())
                            .accessibility_label("Copy full path of file to discard")
                            .on_action(move |_: &Confirm, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(keyboard_copy_path.clone())))
                            .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()))))
                        .child(div().flex_1())
                        .child(button("cancel-discard", "Cancel", "", false)
                            .secondary()
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
}
