use crate::*;
use columns::ColumnId;
use gpui_kit::base::ElementExt;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder;
use std::cell::RefCell;

fn reveal_resized_selection(
    previous: &mut Option<(Size<Pixels>, Pixels)>,
    next: (Size<Pixels>, Pixels),
    selected: Option<usize>,
    scroll: &UniformListScrollHandle,
) -> bool {
    if *previous == Some(next) {
        return false;
    }
    // Initial layout keeps a restored scroll position. Only a later viewport
    // or row-size change reveals the selection, and only as far as necessary.
    if previous.replace(next).is_none() {
        return false;
    }
    if let Some(selected) = selected {
        scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
        true
    } else {
        false
    }
}

/// What the preview says while no file is chosen, as in Changes before a
/// working file is selected. Short enough for one line at the default text
/// size in a 640 px window, where the earlier "Select a changed file in the
/// right panel, or return to history." left "history." alone on a line.
const CHOOSE_A_FILE: &str = "Select a changed file in the list on the right.";

fn history_scroll_viewport(
    horizontal: &ScrollHandle,
    content: impl IntoElement,
) -> impl IntoElement {
    div()
        .id("history-horizontal")
        .flex_1()
        .min_h_0()
        .min_w_0()
        // Base Scrollbar paints an absolute overlay. Reserve its track inside
        // the tracked viewport so Nearest reveals an entire row above it.
        .pb(Scrollbar::width())
        .overflow_x_scroll()
        .track_scroll(horizontal)
        .child(content)
}

