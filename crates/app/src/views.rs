use crate::*;
use columns::ColumnId;
use gpui_kit::base::ElementExt;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;

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

    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
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
            .child(
                button(
                    "page-back",
                    if back_to_projects { "Projects" } else { "" },
                    if back_to_projects {
                        "folder"
                    } else {
                        "arrow-left"
                    },
                    false,
                )
                .accessibility_label(back_label)
                .disabled(busy && self.mode == WorkspaceMode::History)
                .tooltip(format!("{back_label} · {}[", primary_label()))
                .on_click(cx.listener(|this, _, window, cx| this.navigate_back(window, cx))),
            )
            .child(app_icon(f32::from(appearance::ui_size(28.))))
            .child(
                div()
                    .id("repository-heading")
                    .role(Role::Label)
                    .aria_label(format!(
                        "Repository {}. {}",
                        self.repository
                            .as_ref()
                            .map(|repo| repo.name())
                            .unwrap_or_default(),
                        self.path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default(),
                    ))
                    .tooltip({
                        let path = self
                            .path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "GitTurtle".into());
                        move |window, cx| Tooltip::new(path.clone()).build(window, cx)
                    })
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
                            .child(
                                self.repository
                                    .as_ref()
                                    .map(|r| r.name())
                                    .unwrap_or("GitTurtle".into()),
                            ),
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
                                    self.work_status
                                        .as_ref()
                                        .map(|status| {
                                            if status.entries.is_empty() {
                                                "Changes".into()
                                            } else {
                                                format!("Changes · {}", status.entries.len())
                                            }
                                        })
                                        .unwrap_or_else(|| "Changes".into()),
                                    "",
                                    self.mode == WorkspaceMode::Working,
                                )
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
                            .disabled(busy)
                            .tooltip("Discover pull requests and review captured changes")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_github(window, cx)),
                            ),
                    )
            })
            .child(div().flex_1())
            .children(self.operation_busy.map(|label| {
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
                                    format!(
                                        " · {}",
                                        path.file_name()
                                            .unwrap_or(path.as_os_str())
                                            .to_string_lossy()
                                    )
                                }),
                    )
            }))
            .when(busy, |header| {
                header.child(self.render_operation_cancel(cx))
            })
            .when(!back_to_projects, |header| {
                header.child(
                    button("projects", "", "folder", false)
                        .accessibility_label("Projects")
                        .tooltip("Open Projects")
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.show_projects(window, cx)),
                        ),
                )
            })
            .child(self.render_profile_button(cx))
            .child(
                button("settings", "", "settings", false)
                    .accessibility_label("Settings")
                    .tooltip(format!("Settings · {},", primary_label()))
                    .on_click(cx.listener(|this, _, window, cx| this.show_settings(window, cx))),
            )
            .into_any_element()
    }

    pub(super) fn render_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        div()
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
                rail.child(
                    button("rail-sidebar", "", "commit", false)
                        .accessibility_label("Show branches and worktrees")
                        .tooltip("Show branches and worktrees")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar = true;
                            this.history_sidebar = true;
                            cx.notify();
                        })),
                )
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
                    .aria_label("Branches and worktrees. Arrow keys browse; Left and Right collapse or expand; Enter activates; Shift F10 opens branch actions")
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
                    .child("Arrows browse · Enter filters or opens.\nLeft/Right folders · Shift F10 actions."),
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
                .text_size(crate::appearance::ui_text(10.))
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
                    tree.path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    "worktree",
                    self.path.as_ref() == Some(&tree.path),
                    0,
                    format!(
                        "{}\n{}{}{}{}",
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
            .text_color(rgb(if active { colors.accent } else { colors.text }))
            .child(icon(
                symbol,
                15.,
                if active { colors.accent } else { colors.muted },
            ))
            .child(div().flex_1().truncate().child(name))
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
        } else {
            element.into_any_element()
        }
    }

    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let scope = self
            .scope
            .as_ref()
            .map(|s| s.0.clone())
            .unwrap_or("All history".into());
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
                    range
                        .map(|i| this.render_commit_row(i, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .size_full()
            .track_scroll(&self.history_scroll)
            .into_any_element()
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
                div()
                    .relative()
                    .w(px(width))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pl(px(10.))
                    .pr_3()
                    .child(div().truncate().child(id.label()))
                    .when(id == columns::ColumnId::Graph, |cell| {
                        let capacity = self.graph_lane_capacity();
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
                            .cursor_col_resize()
                            .border_r_1()
                            .border_color(rgb(colors.border))
                            .hover(|style| style.bg(rgb(colors.hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                    this.column_drag = Some((id, event.position.x, width));
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            ),
                    )
            }));
        div()
            .relative()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.canvas))
            .on_prepaint(move |bounds, _, cx| {
                let _ = view.update(cx, |this, cx| {
                    let width = f32::from(bounds.size.width);
                    if (this.history_width - width).abs() > 1. {
                        this.history_width = width;
                        cx.notify();
                    }
                });
            })
            .child(
                div()
                    .h(crate::appearance::ui_size(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(colors.border))
                    .child(icon("branch", 15., colors.accent))
                    .child(
                        div()
                            .max_w(px(190.))
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(scope),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(colors.muted))
                            .child(if self.history_search_active() { format!("{} commits", self.visible.len()) } else { format!("{}–{}", self.history_paging.offset + usize::from(!self.visible.is_empty()), self.history_paging.offset + self.visible.len()) }),
                    )
                    .child(div().flex_1())
                    .child(
                        button("columns", "Columns", "columns", self.column_menu).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.column_menu = !this.column_menu;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        button("history-newest", "Newest", "", false)
                            .disabled(self.history_search_active() || self.loading.is_some() || self.history_paging.offset == 0)
                            .tooltip("Return to the newest rows in this captured snapshot; Refresh reads current tips")
                            .on_click(cx.listener(|this, _, window, cx| this.request_history_page(0, window, cx))),
                    )
                    .child(
                        button("history-previous", "Previous", "", false)
                            .disabled(self.history_search_active() || self.loading.is_some() || self.history_paging.offset == 0)
                            .tooltip("Read the preceding page; your selected comparison remains available")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.request_history_page(this.history_paging.offset.saturating_sub(history_paging::PAGE_SIZE), window, cx);
                            })),
                    )
                    .child(
                        button("load-more", "Older", "chevron", false)
                            .disabled(self.repository.is_none() || self.history_search_active() || self.loading.is_some() || self.history_paging.next_offset.is_none())
                            .tooltip("Continue the captured history; retains up to 5,000 rows in a bounded window")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(offset) = this.history_paging.next_offset {
                                    this.request_history_page(offset, window, cx);
                                }
                            })),
                    ),
            )
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
                        .child(self.render_columns_controls(cx)),
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
                                    .role(Role::ListBox)
                                    .aria_label("Commit history")
                                    .tab_stop(true)
                                    .key_context("GitTurtleList")
                                    .track_focus(&self.focus)
                                    .border_1()
                                    .border_color(rgb(colors.border))
                                    .focus_visible(|style| style.border_color(rgb(colors.accent)))
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

    fn history_column_layout(&self) -> columns::ColumnLayout {
        self.settings.columns.layout_for_graph(
            self.history_width - 26.,
            graph::required_width(
                self.graph_lanes,
                f32::from(self.settings.graph_spacing) * appearance::ui_scale(),
            ),
        )
    }

    fn graph_lane_capacity(&self) -> usize {
        let width = self
            .history_column_layout()
            .columns
            .iter()
            .find(|column| column.id == columns::ColumnId::Graph)
            .map_or(112., |column| column.width);
        graph::visible_lane_capacity(
            width,
            f32::from(self.settings.graph_spacing) * appearance::ui_scale(),
        )
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

    pub(super) fn render_commit_row(&self, position: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let index = self.visible[position];
        let commit = &self.commits[index];
        let active = self.selected_commit == Some(index);
        let columns = self.history_column_layout();
        let lane_colors = graph::colors(cx);
        let color = lane_colors[self.graph[index].color % lane_colors.len()];
        let refs_width = self.settings.columns.refs.width;
        let mut references = div()
            .w(px(refs_width))
            .flex_shrink_0()
            .pl(px(10.))
            .pr_2()
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden();
        if let Some(names) = self
            .refs
            .get(&commit.oid)
            .filter(|_| self.settings.columns.refs.visible)
        {
            if let Some(name) = names.first() {
                let title = names.join("\n");
                references = references.child(
                    div()
                        .id(("refs", index))
                        .min_w_0()
                        .max_w(px(refs_width - 22.))
                        .truncate()
                        .px_1()
                        .py_0p5()
                        .rounded(px(3.))
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(color))
                        .bg(rgba((color << 8) | 0x1e))
                        .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
                        .child(name.clone()),
                );
            }
            if names.len() > 1 {
                references = references.child(
                    div()
                        .flex_shrink_0()
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(colors.muted))
                        .child(format!("+{}", names.len() - 1)),
                );
            }
        }
        let references = references.into_any_element();
        let mut references = Some(references);
        let graph_filtered = history_paging::graph_is_filtered(
            self.commits.len(),
            self.visible.len(),
            self.automatic.retained_commit,
            self.history_search_active(),
        ) || self.graph_notice.is_some();
        let row = div()
            .id(("commit", index))
            .role(Role::ListBoxOption)
            .aria_label(format!(
                "{} · {} · {} · {} parent{}{}",
                commit.subject,
                commit.author,
                short_oid(&commit.oid),
                commit.parents.len(),
                if commit.parents.len() == 1 { "" } else { "s" },
                if commit.parents.len() > 1 {
                    " · merge commit"
                } else {
                    ""
                }
            ))
            .aria_selected(active)
            .when(active, |row| row.aria_active_descendant())
            .aria_position_in_set(position + 1)
            .aria_size_of_set(self.visible.len())
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
                            if graph_filtered {
                                0
                            } else {
                                self.graph_lane_offset()
                            },
                            f32::from(self.settings.graph_spacing) * appearance::ui_scale(),
                            self.settings.density.history_row_height(),
                            graph::RowStyle {
                                active,
                                merge: commit.parents.len() > 1,
                                filtered: graph_filtered,
                            },
                        ))
                        .into_any_element(),
                    ColumnId::Subject => cell
                        .pl(px(10.))
                        .pr_3()
                        .truncate()
                        .text_size(crate::appearance::ui_text(13.))
                        .child(commit.subject.clone())
                        .into_any_element(),
                    ColumnId::Author => cell
                        .pl(px(10.))
                        .pr_3()
                        .truncate()
                        .text_size(crate::appearance::ui_text(11.))
                        .text_color(rgb(colors.muted))
                        .child(commit.author.clone())
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

    pub(super) fn render_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let oid = commit.oid.clone();
        let message = format!("{}\n\n{}", commit.subject, commit.body);
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
            parents = parents.child(
                div()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .child(format!("Showing 128 of {} parents", commit.parents.len())),
            );
        }
        if commit.parents.is_empty() {
            parents = parents.child(
                div()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .child("Root commit · empty-tree comparison"),
            );
        }
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_l_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .id("commit-metadata")
                    .role(Role::Group)
                    .aria_label("Selected commit details")
                    .max_h(relative(0.45))
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(colors.border))
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_1()
                                    .text_size(crate::appearance::ui_text(10.))
                                    .text_color(rgb(colors.muted))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(appearance::ui_size(60.))
                                            .truncate()
                                            .child("Commit details"),
                                    )
                                    .child(self.render_commit_recovery_menu(commit, cx))
                                    .child(
                                        button("copy-commit", "", "copy", false)
                                            .accessibility_label(format!(
                                                "Copy commit hash {}",
                                                commit.oid
                                            ))
                                            .tooltip(format!(
                                                "Copy full commit hash · {}",
                                                commit.oid
                                            ))
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    oid.clone(),
                                                ))
                                            }),
                                    )
                                    .children((!commit.body.is_empty()).then(|| {
                                        button("message", "Message", "", self.details).on_click(
                                            cx.listener(|this, _, _, cx| {
                                                this.details = !this.details;
                                                cx.notify();
                                            }),
                                        )
                                    })),
                            )
                            .child(
                                div()
                                    .id("commit-subject")
                                    .role(Role::Label)
                                    .aria_label(commit.subject.clone())
                                    .tooltip({
                                        let subject = commit.subject.clone();
                                        move |window, cx| {
                                            Tooltip::new(subject.clone()).build(window, cx)
                                        }
                                    })
                                    .text_size(crate::appearance::ui_text(15.))
                                    .line_height(relative(1.35))
                                    .line_clamp(2)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(commit.subject.clone()),
                            )
                            .child(
                                div()
                                    .id("commit-author-and-identity")
                                    .role(Role::Label)
                                    .aria_label(format!(
                                        "{} · {}. Commit {}",
                                        commit.author,
                                        full_date(commit.timestamp),
                                        commit.oid,
                                    ))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .py_1()
                                    .child(
                                        div()
                                            .size(px(30.))
                                            .flex_shrink_0()
                                            .rounded_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(rgba((colors.hunk << 8) | 0x22))
                                            .text_color(rgb(colors.hunk))
                                            .text_size(crate::appearance::ui_text(11.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(
                                                commit
                                                    .author
                                                    .split_whitespace()
                                                    .filter_map(|part| part.chars().next())
                                                    .take(2)
                                                    .flat_map(char::to_uppercase)
                                                    .collect::<String>(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("commit-author-name")
                                            .tooltip({
                                                let author = commit.author.clone();
                                                move |window, cx| {
                                                    Tooltip::new(author.clone()).build(window, cx)
                                                }
                                            })
                                            .min_w_0()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(crate::appearance::ui_text(12.))
                                                    .child(commit.author.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(crate::appearance::ui_text(11.))
                                                    .text_color(rgb(colors.muted))
                                                    .child(full_date(commit.timestamp)),
                                            ),
                                    ),
                            )
                            .child(parents)
                            .children(self.details.then(|| {
                                div()
                                    .id("commit-message")
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .text_size(crate::appearance::ui_text(12.))
                                    .child(
                                        button("copy-message", "Copy full message", "copy", false)
                                            .on_click(move |_, _, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    message.clone(),
                                                ))
                                            }),
                                    )
                                    .child(div().child(if commit.body.is_empty() {
                                        "No extended commit message.".into()
                                    } else {
                                        commit.body.clone()
                                    }))
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.render_files(cx)))
            .into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let visible = self.filtered_file_indices(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .h(crate::appearance::ui_size(36.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .font_weight(FontWeight::MEDIUM)
                    .gap_2()
                    .child(icon("changes", 15., colors.muted))
                    .child("Changed files")
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(rgb(colors.hover))
                            .text_size(crate::appearance::ui_text(10.))
                            .child(self.files.len().to_string()),
                    ),
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
                    .role(Role::ListBox)
                    .aria_label("Changed files")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.file_focus)
                    .border_1()
                    .border_color(rgb(colors.border))
                    .focus_visible(|style| style.border_color(rgb(colors.accent)))
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
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                let visible = this.filtered_file_indices(cx);
                                range
                                    .filter_map(|i| visible.get(i).copied())
                                    .map(|i| this.render_file_row(i, cx))
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
    pub(super) fn render_file_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
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
            .child(
                div()
                    .flex_shrink_0()
                    .px_1()
                    .py_0p5()
                    .rounded(px(3.))
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.text))
                    .child(label),
            )
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
    pub(super) fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let mut path_controls = div()
            .min_h(appearance::ui_size(28.))
            .min_w(appearance::ui_size(220.))
            .flex_1()
            .flex()
            .items_center()
            .gap_1()
            .children(
                self.working_selected
                    .filter(|_| self.mode == WorkspaceMode::Working)
                    .map(|(_, area)| {
                        let staged = area == gitturtle_core::ChangeArea::Staged;
                        div()
                            .flex_shrink_0()
                            .px_2()
                            .py_1()
                            .rounded(px(4.))
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(if staged {
                                colors.added
                            } else {
                                colors.modified
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
            .children(file.map(|_| {
                button("copy-path", "", "copy", false)
                    .accessibility_label("Copy file path")
                    .tooltip("Copy file path")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()))
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
            path_controls = path_controls.child(
                button("open-blame", "Blame", "", false)
                    .tooltip(
                        "Line attribution and history; working files include uncommitted lines",
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_blame(index, window, cx)),
                    ),
            );
        }
        // Keep the path and its actions together. At narrow widths or larger
        // interface sizes, modes form another row instead of crushing the path.
        let mut toolbar = div()
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
            empty(
                self.loading.unwrap_or("Choose a file"),
                "Select a changed file in the right panel, or return to history.",
            )
        } else if let Some(content) = &self.content {
            match content.as_ref() {
                Content::Text {
                    patch,
                    diagrams,
                    markdown,
                    ..
                } => {
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
                    for (mode, name) in [
                        (TextMode::Unified, "Diff"),
                        (TextMode::Split, "Split"),
                        (TextMode::Before, "Before"),
                        (
                            TextMode::After,
                            if quick_source { "Source" } else { "After" },
                        ),
                        (TextMode::Diagrams, "Diagrams"),
                        (TextMode::Markdown, "Rendered"),
                    ] {
                        if (quick_source
                            && !matches!(
                                mode,
                                TextMode::After | TextMode::Diagrams | TextMode::Markdown
                            ))
                            || (mode == TextMode::Diagrams && diagrams.is_none())
                            || (mode == TextMode::Markdown && markdown.is_none())
                        {
                            continue;
                        }
                        modes = modes.child(
                            button(name, name, "", self.text_mode == mode)
                                .toggled(self.text_mode == mode)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.text_mode = mode;
                                    this.ensure_editor(window, cx);
                                    cx.notify();
                                })),
                        );
                    }
                    toolbar = toolbar.child(modes);
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
                |el| el.child(self.render_text_review(cx)),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
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
                div()
                    .h(crate::appearance::ui_size(24.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .border_t_1()
                    .border_color(rgb(colors.border))
                    .child(if quick_source {
                        format!("Tracked source · mode {}", file.new_mode)
                    } else {
                        format!(
                            "{}   ·   {} → {}   ·   {} → {}",
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
                            file.old_oid.as_deref().map(short_oid).unwrap_or("—".into()),
                            file.new_oid.as_deref().map(short_oid).unwrap_or("—".into())
                        )
                    })
            }))
            .into_any_element()
    }
}

impl GitTurtle {
    fn render_repository(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let left = if self.mode != WorkspaceMode::History {
            div()
                .size_full()
                .flex()
                .min_w_0()
                .child(self.render_rail(cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_preview(cx)),
                )
                .into_any_element()
        } else if self.sidebar {
            h_resizable("history-columns")
                .with_state(&self.history_panels)
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
                        .child(self.render_history(cx)),
                )
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .min_w_0()
                .child(self.render_rail(cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_history(cx)),
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
                    .size_range(px(520.)..px(10000.))
                    .child(left),
            )
            .child(
                resizable_panel()
                    .size(px(self.settings.inspector_width))
                    .size_range(px(280.)..px(480.))
                    .flex_none()
                    .child(if self.mode == WorkspaceMode::Working {
                        self.render_working_inspector(window, cx)
                    } else {
                        self.render_inspector(cx)
                    }),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_git_actions(cx))
            .child(self.render_operation_state(cx))
            .child(div().flex_1().min_h_0().child(workspace))
            .into_any_element()
    }
}

impl Render for GitTurtle {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                eprintln!(
                    "gitturtle.layout viewport={:.0}x{:.0} configured_theme={:?} density={:?} interface={} code={} targets={}",
                    f32::from(layout.0.width),
                    f32::from(layout.0.height),
                    layout.1,
                    layout.2,
                    layout.3,
                    layout.4,
                    layout.5
                );
            }
        }
        image_lifetime::after_draw(window, cx);
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
        let colors = palette(cx);
        let menu_state = (self.repository.is_some(), self.operation_busy.is_some());
        if self.menu_state != Some(menu_state) {
            platform_polish::menus(menu_state.0, menu_state.1, cx);
            self.menu_state = Some(menu_state);
        }
        let body = match self.page {
            AppPage::Repository => self.render_repository(window, cx),
            AppPage::Projects => self.hub.clone().into_any_element(),
            AppPage::Settings => self.render_settings(window, cx),
        };
        div()
            .id("gitturtle")
            .relative()
            .track_focus(&self.app_focus)
            .key_context("GitTurtle")
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
                if this.mode != WorkspaceMode::History {
                    this.back_to_history(window, cx);
                } else {
                    this.sidebar = !this.sidebar;
                    this.history_sidebar = this.sidebar;
                    cx.notify();
                }
            }))
            .child(self.render_repository_tabs(window, cx))
            .when(self.page != AppPage::Projects, |root| {
                root.child(self.render_header(cx))
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
                            .child(
                                div()
                                    .id("operation-notice-summary")
                                    .role(Role::Status)
                                    .a11y_synthetic_children(native_accessibility::polite)
                                    .aria_label(notice.clone())
                                    .flex_1()
                                    .truncate()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .child(notice.clone()),
                            )
                            .child(
                                button("dismiss-operation-notice", "Dismiss", "", false).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.operation_notice = None;
                                        cx.notify();
                                    }),
                                ),
                            )
                    }))
                },
            )
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
                    .child(match self.page {
                        AppPage::Settings => "Tab Move between controls · Esc Back".to_owned(),
                        AppPage::Projects => {
                            format!("{}O Open repository · Esc Back", primary_label())
                        }
                        AppPage::Repository => match self.mode {
                            WorkspaceMode::Compare => "Esc Back to history · ↑↓ Files".to_owned(),
                            WorkspaceMode::Working => {
                                "↑↓ Review files · Esc Back to history".to_owned()
                            }
                            WorkspaceMode::History => format!(
                                "{}F Search · ↑↓ Navigate · Return Open file",
                                primary_label()
                            ),
                        },
                    }),
            )
            .children(Root::render_dialog_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

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
}