impl GitTurtle {
    fn list_viewport_probe(&self, history: bool, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    // Cold loading geometry must not reveal selection over a
                    // saved viewport or a wheel scroll made during restoration.
                    if this.repository_tabs.restoring.is_some() {
                        return;
                    }
                    let row_height = px(if history {
                        this.settings.density.history_row_height()
                    } else {
                        this.settings.density.file_row_height()
                    });
                    let previous = if history {
                        this.history_list_layout
                    } else {
                        this.file_list_layout
                    };
                    let next = (bounds.size, row_height);
                    if previous == Some(next) {
                        return;
                    }
                    let selected = if history {
                        this.selected_commit
                            .and_then(|index| this.visible.iter().position(|i| *i == index))
                    } else {
                        this.selected_file.and_then(|index| {
                            this.filtered_file_indices(cx)
                                .iter()
                                .position(|i| *i == index)
                        })
                    };
                    let (previous, scroll) = if history {
                        (&mut this.history_list_layout, &this.history_scroll)
                    } else {
                        (&mut this.file_list_layout, &this.file_scroll)
                    };
                    if reveal_resized_selection(previous, next, selected, scroll) {
                        cx.notify();
                        // A refresh requested during paint takes effect on the
                        // next frame, after the list has its new dimensions.
                        window.defer(cx, |window, _| window.refresh());
                    }
                });
            },
        )
        .absolute()
        .inset_0()
    }

    /// The page's one return action: Projects at root History, labelled
    /// where `labelled`, otherwise a Back arrow naming its destination.
    fn render_back_button(&self, labelled: bool, cx: &mut Context<Self>) -> Button {
        let back_label = self.back_label();
        let back_to_projects = back_label == "Back to Projects";
        button(
            "page-back",
            if back_to_projects && labelled {
                "Projects"
            } else {
                ""
            },
            if back_to_projects {
                "folder"
            } else {
                "arrow-left"
            },
            false,
        )
        .debug_selector(|| "page-back".into())
        .accessibility_label(back_label)
        .disabled(self.operation_busy.is_some() && self.mode == WorkspaceMode::History)
        .tooltip(format!("{back_label} · {}[", primary_label()))
        .on_click(cx.listener(|this, _, window, cx| this.navigate_back(window, cx)))
    }

    /// The repository's name, or the app's before one opens.
    fn repository_name(&self) -> String {
        self.repository
            .as_ref()
            .map(|repo| self.project_name(repo.path()))
            .unwrap_or("GitTurtle".into())
    }

    /// The header's repository identity, named with its path for assistive
    /// technology and in its tooltip.
    fn repository_heading(&self) -> Stateful<Div> {
        div()
            .id("repository-heading")
            .debug_selector(|| "repository-heading".into())
            .role(Role::Label)
            .aria_label(format!(
                "Repository {}. {}",
                self.repository
                    .as_ref()
                    .map(|repo| self.project_name(repo.path()))
                    .unwrap_or_default(),
                self.path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ))
            .tooltip({
                let identity = self
                    .path
                    .as_ref()
                    .map(|path| format!("{}\n{}", self.project_name(path), path.display()))
                    .unwrap_or_else(|| "GitTurtle".into());
                move |window, cx| Tooltip::new(identity.clone()).build(window, cx)
            })
    }

    /// The Changes mode's name with its working-file count, as its tab and
    /// its compact toggle's accessible name read it.
    pub(super) fn changes_label(&self) -> String {
        self.work_status
            .as_ref()
            .map(|status| {
                if status.entries.is_empty() {
                    "Changes".into()
                } else {
                    format!("Changes · {}", status.entries.len())
                }
            })
            .unwrap_or_else(|| "Changes".into())
    }

    /// A running operation's progress, with its repository.
    fn render_operation_progress(&self, label: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let progress = self
            .operation_progress()
            .unwrap_or_else(|| label.to_owned());
        let description = format!(
            "{}{}",
            progress,
            self.operation_repository
                .as_ref()
                .map_or(String::new(), |path| format!(" in {}", path.display()))
        );
        div()
            .id("operation-progress")
            .role(Role::Status)
            .aria_label(description.clone())
            .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_live(gpui::accesskit::Live::Off)
            })
            .max_w(appearance::ui_size(180.))
            .truncate()
            .text_size(appearance::ui_text(11.))
            .text_color(rgb(p.accent))
            .child(
                progress
                    + &self
                        .operation_repository
                        .as_ref()
                        .map_or(String::new(), |path| {
                            format!(" · {}", self.project_name(path))
                        }),
            )
            .into_any_element()
    }

    fn render_projects_button(&self, cx: &mut Context<Self>) -> Button {
        button("projects", "", "folder", false)
            .debug_selector(|| "projects".into())
            .accessibility_label("Projects")
            .tooltip("Open Projects")
            .disabled(self.operation_busy.is_some())
            .on_click(cx.listener(|this, _, window, cx| this.show_projects(window, cx)))
    }

    fn render_settings_button(&self, cx: &mut Context<Self>) -> Button {
        button("settings", "", "settings", false)
            .debug_selector(|| "settings".into())
            .accessibility_label("Settings")
            .tooltip(format!("Settings · {},", primary_label()))
            .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx)))
    }

    /// The header's compact row (`Chrome::Compact`): the return action, the
    /// turtle where it fits, the repository's name alone (its path stays in
    /// the tooltip and accessible name) at its own width up to `name`,
    /// History and Changes as icon toggles, the Changes count as a badge on
    /// its icon, Pull requests as "PRs", then, at the trailing edge,
    /// Projects, the profile and Settings, every control 28 px. `name` is
    /// what the row leaves the name with the Projects slot counted in every
    /// mode, so the toggles keep their place when Projects joins the
    /// trailing controls outside History. The same controls in the same
    /// order as the full header, each with its action, its accessible name
    /// and a tooltip that leads with that name. The row keeps the room a
    /// focused control's ring takes above and below it and draws no bottom
    /// rule: the action bar's compact row follows it on the same surface.
    fn render_compact_header(
        &self,
        turtle: bool,
        name: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        let back_to_projects = self.back_label() == "Back to Projects";
        let ring = appearance::button_ring_room(cx);
        let history = self.mode != WorkspaceMode::Working;
        let changes = self.changes_label();
        let count = self
            .work_status
            .as_ref()
            .map_or(0, |status| status.entries.len());
        div()
            .debug_selector(|| "repository-header".into())
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .pt(ring)
            .pb(ring)
            .bg(rgb(p.panel))
            .child(self.render_back_button(false, cx))
            .when(turtle, |header| {
                header.child(app_icon(f32::from(appearance::ui_size(28.))))
            })
            .child(
                self.repository_heading()
                    .min_w_0()
                    .max_w(name)
                    .truncate()
                    .text_size(appearance::ui_text(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.repository_name()),
            )
            .when(self.repository.is_some(), |header| {
                header
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_0p5()
                            .rounded(appearance::ui_size(7.))
                            .bg(rgb(p.subtle))
                            .child(
                                compact_control_name(
                                    button("history-tab", "", "commit", history),
                                    "history-tab",
                                    "History",
                                )
                                .debug_selector(|| "history-tab".into())
                                .toggled(history)
                                .tooltip(format!("History · {}1", primary_label()))
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.show_history(window, cx)
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .relative()
                                    .child(
                                        compact_control_label(
                                            button("changes-tab", "", "changes", !history),
                                            "changes-tab",
                                            changes.clone(),
                                        )
                                        .debug_selector(|| "changes-tab".into())
                                        .toggled(!history)
                                        .tooltip(format!("{changes} · {}2", primary_label()))
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.show_working(window, cx)
                                            }),
                                        ),
                                    )
                                    // The count the full tab reads, as a badge
                                    // on the icon's corner; the toggle's name
                                    // already says it.
                                    .when(count > 0, |toggle| {
                                        toggle.child(changes_badge(count, p))
                                    }),
                            ),
                    )
                    .child(
                        compact_control_name(
                            button("pull-requests", COMPACT_PULL_REQUESTS, "", false),
                            "pull-requests",
                            "Pull requests",
                        )
                        .debug_selector(|| "pull-requests".into())
                        .track_focus(&self.pull_requests_focus)
                        .disabled(busy)
                        .tooltip(
                            "Pull requests · Discover pull requests and review captured changes",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_github_from_header(window, cx)
                        })),
                    )
            })
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(
                        self.operation_busy
                            .map(|label| self.render_operation_progress(label, cx)),
                    )
                    .when(busy, |trailing| {
                        trailing.child(self.render_operation_cancel(cx))
                    })
                    .when(!back_to_projects, |trailing| {
                        trailing.child(self.render_projects_button(cx))
                    })
                    .child(self.render_profile_button(
                        true,
                        crate::profiles::COMPACT_ROW_CONTROL,
                        cx,
                    ))
                    .child(self.render_settings_button(cx)),
            )
            .into_any_element()
    }

    /// The repository header in `chrome`'s form. The full form's `narrow`
    /// drops the Projects and profile labels, keeping their icons, tooltips
    /// and accessible names.
    pub(super) fn render_header(&self, chrome: Chrome, cx: &mut Context<Self>) -> AnyElement {
        let compact = match chrome {
            Chrome::Compact {
                app_icon: turtle,
                name,
            } if self.page == AppPage::Repository => {
                return self.render_compact_header(turtle, name, cx);
            }
            Chrome::Compact { .. } => false,
            Chrome::Full { narrow } => narrow,
        };
        let p = palette(cx);
        let busy = self.operation_busy.is_some();
        let back_label = self.back_label();
        let back_to_projects = back_label == "Back to Projects";
        if self.page == AppPage::Settings {
            return div()
                .min_h(appearance::ui_size(48.))
                .px_3()
                .py_1p5()
                .flex()
                .items_center()
                .gap_3()
                .bg(rgb(p.panel))
                .border_b_1()
                .border_color(rgb(p.border))
                .child(
                    button("page-back", "", "arrow-left", false)
                        .accessibility_label(back_label)
                        .tooltip(format!("{back_label} · {}[", primary_label()))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.return_from_page(window, cx)),
                        ),
                )
                .child(app_icon(f32::from(appearance::ui_size(28.))))
                .child(
                    div()
                        .text_size(appearance::ui_text(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Settings"),
                )
                .into_any_element();
        }
        div()
            .debug_selector(|| "repository-header".into())
            .min_h(appearance::ui_size(48.))
            .py_1p5()
            .flex_wrap()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(p.panel))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(self.render_back_button(!compact, cx))
            .child(app_icon(f32::from(appearance::ui_size(28.))))
            .child(
                self.repository_heading()
                    .flex_1()
                    .max_w(appearance::ui_size(200.))
                    .min_w(appearance::ui_size(100.))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(appearance::ui_text(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(self.repository_name()),
                    )
                    .child(
                        div()
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            .truncate()
                            .child(
                                self.path
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or("A clearer view of your code".into()),
                            ),
                    ),
            )
            .when(self.repository.is_some(), |header| {
                header
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_0p5()
                            .p_0p5()
                            .rounded(appearance::ui_size(9.))
                            .bg(rgb(p.subtle))
                            .child(
                                button(
                                    "history-tab",
                                    "History",
                                    "",
                                    self.mode != WorkspaceMode::Working,
                                )
                                .debug_selector(|| "history-tab".into())
                                .toggled(self.mode != WorkspaceMode::Working)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.show_history(window, cx)
                                    }),
                                ),
                            )
                            .child(
                                button(
                                    "changes-tab",
                                    self.changes_label(),
                                    "",
                                    self.mode == WorkspaceMode::Working,
                                )
                                .debug_selector(|| "changes-tab".into())
                                .toggled(self.mode == WorkspaceMode::Working)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.show_working(window, cx)
                                    }),
                                ),
                            ),
                    )
                    .child(
                        button("pull-requests", "Pull requests", "", false)
                            .debug_selector(|| "pull-requests".into())
                            .track_focus(&self.pull_requests_focus)
                            .disabled(busy)
                            .tooltip("Discover pull requests and review captured changes")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_github_from_header(window, cx)
                            })),
                    )
            })
            .child(div().flex_1())
            .children(
                self.operation_busy
                    .map(|label| self.render_operation_progress(label, cx)),
            )
            .when(busy, |header| {
                header.child(self.render_operation_cancel(cx))
            })
            .when(!back_to_projects, |header| {
                header.child(self.render_projects_button(cx))
            })
            .child(self.render_profile_button(compact, 36., cx))
            .child(self.render_settings_button(cx))
            .into_any_element()
    }

    /// `narrow` marks a History window too narrow for navigation. Its button
    /// then explains that, whatever the saved choice, instead of changing a
    /// choice nothing would show.
    pub(super) fn render_rail(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        div()
            .debug_selector(|| "repository-rail".into())
            .w(appearance::ui_size(44.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .py_3()
            .gap_3()
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(rgb(colors.border))
            .when(self.mode == WorkspaceMode::History, |rail| {
                // Disabled, it keeps its action name with the reason, as the
                // palette's unavailable commands do.
                let (name, tooltip) = if narrow {
                    (
                        "Show branches and worktrees. Unavailable: widen the window",
                        "Widen the window to show branches and worktrees",
                    )
                } else {
                    ("Show branches and worktrees", "Show branches and worktrees")
                };
                #[cfg(test)]
                self.rail_navigation.set(Some((name, tooltip, narrow)));
                // A refused Toggle Sidebar changes nothing on screen, so it is
                // announced politely. Each refusal gets a fresh node, which
                // screen readers announce when it appears; it is out of flow
                // and draws nothing. AT-SPI announces the node's name and
                // AccessKit's macOS adapter its value, so both carry the text.
                let announcement = (narrow && self.navigation_refusals > 0).then_some((
                    self.navigation_refusals,
                    "Branches and worktrees unavailable: widen the window",
                ));
                #[cfg(test)]
                self.rail_announcement.set(announcement);
                rail.child(
                    button("rail-sidebar", "", "commit", false)
                        .debug_selector(|| "rail-sidebar".into())
                        .accessibility_label(name)
                        .tooltip(tooltip)
                        .disabled(narrow)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar = true;
                            this.history_sidebar = true;
                            cx.notify();
                        })),
                )
                .when_some(announcement, |rail, (refusal, message)| {
                    rail.child(
                        div()
                            .id(("history-navigation-unavailable", refusal))
                            .debug_selector(|| "history-navigation-unavailable".into())
                            .role(Role::Status)
                            .a11y_synthetic_children(native_accessibility::polite)
                            .aria_label(message)
                            .aria_value(message)
                            .absolute()
                            .size(px(1.))
                            .overflow_hidden(),
                    )
                })
            })
            .child(
                button("rail-open", "", "folder", false)
                    .accessibility_label("Open repository")
                    .tooltip("Open repository")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_repository(&OpenRepository, window, cx)
                    })),
            )
            .child(div().flex_1())
            .child(app_icon(f32::from(appearance::ui_size(24.))))
            .into_any_element()
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        div()
            .debug_selector(|| "history-sidebar".into())
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .min_h(crate::appearance::ui_size(40.))
                    .py_1()
                    .flex_shrink_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .children(
                        [
                            (NavMode::Local, "Local"),
                            (NavMode::Remote, "Remote"),
                            (NavMode::Worktrees, "Worktrees"),
                        ]
                        .map(|(mode, name)| {
                            button(name, name, "", self.nav_mode == mode).toggled(self.nav_mode == mode).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.nav_mode = mode;
                                    this.rebuild_navigation(cx);
                                    this.nav_scroll.scroll_to_item(0, ScrollStrategy::Top);
                                    cx.notify();
                                },
                            ))
                        }),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .pb_3()
                    .child(Input::new(&self.nav_search).aria_label("Filter branches and worktrees").text_size(crate::appearance::ui_text(11.))),
            )
            .child(
                div()
                    .id("repository-navigation")
                    .role(Role::Tree)
                    .aria_label("Branches and worktrees. Arrow keys browse; Left and Right collapse or expand; Enter activates; Shift F10 opens branch or worktree actions")
                    .key_context("GitTurtleNavigation")
                    .tab_stop(true)
                    .track_focus(&self.nav_focus)
                    .flex_1().min_h_0()
                    .on_action(cx.listener(|this, _: &native_accessibility::NextNavigation, window, cx| this.move_navigation(true, false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::PreviousNavigation, window, cx| this.move_navigation(false, false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::FirstNavigation, window, cx| this.move_navigation(false, true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::LastNavigation, window, cx| this.move_navigation(true, true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::ExpandNavigation, window, cx| this.expand_navigation(true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::CollapseNavigation, window, cx| this.expand_navigation(false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::ActivateNavigation, window, cx| { if let Some(index)=this.nav_cursor {this.activate_navigation(index, window, cx);} }))
                    .on_action(cx.listener(|this, _: &native_accessibility::ManageNavigation, window, cx| this.manage_navigation(window, cx)))
                    .child(uniform_list("navigation", self.nav_rows.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range.map(|i| this.render_nav_row(i, cx)).collect::<Vec<_>>()
                    })).size_full().track_scroll(&self.nav_scroll)),
            )
            .child(
                div()
                    .p_3()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .border_t_1()
                    .border_color(rgb(colors.border))
                    .child(format!(
                        "Arrows browse · Enter filters or opens.\nLeft/Right folders · {} actions.",
                        shortcuts::key_label("shift-f10")
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_nav_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let row = self.nav_rows[index].clone();
        if let NavRow::Section(label, count) = &row {
            return div()
                .w_full()
                .h(crate::appearance::ui_size(30.))
                .px_3()
                .pt_2()
                .flex()
                .items_center()
                .justify_between()
                .text_size(crate::appearance::ui_text(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(colors.muted))
                .child(*label)
                .child(count.to_string())
                .into_any_element();
        }
        if let NavRow::Folder {
            key,
            label,
            depth,
            count,
            expanded,
        } = &row
        {
            let _ = key;
            return div()
                .id(("nav-folder", index))
                .role(Role::TreeItem)
                .aria_level(depth + 1)
                .when(self.nav_cursor == Some(index), |row| {
                    row.aria_active_descendant()
                        .border_l_2()
                        .border_color(rgb(colors.accent))
                })
                .aria_expanded(*expanded)
                .aria_label(format!(
                    "{} {} · {} branches",
                    if *expanded { "Collapse" } else { "Expand" },
                    label,
                    count
                ))
                .w_full()
                .h(crate::appearance::ui_size(30.))
                .pl(px(12. + *depth as f32 * 12.))
                .pr_3()
                .flex()
                .items_center()
                .gap_1()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(colors.muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors.hover)).text_color(rgb(colors.text)))
                .active(|s| s.bg(rgb(colors.selected)))
                .child(div().w(px(12.)).child(if *expanded { "⌄" } else { "›" }))
                .child(icon("folder", 14., colors.muted))
                .child(div().flex_1().truncate().child(label.clone()))
                .child(
                    div()
                        .text_size(crate::appearance::ui_text(10.))
                        .child(count.to_string()),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_navigation(index, window, cx)
                }))
                .into_any_element();
        }
        let (name, symbol, active, depth, tooltip) = match &row {
            NavRow::All => (
                "All history".to_string(),
                "commit",
                self.scope.is_none(),
                0,
                "All history".to_string(),
            ),
            NavRow::Branch(i, depth) => {
                let branch = &self.branches[*i];
                (
                    format!(
                        "{}{}",
                        if branch.current { "• " } else { "" },
                        navigation::branch_label(&branch.name, *depth)
                    ),
                    "branch",
                    self.scope.as_ref().is_some_and(|s| s.0 == branch.name),
                    *depth,
                    format!(
                        "{}{}",
                        branch.name,
                        if branch.current { " · HEAD" } else { "" }
                    ),
                )
            }
            NavRow::Worktree(i) => {
                let tree = &self.worktrees[*i];
                (
                    self.project_name(&tree.path),
                    "worktree",
                    self.path.as_ref() == Some(&tree.path),
                    0,
                    format!(
                        "{}\n{}\n{}{}{}{}",
                        self.project_name(&tree.path),
                        tree.path.display(),
                        tree.branch.as_deref().unwrap_or("Detached HEAD"),
                        if tree.locked { " · locked" } else { "" },
                        if tree.prunable { " · prunable" } else { "" },
                        if tree.detached { " · detached" } else { "" }
                    ),
                )
            }
            _ => unreachable!("section and folder rows handled above"),
        };
        let contextual_branch = match &row {
            NavRow::Branch(index, _) => Some((
                self.branches[*index].name.clone(),
                self.branches[*index].remote,
            )),
            _ => None,
        };
        let branch_owner = cx.entity().downgrade();
        let branch_repository = self.path.clone();
        let contextual_worktree = match &row {
            NavRow::Worktree(index) => Some(self.worktrees[*index].clone()),
            _ => None,
        };
        let element = div()
            .id(("nav", index))
            .role(Role::TreeItem)
            .aria_level(depth + 1)
            .when(self.nav_cursor == Some(index), |row| {
                row.aria_active_descendant()
                    .border_l_2()
                    .border_color(rgb(colors.accent))
            })
            .aria_label(tooltip.clone())
            .aria_selected(active)
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .w_full()
            .h(crate::appearance::ui_size(30.))
            .pl(px(12. + depth as f32 * 12.))
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .text_size(crate::appearance::ui_text(12.))
            .overflow_hidden()
            .cursor_pointer()
            .bg(rgb(if active {
                colors.selected
            } else {
                colors.panel
            }))
            .hover(|s| s.bg(rgb(colors.row_hover(active))))
            .active(|s| s.bg(rgb(colors.selected)))
            .text_color(rgb(colors.sidebar_row(active).0))
            .when(active, |row| row.font_weight(FontWeight::MEDIUM))
            .child(icon(symbol, 15., colors.sidebar_row(active).1))
            .child(div().flex_1().min_w_0().truncate().child(name))
            .on_click(
                cx.listener(move |this, _, window, cx| this.activate_navigation(index, window, cx)),
            );
        if let Some((name, remote)) = contextual_branch {
            element
                .context_menu(move |menu, _, cx| {
                    let owner = branch_owner.clone();
                    let repository = branch_repository.clone();
                    let name = name.clone();
                    let disabled = owner
                        .upgrade()
                        .is_none_or(|owner| owner.read(cx).operation_busy.is_some());
                    menu.label(name.clone()).item(
                        PopupMenuItem::new("Branch actions…")
                            .disabled(disabled)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.path == repository && this.page == AppPage::Repository {
                                        this.open_contextual_branch(
                                            name.clone(),
                                            remote,
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            }),
                    )
                })
                .into_any_element()
        } else if let Some(tree) = contextual_worktree {
            element
                .context_menu(move |menu, _, cx| {
                    let disabled = branch_owner.upgrade().is_none_or(|owner| {
                        let owner = owner.read(cx);
                        owner.operation_busy.is_some()
                            || owner.path != branch_repository
                            || owner.page != AppPage::Repository
                    });
                    let mut menu = menu.label(tree.path.display().to_string());
                    for (label, removal) in [
                        ("Worktree actions…", None),
                        ("Remove worktree…", Some(worktrees::RemovalMode::Ordinary)),
                        (
                            "Force remove worktree…",
                            Some(worktrees::RemovalMode::Force),
                        ),
                    ] {
                        let owner = branch_owner.clone();
                        let repository = branch_repository.clone();
                        let tree = tree.clone();
                        menu = menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.path == repository && this.page == AppPage::Repository {
                                        this.open_worktree_actions(
                                            tree.clone(),
                                            removal,
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            },
                        ));
                    }
                    menu
                })
                .into_any_element()
        } else {
            element.into_any_element()
        }
    }

    /// History's scope toolbar. Where the column is narrower than the
    /// labelled row needs, Columns and the paging buttons keep only their
    /// icons, tooltips and accessible names, and the scope name and count
    /// yield first, so paging stays inside the column.
    fn render_history_scope_toolbar(
        &self,
        scope: SharedString,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let count: SharedString = if self.history_search_active() {
            format!("{} commits", self.visible.len())
        } else {
            format!(
                "{}–{}",
                self.history_paging.offset + usize::from(!self.visible.is_empty()),
                self.history_paging.offset + self.visible.len()
            )
        }
        .into();
        // The width `history_width` last recorded, exactly.
        let column = px(self.history_width);
        let compact = column < labelled_scope_toolbar_width(&count, &scope, window);
        // A control's name, what it does, and the tooltip the compact toolbar
        // gives it, which leads with that name: static text, so a render
        // formats none of them.
        macro_rules! named {
            ($name:literal, $description:literal) => {
                ($name, $description, concat!($name, " · ", $description))
            };
        }
        // Compact, each keeps the app's own artwork, turned: `icon` a quarter
        // turn clockwise per 0.25 of `turn`.
        let paging = |id: &'static str,
                      (name, description, tooltip): (&'static str, &'static str, &'static str),
                      labelled: &str,
                      icon: &'static str,
                      turn: f32| {
            if compact {
                compact_control_name(
                    button(id, "", "", false).icon(
                        Icon::default()
                            .path(icon)
                            .size(appearance::ui_size(16.))
                            .rotate(percentage(turn)),
                    ),
                    id,
                    name,
                )
                .tooltip(tooltip)
            } else {
                button(id, name, labelled, false).tooltip(description)
            }
        };
        let columns = if compact {
            compact_control_name(
                button("columns", "", "columns", self.column_menu),
                "columns",
                "Columns",
            )
            .tooltip("Columns · Choose what appears in history")
        } else {
            button("columns", "Columns", "columns", self.column_menu)
        }
        .debug_selector(|| "columns".into())
        .on_click(cx.listener(|this, _, _, cx| {
            this.column_menu = !this.column_menu;
            cx.notify();
        }));
        // Latest jumps to the top, where the newest rows are; Previous and
        // Older page up and down.
        let latest = paging(
            "history-newest",
            named!(
                "Latest",
                "Read current local history and show its newest rows; keeps the selected inspector"
            ),
            "",
            "icons/arrow-left.svg",
            0.25,
        )
        .debug_selector(|| "history-newest".into())
        .disabled(
            self.repository.is_none() || self.loading.is_some() || self.operation_busy.is_some(),
        )
        .on_click(cx.listener(|this, _, window, cx| this.show_latest_history(window, cx)));
        let previous = paging(
            "history-previous",
            named!(
                "Previous",
                "Read the preceding page; your selected comparison remains available"
            ),
            "",
            "icons/chevron.svg",
            0.75,
        )
        .debug_selector(|| "history-previous".into())
        .disabled(
            self.history_search_active()
                || self.loading.is_some()
                || self.history_paging.offset == 0,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.request_history_page(
                this.history_paging
                    .offset
                    .saturating_sub(history_paging::PAGE_SIZE),
                window,
                cx,
            );
        }));
        let older = paging(
            "load-more",
            named!(
                "Older",
                "Continue the captured history; retains up to 5,000 rows in a bounded window"
            ),
            "chevron",
            "icons/chevron.svg",
            0.25,
        )
        .debug_selector(|| "load-more".into())
        .disabled(
            self.repository.is_none()
                || self.history_search_active()
                || self.loading.is_some()
                || self.history_paging.next_offset.is_none(),
        )
        .on_click(cx.listener(|this, _, window, cx| {
            if let Some(offset) = this.history_paging.next_offset {
                this.request_history_page(offset, window, cx);
            }
        }));
        let name = |scope: SharedString| {
            div()
                .max_w(px(190.))
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(scope)
        };
        let count_label = |count: SharedString| {
            div()
                .text_size(crate::appearance::ui_text(10.))
                .text_color(rgb(colors.muted))
                .child(count)
        };
        let toolbar = div()
            .debug_selector(|| "history-scope-toolbar".into())
            .h(crate::appearance::ui_size(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(rgb(colors.border));
        if compact {
            // The count first, drawn whole or not at all, then the name beside
            // it only where at least its first character and ellipsis fit, so
            // the row shows nothing, the count, or both, as it widens, and
            // never a clipped or lone ellipsis.
            let room = compact_scope_room(column, window);
            let count_width = scope_count_width(&count, window);
            let shows_count = count_width <= room;
            let shows_name = shows_count
                && room - count_width - window.rem_size() * 0.5
                    >= scope_name_min_width(&scope, window);
            // The branch icon names what the row may have cut, formatted only
            // when its tooltip is shown.
            let summary = (scope.clone(), count.clone());
            // The paging buttons sit a quarter rem apart, as the comparison
            // path row's icon buttons do; in the narrowest column the name
            // and count yield entirely.
            toolbar
                .child(
                    icon("branch", 15., colors.accent)
                        .id("history-scope-icon")
                        .debug_selector(|| "history-scope-icon".into())
                        .tooltip(move |window, cx| {
                            let (scope, count) = &summary;
                            Tooltip::new(format!("{scope} · {count}")).build(window, cx)
                        }),
                )
                .child(
                    div()
                        .debug_selector(|| "history-scope-label".into())
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(shows_name, |label| {
                            label.child(
                                name(scope)
                                    .debug_selector(|| "history-scope-name".into())
                                    .min_w_0(),
                            )
                        })
                        .when(shows_count, |label| {
                            label.child(
                                count_label(count)
                                    .debug_selector(|| "history-scope-count".into())
                                    .flex_shrink_0(),
                            )
                        }),
                )
                .child(columns)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(latest)
                        .child(previous)
                        .child(older),
                )
                .into_any_element()
        } else {
            toolbar
                .child(icon("branch", 15., colors.accent))
                .child(name(scope).debug_selector(|| "history-scope-name".into()))
                .child(count_label(count).debug_selector(|| "history-scope-count".into()))
                .child(div().flex_1())
                .child(columns)
                .child(latest)
                .child(previous)
                .child(older)
                .into_any_element()
        }
    }

    pub(super) fn render_history(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let scope = self
            .scope
            .as_ref()
            .map(|s| s.0.clone())
            .unwrap_or("All history".into());
        // Before anything in this frame lays the columns out, rows included.
        self.history_subject_floor
            .set(f32::from(subject_floor(window)));
        let columns = self.history_column_layout();
        let history = if self.commits.is_empty() && self.error.is_some() {
            empty(
                "Could not open repository",
                self.error.as_deref().unwrap_or_default(),
            )
        } else if self.visible.is_empty()
            && let Some((title, description)) = self.history_search_empty()
        {
            empty(title, &description)
        } else if self.visible.is_empty() {
            empty(
                if self.loading.is_some() {
                    "Reading local history…"
                } else if self.repository.is_none() {
                    "Your history, at a glance"
                } else if !self.commits.is_empty() {
                    "No commits match this search"
                } else {
                    "No commits yet"
                },
                if self.loading.is_some() {
                    "Loading commit history from this repository."
                } else if self.repository.is_none() {
                    "Open a Git repository to explore branches, worktrees and changes."
                } else if self.commits.is_empty() {
                    "Add files to your project, then open Changes to make your first commit."
                } else {
                    "Search applies to the loaded history."
                },
            )
        } else {
            uniform_list(
                "commits",
                self.visible.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let rows = this.history_rows(cx);
                    range
                        .map(|i| this.render_commit_row(i, &rows, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .size_full()
            .track_scroll(&self.history_scroll)
            .into_any_element()
        };
        // A narrow window scrolls the columns sideways; an empty state stays
        // centered in the part of the list that is visible, not in all columns.
        let history = if self.visible.is_empty() {
            let visible = self.history_width.min(columns.content_width + 26.) - 2.;
            div()
                .debug_selector(|| "history-empty-state".into())
                .absolute()
                .top_0()
                .bottom_0()
                .map(|state| {
                    if visible > 0. {
                        state
                            .left(-self.history_horizontal.offset().x)
                            .w(px(visible))
                    } else {
                        state.left_0().right_0()
                    }
                })
                .child(history)
                .into_any_element()
        } else {
            history
        };
        let view = cx.entity().downgrade();
        let header = div()
            .h(crate::appearance::ui_size(34.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .border_l_2()
            .border_color(rgb(colors.canvas))
            .border_b_1()
            .bg(rgb(colors.panel))
            .text_size(crate::appearance::ui_text(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(colors.muted))
            .children(columns.columns.iter().map(|column| {
                let id = column.id;
                let width = column.width;
                // Narrow History sets the graph's and the subject's widths
                // itself, so their dividers stay drawn but drag no saved
                // width it would not show. Its compact graph shows its title
                // only where the title fits whole, and has no room for the
                // lane buttons: the selection reveals its lane and the
                // palette's lane commands remain.
                let narrow = columns.narrow;
                let resizable = !(narrow
                    && matches!(id, columns::ColumnId::Graph | columns::ColumnId::Subject));
                let title =
                    !(narrow && id == columns::ColumnId::Graph) || graph_title_fits(width, window);
                div()
                    .debug_selector(move || format!("history-column-{}", id as usize))
                    .relative()
                    .w(px(width))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pl(px(10.))
                    .pr_3()
                    .when(title, |cell| cell.child(div().truncate().child(id.label())))
                    .when(id == columns::ColumnId::Graph && !narrow, |cell| {
                        let capacity = self.layout_lane_capacity(&columns);
                        let offset = self.graph_lane_offset();
                        cell.when(self.graph_lanes > capacity, |cell| {
                            cell.child(div().flex_1())
                                .child(
                                    button("earlier-graph-lanes", "‹", "", false)
                                        .accessibility_label("Show earlier graph lanes")
                                        .disabled(offset == 0)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.shift_graph_lanes(false);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    button("later-graph-lanes", "›", "", false)
                                        .accessibility_label(format!(
                                            "Show later graph lanes; viewing {}–{} of {}",
                                            offset + 1,
                                            (offset + capacity).min(self.graph_lanes),
                                            self.graph_lanes
                                        ))
                                        .disabled(offset + capacity >= self.graph_lanes)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.shift_graph_lanes(true);
                                            cx.notify();
                                        })),
                                )
                        })
                    })
                    .child(
                        div()
                            .id(("column-resize", id as usize))
                            .absolute()
                            .right(px(0.))
                            .top_0()
                            .w(px(7.))
                            .h_full()
                            .border_r_1()
                            .border_color(rgb(colors.border))
                            .when(resizable, |divider| {
                                divider
                                    .cursor_col_resize()
                                    .hover(|style| style.bg(rgb(colors.hover)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                            this.column_drag = Some((id, event.position.x, width));
                                            cx.stop_propagation();
                                            cx.notify();
                                        }),
                                    )
                            }),
                    )
            }));
        div()
            .relative()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.canvas))
            // Layout snaps this width to device pixels, so one window width
            // always measures the same. Keep it exactly: a tolerance would
            // keep a nearby stale width, and Subject takes the leftover, so
            // the columns would land somewhere else after navigation is
            // hidden and shown again. An unchanged width lays nothing out.
            .on_prepaint(move |bounds, _, cx| {
                let _ = view.update(cx, |this, cx| {
                    let width = f32::from(bounds.size.width);
                    if this.history_width != width {
                        #[cfg(test)]
                        HISTORY_WIDTHS.with(|count| count.set(count.get() + 1));
                        this.history_width = width;
                        cx.notify();
                    }
                });
            })
            .child(self.render_history_scope_toolbar(scope.into(), window, cx))
            .child(
                div()
                    .h(crate::appearance::ui_size(38.))
                    .flex_shrink_0()
                    .px_3()
                    .pb_2()
                    .pt_1()
                    .child(Input::new(&self.search).text_size(crate::appearance::ui_text(12.))),
            )
            .child(self.render_history_search_controls(cx))
            .children(self.graph_notice.as_ref().map(|notice| {
                div()
                    .px_3()
                    .py_1()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .child(notice.clone())
            }))
            .when(self.column_menu, |el| {
                el.child(
                    div()
                        .p_3()
                        .border_b_1()
                        .border_color(rgb(colors.border))
                        .child(self.render_columns_controls(false, cx)),
                )
            })
            .child(history_scroll_viewport(
                &self.history_horizontal,
                div()
                    .w(px(columns.content_width + 26.))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .id("history-pane")
                            .debug_selector(|| "history-pane".into())
                            .role(Role::ListBox)
                            .aria_label("Commit history")
                            .tab_stop(true)
                            .key_context("GitTurtleList")
                            .track_focus(&self.focus)
                            .on_action(cx.listener(
                                |this, _: &gpui_kit::component::input::Copy, window, cx| {
                                    this.copy_list_item(ListCopy::CommitHash, window, cx)
                                },
                            ))
                            .border_1()
                            .border_color(rgb(colors.border))
                            .when(self.focus_ring.shows(&self.focus), |pane| {
                                pane.border_color(rgb(colors.accent))
                            })
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.pane = Pane::History;
                                    window.focus(&this.focus, cx);
                                }),
                            )
                            .child(history)
                            .child(self.list_viewport_probe(true, cx)),
                    ),
            ))
            .child(Scrollbar::horizontal(&self.history_horizontal).mode(ScrollbarMode::Always))
            .into_any_element()
    }

    /// The chosen graph lane spacing at the interface size.
    fn lane_spacing(&self) -> f32 {
        f32::from(self.settings.graph_spacing) * appearance::ui_scale()
    }

    /// History's columns for the width it last recorded: the chosen ones,
    /// or narrow History's below the width that leaves the subject its
    /// floor (`ColumnSettings::history_layout`).
    fn history_column_layout(&self) -> columns::ColumnLayout {
        let spacing = self.lane_spacing();
        let compact = graph::compact_spacing(spacing);
        self.settings.columns.history_layout(
            self.history_width - 26.,
            graph::required_width(self.graph_lanes, spacing),
            columns::NarrowHistory {
                subject_floor: self.history_subject_floor.get(),
                graph: graph::required_width(self.graph_lanes, compact),
                graph_min: graph::required_width(1, compact),
            },
        )
    }

    /// The lane pitch `layout` draws the graph at: narrow History's compact
    /// pitch, or the chosen spacing.
    fn layout_lane_spacing(&self, layout: &columns::ColumnLayout) -> f32 {
        if layout.narrow {
            graph::compact_spacing(self.lane_spacing())
        } else {
            self.lane_spacing()
        }
    }

    fn graph_lane_capacity(&self) -> usize {
        self.layout_lane_capacity(&self.history_column_layout())
    }

    fn layout_lane_capacity(&self, layout: &columns::ColumnLayout) -> usize {
        let width = layout
            .columns
            .iter()
            .find(|column| column.id == columns::ColumnId::Graph)
            .map_or(112., |column| column.width);
        graph::visible_lane_capacity(width, self.layout_lane_spacing(layout))
    }

    /// What every History row of a frame shares, laid out once per frame
    /// rather than once per row.
    fn history_rows(&self, cx: &App) -> HistoryRows {
        let columns = self.history_column_layout();
        let filtered = history_paging::graph_is_filtered(
            self.commits.len(),
            self.visible.len(),
            self.automatic.retained_commit,
            self.history_search_active(),
        ) || self.graph_notice.is_some();
        HistoryRows {
            spacing: self.layout_lane_spacing(&columns),
            lane_offset: if filtered {
                0
            } else {
                self.graph_offset.min(
                    self.graph_lanes
                        .saturating_sub(self.layout_lane_capacity(&columns)),
                )
            },
            filtered,
            lane_colors: graph::colors(cx),
            labels: appearance::labels(cx),
            columns,
        }
    }

    fn graph_lane_offset(&self) -> usize {
        self.graph_offset
            .min(self.graph_lanes.saturating_sub(self.graph_lane_capacity()))
    }

    pub(super) fn shift_graph_lanes(&mut self, later: bool) {
        let capacity = self.graph_lane_capacity();
        let offset = self.graph_lane_offset();
        self.graph_offset = if later {
            offset
                .saturating_add(capacity.saturating_sub(1).max(1))
                .min(self.graph_lanes.saturating_sub(capacity))
        } else {
            offset.saturating_sub(capacity.saturating_sub(1).max(1))
        };
    }

    pub(super) fn reveal_graph_lane(&mut self, index: usize) {
        let Some(row) = self.graph.get(index) else {
            return;
        };
        let lane = row.lane;
        let capacity = self.graph_lane_capacity();
        let offset = self.graph_lane_offset();
        self.graph_offset = if lane < offset {
            lane
        } else if lane >= offset + capacity {
            lane + 1 - capacity
        } else {
            offset
        };
    }

    pub(super) fn render_commit_row(
        &self,
        position: usize,
        rows: &HistoryRows,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let index = self.visible[position];
        let commit = &self.commits[index];
        let subject = commit_message::summary(&commit.subject);
        let author = commit_message::summary(&commit.author);
        let active = self.selected_commit == Some(index);
        let columns = &rows.columns;
        let lane = self.graph[index].color % rows.lane_colors.len();
        let color = rows.lane_colors[lane];
        let label = rows.labels.refs[lane];
        let tint = rgba((color << 8) | crate::appearance::REF_TINT_ALPHA);
        let names = self
            .refs
            .get(&commit.oid)
            .filter(|names| self.settings.columns.refs.visible && !names.is_empty());
        let mut references = None;
        let mut pill = None;
        // The whole list, which narrow History gives the row's tooltip and
        // accessible name.
        let mut folded = None;
        if let Some(names) = names {
            // HEAD's branch comes first (`worker::Snapshot::refs`).
            let name = names[0].clone();
            let more = (names.len() > 1).then(|| format!("+{}", names.len() - 1));
            if columns.narrow {
                #[cfg(test)]
                HISTORY_PILLS.with(|pills| {
                    pills
                        .borrow_mut()
                        .insert(index, (name.clone(), more.clone(), label))
                });
                folded = Some(names);
                pill = Some(
                    div()
                        .debug_selector(move || format!("history-ref-pill-{index}"))
                        .flex_shrink_0()
                        .min_w_0()
                        .max_w(relative(0.5))
                        .flex()
                        .items_center()
                        .gap_1()
                        .overflow_hidden()
                        .px_1()
                        .py_0p5()
                        .rounded(px(3.))
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(label))
                        .bg(tint)
                        .child(div().min_w_0().truncate().child(name))
                        .children(more.map(|more| div().flex_shrink_0().child(more))),
                );
            } else {
                let refs_width = self.settings.columns.refs.width;
                let title = names.join("\n");
                references = Some(
                    div()
                        .w(px(refs_width))
                        .flex_shrink_0()
                        .pl(px(10.))
                        .pr_2()
                        .flex()
                        .items_center()
                        .gap_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .id(("refs", index))
                                .min_w_0()
                                .max_w(px(refs_width - 22.))
                                .truncate()
                                .px_1()
                                .py_0p5()
                                .rounded(px(3.))
                                .text_size(crate::appearance::ui_text(10.))
                                .text_color(rgb(label))
                                .bg(tint)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(title.clone()).build(window, cx)
                                })
                                .child(name),
                        )
                        .children(more.map(|more| {
                            div()
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_text(10.))
                                .text_color(rgb(colors.muted))
                                .child(more)
                        }))
                        .into_any_element(),
                );
            }
        }
        let name = format!(
            "{}{} · {} · {} · {} parent{}{}",
            subject,
            folded.map_or_else(String::new, |names| format!(
                " · References: {}",
                names.join(", ")
            )),
            author,
            short_oid(&commit.oid),
            commit.parents.len(),
            if commit.parents.len() == 1 { "" } else { "s" },
            if commit.parents.len() > 1 {
                " · merge commit"
            } else {
                ""
            }
        );
        let tooltip = folded.map(|names| SharedString::from(names.join("\n")));
        #[cfg(test)]
        HISTORY_ROW_NAMES.with(|names| {
            names
                .borrow_mut()
                .insert(index, (name.clone(), tooltip.clone()))
        });
        let row =
            div()
                .id(("commit", index))
                .role(Role::ListBoxOption)
                .aria_label(name)
                .aria_selected(active)
                .when(active, |row| row.aria_active_descendant())
                .aria_position_in_set(position + 1)
                .aria_size_of_set(self.visible.len())
                .when_some(tooltip, |row, tooltip| {
                    row.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                })
                .w_full()
                .h(px(self.settings.density.history_row_height()))
                .flex()
                .items_center()
                .px_3()
                .gap_0()
                .bg(rgb(if active {
                    colors.selected
                } else {
                    colors.canvas
                }))
                .border_l_2()
                .border_color(rgb(if active { colors.accent } else { colors.canvas }))
                .hover(|s| s.bg(rgb(colors.row_hover(active))))
                .active(|s| s.bg(rgb(colors.selected)))
                .cursor_pointer()
                .children(columns.columns.iter().map(|column| {
                    let cell = div()
                        .w(px(column.width))
                        .flex_shrink_0()
                        .min_w_0()
                        .overflow_hidden();
                    match column.id {
                        ColumnId::Refs => cell.children(references.take()).into_any_element(),
                        ColumnId::Graph => cell
                            .child(graph::render(
                                self.graph[index].clone(),
                                column.width,
                                rows.lane_offset,
                                rows.spacing,
                                self.settings.density.history_row_height(),
                                graph::RowStyle {
                                    active,
                                    merge: commit.parents.len() > 1,
                                    filtered: rows.filtered,
                                },
                            ))
                            .into_any_element(),
                        ColumnId::Subject => {
                            let cell = cell
                                .debug_selector(move || format!("history-subject-{index}"))
                                .pl(px(10.))
                                .pr_3()
                                .text_size(crate::appearance::ui_text(13.));
                            // Narrow History folds References into the cell's
                            // start, and the subject truncates beside the pill.
                            match pill.take() {
                                Some(pill) => cell.flex().items_center().gap_1().child(pill).child(
                                    div().flex_1().min_w_0().truncate().child(subject.clone()),
                                ),
                                None => cell.truncate().child(subject.clone()),
                            }
                            .into_any_element()
                        }
                        ColumnId::Author => cell
                            .pl(px(10.))
                            .pr_3()
                            .truncate()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(author.clone())
                            .into_any_element(),
                        ColumnId::Date => cell
                            .pl(px(10.))
                            .pr_3()
                            .truncate()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(short_date(commit.timestamp))
                            .into_any_element(),
                        ColumnId::Sha => cell
                            .pl(px(10.))
                            .pr_3()
                            .truncate()
                            .font_family(mono())
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(short_oid(&commit.oid))
                            .into_any_element(),
                    }
                }));
        let recovery_owner = cx.entity().downgrade();
        let recovery_repository = self.path.clone();
        let recovery_commit = recovery::commit_target(commit);
        row.on_click(cx.listener(move |this, _, window, cx| this.select_commit(index, window, cx)))
            .context_menu(move |menu, _, cx| {
                GitTurtle::recovery_context_menu(
                    menu,
                    &recovery_owner,
                    &recovery_repository,
                    &recovery_commit,
                    cx,
                )
            })
            .into_any_element()
    }

    /// The inspector, its "Changed files" header laid out as `files_header`,
    /// which the window measures where it fixes the inspector's width.
    pub(super) fn render_inspector(
        &self,
        files_header: FilesHeader,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.file_history.is_active() {
            return self.render_file_history(cx);
        }
        if self.revision_inspection.is_active() {
            return self.render_revision_inspector(cx);
        }
        let colors = palette(cx);
        let Some(commit) = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
        else {
            return div()
                .size_full()
                .bg(rgb(colors.panel))
                .border_l_1()
                .border_color(rgb(colors.border))
                .child(empty(
                    "Inspect a commit",
                    "Choose a commit in history to see its details and changed files.",
                ))
                .into_any_element();
        };
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_l_1()
            .border_color(rgb(colors.border))
            .child(self.render_commit_message(commit, false, cx))
            .child(
                div()
                    .id("commit-inspector-files")
                    .debug_selector(|| "commit-inspector-files".into())
                    .flex_1()
                    .min_h_0()
                    .child(self.render_files_with(files_header, cx)),
            )
            .into_any_element()
    }

    /// The author and the date and hash lines are selectable after the
    /// message in `selection`'s document; parents and recovery stay controls.
    pub(super) fn render_commit_metadata(
        &self,
        commit: &Commit,
        selection: &commit_message::Selection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let mut parents = div().flex().flex_wrap().gap_1();
        for (i, parent) in commit.parents.iter().take(128).enumerate() {
            parents = parents.child(
                button(
                    ("parent", i),
                    format!("P{} · {}", i + 1, short_oid(parent)),
                    "",
                    self.parent == i,
                )
                .accessibility_label(format!("Compare against parent {} · {}", i + 1, parent))
                .tooltip(format!("Compare against parent {} · {}", i + 1, parent))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.change_parent(i, window, cx)),
                ),
            );
        }
        if commit.parents.len() > 128 {
            parents = parents.child(format!("Showing 128 of {} parents", commit.parents.len()));
        }
        if commit.parents.is_empty() {
            parents = parents.child("Root commit · empty-tree comparison");
        }
        let [author, date] = selection.metadata([
            commit_message::summary(&commit.author).into(),
            format!(
                "{} · {}",
                full_date(commit.timestamp),
                short_oid(&commit.oid)
            )
            .into(),
        ]);
        div()
            .id("commit-metadata")
            .role(Role::Group)
            .aria_label("Commit author and comparison parents")
            .min_w_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(appearance::ui_text(11.))
            .text_color(rgb(colors.muted))
            .child(div().child(author))
            .child(date)
            .child(parents)
            .child(self.render_commit_recovery_menu(commit, cx))
            .into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_files_with(FilesHeader::WHOLE, cx)
    }

    /// The changed files, their header laid out as `header`.
    fn render_files_with(&self, header: FilesHeader, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let visible = self.filtered_file_indices(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                // The count never shrinks: the label truncates first, and in
                // an inspector the window pins too narrow for its first
                // character and ellipsis it leaves, then the count.
                div()
                    .debug_selector(|| "changed-files-header".into())
                    .h(crate::appearance::ui_size(36.))
                    .flex_shrink_0()
                    .min_w_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .font_weight(FontWeight::MEDIUM)
                    .gap_2()
                    .child(icon("changes", 15., colors.muted))
                    .when(header.label, |row| {
                        row.child(
                            div()
                                .debug_selector(|| "changed-files-label".into())
                                .min_w_0()
                                .truncate()
                                .child("Changed files"),
                        )
                    })
                    .when(header.count, |row| {
                        row.child(
                            div()
                                .debug_selector(|| "changed-files-count".into())
                                .flex_shrink_0()
                                .px_1p5()
                                .py_0p5()
                                .rounded(px(4.))
                                .bg(rgb(colors.hover))
                                .text_size(crate::appearance::ui_text(10.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(crate::appearance::labels(cx).count))
                                .child(self.files.len().to_string()),
                        )
                    }),
            )
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .child(Input::new(&self.file_filter).text_size(appearance::ui_text(12.))),
            )
            .child(
                div()
                    .id("files-pane")
                    .debug_selector(|| "files-pane".into())
                    .role(Role::ListBox)
                    .aria_label("Changed files")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.file_focus)
                    .on_action(cx.listener(
                        |this, _: &gpui_kit::component::input::Copy, window, cx| {
                            this.copy_list_item(ListCopy::FilePath, window, cx)
                        },
                    ))
                    .border_1()
                    .border_color(rgb(colors.border))
                    .when(self.focus_ring.shows(&self.file_focus), |pane| {
                        pane.border_color(rgb(colors.accent))
                    })
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.pane = Pane::Files;
                            window.focus(&this.file_focus, cx);
                        }),
                    )
                    .child(if visible.is_empty() {
                        empty(
                            if self.file_paths.pending {
                                "Filtering changed paths…"
                            } else if self.file_paths.error.is_some() {
                                "Path filter unavailable"
                            } else {
                                self.loading.unwrap_or("No file changes")
                            },
                            self.file_paths.error.as_deref().unwrap_or(
                                if self.file_paths.pending {
                                    "Matching the current file snapshot."
                                } else if self.file_filter.read(cx).value().is_empty() {
                                    "No changes between the displayed targets."
                                } else {
                                    "No matching paths. Clear the path filter to see all files."
                                },
                            ),
                        )
                    } else {
                        uniform_list(
                            "files",
                            visible.len(),
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                let visible = this.filtered_file_indices(cx);
                                range
                                    .filter_map(|i| visible.get(i).copied())
                                    .map(|i| this.render_file_row(i, header.status, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.file_scroll)
                        .into_any_element()
                    })
                    .child(self.list_viewport_probe(false, cx)),
            )
            .into_any_element()
    }
    /// A changed file's row. `status` shows its status word beside the name;
    /// without it, in an inspector too narrow for both (`FilesHeader`), the
    /// status tile's icon and colour carry the status and the name takes the
    /// width, the word staying in the row's accessible name.
    pub(super) fn render_file_row(
        &self,
        index: usize,
        status: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let file = &self.files[index];
        let active = self.selected_file == Some(index);
        let path = file.path();
        let full_path = path.display().to_string();
        let history_owner = cx.entity().downgrade();
        let history_repository = self.path.clone();
        let history_file = file.clone();
        let history_commit = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .map(|commit| commit.oid.clone());
        let (symbol, color, label) = match file.status.letter() {
            "A" => ("file-added", colors.added, "New"),
            "D" => ("file-deleted", colors.removed, "Deleted"),
            "R" => ("file-renamed", colors.renamed, "Renamed"),
            "T" => ("file-type", colors.modified, "Type"),
            _ => ("file-modified", colors.modified, "Modified"),
        };
        div()
            .id(("file", index))
            .when(active, |row| {
                row.debug_selector(|| "selected-changed-file".into())
            })
            .role(Role::ListBoxOption)
            .aria_label(format!("{} · {}", path.display(), file.status.label()))
            .aria_selected(active)
            .when(active, |row| row.aria_active_descendant())
            .tooltip(move |window, cx| Tooltip::new(full_path.clone()).build(window, cx))
            .w_full()
            .h(px(self.settings.density.file_row_height()))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(if active {
                colors.selected
            } else {
                colors.panel
            }))
            .border_l_2()
            .border_color(rgb(if active { colors.accent } else { colors.panel }))
            .hover(|s| s.bg(rgb(colors.row_hover(active))))
            .active(|s| s.bg(rgb(colors.selected)))
            .cursor_pointer()
            .child(
                div()
                    .size(px(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(rgba((color << 8) | 0x18))
                    .child(icon(symbol, 16., color)),
            )
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
                            .text_size(crate::appearance::ui_text(12.))
                            .child(
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(colors.muted))
                            .child(
                                path.parent()
                                    .filter(|p| !p.as_os_str().is_empty())
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or("Repository root".into()),
                            ),
                    ),
            )
            .when(status, |row| {
                row.child(
                    div()
                        .debug_selector(|| "changed-file-status".into())
                        .flex_shrink_0()
                        .px_1()
                        .py_0p5()
                        .rounded(px(3.))
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(colors.text))
                        .child(label),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, window, cx)))
            .context_menu(move |menu, _, _| {
                let owner = history_owner.clone();
                let repository = history_repository.clone();
                let file = history_file.clone();
                let commit = history_commit.clone();
                menu.item(
                    PopupMenuItem::new("File history…").on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.path == repository
                                && this
                                    .selected_commit
                                    .and_then(|i| this.commits.get(i))
                                    .map(|c| &c.oid)
                                    == commit.as_ref()
                                && let Some(index) =
                                    this.files.iter().position(|candidate| candidate == &file)
                            {
                                this.open_file_history(index, window, cx);
                            }
                        });
                    }),
                )
            })
            .into_any_element()
    }
    /// `column_min` is the preview column's narrowest width; the path row keeps
    /// 220 px before the modes wrap below it, but never more than that column
    /// holds, and its buttons wrap below the path rather than clip. `column`
    /// is its width as last laid out, which the review options fit.
    pub(super) fn render_preview(
        &self,
        column_min: Pixels,
        column: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.blame.is_visible() {
            return self.render_blame(cx);
        }
        let colors = palette(cx);
        let quick_source = self.is_quick_source();
        let file = self.selected_file.and_then(|index| self.files.get(index));
        let path = file
            .map(|file| file.path().to_string_lossy().into_owned())
            .unwrap_or("File comparison".into());
        let copy_path = path.clone();
        // The toolbar's `px_3` is 1.5 rem on both sides.
        let path_room = (column_min - appearance::ui_size(19.5)).max(px(0.));
        let mut path_controls = div()
            .min_h(appearance::ui_size(28.))
            .min_w(appearance::ui_size(220.).min(path_room))
            .flex_1()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .children(
                self.working_selected
                    .filter(|_| self.mode == WorkspaceMode::Working)
                    .map(|(_, area)| {
                        let staged = area == gitturtle_core::ChangeArea::Staged;
                        let labels = crate::appearance::labels(cx);
                        div()
                            .flex_shrink_0()
                            .px_2()
                            .py_1()
                            .rounded(px(4.))
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(if staged {
                                labels.added
                            } else {
                                labels.unstaged
                            }))
                            .bg(rgb(if staged {
                                colors.added_background
                            } else {
                                colors.subtle
                            }))
                            .child(if staged { "Staged" } else { "Unstaged" })
                    }),
            )
            .child(
                div()
                    .id("preview-file-path")
                    .debug_selector(|| "preview-file-path".into())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                    .child(
                        file.map(|file| file.path().display().to_string())
                            .unwrap_or("File comparison".into()),
                    ),
            )
            .children(file.map(|file| {
                let mark = copy_feedback::Mark::new("copy-path", file.path());
                let copy_mark = mark.clone();
                copy_feedback::copy_button(mark, "", "Copy file path", cx)
                    .accessibility_label("Copy file path")
                    .on_click(move |_, window, cx| {
                        let path = copy_path.clone();
                        copy_feedback::copy(window, cx, Some(copy_mark.clone()), "file path", path)
                    })
            }))
            .children(
                self.selected_file
                    .filter(|_| {
                        self.mode != WorkspaceMode::Working && !self.file_history.is_active()
                    })
                    .map(|index| {
                        button("file-history", "", "clock", false)
                            .accessibility_label("File history")
                            .tooltip("File history · follow renames along the first parent")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_file_history(index, window, cx)
                            }))
                    }),
            );
        let blame_index = if self.mode == WorkspaceMode::Working {
            self.working_selected.map(|(index, _)| index)
        } else {
            self.selected_file
        };
        if let Some(index) =
            blame_index.filter(|_| matches!(self.content.as_deref(), Some(Content::Text { .. })))
        {
            const DETAIL: &str =
                "Line attribution and history; working files include uncommitted lines";
            // Blame keeps its label where the whole path fits beside it;
            // otherwise its icon, so the path is not cut for a label.
            let labelled = file.is_none_or(|file| {
                self.path_fits_beside_labelled_blame(
                    &file.path().display().to_string(),
                    column,
                    column_min,
                    window,
                )
            });
            let blame = if labelled {
                button("open-blame", "Blame", "", false).tooltip(DETAIL)
            } else {
                compact_control_name(
                    button("open-blame", "", "user", false),
                    "open-blame",
                    "Blame",
                )
                .tooltip(format!("Blame · {DETAIL}"))
            };
            path_controls =
                path_controls.child(blame.debug_selector(|| "open-blame".into()).on_click(
                    cx.listener(move |this, _, window, cx| this.open_blame(index, window, cx)),
                ));
        }
        // Keep the path and its actions together. At narrow widths or larger
        // interface sizes, modes form another row instead of crushing the path.
        let mut toolbar = div()
            .debug_selector(|| "compare-toolbar".into())
            .min_h(appearance::ui_size(42.))
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1p5()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(rgb(colors.border))
            .child(path_controls);
        let content = if let Some(error) = &self.error {
            empty("Preview unavailable", error)
        } else if file.is_none()
            && self.mode == WorkspaceMode::Working
            && self.loading.is_none()
            && self
                .work_status
                .as_ref()
                .is_some_and(|status| status.entries.is_empty())
        {
            empty(
                "Working tree clean",
                "Add or edit files in your project, then review and stage them here.",
            )
        } else if file.is_none() {
            empty(self.loading.unwrap_or("Choose a file"), CHOOSE_A_FILE)
        } else if let Some(content) = &self.content {
            match content.as_ref() {
                Content::Text {
                    patch,
                    diagrams,
                    markdown,
                    ..
                } => {
                    let available = [
                        (TextMode::Unified, "Diff"),
                        (TextMode::Split, "Split"),
                        (TextMode::Before, "Before"),
                        (
                            TextMode::After,
                            if quick_source { "Source" } else { "After" },
                        ),
                        (TextMode::Diagrams, "Diagrams"),
                        (TextMode::Markdown, "Rendered"),
                    ]
                    .into_iter()
                    .filter(|&(mode, _)| {
                        !((quick_source
                            && !matches!(
                                mode,
                                TextMode::After | TextMode::Diagrams | TextMode::Markdown
                            ))
                            || (mode == TextMode::Diagrams && diagrams.is_none())
                            || (mode == TextMode::Markdown && markdown.is_none()))
                    })
                    .collect::<Vec<_>>();
                    let names = available.iter().map(|(_, name)| *name).collect::<Vec<_>>();
                    // Narrower than the labelled group, the modes keep one
                    // row as a menu named by the current mode, as the review
                    // options keep theirs.
                    let collapsed =
                        column - window.rem_size() * 1.5 < text_modes_width(&names, window);
                    let mut modes = div()
                        .flex()
                        .flex_wrap()
                        .min_w_0()
                        .max_w(relative(1.))
                        .items_center()
                        .gap_0p5()
                        .p_0p5()
                        .rounded(appearance::ui_size(9.))
                        .bg(rgb(colors.subtle));
                    for &(mode, name) in available.iter().filter(|_| !collapsed) {
                        modes = modes.child(
                            button(name, name, "", self.text_mode == mode)
                                .debug_selector(move || name.into())
                                .toggled(self.text_mode == mode)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.text_mode = mode;
                                    this.ensure_editor(window, cx);
                                    cx.notify();
                                })),
                        );
                    }
                    toolbar = if collapsed {
                        let current = self.text_mode;
                        let label = available
                            .iter()
                            .find(|(mode, _)| *mode == current)
                            .map_or("Diff", |(_, name)| *name);
                        let owner = cx.entity().downgrade();
                        toolbar.child(
                            compact_control_label(
                                button("text-mode-menu", label, "", false),
                                "text-mode-menu",
                                format!("Comparison mode: {label}"),
                            )
                            .debug_selector(|| "text-mode-menu".into())
                            .dropdown_caret(true)
                            .tooltip(format!("Comparison mode · {}", names.join(", ")))
                            .dropdown_menu(move |menu, _, _| {
                                // Built when opened; a choice acts on the page
                                // as it is then, as a mode button does.
                                available.iter().fold(menu, |menu, &(mode, name)| {
                                    let owner = owner.clone();
                                    menu.item(
                                        PopupMenuItem::new(name).checked(mode == current).on_click(
                                            move |_, window, cx| {
                                                let _ = owner.update(cx, |this, cx| {
                                                    if this.page == AppPage::Repository {
                                                        this.text_mode = mode;
                                                        this.ensure_editor(window, cx);
                                                        cx.notify();
                                                    }
                                                });
                                            },
                                        ),
                                    )
                                })
                            }),
                        )
                    } else {
                        toolbar.child(modes)
                    };
                    let editor = match self.text_mode {
                        TextMode::Split | TextMode::Diagrams | TextMode::Markdown => &None,
                        TextMode::Unified => &self.patch_editor,
                        TextMode::Before => &self.before_editor,
                        TextMode::After => &self.after_editor,
                    };
                    if self.text_mode == TextMode::Markdown {
                        self.markdown_view.as_ref().map_or_else(
                            || empty("Loading rendered Markdown…", ""),
                            |view| div().size_full().child(view.clone()).into_any_element(),
                        )
                    } else if self.text_mode == TextMode::Diagrams {
                        diagrams.as_ref().map_or_else(
                            || {
                                empty(
                                    "No Mermaid diagrams",
                                    "Use the source tabs to inspect this file.",
                                )
                            },
                            |preview| self.render_rich_preview(preview, cx),
                        )
                    } else if self.text_mode == TextMode::Split {
                        self.split_view.as_ref().map_or_else(
                            || empty("Loading split comparison…", ""),
                            |view| div().size_full().child(view.clone()).into_any_element(),
                        )
                    } else if self.text_mode == TextMode::Unified && patch.is_empty() {
                        if self.review.options.hide_whitespace {
                            empty(
                                "No visible changes",
                                "Whitespace-only differences are hidden. Source tabs retain the exact content.",
                            )
                        } else {
                            empty("Content unchanged", "Only the file mode or path changed.")
                        }
                    } else if self.text_mode == TextMode::Unified
                        && let Some(view) = &self.patch_view
                    {
                        div().size_full().child(view.clone()).into_any_element()
                    } else if let Some(editor) = editor {
                        crate::editor_find::Editor::new(editor)
                            .h(relative(1.))
                            .readonly(true)
                            .bordered(false)
                            .aria_label(if quick_source {
                                "Read-only tracked source file"
                            } else {
                                "Read-only file comparison"
                            })
                            .text_size(crate::appearance::code_text())
                            .into_any_element()
                    } else {
                        empty("Loading text…", "")
                    }
                }
                Content::Images { old, new } => self.render_image_comparison(old, new, cx),
                Content::Rich(preview) => self.render_rich_preview(preview, cx),
                Content::Conflict(_) => self.conflict_view.as_ref().map_or_else(
                    || empty("Loading conflict…", ""),
                    |view| div().size_full().child(view.clone()).into_any_element(),
                ),
                Content::Notice(message) => empty("File information", message),
            }
        } else {
            empty(self.loading.unwrap_or("No preview"), "")
        };
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.canvas))
            .child(toolbar)
            .child(self.render_lfs_download_actions(cx))
            .when(
                matches!(self.content.as_deref(), Some(Content::Text { .. }))
                    && !self.blame.is_visible()
                    && !matches!(self.text_mode, TextMode::Diagrams | TextMode::Markdown)
                    && !quick_source,
                |el| el.child(self.render_text_review(column, window, cx)),
            )
            .child(
                div()
                    .debug_selector(|| "preview-content".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(content),
            )
            .children(
                self.content
                    .as_ref()
                    .and_then(|content| match content.as_ref() {
                        Content::Text {
                            partial_unavailable,
                            ..
                        } if self.mode == WorkspaceMode::Working => partial_unavailable.as_ref(),
                        _ => None,
                    })
                    .map(|reason| {
                        div()
                            .px_3()
                            .py_2()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(reason.clone())
                    }),
            )
            .children(file.map(|file| {
                let (line, short) = if quick_source {
                    let line = format!("Tracked source · mode {}", file.new_mode);
                    (line.clone(), line)
                } else {
                    let modes = format!(
                        "{}   ·   {} → {}",
                        file.status.label(),
                        if file.old_mode == "000000" {
                            "absent"
                        } else {
                            &file.old_mode
                        },
                        if file.new_mode == "000000" {
                            "absent"
                        } else {
                            &file.new_mode
                        },
                    );
                    (
                        format!(
                            "{modes}   ·   {} → {}",
                            file.old_oid.as_deref().map(short_oid).unwrap_or("—".into()),
                            file.new_oid.as_deref().map(short_oid).unwrap_or("—".into())
                        ),
                        modes,
                    )
                };
                // Narrower than its line, the footer drops the object IDs
                // first and then truncates with an ellipsis; its tooltip
                // keeps the whole line.
                let fits = cached_text_width(&FOOTER_LINE, &line, window, || {
                    line_widths(&[(&line, appearance::ui_text(10.))], window)
                }) <= column - window.rem_size() * 1.5;
                let shown = if fits { line.clone() } else { short };
                div()
                    .id("compare-footer")
                    .debug_selector(|| "compare-footer".into())
                    .h(crate::appearance::ui_size(24.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .border_t_1()
                    .border_color(rgb(colors.border))
                    .when(!fits, |footer| {
                        footer
                            .tooltip(move |window, cx| Tooltip::new(line.clone()).build(window, cx))
                    })
                    .child(
                        div()
                            .debug_selector(|| "compare-footer-text".into())
                            .min_w_0()
                            .truncate()
                            .child(shown),
                    )
            }))
            .into_any_element()
    }
}

// The column and inspector minimums below are those of the default interface
// text size; each is drawn scaled with the interface size (`appearance::ui_size`),
// as the rail and the controls they hold are.
/// Navigation and the history table (`WIDE_CONTENT_MIN`) beside the narrowest
/// inspector (`INSPECTOR_MIN`): below this repository width, History shows
/// the rail.
const SIDEBAR_FIT_WIDTH: f32 = 800.;
const WIDE_CONTENT_MIN: f32 = 520.;
/// The narrowest content column beside the rail in a narrow window.
const NARROW_CONTENT_MIN: f32 = 240.;
/// The narrowest inspector while the rail and the narrow content column fit.
const INSPECTOR_MIN: f32 = 280.;
/// Where even those do not fit, as in a half tile of a display about
/// 1,000 px wide at a fractional scale, the inspector yields to this width
/// first and then the content column to `TIGHT_CONTENT_MIN`, so the
/// inspector stays whole down to about 460 px at the default size. The
/// column keeps that minimum, which holds History's compact scope toolbar,
/// and in a window narrower still, or at a larger interface size, the
/// inspector takes exactly the width the column leaves it.
const TIGHT_INSPECTOR_MIN: f32 = 240.;
const TIGHT_CONTENT_MIN: f32 = 176.;
/// The content column's minimum width, which includes the rail beside it in a
/// narrow window, and the inspector's, for a repository page `width` wide.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ColumnMinimums {
    content: Pixels,
    inspector: Pixels,
    /// Whether the inspector is exactly `inspector` wide: the column at its
    /// tight minimum leaves it less than its own, so it takes what is left
    /// and cannot be resized.
    pinned: bool,
}

impl ColumnMinimums {
    fn new(width: Pixels, rail: Pixels, narrow: bool) -> Self {
        let scaled = appearance::ui_size;
        if !narrow {
            return Self {
                content: scaled(WIDE_CONTENT_MIN),
                inspector: scaled(INSPECTOR_MIN),
                pinned: false,
            };
        }
        let beside_rail = width - rail;
        let inspector = (beside_rail - scaled(NARROW_CONTENT_MIN))
            .max(scaled(TIGHT_INSPECTOR_MIN))
            .min(scaled(INSPECTOR_MIN));
        let content = (beside_rail - inspector)
            .max(scaled(TIGHT_CONTENT_MIN))
            .min(scaled(NARROW_CONTENT_MIN));
        let left = (beside_rail - content).max(px(0.));
        Self {
            content: rail + content,
            inspector: inspector.min(left),
            pinned: left < inspector,
        }
    }
}

/// What Copy Selected List Item takes from a focused list's selected row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ListCopy {
    /// History: the full commit hash.
    CommitHash,
    /// Changed files: the repository-relative path, as Copy file path writes it.
    FilePath,
    /// Working Changes: each selected file's repository-relative path, once,
    /// one per line in list order.
    WorkingPaths,
    /// File history: the selected revision's full commit hash.
    RevisionHash,
}

impl GitTurtle {
    /// Copy Selected List Item. A non-empty text selection wins, so text
    /// selected in the message still copies after focus moves to a list: the
    /// message's own selection byte for byte, any other through the toolkit
    /// root. Otherwise the selected row's identity is copied.
    pub(super) fn copy_list_item(
        &self,
        item: ListCopy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inspector_message.copy_selection(cx) {
            return;
        }
        if !gpui_kit::base::TextSelection::selected_text(window, cx)
            .trim()
            .is_empty()
        {
            cx.propagate();
            return;
        }
        let text = match item {
            ListCopy::CommitHash => self
                .selected_commit
                .and_then(|index| self.commits.get(index))
                .map(|commit| commit.oid.clone()),
            ListCopy::FilePath => self
                .selected_file
                .and_then(|index| self.files.get(index))
                .map(|file| file.path().to_string_lossy().into_owned()),
            ListCopy::WorkingPaths => self.working_copy_paths(),
            ListCopy::RevisionHash => self
                .file_history_entry()
                .map(|entry| entry.commit.oid.clone()),
        };
        if let Some(text) = text {
            // Announced, with no button to mark.
            let what = match item {
                ListCopy::CommitHash | ListCopy::RevisionHash => "commit hash",
                ListCopy::WorkingPaths if text.contains('\n') => "file paths",
                ListCopy::FilePath | ListCopy::WorkingPaths => "file path",
            };
            copy_feedback::copy(window, cx, None, what, text);
        }
    }
}

/// How the inspector's "Changed files" header shares its row. The count comes
/// first, drawn whole or not at all, and the label truncates beside it only
/// where at least its first character and ellipsis fit, so as the inspector
/// narrows the label leaves, then the count, and neither shows a clipped
/// count or a lone ellipsis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FilesHeader {
    label: bool,
    count: bool,
    /// Whether the file rows below show their status words.
    status: bool,
}

impl FilesHeader {
    /// The header of an inspector at least its tight minimum wide, where the
    /// label and count fit whole at every interface size.
    pub(super) const WHOLE: Self = Self {
        label: true,
        count: true,
        status: true,
    };

    /// The header of an inspector the window pins `width` wide, holding
    /// `count` changed files, measured from the interface font at the
    /// interface size and the header's medium weight: the inspector's 1 px
    /// left border and the file list's 1 px right border, the row's `px_3` on
    /// either side and its 15 px icon, then the count, its text between
    /// `px_1p5` on either side, and the label, each `gap_2` after what
    /// precedes it.
    fn within(width: Pixels, count: &str, window: &Window) -> Self {
        let rem = window.rem_size();
        let gap = rem * 0.5;
        let room = width - px(2.) - rem * 1.5 - px(15.);
        let count = gap
            + rem * 0.75
            + cached_text_width(&FILES_COUNT, count, window, || {
                shaped_width(count, appearance::ui_text(10.), FontWeight::MEDIUM, window).ceil()
            });
        let shows_count = count <= room;
        // A file row has the same borders and inset, its 26 px status tile
        // and then the name, `gap_2` after it. The status word, between
        // `px_1` on either side and `gap_2` after the name, stays where the
        // name keeps at least the widest word's width beside it.
        let word = rem * 0.25 * 2.
            + cached_width(&STATUS_WORDS, &LineKey::current(window), || {
                ["New", "Deleted", "Renamed", "Type", "Modified"]
                    .into_iter()
                    .map(|word| line_widths(&[(word, appearance::ui_text(10.))], window))
                    .fold(px(0.), Pixels::max)
            });
        let name = width - px(2.) - rem * 1.5 - px(26.) - gap;
        Self {
            label: shows_count && room - count - gap >= files_label_min_width(window),
            count: shows_count,
            status: name - gap - word >= word,
        }
    }
}

/// The narrowest "Changed files" is drawn in without a lone ellipsis: cut to
/// its first character and the ellipsis, measured as `scope_name_min_width`
/// measures a cut scope name.
fn files_label_min_width(window: &Window) -> Pixels {
    cached_width(&FILES_LABEL, &LineKey::current(window), || {
        shaped_width("C…", appearance::ui_text(11.), FontWeight::MEDIUM, window).floor() + px(1.)
    })
}

/// Below this window width, at the interface text size, the header drops the
/// Projects and profile labels so it stays on one row.
const COMPACT_HEADER_WIDTH: f32 = 800.;
/// Pull requests' label in the header's compact row.
const COMPACT_PULL_REQUESTS: &str = "PRs";

/// How the repository header and the Git action bar are drawn in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Chrome {
    /// Their full forms, each wrapping onto further rows as the width
    /// requires. `narrow`, below the scaled `COMPACT_HEADER_WIDTH`, drops the
    /// header's Projects and profile labels.
    Full { narrow: bool },
    /// One row each of 28 px controls, where the full forms would leave
    /// History less than its toolbars and three rows, or Compare less than
    /// its toolbars and three diff lines (`GitTurtle::chrome`). `app_icon` is
    /// whether the turtle fits beside the repository name, and `name` the
    /// most the name takes, the same in every mode.
    Compact { app_icon: bool, name: Pixels },
}

impl Chrome {
    pub(super) fn is_compact(self) -> bool {
        matches!(self, Self::Compact { .. })
    }
}

/// The text the full chrome lays out, other than its fixed labels.
#[derive(Clone, Default, PartialEq)]
struct ChromeText {
    changes: String,
    profile: String,
    branch: String,
    ahead: String,
    behind: String,
    operation: String,
}

/// The widths `ChromeText` and the chrome's fixed labels take, each as its
/// control lays it out: at its size and weight, rounded up to a whole pixel.
#[derive(Clone, Copy, Default)]
struct ChromeWidths {
    projects: Pixels,
    history: Pixels,
    changes: Pixels,
    pull_requests: Pixels,
    compact_pull_requests: Pixels,
    profile: Pixels,
    branch: Pixels,
    ahead: Pixels,
    behind: Pixels,
    no_upstream: Pixels,
    operation: Pixels,
    fetch: Pixels,
    pull: Pixels,
    push: Pixels,
    targets: Pixels,
}

impl ChromeWidths {
    fn measure(text: &ChromeText, window: &Window) -> Self {
        let regular = |text: &str, size: Pixels| line_widths(&[(text, size)], window);
        let medium = |text: &str| {
            shaped_width(text, appearance::ui_text(12.), FontWeight::MEDIUM, window).ceil()
        };
        let label = appearance::ui_text(12.);
        let small = appearance::ui_text(11.);
        Self {
            projects: regular("Projects", label),
            history: regular("History", label),
            changes: regular(&text.changes, label),
            pull_requests: regular("Pull requests", label),
            compact_pull_requests: regular(COMPACT_PULL_REQUESTS, label),
            // A medium kit Button's label is the rem size.
            profile: regular(&text.profile, window.rem_size()),
            branch: medium(&text.branch),
            ahead: regular(&text.ahead, small),
            behind: regular(&text.behind, small),
            no_upstream: regular("No upstream", small),
            operation: regular(&text.operation, small),
            fetch: medium("Fetch"),
            pull: medium("Pull"),
            push: medium("Push"),
            targets: regular("Targets", label),
        }
    }
}

thread_local! {
    static CHROME_WIDTHS: RefCell<Option<(LineKey, ChromeText, ChromeWidths)>> =
        const { RefCell::new(None) };
}

/// The height a wrapping flex row of `items`, each its width and height,
/// takes in `room`, `gap` apart on both axes. Lines break as taffy breaks
/// them, before the item that takes the line's length, gaps included, past
/// `room`, and each line is as tall as its tallest item.
fn wrapped_height(items: &[(Pixels, Pixels)], room: Pixels, gap: Pixels) -> Pixels {
    let mut height = px(0.);
    let (mut length, mut tallest) = (px(0.), px(0.));
    for (index, &(width, item)) in items.iter().enumerate() {
        if index > 0 && length + gap + width > room {
            height += tallest + gap;
            (length, tallest) = (width, item);
        } else {
            length += if index > 0 { gap } else { px(0.) } + width;
            tallest = tallest.max(item);
        }
    }
    height + tallest
}

/// The window's size inside a client-decorated frame: the toolkit's
/// `window_paddings` and its 1 px border on each untiled side, as the theme
/// editor measures it. Server decorations have neither.
fn content_size(window: &Window) -> Size<Pixels> {
    let viewport = window.viewport_size();
    let Decorations::Client { tiling } = window.window_decorations() else {
        return viewport;
    };
    let paddings = gpui_kit::component::window_paddings(window);
    let border = |tiled: bool| if tiled { px(0.) } else { px(1.) };
    size(
        viewport.width
            - paddings.left
            - paddings.right
            - border(tiling.left)
            - border(tiling.right),
        viewport.height
            - paddings.top
            - paddings.bottom
            - border(tiling.top)
            - border(tiling.bottom),
    )
}

impl GitTurtle {
    /// The chrome a Repository frame draws, decided before layout so no
    /// frame shows the other form first. The full forms' heights are
    /// predicted for this window: each control's width from the interface
    /// font at the interface size and its fixed padding, icon and gaps, the
    /// rows its header and action bar wrap to, and each row's tallest
    /// control, every length snapped to device pixels as layout snaps it.
    /// Where the window less the tab strip, the status bar and those
    /// heights leaves History less than `history_minimum`, the chrome is
    /// compact. Notices, banners, a running operation's progress and an
    /// expanded Targets take their room from the content in either form and
    /// do not decide it, so opening Targets never changes the form of the
    /// control that opened it.
    pub(super) fn chrome(&self, window: &Window, cx: &App) -> Chrome {
        let viewport = window.viewport_size();
        let narrow = viewport.width < appearance::ui_size(COMPACT_HEADER_WIDTH);
        if self.page != AppPage::Repository {
            return Chrome::Full { narrow };
        }
        let content = content_size(window);
        let widths = self.chrome_widths(window);
        let (header, actions) = self.full_chrome_heights(&widths, window);
        let full = header + actions;
        let tabs = window.pixel_snap(repository_tabs::tab_strip_min_height(
            appearance::button_ring_room(cx),
        ));
        let status = window.pixel_snap(appearance::ui_size(26.));
        let need = self
            .history_minimum(window)
            .max(self.compare_minimum(window));
        if content.height - tabs - status - full >= need {
            return Chrome::Full { narrow };
        }
        // The turtle shows where the name keeps the full header's minimum
        // beside it and the compact row's other controls. Projects counts in
        // every mode, though History's root leaves it out, so the turtle,
        // the name and the toggles after it keep their places across modes.
        let rem = window.rem_size();
        let control = window.pixel_snap(appearance::ui_size(28.));
        let gap = window.pixel_snap(rem * 0.5);
        let (mut fixed, mut items) = (control * 4., 5.);
        if self.repository.is_some() {
            fixed += control * 2.
                + window.pixel_snap(rem * 0.125)
                + self.labelled_width(widths.compact_pull_requests, false, window);
            items += 2.;
        }
        let name = content.width - window.pixel_snap(rem * 0.75) * 2. - fixed - gap * (items - 1.);
        let app_icon = name - control - gap >= window.pixel_snap(appearance::ui_size(100.));
        Chrome::Compact {
            app_icon,
            name: (if app_icon { name - control - gap } else { name }).max(px(0.)),
        }
    }

    /// The full header's and action bar's heights in this window: the header
    /// spans the window, the action bar the repository page beside the
    /// project pane.
    fn full_chrome_heights(&self, widths: &ChromeWidths, window: &Window) -> (Pixels, Pixels) {
        let content = content_size(window);
        let narrow = window.viewport_size().width < appearance::ui_size(COMPACT_HEADER_WIDTH);
        // Beside the pane the page keeps its 520 px minimum (`render`).
        let page = if self.settings.project_pane {
            (content.width - self.project_pane_width(window)).max(px(520.))
        } else {
            content.width
        };
        (
            self.full_header_height(content.width, narrow, widths, window),
            self.full_actions_height(page, widths, window),
        )
    }

    /// What History needs below the chrome to show three whole rows: its
    /// scope toolbar, search row, the horizontal scrollbar's reserved track,
    /// the column header, the list's border and three rows.
    pub(super) fn history_minimum(&self, window: &Window) -> Pixels {
        let scaled = |base: f32| window.pixel_snap(appearance::ui_size(base));
        scaled(40.)
            + scaled(38.)
            + Scrollbar::width()
            + scaled(34.)
            + window.pixel_snap(px(1.)) * 2.
            + window.pixel_snap(px(self.settings.density.history_row_height())) * 3.
    }

    /// Whether the comparison path, `path`, fits whole on its row beside its
    /// actions with Blame labelled, in a preview `column` wide whose
    /// narrowest is `column_min`: the row the toolbar gives the path (all of
    /// it where the modes wrap below), less a Working file's area badge, Copy
    /// path, File history and the labelled Blame, `gap_1` apart.
    fn path_fits_beside_labelled_blame(
        &self,
        path: &str,
        column: Pixels,
        column_min: Pixels,
        window: &Window,
    ) -> bool {
        let rem = window.rem_size();
        let room = column - rem * 1.5;
        let control = appearance::ui_size(28.);
        let allotment =
            appearance::ui_size(220.).min((column_min - appearance::ui_size(19.5)).max(px(0.)));
        let modes = text_modes_width(&["Diff", "Split", "Before", "After"], window);
        let row = if modes <= room && allotment + rem * 0.375 + modes <= room {
            room - rem * 0.375 - modes
        } else {
            room
        };
        let working = self.mode == WorkspaceMode::Working;
        let badge = self
            .working_selected
            .filter(|_| working)
            .map_or(px(0.), |(_, area)| {
                let word = if area == gitturtle_core::ChangeArea::Staged {
                    "Staged"
                } else {
                    "Unstaged"
                };
                rem * 0.5 * 2.
                    + cached_text_width(&AREA_BADGE, word, window, || {
                        line_widths(&[(word, appearance::ui_text(10.))], window)
                    })
                    + rem * 0.25
            });
        let history = if working || self.file_history.is_active() {
            px(0.)
        } else {
            control + rem * 0.25
        };
        let blame = cached_width(&BLAME_LABEL, &LineKey::current(window), || {
            line_widths(&[("Blame", appearance::ui_text(12.))], window)
        }) + appearance::ui_size(20.);
        let needed = cached_text_width(&BLAME_PATH, path, window, || {
            shaped_width(path, appearance::ui_text(12.), FontWeight::MEDIUM, window).ceil()
        });
        row - badge - (control + rem * 0.25) - history - (blame + rem * 0.25) >= needed
    }

    /// What Compare needs below the chrome to show three whole diff lines of
    /// a text comparison: its toolbar's rows in the column it would have
    /// beside the rail and the inspector, the review options' row, the
    /// editor's 8 px padding above and below its text, three lines at the
    /// code size, and the footer. Its text widths are cached as the chrome's
    /// are, so History's frames ask the text system nothing for it either.
    pub(super) fn compare_minimum(&self, window: &Window) -> Pixels {
        let width = self.repository_width(window);
        let narrow = width < appearance::ui_size(SIDEBAR_FIT_WIDTH);
        let rail = appearance::ui_size(44.);
        let columns = ColumnMinimums::new(width, rail, narrow);
        let column = width - rail - self.expected_inspector_width(width, &columns);
        let rem = window.rem_size();
        let options = rem * 0.25 * 2. + rem * 1.5 + px(1.);
        let line = window.pixel_snap(appearance::code_text() * 1.5);
        compare_toolbar_height(column, columns.content - rail, window)
            + options
            + px(8.) * 2.
            + line * 3.
            + window.pixel_snap(appearance::ui_size(24.))
    }

    /// `ChromeWidths` for the chrome's current text, measured again only when
    /// the text, the font or the sizes change, so a steady render asks the
    /// text system nothing.
    fn chrome_widths(&self, window: &Window) -> ChromeWidths {
        let status = self.work_status.as_ref();
        let text = ChromeText {
            changes: self.changes_label(),
            profile: self.profile_title(),
            branch: status
                .and_then(|status| status.branch.clone())
                .unwrap_or("Detached HEAD".into()),
            ahead: status.map_or_else(String::new, |s| format!("{} ahead", s.ahead)),
            behind: status.map_or_else(String::new, |s| format!("{} behind", s.behind)),
            operation: status
                .and_then(|s| s.operation.as_ref())
                .map_or_else(String::new, |operation| format!("{operation} in progress")),
        };
        let key = LineKey::current(window);
        CHROME_WIDTHS.with(|cache| {
            let mut cache = cache.borrow_mut();
            match &*cache {
                Some((cached, cached_text, widths)) if *cached == key && *cached_text == text => {
                    *widths
                }
                _ => {
                    let widths = ChromeWidths::measure(&text, window);
                    *cache = Some((key, text, widths));
                    widths
                }
            }
        })
    }

    /// The shared `button` helper's width around a `label` this wide: its
    /// padding, and an icon and gap before the label where it has `icon`.
    fn labelled_width(&self, label: Pixels, icon: bool, window: &Window) -> Pixels {
        let scaled = |base: f32| window.pixel_snap(appearance::ui_size(base));
        (scaled(10.) * 2.
            + label
            + if icon {
                scaled(16.) + scaled(6.)
            } else {
                px(0.)
            })
        .max(scaled(28.))
    }

    /// The full header's height in a window `width` wide: `render_header`'s
    /// controls in order, wrapped as layout wraps them.
    fn full_header_height(
        &self,
        width: Pixels,
        narrow: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> Pixels {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let scaled = |base: f32| snap(appearance::ui_size(base));
        let line = |size: Pixels| snap(window.text_style().line_height.to_pixels(size.into(), rem));
        let control = scaled(28.);
        let back_to_projects = self.back_label() == "Back to Projects";
        let back = if back_to_projects && !narrow {
            self.labelled_width(widths.projects, true, window)
        } else {
            control
        };
        let mut items = vec![
            (back, control),
            (control, control),
            // The identity takes its minimum until the row's lines are set,
            // above its path.
            (
                scaled(100.),
                line(appearance::ui_text(14.)) + snap(rem * 0.125) + line(appearance::ui_text(11.)),
            ),
        ];
        if self.repository.is_some() {
            items.push((
                snap(rem * 0.125) * 3.
                    + self.labelled_width(widths.history, false, window)
                    + self.labelled_width(widths.changes, false, window),
                control + snap(rem * 0.125) * 2.,
            ));
            items.push((
                self.labelled_width(widths.pull_requests, false, window),
                control,
            ));
        }
        items.push((px(0.), px(0.)));
        if !back_to_projects {
            items.push((control, control));
        }
        // A medium kit Button: `px_3`, the 16 px icon, the label and the
        // rem-sized caret, `gap_2` apart, within its 230 px limit.
        let profile = if narrow {
            scaled(36.)
        } else {
            (snap(rem * 0.75) * 2. + px(16.) + snap(rem * 0.5) * 2. + widths.profile + snap(rem))
                .min(snap(px(230.)))
        };
        items.push((profile, scaled(36.)));
        items.push((control, control));
        let rows = wrapped_height(&items, width - snap(rem * 0.75) * 2., snap(rem * 0.5));
        (rows + snap(rem * 0.375) * 2. + snap(px(1.))).max(scaled(48.))
    }

    /// The full action bar's height, Targets collapsed, in a repository page
    /// `width` wide: `render_git_actions`' controls in order, wrapped as
    /// layout wraps them.
    fn full_actions_height(&self, width: Pixels, widths: &ChromeWidths, window: &Window) -> Pixels {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let scaled = |base: f32| snap(appearance::ui_size(base));
        let line = |size: Pixels| snap(window.text_style().line_height.to_pixels(size.into(), rem));
        let action = scaled(34.);
        let text = line(appearance::ui_text(11.));
        // Medium kit Buttons: `px_3`, a 16 px icon and the label `gap_2`
        // apart, the branch picker's caret after it, within its 280 px limit.
        let mut items = vec![(
            (snap(rem * 0.75) * 2. + px(16.) + snap(rem * 0.5) * 2. + widths.branch + snap(rem))
                .min(snap(px(280.))),
            action,
        )];
        if let Some(status) = &self.work_status {
            let inset = snap(rem * 0.5) * 2.;
            items.push(if status.upstream.is_some() {
                let count = px(12.) + snap(rem * 0.25);
                (
                    inset + count * 2. + widths.ahead + snap(rem * 0.5) + widths.behind,
                    text.max(px(12.)),
                )
            } else {
                (inset + widths.no_upstream, text)
            });
            if status.operation.is_some() {
                items.push((inset + widths.operation, snap(rem * 0.25) * 2. + text));
            }
        }
        items.push((px(0.), px(0.)));
        for label in [widths.fetch, widths.pull, widths.push] {
            items.push((
                snap(rem * 0.75) * 2. + scaled(16.) + snap(rem * 0.5) + label,
                action,
            ));
        }
        items.push((
            self.labelled_width(widths.targets, true, window),
            scaled(28.),
        ));
        let rows = wrapped_height(&items, width - snap(rem * 0.75) * 2., snap(rem * 0.5));
        (rows + snap(rem * 0.25) * 2.).max(scaled(46.)) + snap(px(1.))
    }
}
/// The labelled text-mode group's width for `names`: its `p_0p5` either
/// side, each the shared `button` helper's 10 px of padding either side of
/// its label, `gap_0p5` apart. Laid out again only when the names, the font
/// or the sizes change.
fn text_modes_width(names: &[&str], window: &Window) -> Pixels {
    let rem = window.rem_size();
    let labels = cached_text_width(&TEXT_MODES, &names.join("\n"), window, || {
        line_widths(
            &names
                .iter()
                .map(|name| (*name, appearance::ui_text(12.)))
                .collect::<Vec<_>>(),
            window,
        )
    });
    rem * 0.125 * 2.
        + labels
        + appearance::ui_size(20.) * names.len() as f32
        + rem * 0.125 * names.len().saturating_sub(1) as f32
}

/// Compare's toolbar height in a preview `column` wide whose narrowest is
/// `column_min`: the path's row, held at its 220 px allotment or the whole
/// narrowest column, then the text modes, labelled where they fit a row and
/// otherwise their one-control menu, on that row or the next, between
/// `py_1` and the bottom rule, at least its 42 px.
fn compare_toolbar_height(column: Pixels, column_min: Pixels, window: &Window) -> Pixels {
    const MODES: [&str; 4] = ["Diff", "Split", "Before", "After"];
    let rem = window.rem_size();
    let room = column - rem * 1.5;
    let control = appearance::ui_size(28.);
    let path = appearance::ui_size(220.).min((column_min - appearance::ui_size(19.5)).max(px(0.)));
    let labelled = text_modes_width(&MODES, window);
    let (modes, height) = if labelled <= room {
        (labelled, control + rem * 0.25)
    } else {
        // The menu reads the widest mode, then its caret.
        let widest = cached_width(&TEXT_MODE_WIDEST, &LineKey::current(window), || {
            line_widths(&[("Before", appearance::ui_text(12.))], window)
        });
        (
            appearance::ui_size(10. * 2. + 6.) + widest + rem * 0.875,
            control,
        )
    };
    let gap = rem * 0.375;
    let rows = if path + gap + modes <= room {
        control.max(height)
    } else {
        control + gap + height
    };
    (rem * 0.25 * 2. + rows + px(1.)).max(appearance::ui_size(42.))
}

/// The width History's scope toolbar needs to keep its labels: `px_3` on
/// either side, seven `gap_2` gaps, the fixed-size branch icon, the count, the
/// scope name at the narrowest it is drawn in, and the Columns, Latest,
/// Previous and Older buttons, each the shared `button` helper's 10 px of
/// padding on either side of its label, and Columns and Older a 16 px icon
/// 6 px before it. In a narrower column the toolbar keeps only icons.
fn labelled_scope_toolbar_width(count: &str, scope: &str, window: &Window) -> Pixels {
    let key = LineKey::current(window);
    let label = appearance::ui_text(12.);
    let labels = cached_width(&SCOPE_LABELS, &key, || {
        line_widths(
            &[
                ("Columns", label),
                ("Latest", label),
                ("Previous", label),
                ("Older", label),
            ],
            window,
        )
    });
    window.rem_size() * (2. * 0.75 + 7. * 0.5)
        + appearance::ui_size(4. * 20. + 2. * 22.)
        + px(15.)
        + labels
        + scope_count_width(count, window)
        + scope_name_min_width(scope, window)
}

/// The width the scope toolbar's `count` takes whole. The count changes with
/// the page and search; it is laid out again only then, and the labels only
/// when the font or the sizes change.
fn scope_count_width(count: &str, window: &Window) -> Pixels {
    cached_text_width(&SCOPE_COUNT, count, window, || {
        line_widths(&[(count, appearance::ui_text(10.))], window)
    })
}

/// What the compact scope toolbar leaves its scope name and count in a
/// `column`-wide History: the column less `px_3` on either side, three
/// `gap_2` gaps, the branch icon, and the 28 px Columns and paging buttons,
/// the paging buttons `gap_1` apart.
fn compact_scope_room(column: Pixels, window: &Window) -> Pixels {
    column
        - window.rem_size() * (2. * 0.75 + 3. * 0.5 + 2. * 0.25)
        - appearance::ui_size(4. * 28.)
        - px(15.)
}

/// The narrowest the scope name can be drawn in without a lone ellipsis, in
/// the inherited interface font and size at its medium weight: the whole name,
/// or cut to its first character and the ellipsis where that is narrower. A
/// truncated line keeps the characters that fit strictly before its ellipsis
/// and drops trailing whitespace and ASCII punctuation, so the cut runs to the
/// first character it keeps, and takes the next whole pixel past its width.
fn scope_name_min_width(scope: &str, window: &Window) -> Pixels {
    cached_text_width(&SCOPE_NAME, scope, window, || {
        let size = window.text_style().font_size.to_pixels(window.rem_size());
        let whole = shaped_width(scope, size, FontWeight::MEDIUM, window).ceil();
        scope
            .char_indices()
            .find(|(_, c)| !(c.is_whitespace() || c.is_ascii_punctuation()))
            .map_or(whole, |(start, c)| {
                let cut = format!("{}…", &scope[..start + c.len_utf8()]);
                (shaped_width(&cut, size, FontWeight::MEDIUM, window).floor() + px(1.)).min(whole)
            })
    })
}

/// What every History row of a frame shares (`GitTurtle::history_rows`).
pub(super) struct HistoryRows {
    columns: columns::ColumnLayout,
    /// The lane pitch the graph is drawn at, compact in narrow History.
    spacing: f32,
    lane_offset: usize,
    filtered: bool,
    lane_colors: [u32; 6],
    labels: appearance::Labels,
}

/// What a History column header cell needs for its whole `title` at the
/// header's 11 pt medium at the interface size: the title and the cell's
/// 10 px and `pr_3` insets. The title is measured from the interface font,
/// and again only when the font or the sizes change, so a steady render asks
/// the text system nothing.
fn header_cell_width(
    cache: &'static std::thread::LocalKey<RefCell<Option<(LineKey, Pixels)>>>,
    title: &str,
    window: &Window,
) -> Pixels {
    let title = cached_width(cache, &LineKey::current(window), || {
        shaped_width(title, appearance::ui_text(11.), FontWeight::MEDIUM, window).ceil()
    });
    px(10.) + window.rem_size() * 0.75 + title
}

/// Narrow History's subject floor: the cell the subject's column header
/// needs for its whole title, "Commit message", so neither the title nor a
/// row's subject is cut to nothing. About 113 px at the default size and
/// 152 px at 18 pt with the embedded font.
pub(super) fn subject_floor(window: &Window) -> Pixels {
    header_cell_width(&SUBJECT_TITLE, ColumnId::Subject.label(), window)
}

/// Whether narrow History's Graph title fits whole in its `width`-wide
/// header cell.
fn graph_title_fits(width: f32, window: &Window) -> bool {
    px(width) >= header_cell_width(&GRAPH_TITLE, ColumnId::Graph.label(), window)
}

/// What a line of interface text is laid out with besides its text: the
/// inherited font, and the interface and rem sizes its text size comes from.
#[derive(Clone, PartialEq)]
pub(crate) struct LineKey {
    font: Font,
    scale: u32,
    rem: u32,
}

impl LineKey {
    pub(crate) fn current(window: &Window) -> Self {
        Self {
            font: window.text_style().font(),
            scale: appearance::ui_scale().to_bits(),
            rem: f32::from(window.rem_size()).to_bits(),
        }
    }
}

/// One text's laid-out width, with the text and what it was laid out with.
pub(crate) type TextWidth = RefCell<Option<(LineKey, String, Pixels)>>;

thread_local! {
    static SCOPE_LABELS: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static SCOPE_COUNT: TextWidth = const { RefCell::new(None) };
    static SCOPE_NAME: TextWidth = const { RefCell::new(None) };
    static FILES_COUNT: TextWidth = const { RefCell::new(None) };
    static FILES_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static TEXT_MODES: TextWidth = const { RefCell::new(None) };
    static FOOTER_LINE: TextWidth = const { RefCell::new(None) };
    static BLAME_PATH: TextWidth = const { RefCell::new(None) };
    static AREA_BADGE: TextWidth = const { RefCell::new(None) };
    static STATUS_WORDS: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static BLAME_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static TEXT_MODE_WIDEST: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static SUBJECT_TITLE: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static GRAPH_TITLE: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
}
#[cfg(test)]
thread_local! {
    /// Lines [`line_widths`] has asked the text system to lay out.
    pub(crate) static LINE_LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Widths History has taken from its measured bounds, each of which
    /// notifies so its columns are laid out again.
    static HISTORY_WIDTHS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The accessible name each control of a compact layout was given since
    /// the map was last cleared, by debug selector
    /// (`compact_controls_carry_their_accessible_names`).
    pub(crate) static COMPACT_NAMES: RefCell<std::collections::BTreeMap<&'static str, &'static str>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// [`COMPACT_NAMES`] for the names a compact control is given from what
    /// it shows, such as the Changes count (`compact_control_label`).
    pub(crate) static COMPACT_LABELS: RefCell<std::collections::BTreeMap<&'static str, SharedString>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// Narrow History's reference pill as each row last drew it, by commit
    /// index.
    static HISTORY_PILLS: RefCell<std::collections::BTreeMap<usize, PillProbe>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// Each History row's accessible name and tooltip as it last drew them,
    /// by commit index.
    static HISTORY_ROW_NAMES: RefCell<std::collections::BTreeMap<usize, RowNameProbe>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
}
/// A drawn pill's name, its "+N" and its label colour.
#[cfg(test)]
type PillProbe = (String, Option<String>, u32);
/// A drawn History row's accessible name and tooltip.
#[cfg(test)]
type RowNameProbe = (String, Option<SharedString>);

/// The compact Changes toggle's working-file count, a small accent pill on
/// the icon's top-right corner inside the toggle, so the toggle keeps its
/// width and its focus ring. Above 99 it reads "99+".
fn changes_badge(count: usize, p: appearance::Palette) -> impl IntoElement {
    div()
        .debug_selector(|| "changes-count-badge".into())
        .absolute()
        .top(appearance::ui_size(1.))
        .right(appearance::ui_size(1.))
        .min_w(appearance::ui_size(14.))
        .h(appearance::ui_size(14.))
        .px(appearance::ui_size(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(p.accent))
        .text_color(rgb(p.accent_foreground))
        .text_size(appearance::ui_text(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(if count > 99 {
            "99+".to_owned()
        } else {
            count.to_string()
        })
}

/// [`compact_control_name`] for a `name` that changes with what the control
/// stands for, such as the Changes count or a Git action's target.
pub(crate) fn compact_control_label(
    control: Button,
    selector: &'static str,
    name: impl Into<SharedString>,
) -> Button {
    let name = name.into();
    #[cfg(test)]
    COMPACT_LABELS.with(|names| names.borrow_mut().insert(selector, name.clone()));
    #[cfg(not(test))]
    let _ = selector;
    control.accessibility_label(name)
}

/// `control` named `name` for assistive technology: a control that a compact
/// layout draws without its visible label. `selector` is its debug selector,
/// by which a test reads the name back.
pub(crate) fn compact_control_name(
    control: Button,
    selector: &'static str,
    name: &'static str,
) -> Button {
    #[cfg(test)]
    COMPACT_NAMES.with(|names| names.borrow_mut().insert(selector, name));
    #[cfg(not(test))]
    let _ = selector;
    control.accessibility_label(name)
}

/// `width` for `key`, computed again only when `key` changes, so a steady
/// render asks the text system nothing.
pub(crate) fn cached_width(
    cache: &'static std::thread::LocalKey<RefCell<Option<(LineKey, Pixels)>>>,
    key: &LineKey,
    width: impl FnOnce() -> Pixels,
) -> Pixels {
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((cached, width)) if cached == key => *width,
            _ => {
                let width = width();
                *cache = Some((key.clone(), width));
                width
            }
        }
    })
}

/// `width` of `text` for the current font and sizes, computed again only when
/// either changes.
pub(crate) fn cached_text_width(
    cache: &'static std::thread::LocalKey<TextWidth>,
    text: &str,
    window: &Window,
    width: impl FnOnce() -> Pixels,
) -> Pixels {
    let key = LineKey::current(window);
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((cached, cached_text, width)) if *cached == key && cached_text == text => *width,
            _ => {
                let width = width();
                *cache = Some((key, text.to_owned(), width));
                width
            }
        }
    })
}

/// The widths `texts` take as single lines in the inherited interface font,
/// summed: each shaped by the text system, kerning and fallback fonts
/// included, and rounded up to a whole pixel as a text element sizes it.
pub(crate) fn line_widths(texts: &[(&str, Pixels)], window: &Window) -> Pixels {
    let weight = window.text_style().font_weight;
    texts
        .iter()
        .map(|(text, size)| shaped_width(text, *size, weight, window).ceil())
        .sum()
}

/// One line of `text` as [`line_widths`] lays it out, at `weight`, unrounded.
pub(crate) fn shaped_width(
    text: &str,
    size: Pixels,
    weight: FontWeight,
    window: &Window,
) -> Pixels {
    let mut style = window.text_style();
    style.font_weight = weight;
    #[cfg(test)]
    LINE_LAYOUTS.with(|count| count.set(count.get() + 1));
    window
        .text_system()
        .layout_line(text, size, &[style.to_run(text.len())], None)
        .width
}

impl GitTurtle {
    /// A narrow window shrinks the project pane toward its minimum before the
    /// repository's own panes lose room; the saved width is untouched.
    fn project_pane_width(&self, window: &Window) -> Pixels {
        px(self.settings.project_pane_width)
            .min((window.viewport_size().width - px(800.)).max(px(180.)))
    }

    /// Whether History has room for its navigation beside the table and the
    /// narrowest inspector.
    pub(super) fn navigation_fits(&self, window: &Window) -> bool {
        self.repository_width(window) >= appearance::ui_size(SIDEBAR_FIT_WIDTH)
    }

    /// The shortcut, the menu bar and the command palette share this. Outside
    /// History it returns there. In History too narrow for navigation it keeps
    /// the saved choice, since flipping it would show nothing until the window
    /// widens, and the rail announces why; the rail button and the palette
    /// entry are disabled there.
    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != WorkspaceMode::History {
            self.back_to_history(window, cx);
        } else if self.navigation_fits(window) {
            self.sidebar = !self.sidebar;
            self.history_sidebar = self.sidebar;
            cx.notify();
        } else {
            self.navigation_refusals = self.navigation_refusals.wrapping_add(1);
            cx.notify();
        }
    }

    /// The width the repository page receives beside the project pane.
    /// The inspector's width in a repository `width` wide before it is laid
    /// out: exactly its minimum where the window pins it, otherwise its saved
    /// width within what the content column leaves it.
    fn expected_inspector_width(&self, width: Pixels, columns: &ColumnMinimums) -> Pixels {
        if columns.pinned {
            columns.inspector
        } else {
            px(self.settings.inspector_width)
                .min(px(480.).max(columns.inspector))
                .min(width - columns.content)
                .max(columns.inspector)
        }
    }

    fn repository_width(&self, window: &Window) -> Pixels {
        let width = window.viewport_size().width;
        if self.settings.project_pane && self.page != AppPage::Projects {
            width - self.project_pane_width(window)
        } else {
            width
        }
    }

    fn render_repository(
        &self,
        chrome: Chrome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Tiling window managers ignore the window's minimum size. Below the
        // width that fits navigation, the history table and the narrowest
        // inspector, History shows the rail and the content column accepts
        // less, so the inspector stays whole; saved widths and the user's
        // navigation choice are untouched.
        let width = self.repository_width(window);
        let narrow = width < appearance::ui_size(SIDEBAR_FIT_WIDTH);
        let rail = appearance::ui_size(44.);
        let columns = ColumnMinimums::new(width, rail, narrow);
        let (content_min, inspector_min) = (columns.content, columns.inspector);
        // Where the inspector is pinned, its message toolbar knows its width
        // and reserves the rows its copy controls take there.
        self.inspector_message.lay_out_toolbar(
            columns
                .pinned
                .then(|| commit_message::Toolbar::within(columns.inspector, window)),
        );
        // So does its "Changed files" header, which keeps its count whole.
        let files_header = if columns.pinned && self.mode != WorkspaceMode::Working {
            FilesHeader::within(columns.inspector, &self.files.len().to_string(), window)
        } else {
            FilesHeader::WHOLE
        };
        let left = if self.mode != WorkspaceMode::History {
            // The preview column as last laid out. Entering from History it is
            // the column History left: its own, beside the rail or beside
            // navigation. The state lives while this branch is drawn.
            let measured = window.use_keyed_state("preview-column-width", cx, |_, _| None);
            let column = measured.read(cx).unwrap_or_else(|| {
                if self.history_sidebar && !narrow {
                    px(self.settings.navigation_width + self.history_width) - rail
                } else {
                    px(self.history_width)
                }
            });
            div()
                .size_full()
                .flex()
                .min_w_0()
                .child(self.render_rail(narrow, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .on_prepaint(move |bounds, _, cx| {
                            measured.update(cx, |width, cx| {
                                if *width != Some(bounds.size.width) {
                                    *width = Some(bounds.size.width);
                                    cx.notify();
                                }
                            })
                        })
                        .child(self.render_preview(content_min - rail, column, window, cx)),
                )
                .into_any_element()
        } else if self.sidebar && !narrow {
            h_resizable("history-columns")
                .with_state(&self.history_panels)
                // The kit paints its divider over History's first pixel,
                // where the commit list draws its left edge and focus ring;
                // the sidebar's own right border is the divider. Paint the
                // line on that border instead, in its color at rest and the
                // ring's while dragged; the hit area and drag stay the kit's.
                .with_handle_appearance(std::rc::Rc::new(
                    |handle: &gpui_kit::base::ResizeHandleContext, _, cx| {
                        let line = if handle.is_active() {
                            gpui_kit::component::Theme::global(cx).colors.ring
                        } else {
                            rgb(palette(cx).border).into()
                        };
                        Some(
                            div()
                                .flex_none()
                                .h_full()
                                .w(px(1.))
                                .ml(px(-1.))
                                .bg(line)
                                .into_any_element(),
                        )
                    },
                ))
                .on_resize({
                    let owner = cx.entity().downgrade();
                    move |panels, window, cx| {
                        if let Some(width) = panels.read(cx).sizes().first().copied() {
                            let _ = owner.update(cx, |this, cx| {
                                this.settings.navigation_width = f32::from(width);
                                this.save_preferences(window, cx);
                            });
                        }
                    }
                })
                .child(
                    resizable_panel()
                        .size(px(self.settings.navigation_width))
                        .size_range(px(180.)..px(360.))
                        .flex_none()
                        .child(self.render_sidebar(cx)),
                )
                .child(
                    resizable_panel()
                        .size_range(px(320.)..px(10000.))
                        .child(self.render_history(window, cx)),
                )
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .min_w_0()
                .child(self.render_rail(narrow, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_history(window, cx)),
                )
                .into_any_element()
        };
        // The app retains both panel states while Repository is not rendered.
        // Element-local state alone is dropped across Settings/Projects and
        // would restore the initial widths, including after a manual drag.
        let workspace = h_resizable("content-columns")
            .with_state(&self.content_panels)
            .on_resize({
                let owner = cx.entity().downgrade();
                move |panels, window, cx| {
                    if let Some(width) = panels.read(cx).sizes().get(1).copied() {
                        let _ = owner.update(cx, |this, cx| {
                            this.settings.inspector_width = f32::from(width);
                            this.save_preferences(window, cx);
                        });
                    }
                }
            })
            .child(
                resizable_panel()
                    .size_range(content_min..px(10000.))
                    .child(left),
            )
            .child(
                resizable_panel()
                    .size(px(self.settings.inspector_width))
                    // A scaled minimum can pass the 480 px maximum, as at
                    // 18 pt with a desktop text scale of 1.25.
                    .size_range(
                        inspector_min..if columns.pinned {
                            inspector_min
                        } else {
                            px(480.).max(inspector_min)
                        },
                    )
                    .flex_none()
                    .child(if self.mode == WorkspaceMode::Working {
                        self.render_working_inspector(
                            self.expected_inspector_width(width, &columns),
                            columns.pinned,
                            window,
                            cx,
                        )
                    } else {
                        self.render_inspector(files_header, cx)
                    }),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_git_actions(chrome.is_compact(), cx))
            .child(self.render_operation_state(cx))
            .child(div().flex_1().min_h_0().child(workspace))
            .into_any_element()
    }

    /// The keyboard hints at the status bar's right: the page's, or, under a
    /// modal dialog or sheet, those of the dialog control holding focus,
    /// which has keys of its own, or none. The page's keys do not reach the
    /// page beneath a modal, so its hints never show there.
    pub(super) fn status_hints(&self, window: &mut Window, cx: &mut App) -> Option<String> {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return self
                .dialog_hints
                .focused(window)
                .map(shortcuts::DialogKeys::hints);
        }
        Some(match self.page {
            AppPage::Settings => "Tab Move between controls · Esc Back".to_owned(),
            AppPage::Projects => format!("{}O Open repository · Esc Back", primary_label()),
            AppPage::Repository => match self.mode {
                WorkspaceMode::Compare => "Esc Back to history · ↑↓ Files".to_owned(),
                WorkspaceMode::Working => "↑↓ Review files · Esc Back to history".to_owned(),
                WorkspaceMode::History => format!(
                    "{}F Search · ↑↓ Navigate · {} Open file",
                    primary_label(),
                    shortcuts::ENTER
                ),
            },
        })
    }
}

impl Render for GitTurtle {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.draws.push(appearance::palette(cx));
        static TRACE_LAYOUT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *TRACE_LAYOUT.get_or_init(|| std::env::var_os("GITTURTLE_TRACE").is_some()) {
            let layout = (
                window.viewport_size(),
                self.settings.theme,
                self.settings.density,
                self.settings.interface_text_size,
                self.settings.code_text_size,
                self.git_actions_open,
            );
            if self.layout_trace != Some(layout) {
                self.layout_trace = Some(layout);
                // Built-ins keep the recorded `configured_theme=Daylight` form.
                let configured_theme = match layout.1 {
                    appearance::custom::ThemeSelection::BuiltIn(choice) => format!("{choice:?}"),
                    appearance::custom::ThemeSelection::Custom(id) => format!("custom:{id}"),
                    appearance::custom::ThemeSelection::Omarchy => "omarchy".into(),
                };
                eprintln!(
                    "gitturtle.layout viewport={:.0}x{:.0} configured_theme={} density={:?} interface={} code={} targets={}",
                    f32::from(layout.0.width),
                    f32::from(layout.0.height),
                    configured_theme,
                    layout.2,
                    layout.3,
                    layout.4,
                    layout.5
                );
            }
        }
        image_lifetime::after_draw(window, cx);
        // A refused Toggle Sidebar is announced only from the narrow History
        // rail. Forget it once that is not drawn, so the node cannot reappear,
        // and be announced again, when the window narrows or History returns.
        if self.navigation_refusals > 0
            && (self.page != AppPage::Repository
                || self.mode != WorkspaceMode::History
                || self.navigation_fits(window))
        {
            self.navigation_refusals = 0;
        }
        if self.dialog_layer_subscription.is_none()
            && let Some(Some(root)) = window.root::<Root>()
        {
            // This view owns render_dialog_layer. Root's modal collection is a
            // separate entity: observing it invalidates our cached child tree
            // when any native dialog opens or closes, without a resize/input.
            self.dialog_layer_subscription =
                Some(cx.observe_in(&root, window, |this, _, window, cx| {
                    // Invalidate an older close repair even if a replacement
                    // dialog opens before the next GitTurtle render.
                    if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
                        this.modal_focus_generation = this.modal_focus_generation.wrapping_add(1);
                    }
                    window.refresh();
                    cx.notify();
                }));
        }
        self.update_modal_focus(window, cx);
        self.return_focus_after_delete(window, cx);
        // After the focus repairs above, before any list is built.
        self.focus_ring.begin_frame(window, cx);
        let colors = palette(cx);
        let menu_state = (self.repository.is_some(), self.operation_busy.is_some());
        if self.menu_state != Some(menu_state) {
            platform_polish::menus(menu_state.0, menu_state.1, cx);
            self.menu_state = Some(menu_state);
        }
        let chrome = self.chrome(window, cx);
        let page = match self.page {
            AppPage::Repository => self.render_repository(chrome, window, cx),
            AppPage::Projects => self.hub.clone().into_any_element(),
            AppPage::Settings => self.settings_page_element(window),
        };
        // The pane spans the whole body so a project stays one click away on
        // Repository and Settings. The hub already lists projects full width.
        let body = if self.settings.project_pane && self.page != AppPage::Projects {
            let pane_width = self.project_pane_width(window);
            h_resizable("workspace-columns")
                .with_state(&self.project_panels)
                .on_resize({
                    let owner = cx.entity().downgrade();
                    move |panels, window, cx| {
                        if let Some(width) = panels.read(cx).sizes().first().copied() {
                            let _ = owner.update(cx, |this, cx| {
                                this.settings.project_pane_width = f32::from(width);
                                this.save_preferences(window, cx);
                            });
                        }
                    }
                })
                .child(
                    resizable_panel()
                        .size(pane_width)
                        .size_range(px(180.)..px(360.))
                        .flex_none()
                        .child(self.render_project_pane(window, cx)),
                )
                .child(
                    resizable_panel()
                        .size_range(px(520.)..px(10000.))
                        .child(page),
                )
                .into_any_element()
        } else {
            page
        };
        div()
            .id("gitturtle")
            .relative()
            .track_focus(&self.app_focus)
            .key_context("GitTurtle")
            .child(self.focus_ring.pointer_listener())
            .on_action(cx.listener(|this, _: &theme_editor::PageUpAction, _, cx| {
                this.scroll_settings_page(settings::PageStep::Up, cx)
            }))
            .on_action(cx.listener(|this, _: &theme_editor::PageDownAction, _, cx| {
                this.scroll_settings_page(settings::PageStep::Down, cx)
            }))
            .on_action(cx.listener(|this, _: &theme_editor::PageTopAction, _, cx| {
                this.scroll_settings_page(settings::PageStep::Top, cx)
            }))
            .on_action(cx.listener(|this, _: &theme_editor::PageBottomAction, _, cx| {
                this.scroll_settings_page(settings::PageStep::Bottom, cx)
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors.canvas))
            .text_color(rgb(colors.text))
            .text_size(crate::appearance::ui_text(13.))
            // Keep an active image drag continuous across the toolbar, inspector,
            // and either image viewport until the mouse is released.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.move_image_drag(event, cx);
                if let Some((id, start, width)) = this.column_drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.settings
                            .columns
                            .set_width(id, width + f32::from(event.position.x - start));
                        cx.notify();
                    }
                    return;
                }
                let Some((start, offset)) = this.image_drag else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    this.end_image_drag();
                    if this.image_drag.take().is_some() {
                        cx.notify();
                    }
                    return;
                }
                let requested = offset + event.position - start;
                let maximum = this.image_scroll.max_offset();
                this.image_scroll.set_offset(point(
                    requested.x.clamp(-maximum.x, px(0.)),
                    requested.y.clamp(-maximum.y, px(0.)),
                ));
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.column_drag.take().is_some() {
                        this.save_preferences(window, cx);
                    }
                    this.end_image_drag();
                    if this.image_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.column_drag.take().is_some() {
                        this.save_preferences(window, cx);
                    }
                    this.end_image_drag();
                    if this.image_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &ExtendNextWorking, window, cx| {
                if this.mode == WorkspaceMode::Working {
                    this.extend_working_selection(true, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ExtendPreviousWorking, window, cx| {
                if this.mode == WorkspaceMode::Working {
                    this.extend_working_selection(false, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SelectAllWorking, _, cx| this.select_all_working(cx)))
            // Selected commit-message text copies byte for byte wherever
            // focus is; otherwise the toolkit root copies other selected text.
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Copy, _, cx| {
                if !this.inspector_message.copy_selection(cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &ShowActivity, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.open_activity(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &QuickOpenFile, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.open_quick_file(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MainMenu, window, cx| {
                #[cfg(target_os = "linux")]
                {
                    let menu = this.primary_menu.clone();
                    window.defer(cx, move |window, cx| {
                        menu.update(cx, |menu, cx| menu.toggle(window, cx))
                    });
                }
                #[cfg(not(target_os = "linux"))]
                let _ = (this, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowCommandPalette, window, cx| {
                this.open_command_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &CompareRevisions, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.open_revision_comparison(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.show_history(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &RevealRepository, _, cx| {
                if let Some(repo) = &this.repository {
                    cx.reveal_path(repo.path());
                }
            }))
            .on_action(
                cx.listener(|this, _: &OpenEditor, window, cx| {
                    this.open_external_editor(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenTerminal, window, cx| {
                this.open_terminal(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShortcutHelp, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.shortcut_help(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NextRepositoryTab, window, cx| {
                this.cycle_repository_tab(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousRepositoryTab, window, cx| {
                this.cycle_repository_tab(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseRepositoryTab, window, cx| {
                if let Some(index) = this.repository_tabs.active {
                    this.close_repository_tab(index, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab1, window, cx| {
                this.switch_repository_tab(0, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab2, window, cx| {
                this.switch_repository_tab(1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab3, window, cx| {
                this.switch_repository_tab(2, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab4, window, cx| {
                this.switch_repository_tab(3, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab5, window, cx| {
                this.switch_repository_tab(4, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab6, window, cx| {
                this.switch_repository_tab(5, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab7, window, cx| {
                this.switch_repository_tab(6, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectRepositoryTab8, window, cx| {
                this.switch_repository_tab(7, window, cx)
            }))
            .on_action(cx.listener(|_, _: &MinimizeWindow, window, _| window.minimize_window()))
            .on_action(cx.listener(|this, _: &CloseWindow, window, _| {
                if this.operation_busy.is_none() {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
            .on_action(cx.listener(|this, _: &ShowProjects, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.show_projects(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.show_settings(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowChanges, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.show_working(window, cx);
                }
            }))
            .on_action(cx.listener(|this, action: &OpenRepository, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.choose_repository(action, window, cx);
                }
            }))
            .on_action(cx.listener(|this, action: &Refresh, window, cx| {
                if !window.has_active_dialog(cx)
                    && !window.has_active_sheet(cx)
                    && this.page == AppPage::Repository
                {
                    this.refresh(action, window, cx);
                }
            }))
            .on_action(cx.listener(Self::search))
            .on_action(cx.listener(Self::clear_search))
            // Editor Escape is a distinct action from the list binding. Find
            // and native editor popovers consume it first; otherwise it is Back.
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                    this.clear_search(&ClearSearch, window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &BackHistory, window, cx| this.navigate_back(window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTextChange, window, cx| {
                this.navigate_text_change(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousTextChange, window, cx| {
                this.navigate_text_change(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextRow, window, cx| {
                this.move_selection(1, false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousRow, window, cx| {
                this.move_selection(-1, false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FirstRow, window, cx| {
                this.move_selection(-1, true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &LastRow, window, cx| {
                    this.move_selection(1, true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &NextPane, window, cx| {
                if this.page != AppPage::Repository
                    || window.has_active_dialog(cx)
                    || window.has_active_sheet(cx)
                {
                    return;
                }
                if this.blame.owns_focus(window) {
                    this.open_blame_commit(window, cx);
                    return;
                }
                if this.mode == WorkspaceMode::Working {
                    return;
                }
                if let Some(index) = this.selected_file {
                    this.select_file(index, window, cx);
                } else {
                    this.pane = Pane::Files;
                    window.focus(&this.file_focus, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                if this.page != AppPage::Repository
                    || window.has_active_dialog(cx)
                    || window.has_active_sheet(cx)
                {
                    return;
                }
                this.toggle_sidebar(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleProjectPane, window, cx| {
                this.toggle_project_pane(window, cx)
            }))
            .child(self.render_repository_tabs(window, cx))
            .when(self.page != AppPage::Projects, |root| {
                root.child(self.render_header(chrome, cx))
            })
            .when(
                self.page == AppPage::Projects && self.operation_busy.is_some(),
                |root| {
                    root.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .p_3()
                            .bg(rgb(colors.panel))
                            .child(
                                self.operation_progress()
                                    .unwrap_or_else(|| "Working…".into()),
                            )
                            .child(self.render_operation_cancel(cx)),
                    )
                },
            )
            .when(self.page == AppPage::Repository, |el| {
                el.children(self.operation_error.as_ref().map(|error| {
                    div()
                        .max_h(px(100.))
                        .overflow_hidden()
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_3()
                        .bg(rgb(palette(cx).removed_background))
                        .text_color(rgb(palette(cx).removed))
                        .child(
                            div()
                                .id("operation-error-summary")
                                .debug_selector(|| "operation-error-summary".into())
                                .role(Role::Alert)
                                .a11y_synthetic_children(native_accessibility::assertive)
                                .aria_label(error.clone())
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(crate::appearance::ui_text(11.))
                                .child(
                                    error
                                        .lines()
                                        .next()
                                        .unwrap_or(error)
                                        .chars()
                                        .take(220)
                                        .collect::<String>(),
                                ),
                        )
                        .child(
                            button("operation-error-details", "Details…", "", false).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.show_operation_error(window, cx)
                                }),
                            ),
                        )
                        .child(
                            button("dismiss-operation-error", "Dismiss", "", false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.operation_error = None;
                                    cx.notify();
                                }),
                            ),
                        )
                }))
            })
            .when(
                self.page == AppPage::Repository && self.operation_error.is_none(),
                |el| {
                    el.children(self.operation_notice.as_ref().map(|notice| {
                        div()
                            .px_4()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .bg(rgb(colors.selected))
                            .text_color(rgb(colors.accent))
                            // The notice wraps to as many lines as it needs, so
                            // a long explanation (a vanished scope's branch or
                            // worktree path) is read whole at every text size
                            // beside a reachable Dismiss; an unusually long one
                            // scrolls inside a bounded height rather than
                            // pushing the page away.
                            .child(
                                div()
                                    .id("operation-notice-summary")
                                    .debug_selector(|| "operation-notice-summary".into())
                                    .role(Role::Status)
                                    .a11y_synthetic_children(native_accessibility::polite)
                                    .aria_label(notice.clone())
                                    .flex_1()
                                    .min_w_0()
                                    .max_h(crate::appearance::ui_size(100.))
                                    .overflow_y_scroll()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .child(
                                        div()
                                            .debug_selector(|| "operation-notice-text".into())
                                            .child(notice.clone()),
                                    ),
                            )
                            .when(self.history_updates.committed.as_ref().is_some_and(|oid| notice.starts_with(&format!("Committed {}", short_oid(oid)))), |row| row.child(
                                button("view-created-commit", "View commit", "", false)
                                    .flex_shrink_0()
                                    .disabled(self.loading.is_some() || self.operation_busy.is_some())
                                    .tooltip("Inspect the commit you created; the next commit draft stays saved")
                                    .on_click(cx.listener(|this, _, window, cx| this.view_created_commit(window, cx)))
                            ))
                            .child(
                                button("dismiss-operation-notice", "Dismiss", "", false)
                                    .debug_selector(|| "dismiss-operation-notice".into())
                                    .flex_shrink_0()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.operation_notice = None;
                                        cx.notify();
                                    })),
                            )
                    }))
                },
            )
            .when(self.page == AppPage::Repository, |el| el.children(self.render_history_update_notice(cx)))
            .child(div().flex_1().min_h_0().child(body))
            .child(
                div()
                    .h(crate::appearance::ui_size(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(colors.border))
                    .bg(rgb(colors.panel))
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .child(div().size(px(5.)).rounded_full().bg(rgb(
                        if self.page == AppPage::Repository && self.error.is_some() {
                            colors.removed
                        } else {
                            colors.accent
                        },
                    )))
                    .child(
                        div().flex_1().truncate().child(match self.page {
                            AppPage::Projects => {
                                "Open a project or start a new repository".to_owned()
                            }
                            AppPage::Settings => "Appearance and layout preferences".to_owned(),
                            AppPage::Repository => self
                                .error
                                .as_deref()
                                .unwrap_or(self.loading.unwrap_or(&self.status))
                                .to_string(),
                        }),
                    )
                    .when(self.page == AppPage::Repository && self.automatic.watch_warning().is_some(), |footer| {
                        footer.child(div().flex().items_center().gap_2()
                            .child(div().id("local-refresh-status").role(Role::Status)
                                .aria_label("Some local changes need Refresh")
                                .child("Some changes need Refresh"))
                            .child(button("retry-local-refresh", "Refresh", "", false).h(crate::appearance::ui_size(20.)).text_size(crate::appearance::ui_text(10.)).disabled(self.operation_busy.is_some())
                                .on_click(cx.listener(|this, _, window, cx| this.retry_automatic_refresh(window, cx))))
                            .child(button("local-refresh-details", "Details…", "", false).h(crate::appearance::ui_size(20.)).text_size(crate::appearance::ui_text(10.))
                                .on_click(cx.listener(|this, _, window, cx| this.show_automatic_refresh_details(window, cx)))))
                    })
                    .children(self.status_hints(window, cx).map(|hints| {
                        div().debug_selector(|| "status-hints".into()).child(hints)
                    })),
            )
            // Copies from any page or dialog are announced from here.
            .children(copy_feedback::announcement(window, cx))
            .children(Root::render_dialog_layer(window, cx))
            // Deferred above the dialog layer: the last paint of the frame
            // that shows an edited draft ends `gitturtle.theme_edit_frame_ms`.
            .children(
                self.theme_editor
                    .take_edit_trace()
                    .map(|started| self.edit_trace_probe(started, cx)),
            )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.executor().run_until_parked();
        }
    }
    pub(crate) async fn settle(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        for _ in 0..8 {
            cx.executor().run_until_parked();
            let tasks = cx.update(|_, cx| {
                app.update(cx, |app, _| {
                    [app.task.take(), app.status_task.take()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                })
            });
            if tasks.is_empty() {
                break;
            }
            for task in tasks {
                task.await;
            }
        }
        draw(cx);
    }

    /// A 1000 × 680 window on a one-commit fixture, in History with the
    /// commit selected. Keep the returned directory until the test ends.
    pub(crate) async fn history_window(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        history_window_from(
            cx,
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline notes.txt\ndata 6\nnotes\n\ndone\n",
        )
        .await
    }

    /// A `git fast-import` stream of `commits` commits on `main`, each
    /// rewriting `notes.txt`; the newest writes `lines` numbered lines, so
    /// its comparison has at least that many diff lines.
    fn history_script(commits: usize, lines: usize) -> String {
        let mut input = String::new();
        for commit in 0..commits {
            let notes: String = if commit + 1 == commits {
                (1..=lines).map(|line| format!("note {line}\n")).collect()
            } else {
                format!("draft {commit}\n")
            };
            let message = format!("fixture notes {commit}");
            input.push_str(&format!(
                "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\nM 100644 inline notes.txt\ndata {}\n{notes}\n",
                1_700_000_000 + commit,
                message.len(),
                notes.len(),
            ));
        }
        input + "done\n"
    }

    /// [`history_window`] on the repository `input` imports.
    async fn history_window_from<'a>(
        cx: &'a mut TestAppContext,
        input: &str,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &'a mut VisualTestContext,
    ) {
        use gpui_kit::component::Root;
        use std::{
            cell::RefCell,
            io::Write,
            process::{Command, Stdio},
            rc::Rc,
        };

        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let fixture = tempfile::tempdir().unwrap();
        let repo = GitRepository::init(fixture.path().join("narrow"), "main").unwrap();
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(repo.path())
            .args(["fast-import", "--quiet"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        let mut import = command.spawn().unwrap();
        import
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let imported = import.wait_with_output().unwrap();
        assert!(
            imported.status.success(),
            "{}",
            String::from_utf8_lossy(&imported.stderr)
        );

        let initial = repo.path().to_owned();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                )
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(680.)));
        settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        (fixture, app, cx)
    }

    /// Tiling window managers such as Hyprland ignore the window's minimum
    /// size, so a half-screen tile is often 600–800 px wide, and at a
    /// fractional scale a half tile of a display about 1,000 logical pixels
    /// wide is 460–560 px wide and shorter than the minimum height.
    #[gpui::test]
    async fn narrow_windows_keep_the_inspector_whole_and_the_header_on_one_row(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = history_window(cx).await;

        let one_row = appearance::ui_size(60.);
        for (width, height) in [
            (1000., 680.),
            (800., 718.),
            (664., 718.),
            (600., 718.),
            (560., 718.),
            (493., 526.),
            (460., 526.),
        ] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let files = cx
                .debug_bounds("commit-inspector-files")
                .expect("the inspector lists the commit's files");
            assert!(
                files.right() <= px(width),
                "inspector clipped at {width}: {files:?}"
            );
            // One row holds down to a half tile of a 1366 px display; a
            // narrower header wraps rather than clipping.
            if width >= 640. {
                let header = cx.debug_bounds("repository-header").unwrap();
                assert!(
                    header.size.height < one_row,
                    "header wrapped at {width}: {header:?}"
                );
            }
            // A label-less kit button is a square icon button; the compact
            // profile button must keep its whole icon rather than a sliver.
            let profile = cx.debug_bounds("profile-button").unwrap();
            assert!(
                profile.right() <= px(width),
                "profile at {width}: {profile:?}"
            );
            if width < COMPACT_HEADER_WIDTH {
                assert_eq!(profile.size.width, appearance::ui_size(36.), "at {width}");
            }
            let sidebar = cx.debug_bounds("history-sidebar").is_some();
            let rail = cx.debug_bounds("repository-rail").is_some();
            assert_eq!(
                (sidebar, rail),
                (width >= SIDEBAR_FIT_WIDTH, width < SIDEBAR_FIT_WIDTH),
                "navigation at {width}"
            );
        }
        // Width alone hides the navigation; the user's choice is untouched.
        app.read_with(cx, |app, _| {
            assert!(app.sidebar);
            assert!(app.history_sidebar);
        });
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        assert!(cx.debug_bounds("history-sidebar").is_some());

        // Compare's path row keeps its buttons in the preview column.
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        for (width, height) in [(1000., 680.), (600., 718.), (493., 526.), (460., 526.)] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let files = cx
                .debug_bounds("commit-inspector-files")
                .expect("Compare keeps the commit's files");
            assert!(
                files.right() <= px(width),
                "Compare files clipped at {width}: {files:?}"
            );
            let blame = cx
                .debug_bounds("open-blame")
                .expect("a text comparison offers Blame");
            assert!(
                blame.right() <= files.left(),
                "Blame under the inspector at {width}: {blame:?}, files {files:?}"
            );
        }
    }

    /// Whether `inner` lies inside `outer`, edges included.
    fn within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
        inner.left() >= outer.left()
            && inner.right() <= outer.right()
            && inner.top() >= outer.top()
            && inner.bottom() <= outer.bottom()
    }

    /// The commit inspector's message toolbar ends above "Changed files",
    /// and the toolbar, its copy controls and the files stay in the window.
    fn assert_inspector_whole(cx: &mut VisualTestContext, width: f32, state: &str) {
        let window = Bounds::new(point(px(0.), px(0.)), size(px(width), px(10_000.)));
        let files = cx.debug_bounds("commit-inspector-files").unwrap();
        let header = cx.debug_bounds("changed-files-header").unwrap();
        let toolbar = cx.debug_bounds("commit-message-toolbar").unwrap();
        assert!(
            toolbar.bottom() <= files.top() && toolbar.bottom() <= header.top(),
            "{state}: toolbar {toolbar:?} over Changed files {header:?}"
        );
        assert!(within(files, window), "{state}: files {files:?}");
        for id in ["copy-message", "copy-commit"] {
            let control = cx.debug_bounds(id).unwrap();
            assert!(
                within(control, toolbar) && control.left() >= files.left(),
                "{state}: {id} {control:?} outside the {toolbar:?} toolbar"
            );
        }
    }

    /// The status bar reports the working tree in Changes: from the moment it
    /// is shown, and through a working preview, whose file list holds only the
    /// previewed file. Back in History the commit's own count returns, and one
    /// file is singular.
    #[gpui::test]
    async fn changes_reports_the_working_tree_in_the_status_bar(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, &history_script(2, 1)).await;
        settle(&app, cx).await;
        let status = |cx: &mut VisualTestContext| cx.read(|cx| app.read(cx).status.clone());
        assert!(
            status(cx).starts_with("1 changed file · "),
            "{}",
            status(cx)
        );

        // The import leaves the index empty, so notes.txt is a staged deletion.
        let worktree = fixture.path().join("narrow");
        for name in ["a.txt", "b.txt"] {
            std::fs::write(worktree.join(name), "new\n").unwrap();
        }
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        assert_eq!(status(cx), "3 working changes · 1 staged");

        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let index = app
                    .work_status
                    .as_ref()
                    .unwrap()
                    .entries
                    .iter()
                    .position(|entry| entry.path == std::path::Path::new("a.txt"))
                    .unwrap();
                app.select_working(index, gitturtle_core::ChangeArea::Unstaged, window, cx);
            })
        });
        settle(&app, cx).await;
        assert!(
            status(cx).starts_with("3 working changes · 1 staged · content read "),
            "{}",
            status(cx)
        );

        cx.update(|window, cx| app.update(cx, |app, cx| app.back_to_history(window, cx)));
        settle(&app, cx).await;
        assert_eq!(status(cx), "1 changed file");
    }

    /// At an 18 pt interface size the column and inspector minimums grow with
    /// the text: in a 461 × 490 half tile History's compact paging stays in
    /// the column, the inspector ends at the window's edge, its message
    /// toolbar reserves the rows it wraps to above "Changed files", in History
    /// and Compare alike, and Changes' filter field, whose text area clips its
    /// placeholder, stays inside the window, focused or not.
    #[gpui::test]
    async fn narrow_large_text_keeps_paging_and_the_inspector_whole(cx: &mut TestAppContext) {
        let (_fixture, app, cx) =
            history_window_from(cx, &history_script(history_paging::PAGE_SIZE + 20, 1)).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        // At 560 px the inspector is wide enough for the labelled Copy
        // message, and Hash, as wide as its copied state, wraps below it; at
        // 461 only their icons fit, side by side on one row.
        for (width, height, compact) in [(461., 490., true), (560., 600., false)] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let toolbar = cx.debug_bounds("history-scope-toolbar").unwrap();
            for selector in ["columns", "history-newest", "history-previous", "load-more"] {
                let control = cx.debug_bounds(selector).unwrap();
                assert!(
                    control.left() >= toolbar.left()
                        && control.right() <= toolbar.right()
                        && control.right() <= px(width),
                    "{selector} outside the {toolbar:?} column at {width}: {control:?}"
                );
            }
            assert_inspector_whole(cx, width, &format!("History at {width}"));
            let copy = cx.debug_bounds("copy-message").unwrap();
            let hash = cx.debug_bounds("copy-commit").unwrap();
            assert_eq!(
                copy.size.width == appearance::ui_size(28.).ceil(),
                compact,
                "Copy message at {width}: {copy:?}"
            );
            assert_eq!(hash.size == copy.size, compact, "Hash at {width}: {hash:?}");
            if compact {
                assert_eq!(hash.top(), copy.top(), "Hash at {width}: {hash:?}");
            } else {
                assert!(hash.top() > copy.top(), "Hash at {width}: {hash:?}");
            }
        }
        for (selector, name) in [
            ("copy-message", "Copy full commit message"),
            ("copy-commit", "Copy full commit hash"),
        ] {
            assert_eq!(
                COMPACT_NAMES.with(|names| names.borrow().get(selector).copied()),
                Some(name)
            );
        }

        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        for (width, height) in [(461., 490.), (560., 600.)] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            assert_inspector_whole(cx, width, &format!("Compare at {width}"));
        }

        cx.simulate_resize(size(px(461.), px(490.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        for focused in [false, true] {
            if focused {
                cx.update(|window, cx| {
                    let filter = app.read(cx).working_filter.clone();
                    filter.update(cx, |filter, cx| filter.focus(window, cx));
                });
            }
            draw(cx);
            // The half tile at 18 pt takes the tight form's one row.
            let selection = cx.debug_bounds("working-toolbar").unwrap();
            let (field, text) = cx.read(|cx| {
                let filter = app.read(cx).working_filter.read(cx);
                (filter.input_bounds(), filter.text_bounds().unwrap())
            });
            assert!(
                selection.right() <= px(461.)
                    && field.left() >= selection.left()
                    && field.right() <= selection.right()
                    && within(text, field),
                "focused {focused}: field {field:?}, text {text:?} in {selection:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A `git fast-import` stream of two commits on `main`: the older adds
    /// `older` files and the newer `newer` more.
    fn files_script(older: usize, newer: usize) -> String {
        let mut input = String::new();
        for (commit, (first, files)) in [(0, older), (older, newer)].into_iter().enumerate() {
            let message = format!("fixture files {commit}");
            input.push_str(&format!(
                "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\n",
                1_700_000_000 + commit,
                message.len(),
            ));
            for file in first..first + files {
                input.push_str(&format!("M 100644 inline file-{file:02}.txt\ndata 2\nx\n"));
            }
            input.push('\n');
        }
        input + "done\n"
    }

    /// The "Changed files" header keeps its count whole inside the inspector
    /// the window pins at 18 pt: in a 461 × 490 half tile the label truncates
    /// beside a count of 12 or of 3, in History and Compare, and as the window
    /// narrows the label leaves, then the count, never clipping either.
    #[gpui::test]
    async fn narrow_large_text_keeps_the_changed_files_count_whole(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &files_script(3, 12)).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        let (gap, inset, label_min) = cx.update(|window, _| {
            let rem = window.rem_size();
            (rem * 0.5, rem * 0.75, files_label_min_width(window))
        });
        let check = |cx: &mut VisualTestContext, width: f32, state: &str| {
            cx.simulate_resize(size(px(width), px(490.)));
            draw(cx);
            let header = cx.debug_bounds("changed-files-header").unwrap();
            let files = cx.debug_bounds("commit-inspector-files").unwrap();
            let count = cx.debug_bounds("changed-files-count");
            let label = cx.debug_bounds("changed-files-label");
            // Across the inspector only: the vertical chrome at this height
            // is the compact-chrome task's.
            assert!(
                header.left() >= files.left()
                    && header.right() <= files.right()
                    && files.right() <= px(width),
                "{state} at {width}: header {header:?} in {files:?}"
            );
            if let Some(count) = count {
                assert!(
                    count.left() >= header.left() + inset
                        && count.right() <= header.right() - inset,
                    "{state} at {width}: count {count:?} cut in {header:?}"
                );
            }
            if let Some(label) = label {
                let count = count.expect("the label shows only beside the count");
                assert!(
                    label.size.width >= label_min && label.right() + gap <= count.left(),
                    "{state} at {width}: label {label:?} beside {count:?}"
                );
            }
            (label.is_some(), count.is_some())
        };
        for (commit, count) in [(0, "12"), (1, "3")] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.show_history(window, cx);
                    app.select_commit(commit, window, cx);
                })
            });
            settle(&app, cx).await;
            assert_eq!(cx.read(|cx| app.read(cx).files.len().to_string()), count);
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
            settle(&app, cx).await;
            assert_eq!(
                check(cx, 461., "Compare"),
                (true, true),
                "Compare, {count} files"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
            settle(&app, cx).await;
            assert_eq!(
                check(cx, 461., "History"),
                (true, true),
                "History, {count} files"
            );
            let mut shown = Vec::new();
            for width in (341..=461).rev().step_by(4) {
                shown.push(check(cx, width as f32, "History"));
            }
            assert!(
                shown
                    .windows(2)
                    .all(|pair| pair[0].0 >= pair[1].0 && pair[0].1 >= pair[1].1),
                "{count} files: the label and count only leave as the window narrows: {shown:?}"
            );
            assert!(
                shown.contains(&(false, true)) && shown.contains(&(false, false)),
                "{count} files: the label leaves before the count: {shown:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// At the default interface size the narrow half tiles keep origin/main's
    /// layout: these are the bounds its fixed minimums gave (the rail, a
    /// 177 px column and a 240 px inspector at 461 px; a 209 px column at
    /// 493 px), measured on origin/main at f695dce. Only Hash differs: it is
    /// as wide as its copied state, the check icon and "Copied", and still
    /// shares Copy message's row.
    #[gpui::test]
    async fn default_text_size_keeps_the_narrow_layout(cx: &mut TestAppContext) {
        let (_fixture, app, cx) =
            history_window_from(cx, &history_script(history_paging::PAGE_SIZE + 20, 1)).await;
        let bounds = |x: f32, y: f32, width: f32, height: f32| {
            Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
        };
        for (width, height, column, inspector, files_top) in [
            (461., 490., 177., 222., 322.5),
            (493., 526., 209., 254., 339.),
        ] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let files_height = height - 26. - files_top;
            let expected = [
                ("repository-rail", bounds(0., 207., 44., height - 233.)),
                ("history-scope-toolbar", bounds(44., 207., column, 40.)),
                ("columns", bounds(column - 90., 212.5, 28., 28.)),
                ("load-more", bounds(column + 6.5, 212.5, 28., 28.)),
                (
                    "commit-inspector-files",
                    bounds(inspector, files_top, 239., files_height),
                ),
                ("commit-message-toolbar", bounds(inspector, 207., 239., 34.)),
                ("copy-message", bounds(inspector + 9.5, 210., 129., 28.)),
                ("copy-commit", bounds(inspector + 141.5, 210., 86., 28.)),
                (
                    "changed-files-header",
                    bounds(inspector, files_top, 238., 36.),
                ),
                (
                    "changed-files-count",
                    bounds(inspector + 123.5, files_top + 8.5, 16., 19.),
                ),
            ];
            for (selector, expected) in expected {
                assert_eq!(
                    cx.debug_bounds(selector),
                    Some(expected),
                    "{selector} at {width} × {height}"
                );
            }
        }
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        // The filter, Actions and Refresh share one row so that the file
        // list keeps its heading and first file whole and the composer its
        // title field (`short_working_inspector_keeps_a_whole_file_row_and_
        // whole_controls`); origin/main left the controls 136 px and the list
        // 6 px.
        for (width, height, inspector) in [(461., 490., 221.), (493., 526., 253.)] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            assert_eq!(
                cx.debug_bounds("working-toolbar"),
                Some(bounds(inspector, 207., 240., 34.)),
                "Changes at {width} × {height}"
            );
            let field = cx.read(|cx| app.read(cx).working_filter.read(cx).input_bounds());
            assert_eq!(
                field,
                bounds(inspector + 20.5, 216., 137., 16.),
                "at {width}"
            );
        }
    }

    /// In a half tile of a display about 1,000 px wide, History's 176 px
    /// column still holds Latest, Previous and Older, and each still acts.
    /// The count beside the scope name is drawn whole or not at all, and the
    /// name at least as wide as its first character and ellipsis or not at
    /// all, so neither shows a clipped or lone ellipsis.
    #[gpui::test]
    async fn narrow_history_keeps_paging_in_the_column(cx: &mut TestAppContext) {
        let (_fixture, app, cx) =
            history_window_from(cx, &history_script(history_paging::PAGE_SIZE + 20, 1)).await;
        // Which of the count and the name each width has room for: at 493
        // the name yields to the whole count, at 514 it truncates beside it,
        // and in the 176 px column of a 461 px window both yield.
        for (width, height, shown) in [
            (493., 526., [true, false]),
            (514., 526., [true, true]),
            (461., 490., [false, false]),
        ] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let toolbar = cx.debug_bounds("history-scope-toolbar").unwrap();
            for selector in ["columns", "history-newest", "history-previous", "load-more"] {
                let control = cx.debug_bounds(selector).unwrap();
                assert!(
                    control.left() >= toolbar.left()
                        && control.right() <= toolbar.right()
                        && control.right() <= px(width),
                    "{selector} outside the {toolbar:?} column at {width}: {control:?}"
                );
                assert_eq!(control.size.height, appearance::ui_size(28.), "{selector}");
            }
            let label = cx.debug_bounds("history-scope-label").unwrap();
            let count = SCOPE_COUNT
                .with(|cache| cache.borrow().as_ref().map(|(_, _, width)| *width))
                .expect("the count's measured width");
            let least_name = SCOPE_NAME
                .with(|cache| cache.borrow().as_ref().map(|(_, _, width)| *width))
                .expect("the name's measured minimum");
            for ((selector, least), shown) in [
                ("history-scope-count", count),
                ("history-scope-name", least_name),
            ]
            .into_iter()
            .zip(shown)
            {
                let text = cx.debug_bounds(selector);
                assert_eq!(text.is_some(), shown, "{selector} at {width} in {label:?}");
                if let Some(text) = text {
                    assert!(
                        text.size.width >= least && text.right() <= label.right(),
                        "{selector} at {width}: {text:?}, at least {least:?} in {label:?}"
                    );
                }
            }
        }
        // A steady render lays out no text to decide the toolbar.
        let layouts = || LINE_LAYOUTS.with(std::cell::Cell::get);
        let before = layouts();
        app.update(cx, |_, cx| cx.notify());
        draw(cx);
        assert_eq!(layouts(), before, "text laid out again by a steady render");
        let paging = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                (
                    app.history_paging.offset,
                    app.visible.len(),
                    app.history_paging.next_offset,
                )
            })
        };
        let click = |cx: &mut VisualTestContext, selector| {
            let control = cx.debug_bounds(selector).unwrap();
            cx.simulate_click(control.center(), Modifiers::default());
        };
        let page = history_paging::PAGE_SIZE;
        // Older continues the captured history into the bounded window.
        click(cx, "load-more");
        settle(&app, cx).await;
        assert_eq!(paging(cx), (0, page + 20, None), "after Older");
        // Only the new count is laid out.
        assert_eq!(layouts(), before + 1, "after Older");
        // Latest reads current history again and shows its newest rows.
        click(cx, "history-newest");
        settle(&app, cx).await;
        assert_eq!(paging(cx), (0, page, Some(page)), "after Latest");
        // Previous reads the preceding page of a window evicted past its
        // first page.
        app.update(cx, |app, cx| {
            app.history_paging.offset = page;
            cx.notify();
        });
        draw(cx);
        click(cx, "history-previous");
        settle(&app, cx).await;
        assert_eq!(paging(cx), (0, page, Some(page)), "after Previous");
    }

    /// As History widens, its scope toolbar shows beside the branch icon
    /// nothing, then the whole count, then the count and the name, then the
    /// labelled row, in that order and never back. A drawn count is whole and
    /// a drawn name at least its first character and ellipsis wide, so
    /// neither shows a clipped or lone ellipsis. With this fixture in the test
    /// font the count fits from a 192 px column, the name beside it from
    /// 214 px and the labelled row from 428 px, which the rail layout reaches
    /// in a 752 px window and, on this path of resizes, the layout beside
    /// navigation in an 888 px one; each band is sampled every 4 px, and
    /// every width where it changes.
    #[gpui::test]
    async fn scope_toolbar_grows_in_order_as_history_widens(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 1)).await;
        let measured = |cache: &'static std::thread::LocalKey<TextWidth>| {
            cache
                .with(|cache| cache.borrow().as_ref().map(|(_, _, width)| *width))
                .expect("the width the toolbar measured")
        };
        // The recorded History width at `width`, and what its toolbar draws
        // there: 0 nothing beside the icon, 1 the count, 2 the count and the
        // name, 3 the labelled row.
        let observe = |cx: &mut VisualTestContext, width: u32| {
            cx.simulate_resize(size(px(width as f32), px(526.)));
            draw(cx);
            let column = app.read_with(cx, |app, _| app.history_width);
            let at = format!("at {width} (History {column} px)");
            let label = cx.debug_bounds("history-scope-label");
            let inside =
                |text: Bounds<Pixels>| label.is_none_or(|label| text.right() <= label.right());
            let count = cx.debug_bounds("history-scope-count");
            if let Some(count) = count {
                let whole = measured(&SCOPE_COUNT);
                assert!(
                    count.size.width >= whole && inside(count),
                    "count cut {at}: {count:?}, whole {whole:?}, in {label:?}"
                );
            }
            let name = cx.debug_bounds("history-scope-name");
            if let Some(name) = name {
                let least = measured(&SCOPE_NAME);
                assert!(
                    name.size.width >= least && inside(name),
                    "name narrower than its first character and ellipsis {at}: {name:?}, \
                     at least {least:?}, in {label:?}"
                );
            }
            let state = match (label.is_some(), count.is_some(), name.is_some()) {
                (false, true, true) => 3,
                (true, false, false) => 0,
                (true, true, false) => 1,
                (true, true, true) => 2,
                (true, false, true) => panic!("the name without the count {at}"),
                (false, ..) => panic!("the labelled row without its name or count {at}"),
            };
            (column, state, width)
        };
        // Each band, reached from the one before, and the states it crosses.
        let mut seen = Vec::new();
        for (first, last, crosses) in [
            (461, 521, &[0, 1, 2][..]),
            (724, 772, &[2, 3]),
            (864, 912, &[2, 3]),
        ] {
            let mut band = Vec::new();
            let mut previous: Option<(u32, u8)> = None;
            for width in (first..=last).step_by(4) {
                let observed = observe(cx, width);
                band.push(observed);
                if let Some((before, state)) = previous
                    && state != observed.1
                {
                    band.extend((before + 1..width).map(|between| observe(cx, between)));
                }
                previous = Some((width, observed.1));
            }
            let states: std::collections::BTreeSet<u8> = band.iter().map(|seen| seen.1).collect();
            assert_eq!(
                states,
                crosses.iter().copied().collect(),
                "{first}–{last} crosses its breakpoints: {band:?}"
            );
            seen.extend(band);
        }
        // The toolbar decides from the recorded width, so its states grow
        // with it.
        seen.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in seen.windows(2) {
            assert!(
                pair[0].1 <= pair[1].1,
                "the toolbar gave way as History widened: {pair:?}"
            );
        }
        // Another scope's name is laid out once, whole and cut, and a steady
        // render lays out nothing.
        let layouts = || LINE_LAYOUTS.with(std::cell::Cell::get);
        let before = layouts();
        app.update(cx, |app, cx| {
            let name = "feature/narrow";
            app.scope = Some((
                name.into(),
                crate::worker::Scope::Branch {
                    name: name.into(),
                    remote: false,
                },
            ));
            cx.notify();
        });
        draw(cx);
        assert_eq!(layouts(), before + 2, "another scope's name");
        app.update(cx, |_, cx| cx.notify());
        draw(cx);
        assert_eq!(
            layouts(),
            before + 2,
            "text laid out again by a steady render"
        );
    }

    /// The narrowest the scope name is drawn in keeps a character before its
    /// ellipsis, the first one a truncated line keeps, or is the whole name
    /// where that is narrower; each name is measured afresh.
    #[gpui::test]
    fn scope_name_keeps_a_character_before_its_ellipsis(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, _| {
            let size = window.text_style().font_size.to_pixels(window.rem_size());
            let mut style = window.text_style();
            style.font_weight = FontWeight::MEDIUM;
            let width = |text: &str| {
                window
                    .text_system()
                    .layout_line(text, size, &[style.to_run(text.len())], None)
                    .width
            };
            // Drawn only where the element is wider than the cut line.
            let cut = |text: &str| width(text).floor() + px(1.);
            for (name, least) in [
                ("All history", cut("A…")),
                ("a", width("a").ceil()),
                // A truncated line drops trailing punctuation, which would
                // leave "(" a lone ellipsis.
                ("(detached", cut("(d…")),
                ("...", width("...").ceil()),
                ("東京 main", cut("東…")),
                ("All history", cut("A…")),
            ] {
                assert_eq!(scope_name_min_width(name, window), least, "{name:?}");
            }
        });
    }

    /// History's compact Columns, Latest, Previous and Older, and the
    /// collapsed review options' Previous change, Next change and Options
    /// trigger, are each given a specific accessible name when drawn without
    /// their visible labels. The names are read from the test bookkeeping in
    /// `compact_control_name`: the test platform builds no accessibility
    /// tree, and a kit button's rendered AccessKit node is not reachable.
    #[gpui::test]
    async fn compact_controls_carry_their_accessible_names(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        let drawn = |cx: &mut VisualTestContext| {
            COMPACT_NAMES.with(|names| names.borrow_mut().clear());
            draw(cx);
            COMPACT_NAMES.with(|names| names.borrow().clone())
        };
        let expect = |cx: &mut VisualTestContext, expected: &[(&'static str, &'static str)]| {
            for (selector, _) in expected {
                assert!(cx.debug_bounds(selector).is_some(), "{selector} is drawn");
            }
            expected
                .iter()
                .copied()
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        cx.simulate_resize(size(px(461.), px(490.)));
        let names = drawn(cx);
        assert_eq!(
            names,
            expect(
                cx,
                &[
                    ("columns", "Columns"),
                    ("history-newest", "Latest"),
                    ("history-previous", "Previous"),
                    ("load-more", "Older"),
                ]
            )
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let names = drawn(cx);
        assert_eq!(
            names,
            expect(
                cx,
                &[
                    ("previous-text-change", "Previous change"),
                    ("next-text-change", "Next change"),
                    ("text-review-menu", "Review options"),
                    // The whole path does not fit beside a labelled Blame.
                    ("open-blame", "Blame"),
                ]
            )
        );
        // Labelled, the controls name themselves.
        cx.simulate_resize(size(px(1100.), px(680.)));
        assert_eq!(drawn(cx), Default::default());
    }

    /// Visible diff lines: the patch editor's text within the preview's
    /// content area and the window.
    fn visible_diff_lines(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, height: f32) -> f32 {
        let content = cx.debug_bounds("preview-content").unwrap();
        let (line, text) = cx.update(|_, cx| {
            let editor = app.read(cx).patch_editor.clone().expect("a unified diff");
            let state = editor.read(cx);
            (state.line_height().unwrap(), state.text_bounds().unwrap())
        });
        let bottom = text.bottom().min(content.bottom()).min(px(height));
        ((bottom - text.top().max(content.top())) / line)
            .floor()
            .max(0.)
    }

    /// Beside the header and the Git action bar on two rows each, Compare's
    /// review options collapse to one row so a 461 × 490 window still shows
    /// the diff.
    #[gpui::test]
    async fn narrow_compare_keeps_three_diff_lines(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        for (width, height) in [(493., 526.), (461., 490.)] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let lines = visible_diff_lines(&app, cx, height);
            assert!(lines >= 3., "{lines} diff lines at {width} × {height}");
            let layouts = LINE_LAYOUTS.with(std::cell::Cell::get);
            app.update(cx, |_, cx| cx.notify());
            draw(cx);
            assert_eq!(
                LINE_LAYOUTS.with(std::cell::Cell::get),
                layouts,
                "text laid out again by a steady render at {width}"
            );
            let row = cx.debug_bounds("text-review-options").unwrap();
            for selector in [
                "previous-text-change",
                "next-text-change",
                "text-review-menu",
            ] {
                let control = cx.debug_bounds(selector).unwrap();
                assert!(
                    control.top() >= row.top()
                        && control.bottom() <= row.bottom()
                        && control.right() <= row.right(),
                    "{selector} off the one options row at {width}: {control:?}, row {row:?}"
                );
                assert!(
                    control.bottom() - control.top() > row.size.height / 2.,
                    "{selector} {row:?}"
                );
            }
        }
    }

    /// A local bare `origin` that `main` tracks one commit behind, read again
    /// by Refresh, so Fetch, Pull and Push are enabled and the action bar
    /// shows the ahead and behind counts. Nothing is fetched or pushed.
    async fn track_a_local_origin(
        fixture: &std::path::Path,
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) {
        let repository = fixture.join("narrow");
        let bare = fixture.join("origin.git");
        fixture_git(
            fixture,
            &["init", "--quiet", "--bare", bare.to_str().unwrap()],
        );
        fixture_git(
            &repository,
            &["remote", "add", "origin", bare.to_str().unwrap()],
        );
        fixture_git(
            &repository,
            &["update-ref", "refs/remotes/origin/main", "main~1"],
        );
        fixture_git(
            &repository,
            &["branch", "--quiet", "--set-upstream-to=origin/main", "main"],
        );
        press_refresh(app, cx).await;
    }

    /// The full chrome `GitTurtle::chrome` predicts, and the chrome drawn.
    fn predicted_chrome(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> ((Pixels, Pixels), Chrome) {
        cx.update(|window, cx| {
            let app = app.read(cx);
            let widths = app.chrome_widths(window);
            (
                app.full_chrome_heights(&widths, window),
                app.chrome(window, cx),
            )
        })
    }

    /// The full header's and action bar's heights that decide the compact
    /// chrome are predicted before layout from the interface font, and they
    /// are the heights layout then gives them: in History and Compare, at
    /// the default size and 18 pt, at widths where they take one, two and
    /// three rows, with and without an upstream and beside the project pane.
    #[gpui::test]
    async fn chrome_prediction_matches_the_laid_out_header_and_action_bar(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, &history_script(2, 3)).await;
        let check = |cx: &mut VisualTestContext, state: &str| {
            for text in [13, 18] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                for width in [400., 461., 493., 560., 640., 700., 800., 1000., 1480.] {
                    // Tall enough for the full chrome everywhere.
                    cx.simulate_resize(size(px(width), px(1400.)));
                    draw(cx);
                    let ((header, actions), chrome) = predicted_chrome(&app, cx);
                    assert!(!chrome.is_compact(), "{state}, {text} pt at {width}");
                    let at = format!("{state}, {text} pt at {width}");
                    let drawn = cx.debug_bounds("repository-header").unwrap().size.height;
                    assert!(
                        (drawn - header).abs() < px(0.01),
                        "{at}: header {drawn:?}, predicted {header:?}"
                    );
                    let drawn = cx.debug_bounds("git-actions").unwrap().size.height;
                    assert!(
                        (drawn - actions).abs() < px(0.01),
                        "{at}: action bar {drawn:?}, predicted {actions:?}"
                    );
                }
            }
        };
        check(cx, "History without an upstream");
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        check(cx, "Compare without an upstream");
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        track_a_local_origin(fixture.path(), &app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app
                .work_status
                .as_ref()
                .map(|s| (s.ahead, s.behind))),
            Some((1, 0))
        );
        check(cx, "History tracking origin");
        app.update(cx, |app, cx| {
            app.settings.project_pane = true;
            cx.notify();
        });
        check(cx, "History beside the project pane");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// At 18 pt in a 461 × 490 half tile the full header and action bar
    /// take five rows; their compact rows leave History three whole rows and
    /// Compare three whole diff lines.
    #[gpui::test]
    async fn short_large_text_keeps_history_rows_and_diff_lines(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(40, 40)).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let (list, row) = app
            .read_with(cx, |app, _| app.history_list_layout)
            .expect("History's list is laid out");
        assert!(
            list.height >= row * 3.,
            "{:.2} History rows in {list:?}",
            list.height / row
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let lines = visible_diff_lines(&app, cx, 490.);
        assert!(lines >= 3., "{lines} diff lines");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The chrome controls `ringed` looks for, in their Tab order.
    const CHROME_CONTROLS: [&str; 11] = [
        "page-back",
        "history-tab",
        "changes-tab",
        "pull-requests",
        "profile-button",
        "settings",
        "current-branch",
        "fetch",
        "pull",
        "push",
        "git-actions-toggle",
    ];

    /// The control among `CHROME_CONTROLS` whose focus ring is painted: the
    /// quad the installed ring's room outside its bounds.
    fn ringed(cx: &mut VisualTestContext) -> Option<&'static str> {
        let room = cx.read(appearance::button_ring_room);
        let quads = cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.border_widths.top.0 > 0.)
                .map(|quad| {
                    let (origin, extent) = (quad.bounds.origin, quad.bounds.size);
                    let logical = |value: ScaledPixels| px(value.as_f32() / scale);
                    Bounds::new(
                        point(logical(origin.x), logical(origin.y)),
                        size(logical(extent.width), logical(extent.height)),
                    )
                })
                .collect::<Vec<_>>()
        });
        CHROME_CONTROLS.into_iter().find(|selector| {
            cx.debug_bounds(selector).is_some_and(|control| {
                let ring = control.dilate(room);
                quads.iter().any(|quad| {
                    (quad.left() - ring.left()).abs() < px(0.5)
                        && (quad.right() - ring.right()).abs() < px(0.5)
                        && (quad.top() - ring.top()).abs() < px(0.5)
                        && (quad.bottom() - ring.bottom()).abs() < px(0.5)
                })
            })
        })
    }

    /// The chrome controls Tab reaches from History's list, in order, once
    /// round the window.
    fn chrome_tab_order(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Vec<&'static str> {
        cx.update(|window, cx| {
            let focus = app.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        draw(cx);
        let mut order = Vec::new();
        for _ in 0..80 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            if let Some(control) = ringed(cx)
                && order.last() != Some(&control)
            {
                order.push(control);
            }
            if order.last() == Some(&"git-actions-toggle") {
                break;
            }
        }
        order
    }

    /// In the compact chrome every control that loses its label keeps its
    /// action, a tooltip and its accessible name, the header and the action
    /// bar are one row each, the turtle leaves where the name needs its room,
    /// and Tab reaches the chrome's controls in the full chrome's order.
    #[gpui::test]
    async fn compact_chrome_keeps_each_control(cx: &mut TestAppContext) {
        use std::time::Duration;
        let (fixture, app, cx) = history_window_from(cx, &history_script(3, 3)).await;
        track_a_local_origin(fixture.path(), &app, cx).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(1400.)));
        draw(cx);
        assert_eq!(predicted_chrome(&app, cx).1, Chrome::Full { narrow: true });
        let full = chrome_tab_order(&app, cx);
        assert_eq!(full, CHROME_CONTROLS, "the full chrome's Tab order");

        COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        COMPACT_LABELS.with(|names| names.borrow_mut().clear());
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(matches!(
            predicted_chrome(&app, cx).1,
            Chrome::Compact {
                app_icon: false,
                ..
            }
        ));
        assert!(cx.debug_bounds("repository-heading").is_some());
        // One row each: the header's controls, the action bar's, 28 px.
        let (control, room) = cx.update(|window, cx| {
            (
                window.pixel_snap(appearance::ui_size(28.)),
                appearance::button_ring_room(cx),
            )
        });
        let header = cx.debug_bounds("repository-header").unwrap();
        let actions = cx.debug_bounds("git-actions").unwrap();
        assert_eq!(header.size.height, room * 2. + control, "{header:?}");
        assert_eq!(actions.size.height, control + room + px(1.), "{actions:?}");
        for (selectors, bar) in [
            (&CHROME_CONTROLS[..6], header),
            (&CHROME_CONTROLS[6..], actions),
        ] {
            for selector in selectors {
                let bounds = cx.debug_bounds(selector).unwrap();
                assert!(
                    bounds.size.height == control
                        && bounds.top() == bar.top() + if bar == header { room } else { px(0.) }
                        && bounds.right() <= px(461.),
                    "{selector} {bounds:?} off the {bar:?} row"
                );
            }
        }
        let names = COMPACT_NAMES.with(|names| names.borrow().clone());
        for (selector, name) in [
            ("history-tab", "History"),
            ("pull-requests", "Pull requests"),
            ("git-actions-toggle", "Targets"),
        ] {
            assert_eq!(names.get(selector), Some(&name), "{selector}");
        }
        let labels = COMPACT_LABELS.with(|names| names.borrow().clone());
        for (selector, name) in [
            ("changes-tab", "Changes · 1"),
            ("fetch", "Fetch: Fetch from origin"),
            (
                "pull",
                "Pull: Pull origin/main into main · fast-forward only",
            ),
            ("push", "Push: Push main to origin/main"),
        ] {
            assert_eq!(
                labels.get(selector).map(|name| name.as_ref()),
                Some(name),
                "{selector}"
            );
        }
        assert_eq!(
            chrome_tab_order(&app, cx),
            full,
            "the compact chrome's Tab order"
        );

        // Each collapsed control shows its tooltip.
        let popover =
            cx.read(|cx| Background::from(gpui_kit::component::Theme::global(cx).tokens.popover));
        let popups = move |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover)
                    .count()
            })
        };
        for selector in [
            "history-tab",
            "changes-tab",
            "pull-requests",
            "fetch",
            "pull",
            "push",
            "git-actions-toggle",
        ] {
            let control = cx.debug_bounds(selector).unwrap();
            let before = popups(cx);
            cx.simulate_mouse_move(control.center(), None, Modifiers::default());
            cx.executor().advance_clock(Duration::from_millis(500));
            draw(cx);
            assert_eq!(popups(cx), before + 1, "{selector} shows its tooltip");
            cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
            cx.executor().advance_clock(Duration::from_millis(500));
            draw(cx);
        }

        // And each acts. Opening Targets keeps the compact form it opened in.
        let click = |cx: &mut VisualTestContext, selector: &'static str| {
            let control = cx.debug_bounds(selector).unwrap();
            cx.simulate_click(control.center(), Modifiers::default());
            draw(cx);
        };
        click(cx, "changes-tab");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working));
        click(cx, "history-tab");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::History));
        for open in [true, false] {
            click(cx, "git-actions-toggle");
            assert_eq!(app.read_with(cx, |app, _| app.git_actions_open), open);
            assert!(predicted_chrome(&app, cx).1.is_compact());
        }
        click(cx, "pull-requests");
        settle(&app, cx).await;
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Pull requests opens"
        );
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));

        // Where the name keeps its full minimum beside it, the turtle shows.
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(700.), px(330.)));
        draw(cx);
        assert!(matches!(
            predicted_chrome(&app, cx).1,
            Chrome::Compact { app_icon: true, .. }
        ));
    }

    /// In the compact header the History and Changes toggles keep one place
    /// in Changes, History and Compare, though Projects joins the trailing
    /// controls outside History; the Changes toggle shows its count as a
    /// badge inside its bounds; and the profile is one of its 28 px
    /// neighbours, unfilled at rest like Settings.
    #[gpui::test]
    async fn compact_header_keeps_its_toggles_in_place_with_the_count(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        COMPACT_LABELS.with(|names| names.borrow_mut().clear());
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(predicted_chrome(&app, cx).1.is_compact());
        let toggles = |cx: &mut VisualTestContext| {
            (
                cx.debug_bounds("history-tab").unwrap(),
                cx.debug_bounds("changes-tab").unwrap(),
            )
        };
        let changes = toggles(cx);
        assert!(cx.debug_bounds("projects").is_some(), "Projects in Changes");
        let badge = cx.debug_bounds("changes-count-badge").expect("the count");
        assert!(
            badge.left() >= changes.1.left()
                && badge.right() <= changes.1.right()
                && badge.top() >= changes.1.top()
                && badge.bottom() <= changes.1.bottom(),
            "badge {badge:?} inside {:?}",
            changes.1
        );
        assert_eq!(
            COMPACT_LABELS.with(|names| names.borrow().get("changes-tab").cloned()),
            Some("Changes · 3".into())
        );
        // The profile rests unfilled, as Settings does, at their height.
        let (profile, settings) = (
            cx.debug_bounds("profile-button").unwrap(),
            cx.debug_bounds("settings").unwrap(),
        );
        assert_eq!(profile.size, settings.size);
        assert_eq!(
            painted_fills(cx, "profile-button"),
            painted_fills(cx, "settings"),
            "the profile's resting fill"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        settle(&app, cx).await;
        assert!(
            cx.debug_bounds("projects").is_none(),
            "no Projects in History"
        );
        assert_eq!(toggles(cx), changes, "History");
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        assert_eq!(toggles(cx), changes, "Compare");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The compact chrome's breakpoint counts Compare's need as well as
    /// History's: wherever the full chrome is drawn, Compare shows three
    /// whole diff lines, at the default and 18 pt interface sizes and code
    /// at 12 and 24 pt, and at 13 pt with 24 pt code a 461 × 470 window,
    /// where History alone would keep the full chrome, takes the compact
    /// one. In a narrow column the text modes keep one row as a menu named
    /// by the current mode, which chooses another.
    #[gpui::test]
    async fn the_chrome_breakpoint_keeps_compare_diff_lines(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(40, 60)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        for (text, code) in [(13, 12), (13, 24), (18, 12), (18, 24)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, code, window, cx));
            let mut height = 700.;
            while height >= 380. {
                cx.simulate_resize(size(px(461.), px(height)));
                draw(cx);
                let (_, chrome) = predicted_chrome(&app, cx);
                if !chrome.is_compact() {
                    let lines = visible_diff_lines(&app, cx, height);
                    assert!(
                        lines >= 3.,
                        "{lines} diff lines under the full chrome, {text}/{code} pt at 461 × {height}"
                    );
                }
                height -= 3.;
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 24, window, cx));
        cx.simulate_resize(size(px(461.), px(470.)));
        draw(cx);
        let ((header, actions), chrome) = predicted_chrome(&app, cx);
        let (room, history) = cx.update(|window, cx| {
            let tabs = window.pixel_snap(repository_tabs::tab_strip_min_height(
                appearance::button_ring_room(cx),
            ));
            let status = window.pixel_snap(appearance::ui_size(26.));
            (
                content_size(window).height - tabs - status - header - actions,
                app.read(cx).history_minimum(window),
            )
        });
        assert!(
            chrome.is_compact() && room >= history,
            "{chrome:?} with {room:?} left beside History's {history:?}"
        );
        assert!(visible_diff_lines(&app, cx, 470.) >= 3.);

        // At 18 pt the modes are one menu; choosing Split from it.
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(cx.debug_bounds("Split").is_none());
        let menu = cx.debug_bounds("text-mode-menu").expect("the modes' menu");
        let toolbar = cx.debug_bounds("compare-toolbar").unwrap();
        let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
        assert!(
            toolbar.size.height <= control * 2. + px(20.),
            "the toolbar takes two rows: {toolbar:?}"
        );
        cx.simulate_click(menu.center(), Modifiers::default());
        draw(cx);
        cx.simulate_keystrokes("down down enter");
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.text_mode == TextMode::Split));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// In the 156 px inspector and 243 px comparison column of a 461 × 490
    /// window at 18 pt, no text gives way to a lone ellipsis or a clipped
    /// character: the changed-file rows leave out their status words so the
    /// name takes the width, Blame keeps only its icon so the path is whole,
    /// and the footer drops its object IDs and stays inside the column. At
    /// the default size, and in a wide window at 18 pt, each keeps its word.
    #[gpui::test]
    async fn narrow_large_text_rows_and_footer_keep_their_text(cx: &mut TestAppContext) {
        // A path that fits whole at 18 pt only once Blame gives up its label.
        let script = history_script(2, 40).replace("notes.txt", "note.txt");
        let (_fixture, app, cx) = history_window_from(cx, &script).await;
        let control = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)))
        };
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(cx.debug_bounds("changed-file-status").is_none(), "History");
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        assert!(cx.debug_bounds("changed-file-status").is_none(), "Compare");
        let footer = cx.debug_bounds("compare-footer").unwrap();
        let text = cx.debug_bounds("compare-footer-text").unwrap();
        let inset = cx.update(|window, _| window.rem_size() * 0.75);
        assert!(
            text.right() <= footer.right() - inset + px(0.5),
            "footer text {text:?} in {footer:?}"
        );
        let blame = cx.debug_bounds("open-blame").unwrap();
        assert_eq!(blame.size.width, control(cx), "Blame keeps its icon");
        assert_eq!(
            COMPACT_NAMES.with(|names| names.borrow().get("open-blame").copied()),
            Some("Blame")
        );
        let (path, whole) = (
            cx.debug_bounds("preview-file-path").unwrap(),
            cx.update(|window, _| {
                shaped_width(
                    "note.txt",
                    appearance::ui_text(12.),
                    FontWeight::MEDIUM,
                    window,
                )
            }),
        );
        assert!(path.size.width >= whole, "path {path:?}, whole {whole:?}");
        // Wide enough each keeps its word; at the default size the file rows
        // keep theirs even in the half tile.
        for (text, width, height) in [(18, 1000., 680.), (13, 1000., 680.), (13, 461., 490.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt at {width}");
            assert!(cx.debug_bounds("changed-file-status").is_some(), "{at}");
            if width > 461. {
                let blame = cx.debug_bounds("open-blame").unwrap();
                assert!(blame.size.width > control(cx), "{at}: {blame:?}");
                assert!(cx.debug_bounds("compare-footer").is_some());
            }
        }
    }

    /// A 40-commit History in Working Changes with two staged new files and
    /// one modified file, as the native review's fixture has.
    async fn working_window(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        let (fixture, app, cx) = history_window_from(cx, &history_script(40, 3)).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        std::fs::write(root.join("alpha.txt"), "alpha\n").unwrap();
        std::fs::write(root.join("beta.txt"), "beta\n").unwrap();
        fixture_git(&root, &["add", "alpha.txt", "beta.txt"]);
        std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| (
                app.working_paths.staged_count,
                app.work_status.as_ref().map(|status| status.entries.len())
            )),
            (2, Some(3))
        );
        (fixture, app, cx)
    }

    /// The bounds of the painted quad whose border is `color` and that
    /// matches `bounds`, if any.
    fn bordered_quad(
        cx: &mut VisualTestContext,
        bounds: Bounds<Pixels>,
        color: Hsla,
    ) -> Option<Bounds<Pixels>> {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.border_widths.top.0 > 0. && quad.border_color == color)
                .map(|quad| {
                    Bounds::new(
                        point(logical(quad.bounds.origin.x), logical(quad.bounds.origin.y)),
                        size(
                            logical(quad.bounds.size.width),
                            logical(quad.bounds.size.height),
                        ),
                    )
                })
                .find(|quad| {
                    (quad.top() - bounds.top()).abs() < px(0.5)
                        && (quad.bottom() - bounds.bottom()).abs() < px(0.5)
                        && (quad.left() - bounds.left()).abs() < px(0.5)
                })
        })
    }

    /// The Working Changes inspector with two staged files and one modified,
    /// in a 461 × 490 half tile and a 640 × 680 window, at the default size
    /// and at 18 pt. The file list keeps its Staged heading and its first
    /// file row whole inside its border, which its focus ring surrounds.
    /// Every control above the list is whole: where the filter, the selection
    /// controls and the Changes header fit whole beside the list's floor and
    /// the composer's minimum they keep their rows, and otherwise the filter,
    /// Actions and Refresh share one row, Refresh joining the Actions menu
    /// where the filter would lose its room beside it. The composer keeps the
    /// title's label and field a scaled 4 px apart above the commit action;
    /// its heading reads "Commit to" and the branch's first character, or, in
    /// the tightest inspector, leaves whole. Every row starts at one leading
    /// inset, and a steady frame asks the text system nothing.
    #[gpui::test]
    async fn short_working_inspector_keeps_a_whole_file_row_and_whole_controls(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        assert!(app.read_with(cx, |app, _| matches!(
            app.working_rows[..2],
            [
                workspace::WorkingRow::Heading(..),
                workspace::WorkingRow::File(..)
            ]
        )));
        // Each size's form: whether the controls share one row, whether
        // Refresh keeps its control there, and whether the composer keeps its
        // heading.
        for (width, height, text, tight, refresh, heading) in [
            (461., 490., 13, true, true, true),
            (461., 490., 18, true, false, false),
            (640., 680., 13, false, true, true),
            (640., 680., 18, true, true, true),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            COMPACT_NAMES.with(|names| names.borrow_mut().clear());
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let (row, border, inset, gap, field, status, cut) = cx.update(|window, cx| {
                let snap = |length: Pixels| window.pixel_snap(length);
                (
                    snap(px(app.read(cx).settings.density.file_row_height())),
                    snap(px(1.)),
                    snap(window.rem_size() * 0.75),
                    snap(appearance::ui_size(4.)),
                    window.rem_size() * 2.,
                    snap(appearance::ui_size(26.)),
                    shaped_width(
                        "Commit to m…",
                        appearance::ui_text(13.),
                        FontWeight::SEMIBOLD,
                        window,
                    )
                    .floor(),
                )
            });
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {at}"))
            };
            // The Staged heading and the first file, whole inside the border.
            let pane = bounds(cx, "working-pane");
            for selector in ["working-row-0", "working-row-1"] {
                let item = bounds(cx, selector);
                assert!(
                    item.size.height >= row - px(0.01)
                        && item.top() >= pane.top() + border - px(0.01)
                        && item.bottom() <= pane.bottom() - border + px(0.01),
                    "{at}: {selector} {item:?} in the list {pane:?}"
                );
            }
            // Each control whole, inside the inspector's inset.
            let whole = |outer: Bounds<Pixels>, inner: Bounds<Pixels>| {
                inner.top() >= outer.top() - px(0.01)
                    && inner.bottom() <= outer.bottom() + px(0.01)
                    && inner.left() >= outer.left() + inset - px(0.5)
                    && inner.right() <= outer.right() - inset + px(0.5)
            };
            let toolbar = cx.debug_bounds("working-toolbar");
            assert_eq!(toolbar.is_some(), tight, "{at}: {toolbar:?}");
            let (inspector, first) = if let Some(toolbar) = toolbar {
                for selector in [
                    "working-selection",
                    "working-header",
                    "group-working",
                    "stage-selection",
                    "unstage-selection",
                ] {
                    assert!(cx.debug_bounds(selector).is_none(), "{at}: {selector}");
                }
                assert_eq!(
                    cx.debug_bounds("refresh-working").is_some(),
                    refresh,
                    "{at}: Refresh"
                );
                let mut controls = vec!["working-filter", "working-recovery-menu"];
                if refresh {
                    controls.push("refresh-working");
                }
                for selector in controls {
                    let control = bounds(cx, selector);
                    assert!(
                        whole(toolbar, control),
                        "{at}: {selector} {control:?} in {toolbar:?}"
                    );
                }
                assert!(
                    toolbar.bottom() <= pane.top() + px(0.01),
                    "{at}: {toolbar:?}"
                );
                (toolbar, bounds(cx, "working-filter"))
            } else {
                let selection = bounds(cx, "working-selection");
                for selector in [
                    "working-filter",
                    "group-working",
                    "stage-selection",
                    "unstage-selection",
                    "working-selection-count",
                ] {
                    let control = bounds(cx, selector);
                    assert!(
                        whole(selection, control),
                        "{at}: {selector} {control:?} in {selection:?}"
                    );
                }
                let header = bounds(cx, "working-header");
                for selector in ["working-recovery-menu", "refresh-working"] {
                    let control = bounds(cx, selector);
                    assert!(
                        whole(header, control),
                        "{at}: {selector} {control:?} in {header:?}"
                    );
                }
                let first_in_header = cx
                    .debug_bounds("working-header-icon")
                    .or_else(|| cx.debug_bounds("working-header-count"))
                    .unwrap();
                assert!(
                    (first_in_header.left() - selection.left() - inset).abs() < px(0.5),
                    "{at}: the header starts at {first_in_header:?}"
                );
                (selection, bounds(cx, "working-filter"))
            };
            // The composer below the list: its heading whole or absent, then
            // the title's label and field a scaled 4 px apart, whole above the
            // commit action and the status bar.
            let composer = bounds(cx, "commit-composer");
            let label = bounds(cx, "commit-title-label");
            let title = bounds(cx, "commit-title");
            let commit = bounds(cx, "commit-staged");
            let shown = cx.debug_bounds("commit-composer-heading");
            assert_eq!(shown.is_some(), heading, "{at}: heading {shown:?}");
            let above = shown.map_or(composer.top(), |shown| {
                assert!(shown.size.width >= cut, "{at}: heading {shown:?}");
                shown.bottom()
            });
            assert!(
                composer.top() >= pane.bottom() - px(0.01)
                    && label.top() - above >= gap - px(0.01)
                    && title.top() - label.bottom() >= gap - px(0.01)
                    && commit.top() - title.bottom() >= gap - px(0.01)
                    && title.size.height >= field
                    && commit.bottom() <= px(height) - status,
                "{at}: {composer:?}, title {label:?} {title:?}, commit {commit:?}"
            );
            // One leading inset for every row.
            let leading = inspector.left() + inset;
            let first_in_heading = cx
                .debug_bounds("commit-composer-icon")
                .or(shown)
                .unwrap_or(title);
            for (what, edge) in [
                ("filter", first.left()),
                ("heading", first_in_heading.left()),
                ("title", title.left()),
                ("commit", commit.left()),
            ] {
                assert!(
                    (edge - leading).abs() < px(0.5),
                    "{at}: {what} at {edge:?}, the inset at {leading:?}"
                );
            }
            // The focused list draws its ring around both rows.
            cx.update(|window, cx| {
                let focus = app.read(cx).file_focus.clone();
                window.focus(&focus, cx);
            });
            cx.simulate_keystrokes("shift");
            draw(cx);
            let accent = cx.update(|_, cx| Hsla::from(rgb(palette(cx).accent)));
            let focused = bounds(cx, "working-pane");
            let ring = bordered_quad(cx, focused, accent);
            assert!(
                ring.is_some_and(|ring| ring.size.height >= row * 2. + border * 2.),
                "{at}: the focused list's ring {ring:?}"
            );
            // A steady frame lays out no text.
            let layouts = LINE_LAYOUTS.with(std::cell::Cell::get);
            app.update(cx, |_, cx| cx.notify());
            draw(cx);
            assert_eq!(LINE_LAYOUTS.with(std::cell::Cell::get), layouts, "{at}");
            if tight {
                assert_eq!(
                    COMPACT_NAMES
                        .with(|names| names.borrow().get("working-recovery-menu").copied()),
                    Some("Working changes and recovery actions"),
                    "{at}"
                );
            }
        }
        // In the tightest form the Actions menu leads with the selection
        // controls and Refresh, and they act.
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let menu = cx.debug_bounds("working-recovery-menu").unwrap();
        cx.simulate_click(menu.center(), Modifiers::default());
        draw(cx);
        for selector in [
            "menu-item: Stage 0 selected · MenuItem disabled",
            "menu-item: Unstage 0 selected · MenuItem disabled",
            "menu-item: Refresh working changes · MenuItem",
        ] {
            assert!(cx.debug_bounds(selector).is_some(), "{selector}");
        }
        let directories = cx
            .debug_bounds("menu-item: Directories · MenuItemCheckBox False")
            .expect("Directories, unchecked");
        cx.simulate_click(directories.center(), Modifiers::default());
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.working_selection.grouped));
        // A wide window keeps the labelled forms.
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
        for selector in [
            "group-working",
            "stage-selection",
            "unstage-selection",
            "working-recovery-menu",
        ] {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(bounds.size.width > control, "{selector} {bounds:?}");
        }
        assert!(cx.debug_bounds("working-header-label").is_some());
        assert!(cx.debug_bounds("commit-composer-icon").is_some());
    }

    /// From a 461 and a 640 px wide window's shortest height to a tall one,
    /// at the default size and at 18 pt, the Working Changes inspector never
    /// cuts a control above its list: the selection controls are whole in
    /// their own rows, the icon-only ones in a narrow inspector included, or
    /// they have given way to the one row of the tight form, and the list
    /// keeps its heading and first file whole.
    #[gpui::test]
    async fn working_controls_stay_whole_at_every_height(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        let mut forms = std::collections::BTreeSet::new();
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for width in [461., 640.] {
                for height in (490..=1010).step_by(26) {
                    cx.simulate_resize(size(px(width), px(height as f32)));
                    draw(cx);
                    let at = format!("{text} pt in {width} × {height}");
                    let (outer, controls) = match cx.debug_bounds("working-toolbar") {
                        Some(toolbar) => (toolbar, vec!["working-filter", "working-recovery-menu"]),
                        None => (
                            cx.debug_bounds("working-selection").expect(&at),
                            vec![
                                "working-filter",
                                "group-working",
                                "stage-selection",
                                "unstage-selection",
                                "working-selection-count",
                            ],
                        ),
                    };
                    forms.insert((
                        text,
                        cx.debug_bounds("working-toolbar").is_some(),
                        cx.debug_bounds("group-working")
                            .is_some_and(|control| control.size.width <= control.size.height),
                    ));
                    for selector in controls {
                        let control = cx.debug_bounds(selector).expect(selector);
                        assert!(
                            control.top() >= outer.top() - px(0.01)
                                && control.bottom() <= outer.bottom() + px(0.01),
                            "{at}: {selector} {control:?} in {outer:?}"
                        );
                    }
                    let pane = cx.debug_bounds("working-pane").expect(&at);
                    let file = cx.debug_bounds("working-row-1").expect(&at);
                    assert!(
                        file.top() >= pane.top() && file.bottom() <= pane.bottom(),
                        "{at}: {file:?} in {pane:?}"
                    );
                }
            }
        }
        // The sweep reached the tight form, the labelled controls and the
        // icon-only ones at 18 pt.
        for form in [
            (13, true, false),
            (13, false, false),
            (18, true, false),
            (18, false, true),
        ] {
            assert!(forms.contains(&form), "{form:?} in {forms:?}");
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        let _ = app;
    }

    /// The Working Changes list, whose five rows overflow it in a 461 × 490
    /// and a 640 × 680 window at the default size and at 18 pt, is a whole
    /// number of rows tall inside its border: every row it shows is whole,
    /// and the next begins at its bottom edge rather than being cut through
    /// its text, as the Unstaged heading was at 640 × 680. The composer takes
    /// the remainder.
    #[gpui::test]
    async fn working_list_shows_whole_rows(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        let total = app.read_with(cx, |app, _| app.working_rows.len());
        assert_eq!(total, 5);
        for (width, height, text) in [
            (461., 490., 13),
            (461., 490., 18),
            (640., 680., 13),
            (640., 680., 18),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let (row, border) = cx.update(|window, cx| {
                (
                    window.pixel_snap(px(app.read(cx).settings.density.file_row_height())),
                    window.pixel_snap(px(1.)),
                )
            });
            let pane = cx.debug_bounds("working-pane").expect(&at);
            let (top, bottom) = (pane.top() + border, pane.bottom() - border);
            let rows = (bottom - top) / row;
            assert!(
                (rows - rows.round()).abs() < 0.01 && rows.round() < total as f32,
                "{at}: the list {pane:?} holds {rows} rows of {row:?}"
            );
            let mut shown = 0;
            for index in 0..total {
                let selector: &'static str = format!("working-row-{index}").leak();
                let Some(item) = cx.debug_bounds(selector) else {
                    continue;
                };
                if item.bottom() <= top + px(0.01) || item.top() >= bottom - px(0.01) {
                    continue;
                }
                assert!(
                    item.top() >= top - px(0.01) && item.bottom() <= bottom + px(0.01),
                    "{at}: {selector} {item:?} cut by the list {pane:?}"
                );
                shown += 1;
            }
            assert_eq!(shown as f32, rows.round(), "{at}");
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// With two staged files in a 461 × 490 window, the composer's heading
    /// is drawn whole or not at all, above the commit action and the status
    /// bar: whole at the default size, and at 18 pt, where the inspector
    /// cannot keep it beside the list's heading and first file and the
    /// title's field, it leaves and the title's label leads the composer.
    #[gpui::test]
    async fn composer_heading_stays_whole_in_a_short_window(cx: &mut TestAppContext) {
        // The import leaves the index empty: both files are staged deletions.
        let (_fixture, app, cx) = history_window_from(cx, &files_script(1, 1)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            2
        );
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let (status, line) = cx.update(|window, _| {
                let text = window.text_style();
                (
                    window.pixel_snap(appearance::ui_size(26.)),
                    window.pixel_snap(
                        text.line_height
                            .to_pixels(appearance::ui_text(13.).into(), window.rem_size()),
                    ),
                )
            });
            let composer = cx.debug_bounds("commit-composer").unwrap();
            let label = cx.debug_bounds("commit-title-label").unwrap();
            let commit = cx.debug_bounds("commit-staged").unwrap();
            assert!(
                commit.bottom() <= px(490.) - status && label.top() >= composer.top(),
                "{text} pt: {composer:?}, {label:?}, {commit:?}"
            );
            let heading = cx.debug_bounds("commit-composer-heading");
            if text == 13 {
                let heading = heading.unwrap();
                assert!(
                    heading.size.height >= line
                        && heading.top() >= composer.top()
                        && heading.bottom() <= label.top()
                        && heading.right() <= px(461.),
                    "{text} pt: heading {heading:?} in {composer:?}"
                );
            } else {
                assert!(heading.is_none(), "{text} pt: heading {heading:?}");
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// In a 461 × 490 window, the compact Latest and Older tooltips and the
    /// Options tooltip, each wider than the window on one line, wrap inside
    /// it above their controls, with the tooltip positioner's margin and the
    /// popup's own on both sides. The shorter Columns tooltip keeps one line.
    #[gpui::test]
    async fn narrow_tooltips_wrap_inside_the_window(cx: &mut TestAppContext) {
        use gpui_kit::component::Theme;
        use std::time::Duration;

        /// gpui-base's tooltip `WINDOW_MARGIN`, kept from the viewport's
        /// edges, and its show delay.
        const POSITIONER_MARGIN: Pixels = px(4.);
        const SHOW_DELAY: Duration = Duration::from_millis(500);
        const WIDTH: f32 = 461.;

        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        // The popup enters where it rests, without sliding or fading in.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.simulate_resize(size(px(WIDTH), px(490.)));
        draw(cx);
        let popover = cx.read(|cx| Background::from(Theme::global(cx).tokens.popover));
        let popups = move |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                let scale = window.scale_factor();
                let logical = |value: ScaledPixels| px(value.as_f32() / scale);
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover)
                    .map(|quad| {
                        let (origin, extent) = (quad.bounds.origin, quad.bounds.size);
                        Bounds::new(
                            point(logical(origin.x), logical(origin.y)),
                            size(logical(extent.width), logical(extent.height)),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        };
        // The control and the popup that hovering it shows; then the pointer
        // leaves the window until the tooltip has gone.
        let hover = |cx: &mut VisualTestContext, selector: &'static str| {
            let control = cx.debug_bounds(selector).expect(selector);
            let before = popups(cx);
            cx.simulate_mouse_move(control.center(), None, Modifiers::default());
            cx.executor().advance_clock(SHOW_DELAY);
            draw(cx);
            let shown = popups(cx)
                .into_iter()
                .filter(|popup| !before.contains(popup))
                .collect::<Vec<_>>();
            cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
            cx.executor().advance_clock(SHOW_DELAY);
            draw(cx);
            assert_eq!(shown.len(), 1, "{selector} shows one tooltip: {shown:?}");
            (control, shown[0])
        };
        let (edge, device, line, one_line) = cx.update(|window, _| {
            let rem = window.rem_size();
            let inset = window.client_inset().unwrap_or_default();
            // The popup's line: `text_sm` at the default line height, which
            // the text element snaps to device pixels.
            let font = rems(0.875).to_pixels(rem);
            let line =
                window.pixel_snap(TextStyle::default().line_height.to_pixels(font.into(), rem));
            (
                POSITIONER_MARGIN + inset + rem * 0.75,
                px(1. / window.scale_factor()),
                line,
                // Within the vertical padding (`py_0p5`) and border.
                line + (rem * 0.125 + px(1.)) * 2.,
            )
        });
        let (_, columns) = hover(cx, "columns");
        assert!(
            (columns.size.height - one_line).abs() <= device,
            "Columns on one line of {one_line:?}: {columns:?}"
        );
        let check = |selector: &str, control: Bounds<Pixels>, popup: Bounds<Pixels>| {
            let at = format!("{selector}: {popup:?} over {control:?}");
            assert!(popup.bottom() <= control.top() + device, "above, {at}");
            assert!(
                (popup.left() - edge).abs() <= device
                    && (px(WIDTH) - popup.right() - edge).abs() <= device,
                "inset by {edge:?} on both sides, {at}"
            );
            assert!(popup.size.height >= one_line + line - device, "wraps, {at}");
        };
        for selector in ["history-newest", "load-more"] {
            let (control, popup) = hover(cx, selector);
            check(selector, control, popup);
        }
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let (control, popup) = hover(cx, "text-review-menu");
        check("text-review-menu", control, popup);
    }

    /// Hide whitespace, Context and Reset stay reachable from the keyboard
    /// in the Options menu, with their buttons' disabled states; the menu
    /// itself opens from the keyboard.
    #[gpui::test]
    async fn narrow_review_options_act_from_the_menu(cx: &mut TestAppContext) {
        use crate::text_review::Options;

        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(cx.debug_bounds("hide-whitespace-changes").is_none());
        let options = |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.review.options);
        let menu_open = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .context_stack()
                    .iter()
                    .any(|context| context.contains("PopupMenu"))
            })
        };
        let open = |cx: &mut VisualTestContext| {
            let menu = cx
                .debug_bounds("text-review-menu")
                .expect("the Options menu button");
            cx.simulate_click(menu.center(), Modifiers::default());
            draw(cx);
            assert!(menu_open(cx));
        };
        // Down selects the first enabled item, each further Down the next
        // enabled one, wrapping; Enter chooses it.
        let choose = |cx: &mut VisualTestContext, downs: usize| {
            for _ in 0..downs {
                cx.simulate_keystrokes("down");
            }
            cx.simulate_keystrokes("enter");
        };

        // With the original diff, Reset review is disabled: the third Down
        // passes over it back to Hide whitespace.
        open(cx);
        choose(cx, 3);
        settle(&app, cx).await;
        let hidden = Options {
            hide_whitespace: true,
            context: 3,
        };
        assert_eq!(options(cx), hidden);

        // The Options button follows the arrows in the tab order, just before
        // the diff, and Enter opens its menu.
        cx.update(|window, cx| {
            let editor = app.read(cx).patch_editor.clone().expect("a unified diff");
            let focus = editor.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        draw(cx);
        cx.update(|window, cx| window.focus_prev(cx));
        draw(cx);
        cx.update(|window, _| {
            assert!(
                window
                    .context_stack()
                    .iter()
                    .any(|context| context.contains("Popover")),
                "focus on the Options trigger: {:?}",
                window.context_stack()
            )
        });
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(menu_open(cx), "Enter opens the Options menu");
        choose(cx, 2);
        settle(&app, cx).await;
        assert_eq!(
            options(cx),
            Options {
                context: 12,
                ..hidden
            }
        );

        // Context steps to its last value, where it is disabled: the second
        // Down passes over it to Reset review.
        for context in [48, 192] {
            open(cx);
            choose(cx, 2);
            settle(&app, cx).await;
            assert_eq!(options(cx), Options { context, ..hidden });
        }
        open(cx);
        choose(cx, 2);
        settle(&app, cx).await;
        assert_eq!(options(cx), Options::default());
    }

    /// The fills painted exactly over a control's bounds.
    fn painted_fills(
        cx: &mut VisualTestContext,
        selector: &'static str,
    ) -> Vec<gpui_kit::Background> {
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let near = |device: gpui_kit::ScaledPixels, logical: Pixels| {
                (device.as_f32() - logical.as_f32() * scale).abs() < 0.5
            };
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    !quad.background.is_transparent()
                        && near(quad.bounds.left(), bounds.left())
                        && near(quad.bounds.top(), bounds.top())
                        && near(quad.bounds.right(), bounds.right())
                        && near(quad.bounds.bottom(), bounds.bottom())
                })
                .map(|quad| quad.background)
                .collect()
        })
    }

    /// What the last frame's quads leave visible just inside the middle of
    /// each side of `selector`'s bounds, left, top, right and bottom: the
    /// last quad painted there within its content mask, its border color in
    /// a bordered side's band and its fill elsewhere. GPUI paints a border as
    /// side-masked quads, so this reads their union, and a later quad, such
    /// as a divider drawn over an edge, wins.
    fn painted_sides(
        cx: &mut VisualTestContext,
        selector: &'static str,
    ) -> [Option<gpui_kit::Background>; 4] {
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            // Half a device pixel inside each edge.
            let inset = px(0.5 / scale);
            let center = bounds.center();
            [
                point(bounds.left() + inset, center.y),
                point(center.x, bounds.top() + inset),
                point(bounds.right() - inset, center.y),
                point(center.x, bounds.bottom() - inset),
            ]
            .map(|position| {
                let (x, y) = (position.x.as_f32() * scale, position.y.as_f32() * scale);
                let contains = |area: gpui_kit::Bounds<gpui_kit::ScaledPixels>| {
                    x >= area.left().as_f32()
                        && x < area.right().as_f32()
                        && y >= area.top().as_f32()
                        && y < area.bottom().as_f32()
                };
                quads.iter().rev().find_map(|quad| {
                    if !contains(quad.bounds) || !contains(quad.content_mask.bounds) {
                        return None;
                    }
                    let (area, widths) = (quad.bounds, quad.border_widths);
                    let in_border = x < area.left().as_f32() + widths.left.as_f32()
                        || x >= area.right().as_f32() - widths.right.as_f32()
                        || y < area.top().as_f32() + widths.top.as_f32()
                        || y >= area.bottom().as_f32() - widths.bottom.as_f32();
                    if in_border && quad.border_color.a > 0. {
                        Some(gpui_kit::Background::from(quad.border_color))
                    } else {
                        (!quad.background.is_transparent()).then_some(quad.background)
                    }
                })
            })
        })
    }

    /// The lines `text` takes when the window lays it out `room` wide at
    /// `size` in the interface font, the bundled IBM Plex Sans a Linux
    /// session draws `.SystemUIFont` in.
    #[cfg(target_os = "linux")]
    fn interface_lines(text: &str, size: Pixels, room: Pixels) -> Vec<String> {
        let system = gpui_kit::WindowTextSystem::new(crate::desktop_text::interface_text());
        let run = gpui_kit::TextRun {
            len: text.len(),
            font: gpui_kit::font(".SystemUIFont"),
            color: gpui_kit::Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = system
            .shape_text(text.to_owned().into(), size, &[run], Some(room), None)
            .unwrap();
        let line = &shaped[0];
        let mut starts = vec![0];
        starts.extend(line.wrap_boundaries().iter().map(|boundary| {
            line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
        }));
        starts.push(text.len());
        starts
            .windows(2)
            .map(|span| text[span[0]..span[1]].trim().to_owned())
            .collect()
    }

    /// In Changes, before a working file is chosen, the preview's sentence
    /// keeps to one line at the default text size in a 640 × 680 window,
    /// where "Select a changed file in the right panel, or return to
    /// history." left "history." alone on its second line; at every other
    /// interface size its last line keeps at least two words. The room is
    /// the empty state's own, measured in the window; the lines are the
    /// bundled interface font's, laid out as the window lays them out.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn the_choose_a_file_sentence_keeps_its_words_together_at_640_px(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = history_window(cx).await;
        std::fs::write(fixture.path().join("narrow/notes.txt"), "edited\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        cx.simulate_resize(size(px(640.), px(680.)));
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working);
            assert!(app.working_selected.is_none(), "no working file is chosen");
        });
        let lines_at = |cx: &mut VisualTestContext, interface: u8| {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            draw(cx);
            let state = cx
                .debug_bounds("empty-state")
                .expect("the preview's empty state");
            // The sentence lies inside the state's 1.5 rem padding and is at
            // most 600 px wide.
            let padding = cx.update(|window, _| window.rem_size() * 1.5);
            let room = (state.size.width - padding * 2.).min(px(600.));
            (
                interface_lines(CHOOSE_A_FILE, appearance::ui_text(12.), room),
                room,
            )
        };
        let (lines, room) = lines_at(cx, 13);
        assert_eq!(lines.len(), 1, "{lines:?} in {room:?}");
        for interface in 11..=18 {
            let (lines, room) = lines_at(cx, interface);
            let last = lines.last().unwrap();
            assert!(
                last.split_whitespace().count() >= 2,
                "{interface} pt: {lines:?} in {room:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A list focused from the keyboard keeps its accent ring, whole on all
    /// four sides, while the pointer moves over the window: GPUI's
    /// `focus_visible` dropped the ring at the first pointer movement, though
    /// the list kept focus. A pointer press ends the ring, as a click never
    /// marked a list, and the next key shows it again. History, the
    /// changed-file list and Working Changes' list all behave so, and
    /// History's left side shows: the navigation divider was painted over it.
    #[gpui::test]
    async fn a_keyboard_focused_list_keeps_its_ring_while_the_pointer_moves(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = history_window(cx).await;
        // A local edit, so Working Changes lists a file.
        std::fs::write(fixture.path().join("narrow/notes.txt"), "edited\n").unwrap();
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        draw(cx);
        assert!(cx.debug_bounds("history-sidebar").is_some());
        let (ring, edge) = cx.update(|_, cx| {
            let p = palette(cx);
            let solid = |color| Some(gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color))));
            ([solid(p.accent); 4], [solid(p.border); 4])
        });
        let focus_by_key = |cx: &mut VisualTestContext, handle: FocusHandle| {
            cx.update(|window, cx| window.focus(&handle, cx));
            cx.simulate_keystrokes("down");
            draw(cx);
        };
        let move_pointer = |cx: &mut VisualTestContext, list: &'static str| {
            let bounds = cx.debug_bounds(list).expect(list);
            for position in [bounds.center(), bounds.center() + point(px(30.), px(10.))] {
                cx.simulate_mouse_move(position, None, Modifiers::default());
                draw(cx);
            }
        };
        let (history, files) =
            app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));
        for (handle, list) in [(history, "history-pane"), (files.clone(), "files-pane")] {
            assert_eq!(painted_sides(cx, list), edge, "{list} at rest");
            focus_by_key(cx, handle.clone());
            assert_eq!(painted_sides(cx, list), ring, "{list} focused by a key");
            move_pointer(cx, list);
            assert!(
                cx.update(|window, _| handle.is_focused(window)),
                "{list} keeps focus"
            );
            assert_eq!(
                painted_sides(cx, list),
                ring,
                "{list} keeps its ring as the pointer moves"
            );
        }

        // A press in the list's empty space below its row keeps focus there
        // and ends the ring; a key shows it again.
        let files_list = cx.debug_bounds("files-pane").unwrap();
        let below = point(files_list.center().x, files_list.bottom() - px(6.));
        cx.simulate_mouse_down(below, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
        draw(cx);
        assert!(cx.update(|window, _| files.is_focused(window)));
        assert_eq!(painted_sides(cx, "files-pane"), edge, "pressed");
        move_pointer(cx, "files-pane");
        assert_eq!(painted_sides(cx, "files-pane"), edge, "pressed, then moved");
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(
            painted_sides(cx, "files-pane"),
            ring,
            "a key after the press"
        );

        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        focus_by_key(cx, files.clone());
        assert_eq!(painted_sides(cx, "working-pane"), ring, "Working Changes");
        move_pointer(cx, "working-pane");
        assert!(cx.update(|window, _| files.is_focused(window)));
        assert_eq!(
            painted_sides(cx, "working-pane"),
            ring,
            "Working Changes keeps its ring as the pointer moves"
        );
    }

    /// While an operation runs, the toolbar's Push, a primary Button, rests
    /// on the neutral fill the disabled Fetch and Pull beside it take,
    /// `subtle`, as the composer's disabled commit button does: the primary
    /// drew a 15% tint of the accent, grey #2a292f in Omarchy Retro-82's navy
    /// toolbar.
    #[gpui::test]
    async fn disabled_primary_actions_rest_on_the_neutral_fill(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.operation_busy = Some("Fetching remote…");
                cx.notify()
            })
        });
        let palettes = crate::appearance::test_palettes()
            .into_iter()
            .filter(|(name, _)| ["Midnight", "Braden", "Omarchy retro-82"].contains(&name.as_str()))
            .collect::<Vec<_>>();
        assert!(palettes.len() >= 2);
        for (name, palette) in palettes {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            draw(cx);
            let neutral =
                gpui_kit::Background::from(gpui_kit::Hsla::from(gpui_kit::rgb(palette.subtle)));
            for selector in ["fetch", "pull", "push", "commit-staged"] {
                assert_eq!(
                    painted_fills(cx, selector),
                    vec![neutral],
                    "{name} disabled {selector}"
                );
            }
        }
    }

    /// The edge colors of the quads the last frame painted exactly at
    /// `selector`'s bounds with a border on all four sides, each color once:
    /// GPUI paints a border once per side.
    pub(crate) fn painted_edges(
        cx: &mut VisualTestContext,
        selector: &'static str,
    ) -> Vec<gpui_kit::Hsla> {
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let near = |device: gpui_kit::ScaledPixels, logical: Pixels| {
                (device.as_f32() - logical.as_f32() * scale).abs() < 0.5
            };
            let mut edges = Vec::new();
            for quad in window.painted_quads() {
                let widths = quad.border_widths;
                if [widths.top, widths.right, widths.bottom, widths.left]
                    .into_iter()
                    .all(|width| width.as_f32() > 0.)
                    && near(quad.bounds.left(), bounds.left())
                    && near(quad.bounds.top(), bounds.top())
                    && near(quad.bounds.right(), bounds.right())
                    && near(quad.bounds.bottom(), bounds.bottom())
                    && !edges.contains(&quad.border_color)
                {
                    edges.push(quad.border_color);
                }
            }
            edges
        })
    }

    /// Disabled, the commit action rests on the neutral fill, which is the
    /// composer's own `subtle` (#edf1f7 on #edf1f7 in Braden, #131a25 on
    /// #131a25 in Midnight, 1.000:1), so it drew only its dim label, "Set
    /// your commit identity" under a heading of the same words. In every
    /// palette it keeps its shape: its fill, or else a 1 px edge 1.4:1 or more
    /// off the composer, stands off the surface beneath it, and it keeps the
    /// composer's width and its 36 px height.
    #[gpui::test]
    async fn the_disabled_commit_stands_off_the_composer(cx: &mut TestAppContext) {
        use crate::appearance::custom::contrast;
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.operation_busy = Some("Fetching remote…");
                cx.notify()
            })
        });
        let solid = |fill: gpui_kit::Background| {
            let color = fill.as_solid().expect("a solid fill").to_rgb();
            let channel = |value: f32| (value * 255.).round() as u32;
            (channel(color.r) << 16) | (channel(color.g) << 8) | channel(color.b)
        };
        let mut failures = Vec::new();
        let mut edged = 0;
        for (name, palette) in crate::appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            draw(cx);
            let [surface] = painted_fills(cx, "commit-composer")[..] else {
                panic!("{name}: the composer paints one fill");
            };
            let surface = solid(surface);
            let [fill] = painted_fills(cx, "commit-staged")[..] else {
                panic!("{name}: the commit action paints one fill");
            };
            let fill = solid(fill);
            if contrast(fill, surface) < 1.05 {
                edged += 1;
                match painted_edges(cx, "commit-staged")[..] {
                    [edge] if contrast(solid(edge.into()), surface) >= 1.4 => {}
                    ref edges => failures.push(format!(
                        "{name}: fill {fill:06x} on {surface:06x} with edges {edges:?}"
                    )),
                }
            }
            let (commit, composer) = (
                cx.debug_bounds("commit-staged").unwrap(),
                cx.debug_bounds("commit-composer").unwrap(),
            );
            assert_eq!(
                commit.size.height,
                crate::appearance::ui_size(36.),
                "{name}"
            );
            assert_eq!(
                composer.right() - commit.right(),
                commit.left() - composer.left(),
                "{name}: {commit:?} spans {composer:?}"
            );
        }
        assert!(
            edged > 0,
            "some palette rests the commit on its own surface"
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A pressed ghost Button takes the shared controls' pressed layer, the
    /// palette's `selected` over the panel, as a selected one keeps it.
    #[gpui::test]
    async fn pressed_ghost_buttons_take_the_selected_layer(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        for theme in [
            crate::appearance::ThemeChoice::Midnight,
            crate::appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|_, cx| theme.apply(None, cx));
            let pressed = cx.update(|_, cx| {
                let p = palette(cx);
                gpui_kit::Background::from(gpui_kit::Hsla::from(p.control_fill(p.selected)))
            });
            // The previous-change arrow held down.
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let arrow = cx
                .debug_bounds("previous-text-change")
                .expect("the arrow")
                .center();
            cx.simulate_mouse_move(arrow, None, Modifiers::default());
            cx.simulate_mouse_down(arrow, MouseButton::Left, Modifiers::default());
            draw(cx);
            assert_eq!(
                painted_fills(cx, "previous-text-change"),
                vec![pressed],
                "{theme:?} pressed arrow"
            );
            cx.simulate_mouse_up(arrow, MouseButton::Left, Modifiers::default());
            draw(cx);
        }
    }

    /// The narrow review row's change arrows are the kit's ghost Buttons.
    /// Hovered and enabled they take the shared controls' hover fill, the
    /// palette's hover step over the panel; disabled, they paint none.
    #[gpui::test]
    async fn narrow_review_arrows_lift_on_hover(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        cx.simulate_resize(size(px(461.), px(490.)));
        let fills = painted_fills;
        let hover_over = |cx: &mut VisualTestContext, selector: &'static str| {
            let center = cx.debug_bounds(selector).expect(selector).center();
            cx.simulate_mouse_move(center, None, Modifiers::default());
            draw(cx);
        };
        let away = |cx: &mut VisualTestContext| {
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            draw(cx);
        };
        for theme in [
            crate::appearance::ThemeChoice::Midnight,
            crate::appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|_, cx| theme.apply(None, cx));
            draw(cx);
            let hover = cx.update(|_, cx| {
                let p = palette(cx);
                gpui_kit::Background::from(gpui_kit::Hsla::from(p.control_fill(p.hover)))
            });
            for selector in ["previous-text-change", "next-text-change"] {
                away(cx);
                assert_eq!(fills(cx, selector), vec![], "{theme:?} {selector} at rest");
                hover_over(cx, selector);
                assert_eq!(
                    fills(cx, selector),
                    vec![hover],
                    "{theme:?} {selector} hovered"
                );
            }
        }
        // While an operation runs the arrows are disabled.
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.operation_busy = Some("Fetching remote…");
                cx.notify()
            })
        });
        for selector in ["previous-text-change", "next-text-change"] {
            away(cx);
            hover_over(cx, selector);
            assert_eq!(fills(cx, selector), vec![], "disabled {selector} hovered");
        }
    }

    /// The Options menu's toggle is a check menu item in its state, and its
    /// commands stay plain menu items. Reset review is announced disabled
    /// while there is nothing to reset, as its button is, and available once
    /// an option changes: the AccessKit node each item writes, which the kit
    /// names in its debug selector.
    #[gpui::test]
    async fn review_options_menu_announces_its_toggle_checked(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let open = |cx: &mut VisualTestContext| {
            let menu = cx
                .debug_bounds("text-review-menu")
                .expect("the Options menu button");
            cx.simulate_click(menu.center(), Modifiers::default());
            draw(cx);
        };
        let items = |cx: &mut VisualTestContext, checked: bool| {
            let (shown, other) = if checked {
                ("True", "False")
            } else {
                ("False", "True")
            };
            let reset = |disabled: bool| {
                format!(
                    "menu-item: Reset review · MenuItem{}",
                    if disabled { " disabled" } else { "" }
                )
            };
            for item in [
                format!("menu-item: Hide whitespace · MenuItemCheckBox {shown}"),
                "menu-item: More context, 3 lines shown · MenuItem".into(),
                // Nothing to reset until an option changes.
                reset(!checked),
            ] {
                assert!(cx.debug_bounds(item.clone().leak()).is_some(), "{item}");
            }
            for item in [
                format!("menu-item: Hide whitespace · MenuItemCheckBox {other}"),
                "menu-item: More context, 3 lines shown · MenuItem disabled".into(),
                reset(checked),
            ] {
                assert!(cx.debug_bounds(item.clone().leak()).is_none(), "not {item}");
            }
            // The caption the 461 px row has no room for closes the menu as
            // text in secondary text, not as a fourth, unavailable item.
            let caption = if checked {
                "Review filter active · exact source retained"
            } else {
                "Original Git diff"
            };
            // It reads as a heading by its 12 px medium, not by colour alone.
            let muted = cx.update(|_, cx| gpui_kit::component::Theme::global(cx).muted_foreground);
            for entry in [
                format!("menu-item: {caption} · Label"),
                format!("menu-label: {caption} · {:?}", gpui::Rgba::from(muted)),
                crate::native_accessibility::menu_tests::menu_label_style(
                    caption,
                    appearance::ui_text(appearance::MENU_LABEL_TEXT),
                    FontWeight::MEDIUM,
                ),
            ] {
                assert!(cx.debug_bounds(entry.clone().leak()).is_some(), "{entry}");
            }
            let item = format!("menu-item: {caption} · MenuItem disabled");
            assert!(cx.debug_bounds(item.clone().leak()).is_none(), "not {item}");
        };
        open(cx);
        items(cx, false);
        cx.simulate_keystrokes("down enter");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.review.options.hide_whitespace));
        open(cx);
        items(cx, true);
    }

    /// The context control leads with what it does and shows the context
    /// each hunk has now, "More context (3 lines)", in the full row and in
    /// the narrow Options menu. The button's accessible name says both in
    /// words, and each step relabels it until the last disables it.
    #[gpui::test]
    async fn context_control_says_what_it_does_and_how_much(cx: &mut TestAppContext) {
        use crate::text_review::{Options, context_button};
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        cx.simulate_resize(size(px(1200.), px(680.)));
        draw(cx);
        assert!(
            cx.debug_bounds("text-review-menu").is_none(),
            "the full row"
        );
        // A small button is its `text_sm` label with `px_2` on either side.
        let button_width = |cx: &mut VisualTestContext, label: &str| {
            cx.update(|window, _| {
                let rem = window.rem_size();
                line_widths(&[(label, rem * 0.875)], window) + rem
            })
        };
        for (context, label, name) in [
            (3, "More context (3 lines)", "More context, 3 lines shown"),
            (
                12,
                "More context (12 lines)",
                "More context, 12 lines shown",
            ),
            (
                48,
                "More context (48 lines)",
                "More context, 48 lines shown",
            ),
            (
                192,
                "More context (192 lines)",
                "More context, 192 lines shown",
            ),
        ] {
            assert_eq!(
                app.read_with(cx, |app, _| app.review.options.context),
                context
            );
            let button = cx.debug_bounds("expand-text-context").expect("the button");
            let width = button_width(cx, label);
            assert!(
                (button.size.width - width).abs() < px(1.),
                "{label:?} takes {width:?}, the button {button:?}"
            );
            let options = Options {
                context,
                ..Options::default()
            };
            let nodes = crate::worktrees::ring_room_tests::row_nodes(
                cx,
                vec![context_button(options, false)],
            );
            let [node] = nodes.as_slice() else {
                panic!("one node, not {nodes:?}");
            };
            assert_eq!(node.role(), gpui::accesskit::Role::Button);
            assert_eq!(node.label(), Some(name));
            assert_eq!(node.is_disabled(), context == 192, "{name}");
            cx.simulate_click(button.center(), Modifiers::default());
            settle(&app, cx).await;
        }
        assert_eq!(app.read_with(cx, |app, _| app.review.options.context), 192);

        // The narrow row's Options menu names the item as the button does,
        // in words, while it shows the button's label.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.set_text_review(Options::default(), window, cx)
            })
        });
        settle(&app, cx).await;
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let menu = cx
            .debug_bounds("text-review-menu")
            .expect("the Options menu");
        cx.simulate_click(menu.center(), Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("menu-item: More context, 3 lines shown · MenuItem")
                .is_some(),
            "the item takes the button's accessible name"
        );
        assert!(
            cx.debug_bounds("menu-item: More context (3 lines) · MenuItem")
                .is_none(),
            "not its visible label"
        );
    }

    /// "Original Git diff" and the other captions start where the buttons'
    /// labels do when they wrap to a line of their own.
    #[gpui::test]
    async fn review_caption_aligns_with_the_button_labels(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        // The longer caption no longer fits beside the full row.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.set_text_review(
                    crate::text_review::Options {
                        hide_whitespace: true,
                        context: 3,
                    },
                    window,
                    cx,
                )
            })
        });
        settle(&app, cx).await;
        let inset = cx.update(|window, _| window.rem_size() * 0.5);
        cx.simulate_resize(size(px(1100.), px(680.)));
        draw(cx);
        // The full row, its caption on a line of its own: the column holds
        // the row as it first appears, but not with the longer caption.
        let column = cx.debug_bounds("preview-content").unwrap().size.width;
        let needed = crate::text_review::OPTIONS_ROW
            .with(|cache| cache.borrow().as_ref().map(|(_, labels)| *labels))
            .expect("the row's laid-out labels")
            + cx.update(|window, _| window.rem_size() * 7.75);
        assert!(column >= needed, "column {column:?}, row {needed:?}");
        assert!(cx.debug_bounds("hide-whitespace-changes").is_some());
        assert!(cx.debug_bounds("text-review-menu").is_none());
        let first = cx.debug_bounds("previous-text-change").unwrap();
        let caption = cx.debug_bounds("text-review-caption").unwrap();
        assert!(
            caption.top() >= first.bottom(),
            "the caption wraps: {caption:?}"
        );
        assert_eq!(
            caption.left(),
            first.left() + inset,
            "caption {caption:?}, first button {first:?}"
        );
    }

    /// The collapsed row never shows a cut caption such as "Origin…". From
    /// the column its whole text needs beside the arrows and Options,
    /// measured from the interface font, the caption shows whole after
    /// Options; one pixel narrower it leaves the row and closes the Options
    /// menu instead. At 13 pt a 461 × 490 tile is narrower than that.
    #[gpui::test]
    async fn narrow_review_caption_shows_whole_or_leaves_the_row(cx: &mut TestAppContext) {
        use crate::text_review::collapsed_caption_width;
        const CAPTION: &str = "Original Git diff";
        const IN_MENU: &str = "menu-item: Original Git diff · Label";
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        // Whether the open Options menu ends with the caption.
        let menu_caption = |cx: &mut VisualTestContext| {
            let menu = cx
                .debug_bounds("text-review-menu")
                .expect("the Options menu");
            cx.simulate_click(menu.center(), Modifiers::default());
            draw(cx);
            let reset = cx
                .debug_bounds("menu-item: Reset review · MenuItem disabled")
                .expect("the menu opened");
            let caption = cx.debug_bounds(IN_MENU);
            if let Some(caption) = caption {
                assert!(caption.top() > reset.bottom(), "below the options");
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                cx.debug_bounds("menu-item: Reset review · MenuItem disabled")
                    .is_none(),
                "the menu closed"
            );
            caption.is_some()
        };
        // The caption's column and text, and the row's gap.
        let measure = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                (
                    collapsed_caption_width(CAPTION, window),
                    line_widths(&[(CAPTION, appearance::ui_text(11.))], window),
                    window.rem_size() * 0.25,
                )
            })
        };

        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(cx.debug_bounds("text-review-menu").is_some(), "collapsed");
        assert!(
            cx.debug_bounds("text-review-caption").is_none(),
            "no cut caption at 461"
        );
        assert!(menu_caption(cx), "the caption closes the menu at 461");

        // The window widened until the row spans exactly the caption's column.
        let (needed, text, gap) = measure(cx);
        let mut width = px(461.);
        let mut row = cx.debug_bounds("text-review-options").unwrap();
        for _ in 0..8 {
            if (row.size.width - needed).abs() < px(0.01) {
                break;
            }
            width += needed - row.size.width;
            cx.simulate_resize(size(width, px(490.)));
            draw(cx);
            row = cx.debug_bounds("text-review-options").unwrap();
        }
        assert!(
            (row.size.width - needed).abs() < px(0.01),
            "row {row:?}, needed {needed:?}"
        );
        let menu = cx
            .debug_bounds("text-review-menu")
            .expect("still collapsed");
        let caption = cx
            .debug_bounds("text-review-caption")
            .expect("the caption at its column");
        // The column holds the caption's text and no more than the
        // rounding: the Options button's margin gives back its inset.
        let spare = caption.size.width - text;
        assert!(
            spare > px(-0.5) && spare < px(3.),
            "{spare:?} beside the whole caption: {caption:?} in {row:?}"
        );
        assert!(
            caption.left() >= menu.right()
                && caption.top() < menu.bottom()
                && caption.right() <= row.right(),
            "caption {caption:?} beside Options {menu:?} in {row:?}"
        );
        // The Options trigger's `mr_neg_2`, which its dropdown places, gives
        // back the caption's text inset once: the caption's text starts one
        // gap after the button, as it does after Reset review in the full
        // row, to the device pixel.
        assert!(
            (caption.left() - (menu.right() + gap)).abs() < px(0.5),
            "caption {caption:?} one gap after Options {menu:?}"
        );
        assert!(!menu_caption(cx), "the row shows the caption");

        cx.simulate_resize(size(width - px(1.), px(490.)));
        draw(cx);
        assert!(cx.debug_bounds("text-review-menu").is_some(), "collapsed");
        assert!(
            cx.debug_bounds("text-review-caption").is_none(),
            "a pixel short of its column"
        );
        assert!(menu_caption(cx), "the caption closes the menu");

        // At 18 pt, in either half tile, the caption shows whole or not at all.
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        for (width, height) in [(461., 490.), (493., 526.)] {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let (needed, text, _) = measure(cx);
            let row = cx.debug_bounds("text-review-options").unwrap();
            assert!(cx.debug_bounds("text-review-menu").is_some(), "collapsed");
            let caption = cx.debug_bounds("text-review-caption");
            assert_eq!(
                caption.is_some(),
                row.size.width >= needed,
                "at 18 pt and {width}: row {row:?}, needed {needed:?}"
            );
            if let Some(caption) = caption {
                assert!(caption.size.width - text > px(-0.5), "{caption:?}");
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// With navigation hidden only by a narrow window, the shortcut, the rail
    /// button and the palette entry leave the saved choice alone instead of
    /// flipping one that nothing would show until the window widens.
    #[gpui::test]
    async fn narrow_history_keeps_the_navigation_choice(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;

        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|window, cx| {
            crate::shortcuts::bind_keys(cx);
            let focus = app.read(cx).app_focus.clone();
            window.focus(&focus, cx);
        });
        let toggle = if cfg!(target_os = "macos") {
            "cmd-b"
        } else {
            "ctrl-b"
        };
        let state = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let app = app.read(cx);
                (
                    app.sidebar,
                    app.history_sidebar,
                    app.menu_command_reason(CommandId::Sidebar, window),
                )
            })
        };
        let click_rail = |cx: &mut VisualTestContext| {
            let button = cx
                .debug_bounds("rail-sidebar")
                .expect("History's rail offers navigation");
            cx.simulate_click(button.center(), Modifiers::default());
            draw(cx);
        };
        let press_toggle = |cx: &mut VisualTestContext| {
            cx.simulate_keystrokes(toggle);
            draw(cx);
        };
        let resize = |cx: &mut VisualTestContext, width: f32, height: f32| {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
        };
        let rail =
            |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.rail_navigation.get());
        let narrow = Some("Widen the window to show branches and worktrees");

        // Wide, the shortcut hides the navigation and the rail offers it.
        assert_eq!(state(cx), (true, true, None));
        press_toggle(cx);
        assert_eq!(state(cx), (false, false, None));
        assert!(cx.debug_bounds("history-sidebar").is_none());
        let show = "Show branches and worktrees";
        assert_eq!(rail(cx), Some((show, show, false)));

        // Hidden by choice and then narrowed, nothing can show it: the rail
        // button, the shortcut and the palette entry keep the choice.
        resize(cx, 600., 718.);
        assert_eq!(state(cx), (false, false, narrow));
        assert_eq!(
            rail(cx),
            Some((
                "Show branches and worktrees. Unavailable: widen the window",
                "Widen the window to show branches and worktrees",
                true
            ))
        );
        click_rail(cx);
        press_toggle(cx);
        assert_eq!(state(cx), (false, false, narrow));

        // Widened, the rail button shows the navigation again.
        resize(cx, 1000., 680.);
        click_rail(cx);
        assert_eq!(state(cx), (true, true, None));
        assert!(cx.debug_bounds("history-sidebar").is_some());

        // Shown by choice and then narrowed, the shortcut keeps the choice,
        // so widening brings the navigation back.
        resize(cx, 600., 718.);
        press_toggle(cx);
        assert_eq!(state(cx), (true, true, narrow));
        assert!(cx.debug_bounds("history-sidebar").is_none());
        resize(cx, 1000., 680.);
        assert!(cx.debug_bounds("history-sidebar").is_some());
    }

    /// The boundaries of History's visible column headers, left to right, and
    /// the width History last recorded.
    fn history_columns(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> (f32, Vec<(Pixels, Pixels)>) {
        // The selector each header draws: `history-column-` and its
        // `ColumnId` discriminant.
        const HEADERS: [&str; columns::ColumnId::ALL.len()] = [
            "history-column-0",
            "history-column-1",
            "history-column-2",
            "history-column-3",
            "history-column-4",
            "history-column-5",
        ];
        let boundaries = HEADERS
            .into_iter()
            .filter_map(|header| cx.debug_bounds(header))
            .map(|bounds| (bounds.left(), bounds.right()))
            .collect();
        (app.read_with(cx, |app, _| app.history_width), boundaries)
    }

    /// Hiding navigation with the shortcut and showing it again leaves
    /// History's columns exactly where they were: the same window width gives
    /// the same layout, at the test window's scale factor and at fractional
    /// ones, from the navigation's default width and from a dragged,
    /// fractional one.
    #[gpui::test]
    async fn history_columns_return_after_toggling_navigation(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|window, cx| {
            crate::shortcuts::bind_keys(cx);
            let focus = app.read(cx).app_focus.clone();
            window.focus(&focus, cx);
        });
        let toggle = if cfg!(target_os = "macos") {
            "cmd-b"
        } else {
            "ctrl-b"
        };
        // 1398 px at 1.25 is where a width kept within a pixel moved Subject's
        // right edge, and every column after it, by 0.8 px.
        for width in [1000., 1480., 1398.] {
            for scale in [2., 1., 1.25, 1.5] {
                cx.simulate_resize(size(px(width), px(800.)));
                cx.update(|window, _| window.set_scale_factor(scale));
                draw(cx);
                let before = history_columns(&app, cx);
                assert!(cx.debug_bounds("history-sidebar").is_some());
                assert!(!before.1.is_empty(), "History draws its column headers");
                cx.simulate_keystrokes(toggle);
                draw(cx);
                assert!(cx.debug_bounds("history-sidebar").is_none());
                cx.simulate_keystrokes(toggle);
                draw(cx);
                assert!(cx.debug_bounds("history-sidebar").is_some());
                assert_eq!(
                    history_columns(&app, cx),
                    before,
                    "at {width} px and scale factor {scale}"
                );
            }
        }
    }

    /// History records a new width, and so lays its columns out again, only
    /// when the width it measures changes: frames at an unchanged width,
    /// including at a fractional scale factor, notify nothing.
    #[gpui::test]
    async fn history_lays_out_again_only_for_a_new_width(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        let taken = || HISTORY_WIDTHS.with(std::cell::Cell::get);
        let width = |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.history_width);
        for scale in [2., 1.25] {
            for window in [1398., 1213.] {
                let at = format!("at {window} px and scale factor {scale}");
                let before = taken();
                cx.simulate_resize(size(px(window), px(800.)));
                cx.update(|window, _| window.set_scale_factor(scale));
                draw(cx);
                // The resize measures at the test window's own factor
                // before the fractional one applies, so it may take two.
                assert!(taken() > before, "a new width {at}");
                let measured = width(cx);
                let before = taken();
                for _ in 0..4 {
                    cx.update(|window, _| window.refresh());
                    draw(cx);
                }
                assert_eq!(
                    (taken() - before, width(cx)),
                    (0, measured),
                    "no layout for an unchanged width {at}"
                );
            }
        }
    }

    /// The newest commit of [`references_script`], a merge that HEAD's
    /// `main`, `feature/alpha`, `release` and `origin/main` point at.
    const MERGE_SUBJECT: &str =
        "Merge the side work so this subject has to truncate in narrow History";
    /// The side branch's commit, which `side` alone points at, on lane 1.
    const SIDE_SUBJECT: &str = "Side work";

    /// A `git fast-import` stream of `linear` commits on `main`, then a side
    /// commit and a main commit from the last of them and a merge of both on
    /// `main`, so the graph has two lanes. The merge carries four
    /// references, HEAD's branch second in Git's order; the side commit one.
    fn references_script(linear: usize) -> String {
        let mut input = String::new();
        let mut commit = |reference: &str,
                          mark: usize,
                          from: Option<usize>,
                          merge: Option<usize>,
                          message: &str| {
            input.push_str(&format!(
                "commit {reference}\nmark :{mark}\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\n",
                1_700_000_000 + mark,
                message.len(),
            ));
            if let Some(from) = from {
                input.push_str(&format!("from :{from}\n"));
            }
            if let Some(merge) = merge {
                input.push_str(&format!("merge :{merge}\n"));
            }
            input.push_str(&format!(
                "M 100644 inline notes.txt\ndata {}\n{mark}\n\n",
                mark.to_string().len() + 1
            ));
        };
        for mark in 1..=linear {
            commit(
                "refs/heads/main",
                mark,
                None,
                None,
                &format!("Linear {mark}"),
            );
        }
        commit(
            "refs/heads/side",
            linear + 1,
            Some(linear),
            None,
            SIDE_SUBJECT,
        );
        commit(
            "refs/heads/main",
            linear + 2,
            Some(linear),
            None,
            "Main work",
        );
        commit(
            "refs/heads/main",
            linear + 3,
            None,
            Some(linear + 1),
            MERGE_SUBJECT,
        );
        for reference in [
            "refs/heads/feature/alpha",
            "refs/heads/release",
            "refs/remotes/origin/main",
        ] {
            input.push_str(&format!("reset {reference}\nfrom :{}\n\n", linear + 3));
        }
        input + "done\n"
    }

    /// The bounds the last frame drew `selector` at, for a selector built at
    /// run time.
    fn bounds_of(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
        cx.debug_bounds(selector.leak())
    }

    /// The index of the loaded commit whose subject is `subject`.
    fn commit_index(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, subject: &str) -> usize {
        app.read_with(cx, |app, _| {
            app.commits
                .iter()
                .position(|commit| commit.subject == subject)
                .expect(subject)
        })
    }

    /// In a 461 × 490 half tile and a 640 × 680 window, at 13 and 18 pt,
    /// History is too narrow for the chosen References and Graph to leave
    /// the subject anything readable. The subject keeps at least its floor,
    /// about 120 px scaled with the interface size, inside the viewport;
    /// References fold into one pill at its start, HEAD's branch and "+3",
    /// in the badge's colours, with the whole list in the row's tooltip and
    /// accessible name; and the graph takes its compact pitch, its selected
    /// node on its lane and centred in its row.
    #[gpui::test]
    async fn narrow_history_keeps_the_subject_and_folds_references(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        let merge = commit_index(&app, cx, MERGE_SUBJECT);
        let side = commit_index(&app, cx, SIDE_SUBJECT);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(side, window, cx)));
        settle(&app, cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let (lanes, side_lane, merge_color) = app.read_with(cx, |app, _| {
            (
                app.graph_lanes,
                app.graph[side].lane,
                app.graph[merge].color,
            )
        });
        assert_eq!((lanes, side_lane), (2, 1), "the fixture draws two lanes");
        let list = "main\nfeature/alpha\nrelease\norigin/main";
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for (width, height) in [(461., 490.), (640., 680.)] {
                HISTORY_PILLS.with(|pills| pills.borrow_mut().clear());
                HISTORY_ROW_NAMES.with(|names| names.borrow_mut().clear());
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{width} × {height} at {text} pt");
                let (floor, available, spacing) = app.read_with(cx, |app, _| {
                    (
                        app.history_subject_floor.get(),
                        app.history_width - 26.,
                        app.lane_spacing(),
                    )
                });
                let scale = appearance::ui_scale();
                assert!(
                    (100. * scale..=140. * scale).contains(&floor),
                    "a floor of about 120 px scaled {at}: {floor}"
                );
                assert!(
                    cx.debug_bounds("history-column-0").is_none(),
                    "References fold {at}"
                );

                // The subject cell, inside the viewport the History column
                // shows, keeps its floor.
                let view = cx.debug_bounds("history-scope-toolbar").unwrap();
                for row in [merge, side] {
                    let cell = bounds_of(cx, format!("history-subject-{row}")).unwrap();
                    let visible = cell.right().min(view.right()) - cell.left().max(view.left());
                    assert!(
                        visible >= px(floor) - px(0.5),
                        "row {row}'s subject shows {visible:?} of {floor} {at}: {cell:?} in {view:?}"
                    );
                }

                // One pill at the subject's start, at most half its text.
                let cell = bounds_of(cx, format!("history-subject-{merge}")).unwrap();
                let pill = bounds_of(cx, format!("history-ref-pill-{merge}")).unwrap();
                assert!(
                    (pill.left() - cell.left() - px(10.)).abs() < px(0.5)
                        && pill.right() <= cell.left() + cell.size.width / 2.
                        && pill.top() >= cell.top()
                        && pill.bottom() <= cell.bottom(),
                    "the pill {pill:?} at the start of {cell:?} {at}"
                );
                let label = cx.update(|_, cx| appearance::labels(cx).refs[merge_color % 6]);
                assert_eq!(
                    HISTORY_PILLS.with(|pills| pills.borrow().get(&merge).cloned()),
                    Some(("main".to_owned(), Some("+3".to_owned()), label)),
                    "HEAD's branch and the others' count {at}"
                );
                assert_eq!(
                    HISTORY_PILLS
                        .with(|pills| pills.borrow().get(&side).map(|pill| pill.1.clone())),
                    Some(None),
                    "a lone reference has no count {at}"
                );
                let lane = cx.update(|_, cx| graph::colors(cx)[merge_color % 6]);
                assert_eq!(
                    painted_fills(cx, format!("history-ref-pill-{merge}").leak()),
                    vec![gpui_kit::Background::from(gpui_kit::Hsla::from(rgba(
                        (lane << 8) | crate::appearance::REF_TINT_ALPHA
                    )))],
                    "the badge's tint {at}"
                );
                let (name, tooltip) = HISTORY_ROW_NAMES
                    .with(|names| names.borrow().get(&merge).cloned())
                    .unwrap();
                assert!(
                    name.starts_with(&format!(
                        "{MERGE_SUBJECT} · References: main, feature/alpha, release, origin/main · "
                    )) && name.ends_with(" · 2 parents · merge commit"),
                    "{name} {at}"
                );
                assert_eq!(tooltip.as_deref(), Some(list), "{at}");

                // The graph at half the pitch, within what the floor leaves.
                let compact = graph::compact_spacing(spacing);
                assert!(compact < spacing, "{at}");
                let expected = graph::required_width(lanes, compact)
                    .min(available - floor)
                    .max(graph::required_width(1, compact));
                let column = cx.debug_bounds("history-column-1").unwrap();
                assert!(
                    (f32::from(column.size.width) - expected).abs() < 0.5
                        && f32::from(column.size.width) < graph::required_width(lanes, spacing),
                    "a compact graph of {expected} {at}: {column:?}"
                );
                // The selected side commit's node, 5 px in radius, on lane 1
                // at the compact pitch in its row's graph cell, which ends
                // where the row's subject cell starts, and centred in its row.
                let side_cell = bounds_of(cx, format!("history-subject-{side}")).unwrap();
                let right = f32::from(side_cell.left());
                let left = right - f32::from(column.size.width);
                let nodes: Vec<_> = cx.update(|window, _| {
                    let scale = window.scale_factor();
                    let near = |device: gpui_kit::ScaledPixels, logical: f32| {
                        (device.as_f32() / scale - logical).abs() < 0.01
                    };
                    window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| {
                            near(quad.bounds.size.width, 10.)
                                && near(quad.bounds.size.height, 10.)
                                && near(quad.corner_radii.top_left, 5.)
                        })
                        .map(|quad| {
                            (
                                (quad.bounds.left().as_f32() + quad.bounds.right().as_f32())
                                    / 2.
                                    / scale,
                                (quad.bounds.top().as_f32() + quad.bounds.bottom().as_f32())
                                    / 2.
                                    / scale,
                            )
                        })
                        .filter(|(x, _)| (left..right).contains(x))
                        .collect()
                });
                let [(x, y)] = nodes.as_slice() else {
                    panic!("one selected node in {left}–{right} {at}: {nodes:?}");
                };
                // Paint snaps the node to device pixels, half a pixel here.
                assert!(
                    (x - (left + 10. + compact)).abs() <= 0.25,
                    "the selected node at x {x} on lane 1 of {left}–{right} {at}"
                );
                assert!(
                    (y - f32::from(side_cell.center().y)).abs() < 0.5,
                    "the selected node at y {y} beside its subject {side_cell:?} {at}"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Narrow History changes no preference: widening the window gives the
    /// chosen columns back exactly, from 461 and 640 px at 13 and 18 pt.
    #[gpui::test]
    async fn widening_narrow_history_restores_the_chosen_columns(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        let settings = app.read_with(cx, |app, _| app.settings.clone());
        let chosen = history_columns(&app, cx);
        assert_eq!(chosen.1.len(), 6, "the 1000 px window shows every column");
        for text in [13, 18] {
            for (width, height) in [(461., 490.), (640., 680.)] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{width} × {height} at {text} pt");
                assert!(
                    app.read_with(cx, |app, _| app.history_column_layout().narrow),
                    "narrow {at}"
                );
                assert_eq!(history_columns(&app, cx).1.len(), 5, "References fold {at}");
                cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
                cx.simulate_resize(size(px(1000.), px(680.)));
                draw(cx);
                assert_eq!(history_columns(&app, cx), chosen, "after {at}");
                assert_eq!(
                    app.read_with(cx, |app, _| app.settings.clone()),
                    settings,
                    "the preferences after {at}"
                );
            }
        }
    }

    /// A steady narrow History frame, and a scroll through its rows, ask the
    /// text system nothing: the floor and the Graph title are measured once
    /// for the font and sizes, and the pills are not measured at all.
    #[gpui::test]
    async fn narrow_history_scrolls_without_measuring_text(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(60)).await;
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.history_column_layout().narrow));
        let merge = commit_index(&app, cx, MERGE_SUBJECT);
        let first = bounds_of(cx, format!("history-subject-{merge}")).unwrap();
        let layouts = || LINE_LAYOUTS.with(std::cell::Cell::get);
        let before = layouts();
        app.update(cx, |_, cx| cx.notify());
        draw(cx);
        let row = app.read_with(cx, |app, _| app.settings.density.history_row_height());
        for _ in 0..4 {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: first.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-row * 3.))),
                modifiers: Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            draw(cx);
        }
        assert_ne!(
            bounds_of(cx, format!("history-subject-{merge}")),
            Some(first),
            "the rows scrolled"
        );
        assert_eq!(
            layouts(),
            before,
            "text laid out by a steady or scrolled frame"
        );
    }

    /// Toggle Sidebar in a History window too narrow for navigation changes
    /// nothing on screen, so the rail announces why with a polite status node,
    /// a fresh one for every press. A wide window announces nothing, and
    /// neither does narrowing again after widening or leaving History.
    #[gpui::test]
    async fn narrow_history_announces_the_refused_toggle(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|window, cx| {
            crate::shortcuts::bind_keys(cx);
            let focus = app.read(cx).app_focus.clone();
            window.focus(&focus, cx);
        });
        let toggle = if cfg!(target_os = "macos") {
            "cmd-b"
        } else {
            "ctrl-b"
        };
        let press_toggle = |cx: &mut VisualTestContext| {
            cx.simulate_keystrokes(toggle);
            draw(cx);
        };
        let resize = |cx: &mut VisualTestContext, width: f32, height: f32| {
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
        };
        let choice = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| (app.sidebar, app.history_sidebar))
        };
        // The status node the last frame drew: its refusal and its name.
        let announced = |cx: &mut VisualTestContext| {
            cx.debug_bounds("history-navigation-unavailable")
                .and_then(|_| app.read_with(cx, |app, _| app.rail_announcement.get()))
        };
        let message = "Branches and worktrees unavailable: widen the window";

        // Wide, the shortcut hides and shows navigation without a word.
        press_toggle(cx);
        assert_eq!(choice(cx), (false, false));
        assert_eq!(announced(cx), None);
        press_toggle(cx);
        assert_eq!(choice(cx), (true, true));
        assert_eq!(announced(cx), None);

        // Narrowing alone announces nothing. Each refused press keeps the
        // choice and draws a fresh node, so it is heard again.
        resize(cx, 600., 718.);
        assert_eq!(announced(cx), None);
        press_toggle(cx);
        assert_eq!(choice(cx), (true, true));
        assert_eq!(announced(cx), Some((1, message)));
        press_toggle(cx);
        assert_eq!(choice(cx), (true, true));
        assert_eq!(announced(cx), Some((2, message)));

        // Widening shows the navigation and forgets the refusals.
        resize(cx, 1000., 680.);
        assert!(cx.debug_bounds("history-sidebar").is_some());
        assert_eq!(announced(cx), None);
        resize(cx, 600., 718.);
        assert_eq!(announced(cx), None);

        // So does leaving History for Compare and coming back.
        press_toggle(cx);
        assert_eq!(announced(cx), Some((1, message)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
        cx.update(|window, cx| app.update(cx, |app, cx| app.back_to_history(window, cx)));
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::History));
        assert_eq!(announced(cx), None);
        assert_eq!(choice(cx), (true, true));
    }

    #[gpui::test]
    fn resized_history_rows_clear_the_horizontal_scrollbar(cx: &mut TestAppContext) {
        struct Probe {
            vertical: UniformListScrollHandle,
            horizontal: ScrollHandle,
            row_height: Pixels,
        }
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let rows = uniform_list(
                    "history-viewport-test-rows",
                    8,
                    cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                        range
                            .map(|index| {
                                div()
                                    .id(("history-viewport-test-row", index))
                                    .debug_selector(move || {
                                        format!("history-viewport-test-row-{index}")
                                    })
                                    .h(this.row_height)
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .child(format!("Commit {index}: enlarged history text"))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .track_scroll(&self.vertical);
                div()
                    .relative()
                    .size_full()
                    .flex()
                    .flex_col()
                    // The fixed surrounding chrome leaves a short history pane
                    // at the minimum window, as in the native regression.
                    .child(div().h(px(430.)).flex_shrink_0())
                    .child(history_scroll_viewport(
                        &self.horizontal,
                        div()
                            .w(px(1800.))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .child(div().h(px(47.)).flex_shrink_0().child("History columns"))
                            .child(
                                div()
                                    .border_1()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(rows),
                            ),
                    ))
                    .child(Scrollbar::horizontal(&self.horizontal).mode(ScrollbarMode::Always))
            }
        }
        cx.update(gpui_kit::init);
        let (probe, cx) = cx.add_window_view(|_, _| Probe {
            vertical: UniformListScrollHandle::new(),
            horizontal: ScrollHandle::new(),
            row_height: px(28.),
        });
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..2 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
            }
        }
        fn assert_clear(cx: &mut VisualTestContext, probe: &Entity<Probe>, selector: &'static str) {
            let row = cx.debug_bounds(selector).expect("selected row is rendered");
            let (viewport, track_top) = cx.read(|cx| {
                let probe = probe.read(cx);
                (
                    probe.vertical.0.borrow().base_handle.bounds(),
                    probe.horizontal.bounds().bottom() - Scrollbar::width(),
                )
            });
            assert!(
                row.top() >= viewport.top(),
                "row top must be inside its bordered viewport: {row:?} / {viewport:?}"
            );
            assert!(
                row.bottom() <= viewport.bottom(),
                "entire row must be inside its viewport: {row:?} / {viewport:?}"
            );
            assert!(
                row.bottom() <= track_top,
                "entire row must clear the painted horizontal track: {row:?}, track top={track_top:?}"
            );
        }
        cx.simulate_resize(size(px(1480.), px(981.)));
        draw(cx);
        assert_clear(cx, &probe, "history-viewport-test-row-5");

        probe.update(cx, |probe, cx| {
            probe.row_height = px(28. * 18. / 13.);
            cx.notify();
        });
        cx.simulate_resize(size(px(1000.), px(680.)));
        probe.update(cx, |probe, _| {
            probe.vertical.scroll_to_item(5, ScrollStrategy::Nearest)
        });
        draw(cx);
        assert_clear(cx, &probe, "history-viewport-test-row-5");

        // The final item must also clear the track at the maximum scroll limit.
        probe.update(cx, |probe, _| {
            probe.vertical.scroll_to_item(7, ScrollStrategy::Nearest);
            probe.horizontal.set_offset(point(px(-250.), px(0.)));
        });
        draw(cx);
        assert_clear(cx, &probe, "history-viewport-test-row-7");
        assert_eq!(
            probe.read_with(cx, |probe, _| probe.horizontal.offset().x),
            px(-250.)
        );
    }

    #[test]
    fn list_resize_reveals_selection_without_replacing_restored_or_manual_scroll() {
        let scroll = UniformListScrollHandle::new();
        let restored = point(px(-20.), px(-180.));
        scroll.0.borrow().base_handle.set_offset(restored);
        let mut previous = None;
        let ordinary = (size(px(300.), px(400.)), px(34.));
        assert!(!reveal_resized_selection(
            &mut previous,
            ordinary,
            Some(12),
            &scroll
        ));
        assert_eq!(scroll.0.borrow().base_handle.offset(), restored);
        assert!(scroll.0.borrow().deferred_scroll_to_item.is_none());

        // Scrolling or selecting another row at the same geometry must not
        // repeatedly pull the view back to the selected row during painting.
        assert!(!reveal_resized_selection(
            &mut previous,
            ordinary,
            Some(18),
            &scroll
        ));
        assert!(scroll.0.borrow().deferred_scroll_to_item.is_none());

        let short = (size(px(300.), px(150.)), px(34.));
        assert!(reveal_resized_selection(
            &mut previous,
            short,
            Some(18),
            &scroll
        ));
        let request = scroll
            .0
            .borrow_mut()
            .deferred_scroll_to_item
            .take()
            .unwrap();
        assert_eq!(request.item_index, 18);
        assert_eq!(request.strategy, ScrollStrategy::Nearest);
        assert!(!request.scroll_strict);

        // Font/density changes can change row heights without changing the pane.
        let enlarged = (short.0, px(47.));
        assert!(reveal_resized_selection(
            &mut previous,
            enlarged,
            Some(7),
            &scroll
        ));
        assert_eq!(
            scroll
                .0
                .borrow_mut()
                .deferred_scroll_to_item
                .take()
                .unwrap()
                .item_index,
            7
        );
        assert!(!reveal_resized_selection(
            &mut previous,
            ordinary,
            None,
            &scroll
        ));
        assert!(scroll.0.borrow().deferred_scroll_to_item.is_none());
    }

    /// Run `git` in a fixture repository without the developer's config.
    fn fixture_git(repository: &std::path::Path, args: &[&str]) {
        let mut command = std::process::Command::new("git");
        command
            .arg("-C")
            .arg(repository)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Draw, advancing simulated time past debounces, until `done` holds
    /// while the reader and operation executors answer on their threads.
    fn wait_until(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        what: &str,
        mut done: impl FnMut(&GitTurtle) -> bool,
    ) {
        for _ in 0..1000 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if app.read_with(cx, |app, _| done(app)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{what} never happened");
    }

    /// Scope History of the open fixture to `label`'s `scope`.
    async fn scope_history(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        label: &str,
        scope: worker::Scope,
    ) {
        let label = label.to_owned();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let path = app.path.clone().unwrap();
                app.open(path, Some((label, scope)), window, cx)
            })
        });
        settle(app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.scope.is_some(), "the scope opened");
            assert_eq!(app.error, None);
        });
    }

    /// Press the platform's Refresh shortcut (Ctrl+R, Cmd+R on macOS).
    async fn press_refresh(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            crate::shortcuts::bind_keys(cx);
            let focus = app.read(cx).app_focus.clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-r"
        } else {
            "ctrl-r"
        });
        settle(app, cx).await;
    }

    /// After Refresh found the scope gone: All history lists the commits, the
    /// tab is available, and one polite status names `vanished`.
    fn assert_all_history_with_one_explanation(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        vanished: &str,
    ) {
        draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.scope, None, "Refresh opened All history");
            assert_eq!(app.error, None, "never 'Could not open repository'");
            assert!(!app.commits.is_empty() && !app.visible.is_empty());
            let tab = app
                .repository_tabs
                .active
                .and_then(|index| app.repository_tabs.tabs.get(index))
                .expect("the repository has a tab");
            assert_eq!(tab.error, None, "the tab has no 'unavailable' suffix");
            assert_eq!(app.operation_error, None, "no assertive alert");
            let notice = app.operation_notice.as_deref().unwrap_or_default();
            assert!(
                notice.starts_with("History scope changed: ")
                    && notice.contains(vanished)
                    && notice.ends_with("Showing All history."),
                "{notice}"
            );
        });
        // The explanation is the polite Status node, drawn once; the
        // assertive error summary is absent.
        assert!(cx.debug_bounds("operation-notice-summary").is_some());
        assert!(cx.debug_bounds("operation-error-summary").is_none());
        assert!(cx.debug_bounds("history-scope-name").is_some());
    }

    #[gpui::test]
    async fn refresh_opens_all_history_when_the_scoped_branch_is_gone(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().unwrap());
        fixture_git(&repository, &["branch", "departing"]);
        scope_history(
            &app,
            cx,
            "departing",
            worker::Scope::Branch {
                name: "departing".into(),
                remote: false,
            },
        )
        .await;
        fixture_git(&repository, &["branch", "-D", "departing"]);
        press_refresh(&app, cx).await;
        assert_all_history_with_one_explanation(&app, cx, "Local branch 'departing'");
        // Refreshing again is ordinary All history: nothing further to explain.
        cx.update(|_, cx| app.update(cx, |app, _| app.operation_notice = None));
        press_refresh(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.scope, None);
            assert_eq!(app.operation_notice, None);
            assert_eq!(app.operation_error, None);
        });
    }

    #[gpui::test]
    async fn refresh_opens_all_history_when_the_scoped_worktree_is_gone(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().unwrap());
        let linked = fixture.path().join("linked-tree");
        fixture_git(
            &repository,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "side",
                linked.to_str().unwrap(),
            ],
        );
        press_refresh(&app, cx).await;
        let path = app.read_with(cx, |app, _| {
            app.worktrees
                .iter()
                .find(|worktree| worktree.branch.as_deref() == Some("side"))
                .expect("the linked worktree is listed")
                .path
                .clone()
        });
        scope_history(&app, cx, "linked-tree", worker::Scope::Worktree { path }).await;
        fixture_git(
            &repository,
            &["worktree", "remove", "--force", linked.to_str().unwrap()],
        );
        press_refresh(&app, cx).await;
        assert_all_history_with_one_explanation(&app, cx, "linked-tree");
    }

    /// At 18 pt in the 1000 × 680 minimum window, Refresh's explanation of a
    /// long vanished branch wraps instead of truncating: the banner grows to
    /// hold every line, the whole text lies inside it and the window, and
    /// Dismiss stays reachable beside it.
    #[gpui::test]
    async fn the_vanished_scope_explanation_wraps_whole_at_18_pt(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        // A short notice gives the banner's one-line height at this size.
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.operation_notice = Some("Profile preferences saved".into());
                cx.notify();
            })
        });
        draw(cx);
        let one_line = cx
            .debug_bounds("operation-notice-text")
            .expect("the short notice is drawn")
            .size
            .height;
        cx.update(|_, cx| app.update(cx, |app, _| app.operation_notice = None));

        let branch = "feature/refresh-explains-a-vanished-branch";
        let repository = app.read_with(cx, |app, _| app.path.clone().unwrap());
        fixture_git(&repository, &["branch", branch]);
        scope_history(
            &app,
            cx,
            branch,
            worker::Scope::Branch {
                name: branch.into(),
                remote: false,
            },
        )
        .await;
        fixture_git(&repository, &["branch", "-D", branch]);
        press_refresh(&app, cx).await;
        assert_all_history_with_one_explanation(&app, cx, &format!("Local branch '{branch}'"));

        let viewport = cx.update(|window, _| window.viewport_size());
        assert_eq!(viewport, size(px(1000.), px(680.)), "the minimum window");
        let summary = cx.debug_bounds("operation-notice-summary").unwrap();
        let text = cx.debug_bounds("operation-notice-text").unwrap();
        let dismiss = cx.debug_bounds("dismiss-operation-notice").unwrap();
        assert!(
            text.size.height >= one_line * 2. - px(0.5),
            "the explanation wraps onto at least two lines: {text:?}, one line {one_line:?}"
        );
        // Every wrapped line is inside the drawn summary (nothing clipped or
        // scrolled out of view), which ends before Dismiss and inside the
        // window.
        assert!(
            summary.left() <= text.left()
                && text.right() <= summary.right()
                && summary.top() <= text.top()
                && text.bottom() <= summary.bottom(),
            "the whole explanation is visible: {text:?} in {summary:?}"
        );
        assert!(
            summary.right() <= dismiss.left()
                && dismiss.right() <= viewport.width
                && summary.bottom() <= viewport.height,
            "summary {summary:?}, Dismiss {dismiss:?}"
        );
        cx.simulate_click(dismiss.center(), Modifiers::default());
        draw(cx);
        app.read_with(cx, |app, _| assert_eq!(app.operation_notice, None));
        assert!(cx.debug_bounds("operation-notice-summary").is_none());
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Writes after the scoped branch was deleted report the vanished scope
    /// once; dismissed, it stays dismissed through later writes and quiet
    /// refreshes, while the displayed history and its scope are kept.
    #[gpui::test]
    async fn a_vanished_scope_is_reported_once_across_later_writes(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().unwrap());
        fixture_git(&repository, &["branch", "departing"]);
        scope_history(
            &app,
            cx,
            "departing",
            worker::Scope::Branch {
                name: "departing".into(),
                remote: false,
            },
        )
        .await;
        let reports = std::rc::Rc::new(std::cell::Cell::new(0));
        let scope_report = |app: &GitTurtle| {
            app.operation_error
                .as_deref()
                .is_some_and(|error| error.starts_with("History scope changed: "))
        };
        let write = |cx: &mut VisualTestContext,
                     command: gitturtle_core::WriteCommand,
                     branch: &'static str| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.write(command, "Writing…", window, cx))
            });
            let counted = reports.clone();
            let mut seen = false;
            // Count each report as it appears, until the write's quiet refresh
            // installed the branch list that follows it.
            wait_until(cx, &app, "the write's quiet refresh", |app| {
                if scope_report(app) && !seen {
                    seen = true;
                    counted.set(counted.get() + 1);
                }
                app.operation_busy.is_none()
                    && app.branches.iter().any(|b| b.name == branch) == (branch != "departing")
            });
        };
        let plan = app.read_with(cx, |app, _| {
            app.repository
                .as_ref()
                .unwrap()
                .branch_plan("departing")
                .unwrap()
        });
        write(
            cx,
            gitturtle_core::WriteCommand::Branch(
                gitturtle_core::BranchCommand::Delete { plan }.into(),
            ),
            "departing",
        );
        assert_eq!(reports.get(), 1, "deleting the scoped branch reports it");
        // Dismiss the report and the History updated status.
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.operation_error = None;
                app.history_updates.pending = None;
            })
        });
        for branch in ["first-later", "second-later"] {
            write(
                cx,
                gitturtle_core::WriteCommand::CreateBranch {
                    name: branch.into(),
                    start_point: None,
                },
                branch,
            );
        }
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    local_refresh::LocalChange {
                        rescan: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        settle(&app, cx).await;
        assert_eq!(reports.get(), 1, "two later writes raise no second report");
        app.read_with(cx, |app, _| {
            assert!(!scope_report(app));
            assert_eq!(app.history_updates.pending, None);
            assert!(
                app.scope.is_some(),
                "quiet refresh keeps the displayed history"
            );
            assert!(!app.commits.is_empty());
        });
        assert!(cx.debug_bounds("operation-error-summary").is_none());
    }

    #[gpui::test]
    async fn a_narrow_window_centers_the_search_empty_state_in_view(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        // Half of a 1690 px tiled display, where the columns scroll sideways.
        cx.simulate_resize(size(px(845.), px(980.)));
        draw(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.search.update(cx, |input, cx| {
                    input.set_value("no such commit", window, cx)
                });
                app.history_query_changed(window, cx);
            })
        });
        wait_until(cx, &app, "the search to finish", |app| {
            app.history_search_empty()
                .is_some_and(|(title, _)| title == "No matching commits")
        });
        app.read_with(cx, |app, _| {
            assert!(app.history_column_layout().content_width + 26. > app.history_width);
        });
        let list = cx.debug_bounds("history-scope-toolbar").unwrap();
        let state = cx.debug_bounds("history-empty-state").unwrap();
        assert!(
            state.left() >= list.left() && state.right() <= list.right(),
            "empty state {state:?} outside the visible list {list:?}"
        );
    }

    #[gpui::test]
    async fn search_under_a_vanished_scope_reports_the_scope_change(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        let repository = app.read_with(cx, |app, _| app.path.clone().unwrap());
        fixture_git(&repository, &["branch", "departing"]);
        scope_history(
            &app,
            cx,
            "departing",
            worker::Scope::Branch {
                name: "departing".into(),
                remote: false,
            },
        )
        .await;
        fixture_git(&repository, &["branch", "-D", "departing"]);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.search
                    .update(cx, |input, cx| input.set_value("fixture", window, cx));
                app.history_query_changed(window, cx);
            })
        });
        wait_until(cx, &app, "the search reply", |app| {
            app.history_search_empty()
                .is_some_and(|(title, _)| title == "Search could not complete")
        });
        app.read_with(cx, |app, _| {
            let (_, description) = app.history_search_empty().unwrap();
            assert!(
                description.starts_with("History scope changed: Local branch 'departing'"),
                "{description}"
            );
            assert_eq!(app.error, None, "never 'Could not open repository'");
            let tab = &app.repository_tabs.tabs[app.repository_tabs.active.unwrap()];
            assert_eq!(tab.error, None);
        });
    }

    /// With Working Changes focused, Copy Selected List Item takes every
    /// selected file's path once, in list order; a message selection made in
    /// History before opening Working Changes no longer wins.
    #[gpui::test]
    async fn working_list_copy_takes_the_selected_paths(cx: &mut TestAppContext) {
        use crate::commit_message::tests::{copy_chord, message_selection};
        use gitturtle_core::ChangeArea;
        let (_fixture, app, cx) = history_window(cx).await;
        let root = cx.read(|cx| app.read(cx).repository.as_ref().unwrap().path().to_owned());
        std::fs::write(root.join("notes.txt"), "staged\n").unwrap();
        let added = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["add", "notes.txt"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(added.status.success());
        std::fs::write(root.join("notes.txt"), "staged\nand edited\n").unwrap();
        std::fs::create_dir(root.join("dir")).unwrap();
        std::fs::write(root.join("dir/東京 notes.txt"), "new\n").unwrap();

        cx.update(|_, cx| shortcuts::bind_keys(cx));
        let selection = message_selection(&app, cx);
        cx.update(|window, cx| selection.select_all(window, cx));
        draw(cx);
        assert_eq!(copy_chord(cx), "fixture notes");
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        let choose = |cx: &mut VisualTestContext, path: &str, area: ChangeArea, toggle: bool| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    let index = app
                        .work_status
                        .as_ref()
                        .unwrap()
                        .entries
                        .iter()
                        .position(|entry| entry.path == std::path::Path::new(path))
                        .unwrap();
                    app.choose_working_selection(index, area, toggle, false, window, cx);
                    window.focus(&app.file_focus, cx);
                })
            });
            draw(cx);
        };

        choose(cx, "notes.txt", ChangeArea::Unstaged, false);
        assert_eq!(copy_chord(cx), "notes.txt");
        choose(cx, "notes.txt", ChangeArea::Staged, true);
        choose(cx, "dir/東京 notes.txt", ChangeArea::Unstaged, true);
        assert_eq!(copy_chord(cx), "notes.txt\ndir/東京 notes.txt");
    }
}
