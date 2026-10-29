use crate::*;
use columns::ColumnId;
use gpui_kit::base::ElementExt;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
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
    crate::native_accessibility::clips_children(
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
            .child(content),
    )
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
                    // History's probe sits in its table, which is as wide as
                    // the columns and scrolls sideways inside the viewport;
                    // the columns follow the width History's prepaint
                    // measured a frame earlier. Take the viewport's width
                    // from this frame's prepaint instead, so after a resize
                    // the frame that only finishes the table is no viewport
                    // change: a cold tab's last read can land on the frame
                    // before it and seed the restored layout there.
                    let width = if history {
                        px(this.history_width)
                    } else {
                        bounds.size.width
                    };
                    let next = (size(width, bounds.size.height), row_height);
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
                        // The graph's lanes as this frame lays them out, its
                        // width taken in prepaint: a window narrowed to fewer
                        // lanes, as narrow History's one, shows the selected
                        // commit's lane as well as its row.
                        if history && let Some(index) = this.selected_commit {
                            this.reveal_graph_lane(index);
                        }
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

    /// The page's one return action: Projects at root History, otherwise a
    /// Back arrow naming its destination in its accessible name and tooltip.
    /// `labelled`, in the full header's slot, it reads "Projects" or "Back"
    /// and fills the slot, the labelled Projects' width, with its icon and
    /// label at the slot's leading edge, so Back's hit area, hover and
    /// pressed fill and focus ring span the whole slot rather than leave
    /// empty room before the turtle.
    fn render_back_button(&self, labelled: bool, cx: &mut Context<Self>) -> Button {
        let back_label = self.back_label();
        let back_to_projects = back_label == "Back to Projects";
        button(
            "page-back",
            match (labelled, back_to_projects) {
                (false, _) => "",
                (true, true) => "Projects",
                (true, false) => "Back",
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
        // History's Back opens Projects, which a running write refuses.
        .map(|back| {
            let history = self.mode == WorkspaceMode::History;
            crate::workspace::refusing(
                back,
                || "page-back".into(),
                self.operation_busy.is_some() && history,
                self.operation_shown().is_some() && history,
            )
        })
        .tooltip(format!("{back_label} · {}[", primary_label()))
        .when(labelled, |back| back.w_full())
        // The kit centres a Button's content: the shorter "Back" keeps the
        // leading edge "Projects" takes, and the rest of the slot trails it.
        .when(labelled && !back_to_projects, |back| {
            back.child(div().debug_selector(|| "page-back-room".into()).flex_1())
        })
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
    fn repository_heading(&self) -> native_accessibility::StaticText<Stateful<Div>> {
        native_accessibility::static_text(
            div()
                .id("repository-heading")
                .debug_selector(|| "repository-heading".into()),
            format!(
                "Repository {}. {}",
                self.repository
                    .as_ref()
                    .map(|repo| self.project_name(repo.path()))
                    .unwrap_or_default(),
                self.path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
        )
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
            .debug_selector(|| "operation-progress".into())
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
            // `show_projects` refuses while a write runs.
            .map(|projects| {
                crate::workspace::refusing(
                    projects,
                    || "projects".into(),
                    self.operation_busy.is_some(),
                    self.operation_shown().is_some(),
                )
            })
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
        let busy = self.operation_shown().is_some();
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
                                    .group(CHANGES_TOGGLE)
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
                                        toggle.child(changes_badge(count, !history, p))
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
                        // `open_github_from_header` refuses while a write runs.
                        .map(|open| {
                            crate::workspace::refusing(
                                open,
                                || "pull-requests".into(),
                                self.operation_busy.is_some(),
                                busy,
                            )
                        })
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
                        self.operation_shown()
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
    /// and accessible names. Its return action takes one slot in every
    /// mode, as wide as the labelled Projects where that shows and filled by
    /// the labelled Back outside History's root, so Back leaves the turtle,
    /// the name and the toggles after it in place. Where the trailing
    /// controls take a row of their own ([`Trailing::OwnRow`]) the profile
    /// and Settings lead it in every mode and Projects follows them.
    pub(super) fn render_header(
        &self,
        chrome: Chrome,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let busy = self.operation_shown().is_some();
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
        // The return slot is the labelled Projects' width outside `compact`.
        let widths = self.chrome_widths(window);
        let trailing = self.trailing(compact, &widths, window);
        let own_row = trailing != Trailing::Shared;
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
            .child(
                div()
                    .flex_shrink_0()
                    .when(!compact, |slot| {
                        slot.w(self.labelled_width(widths.projects, true, window))
                    })
                    .child(self.render_back_button(!compact, cx)),
            )
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
                            // `open_github_from_header` refuses while a write
                            // runs.
                            .map(|open| {
                                crate::workspace::refusing(
                                    open,
                                    || "pull-requests".into(),
                                    self.operation_busy.is_some(),
                                    busy,
                                )
                            })
                            .tooltip("Discover pull requests and review captured changes")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_github_from_header(window, cx)
                            })),
                    )
            })
            .when(trailing != (Trailing::OwnRow { spacer: false }), |header| {
                header.child(div().flex_1())
            })
            .children(
                self.operation_shown()
                    .map(|label| self.render_operation_progress(label, cx)),
            )
            .when(busy, |header| {
                header.child(self.render_operation_cancel(cx))
            })
            // History's root leaves Projects to its return action but keeps
            // its slot where that adds no row, so the name shares the same
            // room in every mode. On a row of their own the trailing
            // controls start at the header's edge with the profile and
            // Settings, and Projects follows them where it shows.
            .when(
                back_to_projects && !own_row && self.keeps_projects_slot(compact, &widths, window),
                |header| header.child(div().flex_shrink_0().size(appearance::ui_size(28.))),
            )
            .when(!back_to_projects && !own_row, |header| {
                header.child(self.render_projects_button(cx))
            })
            .child(self.render_profile_button(compact, 36., cx))
            .child(self.render_settings_button(cx))
            .when(!back_to_projects && own_row, |header| {
                header.child(self.render_projects_button(cx))
            })
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

    pub(super) fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let form = self.sidebar_form(window, cx);
        div()
            .debug_selector(|| "history-sidebar".into())
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                // The mode Buttons' frames stand at the pane's edge, where
                // the filter's box, the section labels and the rows' icons
                // do; their labels start a label inset in.
                div()
                    .min_h(crate::appearance::ui_size(40.))
                    .py_1()
                    .flex_shrink_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .px(crate::appearance::ui_size(crate::appearance::PANE_INSET))
                    // Where the three labels do not fit one row, each mode
                    // becomes its glyph, named, before any wraps.
                    .children(NAV_MODES.map(|(mode, name, symbol, selector)| {
                        let selected = self.nav_mode == mode;
                        let control = if form.labelled_modes {
                            button(name, name, "", selected)
                        } else {
                            // As glyphs, the selected mode was told apart
                            // only by its fill, 1.40:1 against the panel in
                            // Midnight; a 2 px accent bar under its glyph,
                            // 3:1 or more against that fill in every
                            // palette, marks it as a row's leading marker
                            // marks the selected row.
                            compact_control_name(button(name, "", symbol, selected), selector, name)
                                .tooltip(name)
                                .when(selected, |control| {
                                    control.relative().child(
                                        div()
                                            .debug_selector(|| "nav-mode-selected-bar".into())
                                            .absolute()
                                            .left(crate::appearance::ui_size(crate::ICON_INSET))
                                            .right(crate::appearance::ui_size(crate::ICON_INSET))
                                            .bottom(crate::appearance::ui_size(2.))
                                            .h(px(2.))
                                            .rounded(px(1.))
                                            .bg(rgb(colors.accent)),
                                    )
                                })
                        };
                        control
                            .debug_selector(move || selector.into())
                            .toggled(selected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.nav_mode = mode;
                                this.rebuild_navigation(cx);
                                this.nav_scroll.scroll_to_item(0, ScrollStrategy::Top);
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .relative()
                    .px(crate::appearance::ui_size(crate::appearance::PANE_INSET))
                    .pb_3()
                    .child(Input::new(&self.nav_search).aria_label(NAV_FILTER_NAME).text_size(crate::appearance::ui_text(11.)))
                    // Painted after the field, it fits its placeholder.
                    .child(crate::workspace::placeholder_fit(
                        self.nav_search.clone(),
                        &NAV_FILTER_FIT,
                        NAV_FILTER_PLACEHOLDERS,
                        11.,
                    )),
            )
            .child(
                crate::native_accessibility::clips_children(div()
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
                    })).size_full().track_scroll(&self.nav_scroll))
                    .children(self.nav_menu.as_ref().map(|menu| {
                        menu.render("navigation-menu-overlay", Self::dismiss_nav_menu, cx)
                    }))),
            )
            // Each of its lines whole, or none of it: the tree's accessible
            // name keeps the keys.
            .when(form.hint, |sidebar| {
                sidebar.child(
                    div()
                        .debug_selector(|| "navigation-hint".into())
                        .p_3()
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(colors.muted))
                        .border_t_1()
                        .border_color(rgb(colors.border))
                        .child(nav_hint_lines().join("\n")),
                )
            })
            .into_any_element()
    }

    /// How the navigation lays out at the width it is drawn at
    /// ([`SidebarForm`]), decided before layout from the interface font: the
    /// width its panel group last gave it, or before that its saved width,
    /// within its 1 px right border.
    pub(crate) fn sidebar_form(&self, window: &Window, cx: &App) -> SidebarForm {
        let width = self
            .history_panels
            .read(cx)
            .sizes()
            .first()
            .copied()
            .filter(|size| *size != gpui_kit::base::PANEL_MIN_SIZE)
            .unwrap_or(px(self.settings.navigation_width))
            - px(1.);
        let widths = cached_width(&SIDEBAR_WIDTHS, &LineKey::current(window), || {
            let label = appearance::ui_text(12.);
            SidebarWidths {
                modes: NAV_MODES.map(|(_, name, _, _)| line_widths(&[(name, label)], window)),
                hint: nav_hint_lines()
                    .iter()
                    .map(|line| line_widths(&[(line, appearance::ui_text(10.))], window))
                    .fold(px(0.), Pixels::max),
            }
        });
        let rem = window.rem_size();
        // The toggles `gap_1` apart, inside the row's pane insets.
        let modes = widths
            .modes
            .iter()
            .map(|label| self.labelled_width(*label, false, window))
            .fold(rem * 0.25 * (NAV_MODES.len() - 1) as f32, |sum, toggle| {
                sum + toggle
            });
        let insets = window.pixel_snap(appearance::ui_size(appearance::PANE_INSET)) * 2.;
        SidebarForm {
            labelled_modes: modes <= width - insets,
            // Inside the hint's `p_3` either side.
            hint: widths.hint <= width - rem * 0.75 * 2.,
        }
    }

    pub(super) fn render_nav_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let row = self.nav_rows[index].clone();
        if let NavRow::Section(label, count) = &row {
            return div()
                .w_full()
                .h(crate::appearance::ui_size(30.))
                .px(crate::appearance::ui_size(crate::appearance::PANE_INSET))
                .pt_2()
                .flex()
                .items_center()
                .justify_between()
                .text_size(crate::appearance::ui_text(
                    crate::appearance::SECTION_LABEL_TEXT,
                ))
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
                .pl(nav_row_inset(*depth, self.nav_cursor == Some(index)))
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
            .debug_selector(move || format!("nav-row-{index}"))
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
            .pl(nav_row_inset(depth, self.nav_cursor == Some(index)))
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
                .app_context_menu(move |menu, _, cx| {
                    let disabled = branch_owner
                        .upgrade()
                        .is_none_or(|owner| owner.read(cx).operation_busy.is_some());
                    branch_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        name.clone(),
                        remote,
                        disabled,
                    )
                })
                .into_any_element()
        } else if let Some(tree) = contextual_worktree {
            element
                .app_context_menu(move |menu, _, cx| {
                    let disabled = branch_owner.upgrade().is_none_or(|owner| {
                        let owner = owner.read(cx);
                        owner.operation_busy.is_some()
                            || owner.path != branch_repository
                            || owner.page != AppPage::Repository
                    });
                    worktree_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        tree.clone(),
                        disabled,
                    )
                })
                .into_any_element()
        } else {
            element.into_any_element()
        }
    }

    /// Shift-F10, or on Linux the Menu key, in the navigator: opens the
    /// cursor row's menu below it, the one a right-click on a branch or a
    /// worktree opens, its actions unavailable while a write runs.
    pub(super) fn manage_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav_menu.is_some() || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(index) = self.nav_cursor else {
            return;
        };
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        let busy = self.operation_busy.is_some();
        let (depth, build): (usize, Box<dyn FnOnce(PopupMenu) -> PopupMenu>) =
            match self.nav_rows.get(index) {
                Some(NavRow::Branch(branch, depth)) => {
                    let branch = &self.branches[*branch];
                    let (name, remote) = (branch.name.clone(), branch.remote);
                    (
                        *depth,
                        Box::new(move |menu| {
                            branch_row_menu(menu, owner, repository, name, remote, busy)
                        }),
                    )
                }
                Some(NavRow::Worktree(tree)) => {
                    let tree = self.worktrees[*tree].clone();
                    let disabled = busy || self.page != AppPage::Repository;
                    (
                        0,
                        Box::new(move |menu| {
                            worktree_row_menu(menu, owner, repository, tree, disabled)
                        }),
                    )
                }
                _ => return,
            };
        let position = native_accessibility::KeyboardMenu::below_row(
            &self.nav_scroll,
            index,
            crate::appearance::ui_size(30.),
            nav_row_inset(depth, true),
        );
        self.nav_menu = Some(native_accessibility::KeyboardMenu::open(
            position,
            self.nav_focus.clone(),
            build,
            Self::dismiss_nav_menu,
            window,
            cx,
        ));
        cx.notify();
    }

    /// Closes the navigator's keyboard menu. A press outside it leaves focus
    /// in the menu, so focus returns to the tree; an item or Escape has
    /// moved it already, to the tree or to the dialog the item opened.
    fn dismiss_nav_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.nav_menu.take() {
            if menu.has_focus(window, cx) || window.focused(cx).is_none() {
                window.focus(&self.nav_focus, cx);
            }
            cx.notify();
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
        // New history a quiet read observed: its words where they fit, and
        // Latest, its action, marked.
        let cue = self.history_update_cue();
        let compact = column
            < labelled_scope_toolbar_width(
                &count,
                &scope,
                cue.as_ref().map(|cue| cue.label.as_ref()),
                window,
            );
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
        // `request_history_discovery` refuses while a write runs; it looks
        // unavailable for that once the write looks busy.
        .map(|latest| {
            let unread = self.repository.is_none() || self.loading.is_some();
            crate::workspace::refusing(
                latest,
                || "history-newest".into(),
                unread || self.operation_busy.is_some(),
                unread || self.operation_shown().is_some(),
            )
        })
        .on_click(cx.listener(|this, _, window, cx| this.show_latest_history(window, cx)));
        // While there is new history, Latest, which shows it and ends the
        // cue, leads its name and tooltip with the cue's words, and an accent
        // dot in its top right corner marks it, so the cue is seen where its
        // words do not fit. The dot is laid over the button from a box of the
        // button's own size, rather than from the content inside the
        // button's padding, where it touched the label and the glyph. It
        // sits 1 px in from the button's edge, 5 px across, so it stands at
        // least 2 px clear of the compact square's up arrow, whose ink ends
        // 8.8 px from the square's right edge at the default size, beside
        // the rows the dot covers; the 6 px dot 2 px in stood 0.8 px from it
        // there and 0.9 px at 18 pt.
        let latest = match &cue {
            Some(cue) => {
                let name = SharedString::from(format!("Latest · {}", cue.label));
                let latest = if compact {
                    compact_control_label(latest, "history-newest", name.clone())
                } else {
                    latest.accessibility_label(name.clone())
                };
                div()
                    .relative()
                    .flex_shrink_0()
                    .child(latest.tooltip(format!(
                        "{name} · Read current local history and show its newest rows; keeps the selected inspector"
                    )))
                    .child(
                        div()
                            .debug_selector(|| "history-newest-dot".into())
                            .absolute()
                            .top(appearance::ui_size(1.))
                            .right(appearance::ui_size(1.))
                            .size(appearance::ui_size(5.))
                            .rounded_full()
                            .bg(rgb(colors.accent)),
                    )
                    .into_any_element()
            }
            None => latest.into_any_element(),
        };
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
        // The cue's polite status, announced once as it appears: its words,
        // whole, beside the count where `drawn`, and otherwise a node out of
        // flow that draws nothing.
        let notice = |cue: &history_updates::UpdateCue, drawn: bool| {
            let node = div()
                .id("history-update-status")
                .debug_selector(|| "history-update-status".into())
                .role(Role::Status)
                .a11y_synthetic_children(native_accessibility::polite)
                .aria_label(cue.label.clone());
            if drawn {
                node.flex_shrink_0()
                    .whitespace_nowrap()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.text))
                    .child(cue.label.clone())
            } else {
                node.absolute().size(px(1.)).overflow_hidden()
            }
        };
        let toolbar = div()
            .debug_selector(|| "history-scope-toolbar".into())
            .relative()
            .h(crate::appearance::ui_size(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(rgb(colors.border));
        if compact {
            // A new-history cue's words first, whole or not at all, since
            // Latest's dot and name carry them too; then the count, drawn
            // whole or not at all, then the name beside it only where at
            // least its first character and ellipsis fit, so the row shows
            // nothing, the count, or both, as it widens, and never a clipped
            // or lone ellipsis.
            let gap = window.rem_size() * 0.5;
            let mut room = compact_scope_room(column, window);
            let shows_notice = cue
                .as_ref()
                .is_some_and(|cue| scope_notice_width(&cue.label, window) <= room);
            if let Some(cue) = cue.as_ref().filter(|_| shows_notice) {
                room -= scope_notice_width(&cue.label, window) + gap;
            }
            let count_width = scope_count_width(&count, window);
            let shows_count = count_width <= room;
            let shows_name =
                shows_count && room - count_width - gap >= scope_name_min_width(&scope, window);
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
                        })
                        .children(
                            cue.as_ref()
                                .filter(|_| shows_notice)
                                .map(|cue| notice(cue, true)),
                        ),
                )
                .children(
                    cue.as_ref()
                        .filter(|_| !shows_notice)
                        .map(|cue| notice(cue, false)),
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
                .children(cue.as_ref().map(|cue| notice(cue, true)))
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
        let metrics = HistoryMetrics::current(window, self.widest_references());
        self.history_metrics.set(metrics);
        let columns = self.history_column_layout();
        let table_width = columns.content_width + metrics.insets();
        let history = if self.commits.is_empty() && self.error.is_some() {
            empty(
                "Could not open repository",
                self.error.as_deref().unwrap_or_default(),
            )
        } else if self.visible.is_empty()
            && let Some(line) = self.history_search_no_match()
        {
            self.history_no_match(line, cx)
        } else if self.visible.is_empty()
            && let Some((title, description)) = self.history_search_empty()
        {
            self.history_search_state(title, description, window, cx)
        } else if self.visible.is_empty() && self.loading.is_none() && self.repository.is_some() {
            if self.commits.is_empty() {
                // The composer makes the first commit.
                EmptyState::new(
                    "history-no-commits",
                    "No commits yet · Your first commit starts in Changes",
                )
                .action(
                    empty_state::action("history-open-changes", "Open Changes", "changes")
                        .on_click(cx.listener(|this, _, window, cx| this.show_working(window, cx))),
                )
                .pane(cx)
            } else {
                self.history_no_match("No loaded commits match this search".into(), cx)
            }
        } else if self.visible.is_empty() {
            empty(
                if self.loading.is_some() {
                    "Reading local history…"
                } else {
                    "Your history, at a glance"
                },
                if self.loading.is_some() {
                    "Loading commit history from this repository."
                } else {
                    "Open a Git repository to explore branches, worktrees and changes."
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
            let visible = self.history_width.min(table_width) - 2.;
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
            .h(crate::appearance::ui_size(HISTORY_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            // Its columns start and end where the rows' do, inside the list's
            // 1 px edge (`HistoryMetrics`).
            .pl(px(metrics.leading - 2.))
            .pr(px(metrics.trailing))
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
            .on_prepaint(move |bounds, window, cx| {
                let _ = view.update(cx, |this, cx| {
                    let width = f32::from(bounds.size.width);
                    if this.history_width != width {
                        #[cfg(test)]
                        HISTORY_WIDTHS.with(|count| count.set(count.get() + 1));
                        this.history_width = width;
                        cx.notify();
                        // A notify during a draw only marks the view for
                        // whichever frame something else asks for: after a
                        // single resize the table kept the old width's
                        // columns until the next input. Ask for the next
                        // frame, which lays the columns out for this width;
                        // that frame measures the same width and asks for
                        // nothing, so an idle window draws no more.
                        window.request_animation_frame();
                    }
                });
            })
            .child(self.render_history_scope_toolbar(scope.into(), window, cx))
            .child(
                div()
                    .relative()
                    .h(crate::appearance::ui_size(38.))
                    .flex_shrink_0()
                    .px_3()
                    .pb_2()
                    .pt_1()
                    .child(
                        Input::new(&self.search)
                            .aria_label(SEARCH_NAME)
                            .text_size(crate::appearance::ui_text(12.)),
                    )
                    // Painted after the field, it fits its placeholder.
                    .child(crate::workspace::placeholder_fit(
                        self.search.clone(),
                        &SEARCH_FIT,
                        SEARCH_PLACEHOLDERS,
                        12.,
                    )),
            )
            .child(self.render_history_search_controls(window, cx))
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
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(history_scroll_viewport(
                        &self.history_horizontal,
                        div()
                            .w(px(table_width))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .child(header)
                            .child(native_accessibility::clips_children(
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
                                    .on_action(cx.listener(
                                        |this, _: &native_accessibility::RowActions, window, cx| {
                                            this.open_commit_menu(window, cx)
                                        },
                                    ))
                                    // The list's 1 px edge is `history_list_edge`'s,
                                    // over the part of the table the viewport shows.
                                    .p(px(1.))
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
                                    .child(self.list_viewport_probe(true, cx))
                                    .children(self.commit_menu.as_ref().map(|menu| {
                                        menu.render(
                                            "commit-menu-overlay",
                                            Self::dismiss_commit_menu,
                                            cx,
                                        )
                                    })),
                            )),
                    ))
                    .child(self.history_list_edge(colors)),
            )
            .child(Scrollbar::horizontal(&self.history_horizontal).mode(ScrollbarMode::Always))
            .into_any_element()
    }

    /// The commit list's 1 px edge and keyboard ring, drawn over the part of
    /// the list the viewport shows rather than around the whole table: where
    /// the columns run sideways past the viewport, as narrow History's
    /// Author, Date and SHA do under the inspector, the table's own right
    /// edge lay beyond the window, and its left one too once it scrolled.
    /// It starts below the column header, whose height it shares, and ends
    /// above the horizontal scrollbar's reserved track, as the list does;
    /// it has no hitbox, so the pointer reaches the rows beneath it.
    fn history_list_edge(&self, colors: appearance::Palette) -> impl IntoElement {
        div()
            .debug_selector(|| "history-list-edge".into())
            .absolute()
            .left_0()
            .right_0()
            .top(crate::appearance::ui_size(HISTORY_HEADER_HEIGHT))
            .bottom(Scrollbar::width())
            .border_1()
            .border_color(rgb(if self.focus_ring.shows(&self.focus) {
                colors.accent
            } else {
                colors.border
            }))
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
        let metrics = self.history_metrics.get();
        self.settings.columns.history_layout(
            self.history_width - metrics.insets(),
            graph::required_width(self.graph_lanes, spacing),
            columns::NarrowHistory {
                subject_floor: metrics.subject_floor,
                lanes: self.graph_lanes,
                pitch: compact,
                pill: metrics.pill_room(),
                trailing_inset: metrics.trailing,
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
            pill: self.history_metrics.get().pill(
                columns
                    .columns
                    .iter()
                    .find(|column| column.id == ColumnId::Subject)
                    .map_or(0., |column| column.width),
            ),
            columns,
        }
    }

    /// The most references any loaded commit has where References show,
    /// which narrow History's compact pill is sized for.
    fn widest_references(&self) -> usize {
        if !self.settings.columns.refs.visible {
            return 0;
        }
        self.refs.widest()
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
        // Prepared with the snapshot (`worker::CommitReferences`): the row
        // shares their strings and joins nothing.
        let names = self
            .refs
            .get(&commit.oid)
            .filter(|refs| self.settings.columns.refs.visible && !refs.names.is_empty());
        let mut references = None;
        let mut pill = None;
        #[cfg(test)]
        let pill_tooltip = names.map(|refs| refs.lines.clone());
        if let Some(refs) = names {
            // The chip's own tooltip lists every reference, so reading the
            // subject beside it brings up no tooltip.
            let title = refs.lines.clone();
            let chip = div()
                .id(("refs", index))
                .debug_selector(move || format!("history-ref-pill-{index}"))
                .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
                .min_w_0()
                .flex()
                .items_center()
                .overflow_hidden()
                .px_1()
                .py_0p5()
                .rounded(px(3.))
                .text_size(crate::appearance::ui_text(10.))
                .text_color(rgb(label))
                .bg(tint);
            // A chip led by a tag, a commit's first reference where it has
            // no branch, carries the tag glyph, as a compact pill led by a
            // branch carries the branch glyph; the colours are the badge's.
            let tag = refs.leads_with_tag();
            let glyph = |path: &'static str| {
                svg()
                    .path(SharedString::new_static(path))
                    .size(crate::appearance::ui_size(10.))
                    .flex_shrink_0()
                    .text_color(rgb(label))
                    .when(tag, |glyph| {
                        glyph.debug_selector(move || format!("history-ref-tag-{index}"))
                    })
            };
            let chip = if columns.narrow && rows.pill.compact {
                // The first reference's glyph and how many references there
                // are, tags included; the names are the tooltip's and the
                // row's accessible name's.
                let count = reference_count(refs.names.len());
                #[cfg(test)]
                HISTORY_PILLS.with(|pills| {
                    pills
                        .borrow_mut()
                        .insert(index, (None, Some(count.to_string()), label))
                });
                chip.gap_0p5()
                    .child(glyph(if tag {
                        "icons/tag.svg"
                    } else {
                        "icons/branch.svg"
                    }))
                    .child(div().flex_shrink_0().child(count))
            } else {
                // HEAD's branch comes first (`worker::Snapshot::refs`), then
                // the other branches and the tags, and "+N" counts the others
                // inside the chip, whole while the name truncates.
                let name = refs.names[0].clone();
                let more = refs.more.clone();
                #[cfg(test)]
                HISTORY_PILLS.with(|pills| {
                    pills.borrow_mut().insert(
                        index,
                        (
                            Some(name.to_string()),
                            more.as_ref().map(ToString::to_string),
                            label,
                        ),
                    )
                });
                chip.gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_0p5()
                            .when(tag, |name| name.child(glyph("icons/tag.svg")))
                            .child(div().min_w_0().truncate().child(name)),
                    )
                    .children(more.map(|more| {
                        div()
                            .debug_selector(move || format!("history-ref-more-{index}"))
                            .flex_shrink_0()
                            .child(more)
                    }))
            };
            if columns.narrow {
                pill = Some(chip.flex_shrink_0().max_w(px(rows.pill.max)));
            } else {
                references = Some(
                    div()
                        .w(px(self.settings.columns.refs.width))
                        .flex_shrink_0()
                        .pl(px(10.))
                        .pr_2()
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .child(chip)
                        .into_any_element(),
                );
            }
        }
        // Every row with references names them after its subject, in the
        // References column's badge as in narrow History's pill.
        let (listing, listed) = names.map_or(("", ""), |refs| (" · References: ", &*refs.listed));
        let name = format!(
            "{}{}{} · {} · {} · {} parent{}{}",
            subject,
            listing,
            listed,
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
        #[cfg(test)]
        HISTORY_ROW_NAMES.with(|names| {
            names
                .borrow_mut()
                .insert(index, (name.clone(), pill_tooltip))
        });
        let row = div()
            .id(("commit", index))
            .debug_selector(move || format!("history-row-{index}"))
            .role(Role::ListBoxOption)
            .aria_label(name)
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
                let id = column.id as usize;
                let cell = div()
                    .debug_selector(move || format!("history-cell-{index}-{id}"))
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
                                div()
                                    .debug_selector(move || format!("history-subject-text-{index}"))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(subject.clone()),
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
        // The row is as wide as the table, which runs sideways past the
        // list's viewport, under the inspector and past the window's edge;
        // its node reports the part of it the list draws, as the list's own
        // node does, rather than extents to x 542.5 in a 461 px window.
        native_accessibility::clips_children(row.on_click(
            cx.listener(move |this, _, window, cx| this.select_commit(index, window, cx)),
        ))
        .app_context_menu(move |menu, _, cx| {
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
            .debug_selector(|| "commit-inspector".into())
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
    /// message in `selection`'s document; parents stay controls. The commit's
    /// Actions stands in the message toolbar, outside this scrolling area.
    /// Each parent's control reads "P1 · ‹hash›" where the widest such label
    /// fits the inspector, and otherwise only "P1", its tooltip and
    /// accessible name keeping the full hash: the 148 px "P1 · ‹hash›" in the
    /// 128.5 px of an 18 pt half tile's inspector widened the metadata past
    /// it, and the date line wrapped at that width ran past the edge.
    pub(super) fn render_commit_metadata(
        &self,
        commit: &Commit,
        selection: &commit_message::Selection,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let labels = commit
            .parents
            .iter()
            .take(128)
            .enumerate()
            .map(|(i, parent)| format!("P{} · {}", i + 1, short_oid(parent)))
            .collect::<Vec<_>>();
        let hashes = self
            .inspector_message
            .details_room()
            .is_none_or(|room| parent_controls_width(&labels, window) <= room);
        let mut parents = div().flex().flex_wrap().gap_1();
        for (i, (parent, label)) in commit.parents.iter().zip(labels).enumerate() {
            let name = format!("Compare against parent {} · {}", i + 1, parent);
            let control = if hashes {
                button(("parent", i), label, "", self.parent == i).accessibility_label(name.clone())
            } else {
                let control = button(("parent", i), format!("P{}", i + 1), "", self.parent == i);
                if i == 0 {
                    compact_control_label(control, "commit-parent-0", name.clone())
                } else {
                    control.accessibility_label(name.clone())
                }
            };
            parents = parents.child(
                control
                    .debug_selector(move || format!("commit-parent-{i}"))
                    .tooltip(name)
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
        let name: SharedString = commit_message::summary(&commit.author).into();
        let [author, date] = selection.metadata([
            name.clone(),
            format!(
                "{} · {}",
                full_date(commit.timestamp),
                short_oid(&commit.oid)
            )
            .into(),
        ]);
        // The message's document is read as a whole and clips nothing, and
        // AccessKit compares a control with its nearest clipping parent
        // alone, so this group clips its controls: the parents below the
        // part of the details the window draws leave the tree. It ends at
        // its last row, so details that fit draw no scrollbar: its bottom
        // padding alone scrolled 13 px at 18 pt in a 1,000 × 680 window.
        native_accessibility::clips_children(
            div()
                .id("commit-metadata")
                .debug_selector(|| "commit-metadata".into())
                .role(Role::Group)
                .aria_label("Commit author and comparison parents")
                // The list's width, as the message's pieces take: sized by
                // its content, the group could lay out wider than the
                // inspector, whose edge then cut its lines.
                .w_full()
                .min_w_0()
                .px_3()
                .pt_3()
                .flex()
                .flex_col()
                .gap_2()
                .text_size(appearance::ui_text(11.))
                .text_color(rgb(colors.muted))
                .child(author_line(name, author))
                .child(date)
                .child(parents),
        )
        .into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_files_with(FilesHeader::WHOLE, cx)
    }

    /// The changed files, their header laid out as `header`.
    fn render_files_with(&self, header: FilesHeader, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let visible = self.filtered_file_indices(cx);
        // A filter that matches nothing offers Clear beside it, where the
        // list below, as short as 74 px at 18 pt in a half tile, has room
        // only for its line.
        let no_match = visible.is_empty()
            && !self.file_paths.pending
            && self.file_paths.error.is_none()
            && self.loading.is_none()
            && !self.file_filter.read(cx).value().is_empty();
        let clear = no_match.then(|| {
            empty_state::action("files-clear-filter", "", "close")
                .accessibility_label("Clear path filter")
                .tooltip("Clear path filter")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.file_filter
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    this.change_file_filter(cx);
                    window.focus(&this.file_focus, cx);
                }))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                // The count never shrinks: in an inspector the window pins
                // too narrow for the whole label beside it, the label leaves,
                // its words staying in the count's tooltip and accessible
                // name, then the count. The label's truncation only guards a
                // measurement a fraction of a pixel short.
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
                                .child(FILES_LABEL_TEXT),
                        )
                    })
                    .when(header.count, |row| {
                        let count = self.files.len();
                        row.child(
                            div()
                                .id("changed-files-count")
                                .debug_selector(|| "changed-files-count".into())
                                // Without its label the count says what it
                                // counts.
                                .when(!header.label, |chip| {
                                    let name: SharedString =
                                        format!("{FILES_LABEL_TEXT} · {count}").into();
                                    note_compact_label("changed-files-count", &name);
                                    // AccessKit adapters read a label's text
                                    // from its value.
                                    chip.role(Role::Label)
                                        .aria_label(name.clone())
                                        .aria_value(name.clone())
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(name.clone()).build(window, cx)
                                        })
                                })
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
                    .relative()
                    .px_3()
                    .pb_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.file_filter)
                                .aria_label(CHANGED_FILTER_NAME)
                                .text_size(appearance::ui_text(12.)),
                        ),
                    )
                    .children(clear)
                    // Painted after the field, it fits its placeholder.
                    .child(crate::workspace::placeholder_fit(
                        self.file_filter.clone(),
                        &CHANGED_FILTER_FIT,
                        &[
                            CHANGED_FILTER_PLACEHOLDER,
                            crate::workspace::FILTER_SHORT_PLACEHOLDER,
                        ],
                        12.,
                    )),
            )
            .child(native_accessibility::clips_children(
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
                    .on_action(cx.listener(
                        |this, _: &native_accessibility::RowActions, window, cx| {
                            this.open_changed_file_menu(window, cx)
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
                    .child(if visible.is_empty() && self.file_paths.pending {
                        empty(
                            "Filtering changed paths…",
                            "Matching the current file snapshot.",
                        )
                    } else if visible.is_empty()
                        && let Some(error) = &self.file_paths.error
                    {
                        empty("Path filter unavailable", error)
                    } else if visible.is_empty()
                        && let Some(loading) = self.loading
                    {
                        empty(loading, "")
                    } else if visible.is_empty() && self.file_filter.read(cx).value().is_empty() {
                        EmptyState::new("files-empty", "No file changes against this parent")
                            .pane(cx)
                    } else if visible.is_empty() {
                        // Clear sits beside the filter, above.
                        EmptyState::new("files-no-match", "No matches").pane(cx)
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
                    .child(self.list_viewport_probe(false, cx))
                    .children(self.file_menu.as_ref().map(|menu| {
                        menu.render("changed-file-menu-overlay", Self::dismiss_file_menu, cx)
                    })),
            ))
            .into_any_element()
    }

    /// Opens the selected commit's menu below its row, from Shift-F10 or the
    /// Menu key: the menu a right-click on the row opens. It starts at the
    /// row's leading inset in the part of the list the viewport shows, where
    /// the columns scroll sideways.
    fn open_commit_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit_menu.is_some() || window.has_active_dialog(cx) || window.has_active_sheet(cx)
        {
            return;
        }
        let Some(index) = self.selected_commit else {
            return;
        };
        let Some(row) = self.visible.iter().position(|visible| *visible == index) else {
            return;
        };
        let Some(commit) = self.commits.get(index) else {
            return;
        };
        let commit = recovery::commit_target(commit);
        let mut position = native_accessibility::KeyboardMenu::below_row(
            &self.history_scroll,
            row,
            window.pixel_snap(px(self.settings.density.history_row_height())),
            appearance::ui_size(appearance::PANE_INSET),
        );
        position.x -= self.history_horizontal.offset().x;
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        let build = self.keyboard_commit_menu(owner, repository, commit);
        self.commit_menu = Some(native_accessibility::KeyboardMenu::open(
            position,
            self.focus.clone(),
            build,
            Self::dismiss_commit_menu,
            window,
            cx,
        ));
        cx.notify();
    }

    /// Closes the commit list's keyboard menu. A press outside it leaves
    /// focus in the menu, so focus returns to the list; an item or Escape
    /// has moved it already, to the list or to the dialog the item opened.
    fn dismiss_commit_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.commit_menu.take() {
            if menu.has_focus(window, cx) || window.focused(cx).is_none() {
                window.focus(&self.focus, cx);
            }
            cx.notify();
        }
    }

    /// Opens the highlighted changed file's menu beside its row, from
    /// Shift-F10 or the Menu key: the menu a right-click on the row opens.
    fn open_changed_file_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_menu.is_some() || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(index) = self.selected_file else {
            return;
        };
        let Some(row) = self
            .filtered_file_indices(cx)
            .iter()
            .position(|visible| *visible == index)
        else {
            return;
        };
        let Some(file) = self.files.get(index).cloned() else {
            return;
        };
        let commit = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .map(|commit| commit.oid.clone());
        let position = native_accessibility::KeyboardMenu::below_row(
            &self.file_scroll,
            row,
            window.pixel_snap(px(self.settings.density.file_row_height())),
            appearance::ui_size(appearance::PANE_INSET),
        );
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        self.file_menu = Some(native_accessibility::KeyboardMenu::open(
            position,
            self.file_focus.clone(),
            move |menu| changed_file_menu(menu, owner, repository, file, commit),
            Self::dismiss_file_menu,
            window,
            cx,
        ));
        cx.notify();
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
        let row = div()
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
            .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, window, cx)));
        // The list draws part of the row its foot cuts, and AT-SPI reported
        // that row whole past the list and the window's foot; its node
        // reports the part of it the list draws, as History's rows do.
        native_accessibility::clips_children(row)
            .app_context_menu(move |menu, _, _| {
                changed_file_menu(
                    menu,
                    history_owner.clone(),
                    history_repository.clone(),
                    history_file.clone(),
                    history_commit.clone(),
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
        window: &mut Window,
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
        let history_index = self
            .selected_file
            .filter(|_| self.mode != WorkspaceMode::Working && !self.file_history.is_active());
        let text = matches!(self.content.as_deref(), Some(Content::Text { .. }));
        let blame_index = if self.mode == WorkspaceMode::Working {
            self.working_selected.map(|(index, _)| index)
        } else {
            self.selected_file
        }
        .filter(|_| text);
        // The path keeps its room before its actions keep theirs: Blame
        // gives up its label, then the actions fold into one menu.
        let actions = file.map_or(PathActions::Labelled, |file| {
            self.path_actions(
                &file.path().display().to_string(),
                history_index.is_some(),
                blame_index.is_some(),
                text,
                (column, column_min),
                window,
            )
        });
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
                // Where not even the actions' menu leaves it room, the file
                // name stays whole and the directory elides first.
                ellipsis_start(
                    div()
                        .id("preview-file-path")
                        .debug_selector(|| "preview-file-path".into())
                        .flex_1()
                        .min_w_0(),
                )
                .text_size(crate::appearance::ui_text(12.))
                .font_weight(FontWeight::MEDIUM)
                .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                .child(
                    file.map(|file| file.path().display().to_string())
                        .unwrap_or("File comparison".into()),
                ),
            );
        if let Some(file) = file.filter(|_| actions == PathActions::Menu) {
            path_controls =
                path_controls.child(self.file_actions_menu(file, history_index, blame_index, cx));
        } else {
            path_controls = path_controls
                .children(file.map(|file| {
                    let mark = copy_feedback::Mark::new("copy-path", file.path());
                    let copy_mark = mark.clone();
                    copy_feedback::copy_button(mark, "", "Copy file path", cx)
                        .accessibility_label("Copy file path")
                        .on_click(move |_, window, cx| {
                            let path = copy_path.clone();
                            copy_feedback::copy(
                                window,
                                cx,
                                Some(copy_mark.clone()),
                                "file path",
                                path,
                            )
                        })
                }))
                .children(history_index.map(|index| {
                    button("file-history", "", "clock", false)
                        .accessibility_label("File history")
                        .tooltip("File history · follow renames along the first parent")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_file_history(index, window, cx)
                        }))
                }));
        }
        if let Some(index) = blame_index.filter(|_| actions != PathActions::Menu) {
            const DETAIL: &str =
                "Line attribution and history; working files include uncommitted lines";
            // Blame keeps its label where the whole path fits beside it;
            // otherwise its icon, so the path is not cut for a label.
            let labelled = actions == PathActions::Labelled;
            let blame = if labelled {
                button("open-blame", "Blame", "", false).tooltip(DETAIL)
            } else {
                compact_control_name(
                    button("open-blame", "", "blame", false),
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
        // The editor the text pane shows, which draws no edge of its own, so
        // the pane rings its edge for it.
        let mut text_focus = None;
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
            EmptyState::new(
                "working-preview-clean",
                empty_state::working_clean_line(self.work_status.as_deref(), true),
            )
            .pane(cx)
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
                        // On a row of its own the transparent menu gives its
                        // inset back to the toolbar's padding, so its label
                        // starts where the path's text does and its focus
                        // ring stays inside the column.
                        let own_row = !modes_share_the_path_row(
                            text_mode_menu_width(window),
                            column,
                            column_min,
                            window,
                        );
                        let menu = compact_control_label(
                            button("text-mode-menu", label, "", false),
                            "text-mode-menu",
                            format!("Comparison mode: {label}"),
                        )
                        .debug_selector(|| "text-mode-menu".into())
                        .px(appearance::ui_size(TEXT_MODE_MENU_INSET))
                        .when(own_row, |menu| {
                            menu.ml(-appearance::ui_size(TEXT_MODE_MENU_INSET))
                        })
                        .dropdown_caret(true)
                        .tooltip(format!("Comparison mode · {}", names.join(", ")))
                        .app_menu(move |menu, _, _| {
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
                        });
                        if own_row {
                            toolbar.child(div().w_full().flex().child(menu))
                        } else {
                            toolbar.child(menu)
                        }
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
                            |preview| self.render_rich_preview(preview, column, window, cx),
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
                        text_focus = self
                            .patch_editor
                            .as_ref()
                            .map(|editor| editor.read(cx).focus_handle(cx));
                        div().size_full().child(view.clone()).into_any_element()
                    } else if let Some(editor) = editor {
                        text_focus = Some(editor.read(cx).focus_handle(cx));
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
                Content::Images { old, new } => {
                    self.render_image_comparison(old, new, column, window, cx)
                }
                Content::Rich(preview) => self.render_rich_preview(preview, column, window, cx),
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
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(content)
                    // The text holding keyboard focus rings the pane's own
                    // edge, over its lines, so none of them moves.
                    .children(
                        text_focus
                            .and_then(|focus| self.focus_ring.edge(&focus, rgb(colors.accent))),
                    ),
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
/// first, drawn whole or not at all, and the label is drawn whole beside it or
/// not at all, its words then in the count's tooltip and accessible name
/// (DESIGN.md, "One responsive order": labels go, the count stays), so as the
/// inspector narrows the label leaves, then the count, and neither is cut.
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
            label: shows_count && room - count - gap >= files_label_width(window),
            count: shows_count,
            status: name - gap - word >= word,
        }
    }
}

/// A navigator branch's menu, which a right-click on its row and Shift-F10
/// or the Menu key on the cursor row open alike: its name, then Branch
/// actions…, which acts only while the repository the menu was built for is
/// still the one shown.
fn branch_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    name: String,
    remote: bool,
    disabled: bool,
) -> PopupMenu {
    menu.label(name.clone()).item(
        PopupMenuItem::new("Branch actions…")
            .disabled(disabled)
            .on_click(move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.path == repository && this.page == AppPage::Repository {
                        this.open_contextual_branch(name.clone(), remote, window, cx);
                    }
                });
            }),
    )
}

/// A navigator worktree's menu, which a right-click on its row and
/// Shift-F10 or the Menu key on the cursor row open alike: its path, then
/// its actions and removals, which act only while the repository the menu
/// was built for is still the one shown.
fn worktree_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    tree: Worktree,
    disabled: bool,
) -> PopupMenu {
    let mut menu = menu.label(tree.path.display().to_string());
    for (label, removal) in [
        ("Worktree actions…", None),
        ("Remove worktree…", Some(worktrees::RemovalMode::Ordinary)),
        (
            "Force remove worktree…",
            Some(worktrees::RemovalMode::Force),
        ),
    ] {
        let owner = owner.clone();
        let repository = repository.clone();
        let tree = tree.clone();
        menu = menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(
            move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.path == repository && this.page == AppPage::Repository {
                        this.open_worktree_actions(tree.clone(), removal, window, cx);
                    }
                });
            },
        ));
    }
    menu
}

/// A History changed file's menu, which a right-click on its row and
/// Shift-F10 or the Menu key on the highlighted row open alike. File history
/// opens only while the repository and commit the menu was built for are
/// still those shown and the file is still among their changed files.
fn changed_file_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    file: FileChange,
    commit: Option<String>,
) -> PopupMenu {
    menu.item(
        PopupMenuItem::new("File history…").on_click(move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == repository
                    && this
                        .selected_commit
                        .and_then(|i| this.commits.get(i))
                        .map(|c| &c.oid)
                        == commit.as_ref()
                    && let Some(index) = this.files.iter().position(|candidate| candidate == &file)
                {
                    this.open_file_history(index, window, cx);
                }
            });
        }),
    )
}

/// The widest of the parents' labelled controls, `labels`: the shared
/// `button` helper's 10 px of padding either side of its label. Laid out
/// again only when the labels, the font or the sizes change.
fn parent_controls_width(labels: &[String], window: &Window) -> Pixels {
    cached_text_width(&PARENT_LABELS, &labels.join("\n"), window, || {
        labels
            .iter()
            .map(|label| line_widths(&[(label, appearance::ui_text(12.))], window))
            .fold(px(0.), Pixels::max)
    }) + appearance::ui_size(20.)
}

/// "Changed files" whole, at the header's 11 pt medium.
fn files_label_width(window: &Window) -> Pixels {
    cached_width(&FILES_LABEL, &LineKey::current(window), || {
        shaped_width(
            FILES_LABEL_TEXT,
            appearance::ui_text(11.),
            FontWeight::MEDIUM,
            window,
        )
        .ceil()
    })
}

/// The inspector's file list heading.
const FILES_LABEL_TEXT: &str = "Changed files";

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
    /// The repository edge's line and its action's label, if any
    /// (`empty_state::RepositoryEdge`).
    edge: String,
    edge_short: String,
    edge_action: String,
    /// The compact bar's line and branch picker label, where an edge makes
    /// them differ from the full bar's.
    edge_compact: String,
    compact_branch: String,
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
    edge: Pixels,
    edge_short: Pixels,
    edge_action: Pixels,
    edge_compact: Pixels,
    compact_branch: Pixels,
    operation: Pixels,
    fetch: Pixels,
    pull: Pixels,
    push: Pixels,
    targets: Pixels,
    /// The tab strip's labelled actions (`GitTurtle::tab_strip_labels`).
    menu: Pixels,
    workspaces: Pixels,
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
            edge: regular(&text.edge, label),
            edge_short: regular(&text.edge_short, label),
            edge_action: regular(&text.edge_action, label),
            edge_compact: regular(&text.edge_compact, label),
            compact_branch: medium(&text.compact_branch),
            operation: regular(&text.operation, small),
            fetch: medium("Fetch"),
            pull: medium("Pull"),
            push: medium("Push"),
            targets: regular("Targets", label),
            menu: regular("Menu", label),
            workspaces: regular("Workspaces", label),
        }
    }
}

thread_local! {
    static CHROME_WIDTHS: RefCell<Option<(LineKey, ChromeText, ChromeWidths)>> =
        const { RefCell::new(None) };
}

/// How the full header lays out its trailing controls: Projects (or History
/// root's empty slot for it), the profile and Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trailing {
    /// After the spacer on a row with other controls: Projects, or the slot,
    /// before the profile and Settings, at the row's trailing edge.
    Shared,
    /// On a row of their own, led from the header's left edge by the profile
    /// and Settings in every mode, with Projects after them where it shows
    /// and no slot at History's root. `spacer` where the spacer stays on the
    /// row above; one that would start this row is left out, since it would
    /// push the row to the trailing edge.
    OwnRow { spacer: bool },
}

/// The lines a wrapping flex row of `items`, each its width and height,
/// takes in `room`, `gap` apart: each line's first item and its tallest
/// item's height. Lines break as taffy breaks them, before the item that
/// takes the line's length, gaps included, past `room`.
fn wrapped_rows(items: &[(Pixels, Pixels)], room: Pixels, gap: Pixels) -> Vec<(usize, Pixels)> {
    let mut rows: Vec<(usize, Pixels)> = Vec::new();
    let mut length = px(0.);
    for (index, &(width, item)) in items.iter().enumerate() {
        match rows.last_mut() {
            Some((_, tallest)) if length + gap + width <= room => {
                length += gap + width;
                *tallest = (*tallest).max(item);
            }
            _ => {
                rows.push((index, item));
                length = width;
            }
        }
    }
    rows
}

/// The height a wrapping flex row of `items` takes in `room`, `gap` apart on
/// both axes: its [`wrapped_rows`], each as tall as its tallest item.
fn wrapped_height(items: &[(Pixels, Pixels)], room: Pixels, gap: Pixels) -> Pixels {
    let rows = wrapped_rows(items, room, gap);
    let Some(((_, last), above)) = rows.split_last() else {
        return px(0.);
    };
    let mut height = px(0.);
    for (_, tallest) in above {
        height += *tallest + gap;
    }
    height + *last
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

    /// Whether the repository tab strip's labelled actions, Menu (on Linux)
    /// and Workspaces, keep their labels in this window: only where every
    /// tab's whole label, to the tab's cap, fits the tab list beside them
    /// (`tabs_whole_width`). Content comes first (DESIGN.md, "One responsive
    /// order"), so elsewhere both become 28 px icons before any tab's name
    /// is cut, as at 18 pt in a 461 × 490 half tile. Like the chrome, it is
    /// decided before layout from the interface font
    /// ([`Self::tab_list_room`]).
    pub(super) fn tab_strip_labels(&self, window: &Window, cx: &App) -> bool {
        self.tabs_whole_width(window) <= self.tab_list_room(true, window, cx)
    }

    /// The widths Menu (none off Linux, where the menu bar holds it) and
    /// Workspaces take labelled: Menu's measured label beside its 16 px
    /// icon, and Workspaces' between its icon and its caret, in the shared
    /// `button` helper's padding and gaps.
    pub(super) fn tab_strip_labelled_actions(&self, window: &Window) -> (Pixels, Pixels) {
        let widths = self.chrome_widths(window);
        let menu = if cfg!(target_os = "linux") {
            self.labelled_width(widths.menu, true, window)
        } else {
            px(0.)
        };
        let workspaces = self.labelled_width(widths.workspaces, true, window)
            + window.pixel_snap(appearance::ui_size(6.))
            + window.pixel_snap(window.rem_size() * 0.875);
        (menu, workspaces)
    }

    /// The room the tab strip leaves its tab list in this window, with Menu
    /// and Workspaces `labelled` or as their 28 px icons: the strip spans
    /// the window inside its `px_2`, its controls `gap_1` apart, and holds
    /// the window controls (`window_chrome::strip_width`), Menu, the list,
    /// the 28 px New tab and Workspaces, every length snapped to device
    /// pixels as layout snaps it. Labels are measured rounded up to a whole
    /// pixel, as the chrome's are, so the labelled room is less than a pixel
    /// per label short of what layout gives: the labels give way that much
    /// early, never late.
    pub(super) fn tab_list_room(&self, labelled: bool, window: &Window, cx: &App) -> Pixels {
        let rem = window.rem_size();
        let gap = window.pixel_snap(rem * 0.25);
        let control = window.pixel_snap(appearance::ui_size(28.));
        let (menu, workspaces) = if labelled {
            self.tab_strip_labelled_actions(window)
        } else {
            (control, control)
        };
        #[cfg(target_os = "linux")]
        let (menu, decorations) = (menu + gap, crate::window_chrome::strip_width(window, cx));
        #[cfg(not(target_os = "linux"))]
        let (menu, decorations) = {
            let _ = (cx, menu);
            (px(0.), px(0.))
        };
        content_size(window).width
            - window.pixel_snap(rem * 0.5) * 2.
            - decorations
            - menu
            - (gap + control)
            - (gap + workspaces)
    }

    /// The full header's and action bar's heights in this window: the header
    /// spans the window, the action bar the repository page beside the
    /// project pane.
    fn full_chrome_heights(&self, widths: &ChromeWidths, window: &Window) -> (Pixels, Pixels) {
        let content = content_size(window);
        let narrow = window.viewport_size().width < appearance::ui_size(COMPACT_HEADER_WIDTH);
        let page = self.full_actions_page(window);
        (
            self.full_header_height(
                content.width,
                narrow,
                self.back_label() != "Back to Projects",
                widths,
                window,
            ),
            self.full_actions_height(page, widths, window),
        )
    }

    /// What History needs below the chrome to show three whole rows: its
    /// scope toolbar, search row, the horizontal scrollbar's reserved track,
    /// the column header, the list's edge and three rows.
    pub(super) fn history_minimum(&self, window: &Window) -> Pixels {
        let scaled = |base: f32| window.pixel_snap(appearance::ui_size(base));
        scaled(40.)
            + scaled(38.)
            + Scrollbar::width()
            + scaled(HISTORY_HEADER_HEIGHT)
            + window.pixel_snap(px(1.)) * 2.
            + window.pixel_snap(px(self.settings.density.history_row_height())) * 3.
    }

    /// How the comparison path's row lays out its actions in a preview
    /// `column` wide whose narrowest is `column_min`, so the whole path,
    /// `path`, keeps its room before they keep theirs (DESIGN.md, "One
    /// responsive order"). The row is what the toolbar gives the path: all of
    /// it where the text modes (`modes`) wrap below it or there are none, less
    /// a Working file's area badge, then Copy path, File history (`history`)
    /// and Blame (`blame`), `gap_1` apart. Blame keeps its label where the
    /// path fits beside it, the actions keep their icons where it fits beside
    /// those, and otherwise, where two or more would fold, they fold into one
    /// File actions menu.
    fn path_actions(
        &self,
        path: &str,
        history: bool,
        blame: bool,
        modes: bool,
        (column, column_min): (Pixels, Pixels),
        window: &Window,
    ) -> PathActions {
        let rem = window.rem_size();
        let room = column - rem * 1.5;
        let control = appearance::ui_size(28.);
        let gap = rem * 0.25;
        let allotment =
            appearance::ui_size(220.).min((column_min - appearance::ui_size(19.5)).max(px(0.)));
        let labelled_modes = text_modes_width(&["Diff", "Split", "Before", "After"], window);
        let row = if modes
            && labelled_modes <= room
            && allotment + rem * 0.375 + labelled_modes <= room
        {
            room - rem * 0.375 - labelled_modes
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
        let needed = cached_text_width(&COMPARE_PATH, path, window, || {
            shaped_width(path, appearance::ui_text(12.), FontWeight::MEDIUM, window).ceil()
        });
        // Copy path, then File history and Blame where the page offers them.
        let count = 1 + usize::from(history) + usize::from(blame);
        let icons = (control + gap) * count as f32;
        let room = row - badge;
        if blame {
            let label = cached_width(&BLAME_LABEL, &LineKey::current(window), || {
                line_widths(&[("Blame", appearance::ui_text(12.))], window)
            }) + appearance::ui_size(20.);
            if room - (icons - control + label) >= needed {
                return PathActions::Labelled;
            }
        }
        if room - icons >= needed || count < 2 {
            PathActions::Icons
        } else {
            PathActions::Menu
        }
    }

    /// Copy file path, File history and Blame folded into one 28 px control
    /// showing the more-actions dots, where the whole path does not fit
    /// beside them as icons (`path_actions`): named "File actions", its
    /// tooltip leading with that name and listing them, its menu acting only
    /// on the file it was built for while Compare still shows it. A copy
    /// chosen there confirms on this control, as the branch picker confirms
    /// a switch: the check takes the dots' place and its tooltip reads
    /// Copied, with its name unchanged.
    fn file_actions_menu(
        &self,
        file: &FileChange,
        history: Option<usize>,
        blame: Option<usize>,
        cx: &Context<Self>,
    ) -> AnyElement {
        const NAME: &str = "File actions";
        let mark = copy_feedback::Mark::new("file-actions-menu", file.path());
        let copied = copy_feedback::confirmed(&mark, cx);
        let symbol = if copied { "check" } else { "more" };
        #[cfg(test)]
        note_control_icon(&"file-actions-menu".into(), &format!("icons/{symbol}.svg"));
        let names = ["Copy file path"]
            .into_iter()
            .chain(history.map(|_| "File history"))
            .chain(blame.map(|_| "Blame"))
            .collect::<Vec<_>>()
            .join(", ");
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        let target = file.path().to_path_buf();
        let text = file.path().to_string_lossy().into_owned();
        compact_control_name(
            button("file-actions-menu", "", "", false).child(
                div()
                    .debug_selector(|| "file-actions-more".into())
                    .flex()
                    .child(control_icon(symbol)),
            ),
            "file-actions-menu",
            NAME,
        )
        .debug_selector(|| "file-actions-menu".into())
        .tooltip(if copied {
            copy_feedback::COPIED.to_owned()
        } else {
            format!("{NAME} · {names}")
        })
        .app_menu(move |menu, _, _| {
            let (mark, text) = (mark.clone(), text.clone());
            let mut menu = menu.item(PopupMenuItem::new("Copy file path").on_click(
                move |_, window, cx| {
                    copy_feedback::copy(window, cx, Some(mark.clone()), "file path", text.clone())
                },
            ));
            let actions: [(Option<usize>, &str, FileAction); 2] = [
                (history, "File history…", GitTurtle::open_file_history),
                (blame, "Blame", GitTurtle::open_blame),
            ];
            for (index, label, action) in actions {
                let Some(index) = index else {
                    continue;
                };
                let (owner, repository, target) =
                    (owner.clone(), repository.clone(), target.clone());
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.shows_file(&repository, &target, index) {
                            action(this, index, window, cx);
                        }
                    });
                }));
            }
            menu
        })
        .into_any_element()
    }

    /// Whether Compare still shows the file at `path` in `repository`, as
    /// `index` names it: a File actions menu built for it acts on nothing
    /// else.
    fn shows_file(
        &self,
        repository: &Option<PathBuf>,
        path: &std::path::Path,
        index: usize,
    ) -> bool {
        let shown = if self.mode == WorkspaceMode::Working {
            self.working_selected.map(|(index, _)| index)
        } else {
            self.selected_file
        };
        self.path == *repository
            && self.page == AppPage::Repository
            && shown == Some(index)
            && self
                .selected_file
                .and_then(|index| self.files.get(index))
                .is_some_and(|file| file.path() == path)
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
        let edge = self.repository_edge();
        let branch = status
            .and_then(|status| status.branch.clone())
            .unwrap_or("Detached HEAD".into());
        let text = ChromeText {
            changes: self.changes_label(),
            profile: self.profile_title(),
            compact_branch: edge
                .as_ref()
                .and_then(|edge| edge.compact_branch())
                .unwrap_or_else(|| branch.clone()),
            branch,
            ahead: status.map_or_else(String::new, |s| format!("{} ahead", s.ahead)),
            behind: status.map_or_else(String::new, |s| format!("{} behind", s.behind)),
            operation: status
                .and_then(|s| s.operation.as_ref())
                .map_or_else(String::new, |operation| format!("{operation} in progress")),
            edge: edge.as_ref().map_or_else(String::new, |edge| edge.line()),
            edge_short: edge
                .as_ref()
                .map_or_else(String::new, |edge| edge.short_line()),
            edge_action: edge
                .as_ref()
                .and_then(|edge| edge.action())
                .map_or_else(String::new, |action| action.label.to_owned()),
            edge_compact: edge
                .as_ref()
                .map_or_else(String::new, |edge| edge.compact_line()),
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

    /// Whether History's root keeps an empty slot where Projects stands in
    /// the other modes: wherever the slot adds no row to the full header,
    /// so History lays out the controls the other modes do and the name
    /// takes the same share of the row. Where it would add a row, the other
    /// modes' row wraps and leaves the name its minimum, as History's
    /// tighter row does without the slot. On the trailing controls' own row
    /// ([`Trailing::OwnRow`]) the slot would only open that row with an
    /// empty space, so `render_header` asks only when they share a row.
    fn keeps_projects_slot(&self, narrow: bool, widths: &ChromeWidths, window: &Window) -> bool {
        let width = content_size(window).width;
        self.full_header_height(width, narrow, true, widths, window)
            <= self.full_header_height(width, narrow, false, widths, window)
    }

    /// How the full header lays out its trailing controls in this window,
    /// decided from the other modes' controls, Projects among them, so
    /// History's root lays them out alike.
    fn trailing(&self, narrow: bool, widths: &ChromeWidths, window: &Window) -> Trailing {
        let width = content_size(window).width;
        self.full_header_items(width, narrow, true, widths, window)
            .1
    }

    /// The full header's height in a window `width` wide: `render_header`'s
    /// controls in order, wrapped as layout wraps them, with Projects or its
    /// slot among the trailing controls where `projects`.
    fn full_header_height(
        &self,
        width: Pixels,
        narrow: bool,
        projects: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> Pixels {
        let (items, _, room, gap) = self.full_header_items(width, narrow, projects, widths, window);
        let snap = |length: Pixels| window.pixel_snap(length);
        let rem = window.rem_size();
        (wrapped_height(&items, room, gap) + snap(rem * 0.375) * 2. + snap(px(1.)))
            .max(snap(appearance::ui_size(48.)))
    }

    /// `render_header`'s full-form controls in a window `width` wide, in
    /// order, each its width and height, with Projects or its slot where
    /// `projects`; how the trailing controls are laid out ([`Trailing`]),
    /// which orders them and keeps or leaves out the spacer; and the room
    /// and gap they wrap in.
    fn full_header_items(
        &self,
        width: Pixels,
        narrow: bool,
        projects: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> (Vec<(Pixels, Pixels)>, Trailing, Pixels, Pixels) {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let scaled = |base: f32| snap(appearance::ui_size(base));
        let line = |size: Pixels| snap(window.text_style().line_height.to_pixels(size.into(), rem));
        let control = scaled(28.);
        // The return slot keeps the labelled Projects' width in every mode.
        let back = if narrow {
            control
        } else {
            self.labelled_width(widths.projects, true, window)
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
        let (room, gap) = (width - snap(rem * 0.75) * 2., snap(rem * 0.5));
        let (spacer, slot) = ((px(0.), px(0.)), (control, control));
        // The row Projects would take after the spacer: one the spacer or
        // Projects starts holds the trailing controls alone.
        let first = wrapped_rows(&[items.as_slice(), &[spacer, slot]].concat(), room, gap)
            .last()
            .map_or(0, |&(first, _)| first);
        let trailing = if first < items.len() {
            Trailing::Shared
        } else {
            Trailing::OwnRow {
                spacer: first > items.len(),
            }
        };
        if trailing != (Trailing::OwnRow { spacer: false }) {
            items.push(spacer);
        }
        if projects && trailing == Trailing::Shared {
            items.push(slot);
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
        if projects && trailing != Trailing::Shared {
            items.push(slot);
        }
        (items, trailing, room, gap)
    }

    /// The full action bar's height, Targets collapsed, in a repository page
    /// `width` wide, with the repository edge in its barest form, which the
    /// bar keeps wherever a richer one would cost it a row (`edge_form`).
    fn full_actions_height(&self, width: Pixels, widths: &ChromeWidths, window: &Window) -> Pixels {
        let barest = empty_state::EdgeForm::ORDER[2];
        self.full_actions_height_with(width, widths, barest, window)
    }

    /// How the full action bar draws the repository edge: the richest form
    /// (`EdgeForm::ORDER`) that costs the bar no row over the barest, as
    /// "One responsive order" makes an action an icon, then lets a caption
    /// go, before anything else gives way. Measured for the repository page
    /// as the chrome measures it. The `compact` bar, one row that never
    /// wraps, draws the barest form, and its line as its glyph where even
    /// its shortest whole form would be cut ([`Self::compact_edge_fits`]).
    pub(super) fn edge_form(&self, compact: bool, window: &Window) -> empty_state::EdgeForm {
        let [.., barest] = empty_state::EdgeForm::ORDER;
        let Some(edge) = self.repository_edge() else {
            return barest;
        };
        let widths = self.chrome_widths(window);
        if compact {
            return empty_state::EdgeForm {
                glyph: !self.compact_edge_fits(&edge, &widths, window),
                ..barest
            };
        }
        let page = self.full_actions_page(window);
        let least = self.full_actions_height_with(page, &widths, barest, window);
        empty_state::EdgeForm::ORDER
            .into_iter()
            .find(|form| self.full_actions_height_with(page, &widths, *form, window) <= least)
            .unwrap_or(barest)
    }

    /// Whether the compact action bar's row holds `edge`'s compact line
    /// whole beside the branch picker's whole label: the row's 28 px
    /// actions (Fetch, Pull, Push, the edge's own and Targets) keep their
    /// room, and the picker and the line share what is left, the line in
    /// its `px_2` box (`render_git_actions`). Where it would be cut, the
    /// bar draws the line's glyph, so the picker's name, the content, stays
    /// whole before a word of the line is cut.
    fn compact_edge_fits(
        &self,
        edge: &empty_state::RepositoryEdge,
        widths: &ChromeWidths,
        window: &Window,
    ) -> bool {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let control = snap(appearance::ui_size(28.));
        let gap = snap(rem * 0.5);
        let actions = 4. + if edge.action().is_some() { 1. } else { 0. };
        let room =
            self.full_actions_page(window) - snap(rem * 0.75) * 2. - (control + gap) * actions;
        // A medium kit Button: `px_3`, the 16 px icon and the label `gap_2`
        // apart, the caret after it, within its 280 px limit.
        let picker = (snap(rem * 0.75) * 2.
            + px(16.)
            + snap(rem * 0.5) * 2.
            + widths.compact_branch
            + snap(rem))
        .min(snap(px(280.)));
        picker + gap + snap(rem * 0.5) * 2. + widths.edge_compact <= room
    }

    /// The repository page's width beside the project pane, which the action
    /// bar spans.
    fn full_actions_page(&self, window: &Window) -> Pixels {
        let content = content_size(window);
        // Beside the pane the page keeps its 520 px minimum (`render`).
        if self.settings.project_pane {
            (content.width - self.project_pane_width(window)).max(px(520.))
        } else {
            content.width
        }
    }

    /// The full action bar's height, Targets collapsed, in a repository page
    /// `width` wide: `render_git_actions`' controls in order, wrapped as
    /// layout wraps them, the repository edge in `form`.
    fn full_actions_height_with(
        &self,
        width: Pixels,
        widths: &ChromeWidths,
        form: empty_state::EdgeForm,
        window: &Window,
    ) -> Pixels {
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
            items.push(if self.repository_edge().is_some() {
                // The edge's line and its action after it, the action below
                // the line where the row cannot hold both.
                let line = line(appearance::ui_text(12.));
                let words = if form.caption {
                    widths.edge
                } else {
                    widths.edge_short
                };
                (inset + words, line)
            } else {
                let count = px(12.) + snap(rem * 0.25);
                (
                    inset + count * 2. + widths.ahead + snap(rem * 0.5) + widths.behind,
                    text.max(px(12.)),
                )
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
        // The edge's action, after Push.
        if self
            .repository_edge()
            .is_some_and(|edge| edge.action().is_some())
        {
            items.push((
                if form.labelled {
                    self.labelled_width(widths.edge_action, true, window)
                } else {
                    scaled(28.)
                },
                scaled(28.),
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

/// How the comparison path's row lays out its actions (`path_actions`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum PathActions {
    /// Copy path and File history as icons, Blame labelled.
    Labelled,
    /// Every action as its icon.
    Icons,
    /// The actions folded into one File actions menu.
    Menu,
}

/// An action File actions offers for the file at an index.
type FileAction = fn(&mut GitTurtle, usize, &mut Window, &mut Context<GitTurtle>);

/// The room GPUI's truncation needs past a line's width. It ends a line in an
/// ellipsis only once the line's width, rounded down, exceeds the room, so a
/// line up to a pixel wider than its box is drawn whole and cut by the box's
/// side: at 18 pt in a 515 × 980 window a Working row drew its 99.7 px
/// "turtle.png" whole in a 99 px box and lost half a pixel of it. A pixel of
/// trailing padding, inside the box's clip, holds what the rounding leaves
/// out, so the line fits its box whole or ends in an ellipsis inside it.
const ELLIPSIS_SLACK: f32 = 1.;

/// One line that fits its box whole or ends in an ellipsis inside it at a
/// character boundary, never cut by the box's side ([`ELLIPSIS_SLACK`]).
pub(crate) fn ellipsis_end<E: Styled>(element: E) -> E {
    element.truncate().pr(px(ELLIPSIS_SLACK))
}

/// [`ellipsis_end`] eliding from the line's start, so a path keeps its file
/// name.
pub(crate) fn ellipsis_start<E: Styled>(element: E) -> E {
    element
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_start()
        .pr(px(ELLIPSIS_SLACK))
}

/// The selected commit's author as one line of the inspector's metadata:
/// whole, or ending in an ellipsis inside its box ([`ellipsis_end`]) where
/// a narrow inspector or a large interface size leaves less room than the
/// name, rather than cut by its box, as "GitTurtle QA" was at 18 pt. Its
/// tooltip and, as a label's value, its accessible name are the whole name.
fn author_line(name: SharedString, text: impl IntoElement) -> Stateful<Div> {
    let tooltip = name.clone();
    ellipsis_end(
        div()
            .id("commit-author")
            .debug_selector(|| "commit-author".into())
            .min_w_0()
            // AccessKit adapters read a label's text from its value.
            .role(Role::Label)
            .aria_label(name.clone())
            .aria_value(name)
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)),
    )
    .child(text)
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
    let labelled = text_modes_width(&MODES, window);
    let (modes, height) = if labelled <= room {
        (labelled, control + rem * 0.25)
    } else {
        (text_mode_menu_width(window), control)
    };
    let gap = rem * 0.375;
    let rows = if modes_share_the_path_row(modes, column, column_min, window) {
        control.max(height)
    } else {
        control + gap + height
    };
    (rem * 0.25 * 2. + rows + px(1.)).max(appearance::ui_size(42.))
}

/// Where a navigation row starts its first glyph: at the pane's edge, where
/// the section labels start, then 12 px further per tree level. The
/// keyboard cursor's 2 px left border takes its width from the inset, so the
/// row's content stays put as the cursor reaches it; it was a fixed 12 px,
/// 2.25 px right of the section labels at 13 pt and 1.5 px left of them at
/// 18 pt, and the cursor's row moved 2 px further right.
fn nav_row_inset(depth: usize, cursor: bool) -> Pixels {
    appearance::ui_size(appearance::PANE_INSET) + px(depth as f32 * 12.)
        - if cursor { px(2.) } else { px(0.) }
}

/// The space beside the collapsed text modes' label, at the default
/// interface size: an icon's inset, which the menu gives back to the
/// toolbar's padding on a row of its own, so its label starts at the path's
/// left edge while its frame and focus ring keep inside the column.
const TEXT_MODE_MENU_INSET: f32 = crate::ICON_INSET;

/// The collapsed text modes' menu at its widest: its inset either side of
/// the widest mode's label, then the shared `button` helper's 6 px gap and
/// the caret.
fn text_mode_menu_width(window: &Window) -> Pixels {
    let widest = cached_width(&TEXT_MODE_WIDEST, &LineKey::current(window), || {
        line_widths(&[("Before", appearance::ui_text(12.))], window)
    });
    appearance::ui_size(TEXT_MODE_MENU_INSET * 2. + 6.) + widest + window.rem_size() * 0.875
}

/// Whether text modes `modes` wide share the path's row in a preview
/// `column` wide whose narrowest is `column_min`: the path held at its
/// 220 px allotment, or the whole narrowest column, then `gap_1p5` and the
/// modes inside the toolbar's `px_3`.
fn modes_share_the_path_row(
    modes: Pixels,
    column: Pixels,
    column_min: Pixels,
    window: &Window,
) -> bool {
    let rem = window.rem_size();
    let path = appearance::ui_size(220.).min((column_min - appearance::ui_size(19.5)).max(px(0.)));
    path + rem * 0.375 + modes <= column - rem * 1.5
}

/// The width History's scope toolbar needs to keep its labels: `px_3` on
/// either side, seven `gap_2` gaps, the fixed-size branch icon, the count, the
/// scope name at the narrowest it is drawn in, and the Columns, Latest,
/// Previous and Older buttons, each the shared `button` helper's 10 px of
/// padding on either side of its label, and Columns and Older a 16 px icon
/// 6 px before it; with a new-history cue, its `notice` words whole and one
/// more gap. In a narrower column the toolbar keeps only icons.
fn labelled_scope_toolbar_width(
    count: &str,
    scope: &str,
    notice: Option<&str>,
    window: &Window,
) -> Pixels {
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
        + notice.map_or(Pixels::ZERO, |notice| {
            window.rem_size() * 0.5 + scope_notice_width(notice, window)
        })
}

/// The width the scope toolbar's new-history cue takes whole: "1 new
/// commit", "History updated".
fn scope_notice_width(notice: &str, window: &Window) -> Pixels {
    cached_text_width(&SCOPE_NOTICE, notice, window, || {
        line_widths(&[(notice, appearance::ui_text(10.))], window)
    })
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
/// the paging buttons `gap_1` apart, each as layout snaps it to the device's
/// pixels: at 18 pt a button's 38.77 px is drawn 39 px wide, so the four
/// left a search's "0 commits" 74.5 px where 75.4 px were counted, and the
/// count ran half a pixel past the label it was shown in.
fn compact_scope_room(column: Pixels, window: &Window) -> Pixels {
    column
        - window.rem_size() * (2. * 0.75 + 3. * 0.5 + 2. * 0.25)
        - window.pixel_snap(appearance::ui_size(28.)) * 4.
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

/// History's column header, above the commit list, before scaling.
const HISTORY_HEADER_HEIGHT: f32 = 34.;

/// What History's table is laid out with besides its columns, from the
/// window's font and sizes. `render_history` records it before anything in
/// its frame lays the columns out (`GitTurtle::history_metrics`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct HistoryMetrics {
    /// Narrow History's subject floor (`subject_floor`): the subject cell's
    /// insets and the floor of its text.
    subject_floor: f32,
    /// The subject cell's 10 px and `px_3` insets.
    subject_insets: f32,
    /// The `gap_1` between narrow History's pill and the subject's text.
    pill_gap: f32,
    /// Narrow History's compact pill at its widest for the loaded
    /// references (`compact_pill_width`), zero where no row has a pill.
    compact_pill: f32,
    /// Where the first column starts in the table: inside the list's 1 px
    /// edge, a row's 2 px selection edge and its `px_3` inset. The header,
    /// above the list, starts its columns there too.
    leading: f32,
    /// What the table keeps after its last column: a row's `px_3` inset and
    /// the list's edge.
    trailing: f32,
}

impl HistoryMetrics {
    /// The metrics for `window`'s font and sizes, with `references` the
    /// most any loaded commit shows.
    fn current(window: &Window, references: usize) -> Self {
        let rem = f32::from(window.rem_size());
        let inset = rem * 0.75;
        Self {
            subject_floor: f32::from(subject_floor(window)),
            subject_insets: 10. + inset,
            pill_gap: rem * 0.25,
            compact_pill: compact_pill_width(references, window),
            leading: 3. + inset,
            trailing: inset + 1.,
        }
    }

    /// The table's width besides its columns.
    fn insets(self) -> f32 {
        self.leading + self.trailing
    }

    /// The least of the subject's own text: its header's whole title.
    fn text_floor(self) -> f32 {
        self.subject_floor - self.subject_insets
    }

    /// What narrow History keeps at the subject's start for the compact
    /// pill and its gap, beside the text's floor.
    fn pill_room(self) -> f32 {
        if self.compact_pill > 0. {
            self.compact_pill + self.pill_gap
        } else {
            0.
        }
    }

    /// Narrow History's pill in a `subject`-wide cell: the whole badge, at
    /// most half the cell's text room, where that half holds the text's
    /// floor, and otherwise the compact pill, so the subject's text keeps
    /// at least its floor beside either.
    fn pill(self, subject: f32) -> Pill {
        let half = (subject - self.subject_insets - self.pill_gap) / 2.;
        if half >= self.text_floor() {
            Pill {
                compact: false,
                max: half,
            }
        } else {
            Pill {
                compact: true,
                max: self.compact_pill,
            }
        }
    }
}

/// The form narrow History draws every row's reference pill in for a frame,
/// and its widest.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pill {
    /// A branch glyph and the count, rather than the first name and "+N".
    compact: bool,
    max: f32,
}

/// Narrow History's compact pill at its widest for `references` on one
/// commit: its `px_1` insets, the branch glyph, its `gap_0p5` and the count
/// at the badge's 10 pt, measured as eights, among the widest figures, once
/// per font, size and number of digits. Zero without references.
fn compact_pill_width(references: usize, window: &Window) -> f32 {
    if references == 0 {
        return 0.;
    }
    let digits = ["8", "88", "888"][(references.ilog10() as usize).min(2)];
    let count = cached_text_width(&COMPACT_COUNT, digits, window, || {
        line_widths(&[(digits, appearance::ui_text(10.))], window)
    });
    let rem = f32::from(window.rem_size());
    rem * 0.5 + f32::from(appearance::ui_size(10.)) + rem * 0.125 + f32::from(count)
}

/// A compact pill's count of `references`, without allocating below ten.
fn reference_count(references: usize) -> SharedString {
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    DIGITS.get(references).map_or_else(
        || references.to_string().into(),
        |digit| SharedString::new_static(digit),
    )
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
    /// Narrow History's reference pill for this frame's subject width.
    pill: Pill,
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
/// row's subject is cut to nothing. It follows the interface font: a row's
/// text keeps the title's own measured width (`HistoryMetrics::text_floor`),
/// about 85.5 px at the default size and 117.5 px at 18 pt in IBM Plex Sans
/// on Linux, and about 93 and 128 px, cells of about 113 and 152 px, in the
/// unit tests' text system.
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
    static SCOPE_NOTICE: TextWidth = const { RefCell::new(None) };
    static SCOPE_NAME: TextWidth = const { RefCell::new(None) };
    static FILES_COUNT: TextWidth = const { RefCell::new(None) };
    static FILES_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static TEXT_MODES: TextWidth = const { RefCell::new(None) };
    static FOOTER_LINE: TextWidth = const { RefCell::new(None) };
    static COMPARE_PATH: TextWidth = const { RefCell::new(None) };
    static PARENT_LABELS: TextWidth = const { RefCell::new(None) };
    static AREA_BADGE: TextWidth = const { RefCell::new(None) };
    static STATUS_WORDS: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static BLAME_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static TEXT_MODE_WIDEST: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static SUBJECT_TITLE: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static GRAPH_TITLE: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static COMPACT_COUNT: TextWidth = const { RefCell::new(None) };
    /// The changed-file filter's placeholder as it fits the width its field
    /// last painted at (`workspace::placeholder_fit`).
    static CHANGED_FILTER_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    /// History's search placeholder as it fits the width its field last
    /// painted at.
    static SEARCH_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    /// The navigation filter's placeholder as it fits the width its field
    /// last painted at.
    static NAV_FILTER_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    /// [`SidebarWidths`] for the current font and sizes.
    static SIDEBAR_WIDTHS: RefCell<Option<(LineKey, SidebarWidths)>> = const { RefCell::new(None) };
}

/// The navigation's mode toggles, in order: each mode, its name, the glyph
/// it shows where the names do not fit one row, and its debug selector.
const NAV_MODES: [(NavMode, &str, &str, &str); 3] = [
    (NavMode::Local, "Local", "branch", "nav-mode-local"),
    (NavMode::Remote, "Remote", "remote", "nav-mode-remote"),
    (
        NavMode::Worktrees,
        "Worktrees",
        "worktree",
        "nav-mode-worktrees",
    ),
];

/// The navigation's keyboard hint, line by line.
pub(crate) fn nav_hint_lines() -> [String; 2] {
    [
        "Arrows browse · Enter filters or opens.".to_owned(),
        format!(
            "Left/Right folders · {} actions.",
            shortcuts::key_label("shift-f10")
        ),
    ]
}

/// How the navigation lays out at the width it is drawn at, in the one
/// responsive order (DESIGN.md): its mode toggles keep their labels where
/// all three fit one row, and otherwise become their 28 px glyphs, each
/// named and with its name in its tooltip, before one wraps under the
/// others; its keyboard hint shows where each of its lines fits whole, and
/// otherwise gives way whole rather than wrap inside a phrase, since the
/// tree's accessible name keeps the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SidebarForm {
    pub(crate) labelled_modes: bool,
    pub(crate) hint: bool,
}

/// What [`SidebarForm`] measures: each mode's name at 12 pt and the
/// hint's widest line at 10 pt.
#[derive(Clone, Copy)]
struct SidebarWidths {
    modes: [Pixels; 3],
    hint: Pixels,
}

/// The navigation filter's placeholders, each a shorter wording of the one
/// before it in whole words: the first that fits the field shows, so a
/// narrow sidebar or a large interface size reads "Filter branches…" or
/// "Filter" rather than "Filter branches & worktre" cut at the field's edge
/// (`workspace::placeholder_fit`).
pub(crate) const NAV_FILTER_PLACEHOLDERS: &[&str] =
    &["Filter branches & worktrees", "Filter branches…", "Filter"];
/// The navigation filter's accessible name, whole whatever form its
/// placeholder takes.
pub(crate) const NAV_FILTER_NAME: &str = "Filter branches and worktrees";

/// History's search field's placeholders, each a shorter wording of the one
/// before it in whole phrases: the first that fits the field shows, so a
/// narrow field reads "Search history…" rather than "…history, a" cut at its
/// edge or a dangling comma, and only "Search", the last, is ever cut before
/// an ellipsis (`workspace::placeholder_fit`).
pub(crate) const SEARCH_PLACEHOLDERS: &[&str] = &[
    "Search repository history, author, hash…",
    "Search history, author, hash…",
    "Search history…",
    "Search",
];
/// History's search field's accessible name, whole whatever form the
/// placeholder takes: the kit names an unlabelled field after the
/// placeholder it shows.
pub(crate) const SEARCH_NAME: &str = "Search repository history";

/// The History inspector's changed-file filter's placeholder, which reads
/// the short "Filter" where it does not fit whole, as the Working filter's
/// does, rather than a word cut at the field's edge ("Filter changed").
pub(crate) const CHANGED_FILTER_PLACEHOLDER: &str = "Filter changed paths…";
/// Its accessible name, whole whatever form the placeholder takes.
pub(crate) const CHANGED_FILTER_NAME: &str = "Filter changed paths";

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
    /// The icon asset each control drew since the map was last cleared, by
    /// its element ID: the shared `button` helper's and the profile's
    /// (`note_control_icon`).
    pub(crate) static CONTROL_ICONS: RefCell<std::collections::BTreeMap<SharedString, SharedString>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// The tooltip text each control was last drawn with, by its element ID,
    /// where a confirmation's word is only in its tooltip, as the branch
    /// picker's is (`note_control_tooltip`).
    pub(crate) static CONTROL_TOOLTIPS: RefCell<std::collections::BTreeMap<SharedString, String>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// Each History row's reference chip as it last drew it, the References
    /// column's badge or narrow History's pill, by commit index.
    static HISTORY_PILLS: RefCell<std::collections::BTreeMap<usize, PillProbe>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// Each History row's accessible name and its pill's tooltip as it last
    /// drew them, by commit index.
    static HISTORY_ROW_NAMES: RefCell<std::collections::BTreeMap<usize, RowNameProbe>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
}
/// A drawn reference chip's name, none in the compact pill; its "+N", or the
/// compact pill's count; and its label colour.
#[cfg(test)]
type PillProbe = (Option<String>, Option<String>, u32);
/// A drawn History row's accessible name and its pill's tooltip.
#[cfg(test)]
type RowNameProbe = (String, Option<SharedString>);

/// The width, at the default interface size, of the ring around the compact
/// Changes count, drawn in the fill the count sits on, which cuts the badge
/// out of the glyph it overlaps.
const CHANGES_BADGE_RING: f32 = 1.;

/// The group of the compact Changes toggle and its count, whose hover and
/// press the count's ring follows.
const CHANGES_TOGGLE: &str = "compact-changes-toggle";

/// The compact Changes toggle's working-file count, a small accent pill on
/// the icon's top-right corner inside the toggle, so the toggle keeps its
/// width and its focus ring. The pill sits in a cut-out a
/// [`CHANGES_BADGE_RING`] wider on every side, filled with the colour
/// beneath it: the toggle group's `subtle` fill, or the toggle's own fill
/// over it while it is `selected`, hovered or pressed
/// ([`appearance::Palette::control_fills`]). So the glyph's strokes stop
/// short of the count rather than run into it, and no halo shows around it.
/// The cut-out is the outermost shape: GPUI paints a bordered quad's fill to
/// its outer edge beneath the border, so a ring drawn as the pill's border
/// let the pill's antialiased edge show past it as a half-pixel fringe of
/// the accent (1.23:1 in Braden, 1.43:1 in Midnight); here the outer edge
/// blends the fill beneath with itself, and the pill's own edge lies inside
/// the cut-out. Above 99 it reads "99+".
fn changes_badge(count: usize, selected: bool, p: appearance::Palette) -> impl IntoElement {
    let ring = appearance::ui_size(CHANGES_BADGE_RING);
    let [rest, hovered, pressed] = p.control_fills(selected, p.subtle);
    div()
        // Named so its `group_active` style has the state to follow a press.
        .id("changes-count-badge")
        .debug_selector(|| "changes-count-badge".into())
        .absolute()
        .top(appearance::ui_size(1.) - ring)
        .right(appearance::ui_size(1.) - ring)
        .p(ring)
        .rounded_full()
        .bg(rgb(rest))
        .group_hover(CHANGES_TOGGLE, move |style| style.bg(rgb(hovered)))
        .group_active(CHANGES_TOGGLE, move |style| style.bg(rgb(pressed)))
        .child(
            div()
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
                }),
        )
}

/// [`compact_control_name`] for a `name` that changes with what the control
/// stands for, such as the Changes count or a Git action's target.
pub(crate) fn compact_control_label(
    control: Button,
    selector: &'static str,
    name: impl Into<SharedString>,
) -> Button {
    let name = name.into();
    note_compact_label(selector, &name);
    control.accessibility_label(name)
}

/// Notes `path`, an `icons/` asset, as the icon the control with element ID
/// `id` draws, for a test to read back ([`CONTROL_ICONS`]); outside tests it
/// does nothing.
pub(crate) fn note_control_icon(id: &ElementId, path: &str) {
    #[cfg(test)]
    if let ElementId::Name(id) = id {
        CONTROL_ICONS.with(|icons| {
            icons
                .borrow_mut()
                .insert(id.clone(), path.to_owned().into())
        });
    }
    #[cfg(not(test))]
    let _ = (id, path);
}

/// Notes `tooltip` as the tooltip text the control with element ID `id` is
/// drawn with, for a test to read back ([`CONTROL_TOOLTIPS`]); outside tests
/// it does nothing.
pub(crate) fn note_control_tooltip(id: &ElementId, tooltip: &str) {
    #[cfg(test)]
    if let ElementId::Name(id) = id {
        CONTROL_TOOLTIPS
            .with(|tooltips| tooltips.borrow_mut().insert(id.clone(), tooltip.to_owned()));
    }
    #[cfg(not(test))]
    let _ = (id, tooltip);
}

/// Notes `name` as the accessible name the compact form of the element whose
/// debug selector is `selector` was given, for a test to read back
/// ([`COMPACT_LABELS`]); outside tests it does nothing.
pub(crate) fn note_compact_label(selector: &'static str, name: &SharedString) {
    #[cfg(test)]
    COMPACT_LABELS.with(|names| names.borrow_mut().insert(selector, name.clone()));
    #[cfg(not(test))]
    let _ = (selector, name);
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
/// render asks the text system nothing. `T` is one width or several taken
/// together.
pub(crate) fn cached_width<T: Copy>(
    cache: &'static std::thread::LocalKey<RefCell<Option<(LineKey, T)>>>,
    key: &LineKey,
    width: impl FnOnce() -> T,
) -> T {
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

    /// The inspector panel's widths in a repository page `width` wide: from
    /// its minimum to 480 px, never past what the content column's minimum
    /// leaves it, so the page holds it whole, and exactly its minimum where
    /// the window pins it. A scaled minimum can pass the 480 px maximum, as
    /// at 18 pt with a desktop text scale of 1.25.
    fn inspector_range(width: Pixels, columns: &ColumnMinimums) -> std::ops::Range<Pixels> {
        let end = if columns.pinned {
            columns.inspector
        } else {
            px(480.)
                .max(columns.inspector)
                .min(width - columns.content)
                .max(columns.inspector)
        };
        columns.inspector..end
    }

    /// The inspector's width in a repository page `width` wide from its
    /// saved width alone, before the panel group lays it out: a choice the
    /// window's size makes before layout, as the chrome's form is
    /// (`compare_minimum`), takes this, so it never changes in the frame
    /// after a resize.
    fn expected_inspector_width(&self, width: Pixels, columns: &ColumnMinimums) -> Pixels {
        let range = Self::inspector_range(width, columns);
        px(self.settings.inspector_width)
            .min(range.end)
            .max(range.start)
    }

    /// The width the inspector is drawn at in a repository page `width`
    /// wide, which its contents are laid out for. The panel group lays the
    /// inspector out at its panel's size within `inspector_range`: the saved
    /// width until the group first measures it, then the size the group
    /// keeps, which it rescales with the window and which a drag sets. That
    /// size can differ from the saved width, which only a drag changes: where
    /// a tiling window manager narrows the window after it opens, the group
    /// draws the inspector narrower than it was saved.
    fn drawn_inspector_width(&self, width: Pixels, columns: &ColumnMinimums, cx: &App) -> Pixels {
        let range = Self::inspector_range(width, columns);
        // The group's placeholder before it measures the panel, as its own
        // first layout tells them apart.
        let size = self
            .content_panels
            .read(cx)
            .sizes()
            .get(1)
            .copied()
            .filter(|size| *size != gpui_kit::base::PANEL_MIN_SIZE)
            .unwrap_or(px(self.settings.inspector_width));
        size.min(range.end).max(range.start)
    }

    /// The width the repository page receives beside the project pane.
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
        let content_min = columns.content;
        // The inspector's message toolbar fits the commit's Actions beside
        // its copy controls at the width the inspector is drawn at, which can
        // be narrower than its saved width, and where the inspector is pinned
        // it reserves the rows they take there.
        let inspector_width = self.drawn_inspector_width(width, &columns, cx);
        self.inspector_message
            .lay_out_toolbar(Some(commit_message::Toolbar::within(
                inspector_width,
                columns.pinned,
                window,
            )));
        // Where it is pinned its "Changed files" header keeps its count whole.
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
                        .child(self.render_sidebar(window, cx)),
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
                    .size_range(Self::inspector_range(width, &columns))
                    .flex_none()
                    .child(if self.mode == WorkspaceMode::Working {
                        self.render_working_inspector(inspector_width, columns.pinned, window, cx)
                    } else {
                        self.render_inspector(files_header, cx)
                    }),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_git_actions(
                chrome.is_compact(),
                self.edge_form(chrome.is_compact(), window),
                cx,
            ))
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
        // Before anything reads it: whether a busy spell looks busy yet.
        self.track_busy_look(cx);
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
                        .child(self.render_project_pane(cx)),
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
        // Everything an open dialog covers. The dialogs and the copy
        // announcement are drawn beside it, inside the element whose actions
        // they dispatch through.
        let page_frame = div()
            .id("gitturtle-page")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.render_repository_tabs(window, cx))
            .when(self.page != AppPage::Projects, |root| {
                root.child(self.render_header(chrome, window, cx))
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
                                    // `request_history_discovery` refuses while
                                    // a write runs.
                                    .map(|view| crate::workspace::refusing(
                                        view,
                                        || "view-created-commit".into(),
                                        self.loading.is_some() || self.operation_busy.is_some(),
                                        self.loading.is_some() || self.operation_shown().is_some(),
                                    ))
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
                            AppPage::Repository => self.status_line().to_owned(),
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
            );
        div()
            .id("gitturtle")
            .relative()
            .track_focus(&self.app_focus)
            .key_context("GitTurtle")
            .child(self.focus_ring.pointer_listener())
            .on_action(cx.listener(|this, _: &theme_editor::PageUpAction, _, cx| {
                this.scroll_settings_page(settings::PageStep::Up, cx)
            }))
            .on_action(
                cx.listener(|this, _: &theme_editor::PageDownAction, _, cx| {
                    this.scroll_settings_page(settings::PageStep::Down, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &theme_editor::PageTopAction, _, cx| {
                this.scroll_settings_page(settings::PageStep::Top, cx)
            }))
            .on_action(
                cx.listener(|this, _: &theme_editor::PageBottomAction, _, cx| {
                    this.scroll_settings_page(settings::PageStep::Bottom, cx)
                }),
            )
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
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Copy, _, cx| {
                    if !this.inspector_message.copy_selection(cx) {
                        cx.propagate();
                    }
                }),
            )
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
            .on_action(
                cx.listener(|this, _: &OpenTerminal, window, cx| this.open_terminal(window, cx)),
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
                this.toggle_sidebar(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleProjectPane, window, cx| {
                this.toggle_project_pane(window, cx)
            }))
            .child(native_accessibility::beneath_modal(
                page_frame,
                window.has_active_dialog(cx),
            ))
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

    pub(crate) fn draw(cx: &mut VisualTestContext) {
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
    pub(crate) fn history_script(commits: usize, lines: usize) -> String {
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

    /// On Linux, Compare offers Open in default app for what the captured
    /// bytes are, never for what the file is called: a PDF named like a
    /// launcher offers it in its After header, a PNG in its After caption and
    /// a text model in its After header, and an ELF stub offers nothing.
    ///
    /// The control lies whole inside the part of its side's header or
    /// caption that the preview's clip leaves visible, its focus ring
    /// included, in a full window and in the narrowest one, at the default
    /// size and at 18 pt: in a 461 × 490 window the image's caption once
    /// wrapped the control to a third line below that clip, so it was
    /// offered to assistive technology but never drawn, while the caption's
    /// own bounds held it. Where its label fits beside the side's
    /// name it is labelled; below that width it is its icon alone, the
    /// scaled 28 px square, so the content beside it keeps its place: in a
    /// 461 × 490 window the PDF header's control read "Open in d" at 13 pt,
    /// cut at its column's edge, and the image's ran into the inspector at
    /// 18 pt, though the test checked only the window's edge.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn open_in_default_app_follows_content_in_compare(cx: &mut TestAppContext) {
        let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n";
        let elf = b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x02\0\x3e\0";
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let png = crate::system_preview::tests::png(None);
        let input = file_commits(&[(
            "media",
            &[
                ("launcher.desktop", pdf),
                ("tool", elf),
                ("picture.png", &png),
                ("shape.obj", obj),
            ],
        )]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        let open = |path: &str, cx: &mut VisualTestContext| {
            let index = app.read_with(cx, |app, _| {
                app.files
                    .iter()
                    .position(|file| file.path() == std::path::Path::new(path))
                    .expect("the commit changes it")
            });
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(index, window, cx)));
        };
        // Each offer: its file, its control, the header or caption it sits
        // in, and whether a full window labels it (the model's row also
        // holds Copy source, so it may not).
        let offers = [
            (
                "launcher.desktop",
                "system-preview-1",
                "rich-side-header-1",
                true,
            ),
            (
                "picture.png",
                "image-system-preview-1",
                "image-side-caption-1",
                true,
            ),
            (
                "shape.obj",
                "model-external-1",
                "model-side-header-1",
                false,
            ),
        ];
        for (text, width, height) in [
            (13, 1480., 980.),
            (13, 461., 490.),
            (18, 1480., 980.),
            (18, 461., 490.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            let at = format!("{text} pt in {width} × {height}");
            let (square, labelled) = cx.update(|window, _| {
                (
                    appearance::ui_size(28.),
                    appearance::ui_size(42.)
                        + line_widths(&[("Open in default app", appearance::ui_text(12.))], window),
                )
            });
            for (path, control, area, wide) in offers {
                open(path, cx);
                settle(&app, cx).await;
                assert!(
                    cx.debug_bounds("system-preview-0").is_none()
                        && cx.debug_bounds("image-system-preview-0").is_none()
                        && cx.debug_bounds("model-external-0").is_none(),
                    "{at}: {path}: Before is absent"
                );
                let bounds = cx
                    .debug_bounds(control)
                    .unwrap_or_else(|| panic!("{at}: {path} offers the action"));
                let side = cx
                    .debug_bounds(area)
                    .unwrap_or_else(|| panic!("{at}: {path}: {area}"));
                // The part of the header or caption the preview's clip
                // leaves visible: a caption pushed below it once held its
                // control, which was offered but never drawn.
                let content = cx.debug_bounds("preview-content").unwrap();
                let visible = side.intersect(&content);
                assert!(
                    holds_with_ring(visible, bounds),
                    "{at}: {path}: {bounds:?} and its ring lie in the visible part {visible:?} of {area} {side:?}"
                );
                assert!(
                    side.left() >= content.left() && side.right() <= content.right() + px(0.5),
                    "{at}: {path}: {side:?} lies in the preview {content:?}"
                );
                let shows_label = cx
                    .debug_bounds(match control {
                        "system-preview-1" => "system-preview-1/rest/text",
                        "image-system-preview-1" => "image-system-preview-1/rest/text",
                        _ => "model-external-1/rest/text",
                    })
                    .is_some();
                if shows_label {
                    assert!(
                        (bounds.size.width - labelled).abs() <= px(1.),
                        "{at}: {path}: labelled, {bounds:?} is {labelled:?} wide"
                    );
                } else {
                    assert!(
                        (bounds.size.width - square).abs() <= px(0.5)
                            && (bounds.size.height - square).abs() <= px(0.5),
                        "{at}: {path}: its icon alone, {bounds:?} is the {square:?} square"
                    );
                }
                let narrow = width < 1000.;
                if narrow || wide {
                    assert_eq!(
                        shows_label, !narrow,
                        "{at}: {path}: labelled only where the label fits"
                    );
                }
            }
            open("tool", cx);
            settle(&app, cx).await;
            let refused = app.read_with(cx, |app, _| match app.content.as_deref() {
                Some(Content::Rich(preview)) => {
                    preview.new.captured.is_some() && preview.new.system_preview.is_none()
                }
                _ => false,
            });
            assert!(refused, "{at}: the ELF stub is captured and offers nothing");
            assert!(cx.debug_bounds("system-preview-1").is_none());
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The `(path, bytes)` files a fixture commit writes.
    #[cfg(target_os = "linux")]
    type Files<'a> = &'a [(&'a str, &'a [u8])];

    /// A `git fast-import` stream of one commit per `(message, files)` on
    /// `main`, oldest first, each writing its files.
    #[cfg(target_os = "linux")]
    fn file_commits(commits: &[(&str, Files<'_>)]) -> Vec<u8> {
        let mut input = Vec::new();
        for (index, (message, files)) in commits.iter().enumerate() {
            input.extend_from_slice(
                format!(
                    "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\n",
                    1_700_000_000 + index,
                    message.len(),
                )
                .as_bytes(),
            );
            for (path, bytes) in *files {
                input.extend_from_slice(
                    format!("M 100644 inline {path}\ndata {}\n", bytes.len()).as_bytes(),
                );
                input.extend_from_slice(bytes);
                input.push(b'\n');
            }
        }
        input.extend_from_slice(b"done\n");
        input
    }

    /// Whether `control`, with the focus ring it draws outside its edge,
    /// lies inside `area`.
    #[cfg(target_os = "linux")]
    fn holds_with_ring(area: Bounds<Pixels>, control: Bounds<Pixels>) -> bool {
        let ring = appearance::BUTTON_FOCUS_RING.width + appearance::BUTTON_FOCUS_RING.gap;
        let slack = px(0.5);
        control.left() - ring >= area.left() - slack
            && control.right() + ring <= area.right() + slack
            && control.top() - ring >= area.top() - slack
            && control.bottom() + ring <= area.bottom() + slack
    }

    /// A changed image offers Open in default app on each side, and each
    /// control sits in its own side's caption, after the side's dimensions,
    /// as a document's header has it: both once shared a row of their own
    /// above the comparison's modes, which began at the Before column's edge
    /// and ran over Before though one of them opens After. Without literal
    /// source to copy, the comparison draws no such row.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn open_in_default_app_sits_in_its_image_sides_caption(cx: &mut TestAppContext) {
        use crate::system_preview::tests::png;
        let (before, after) = (png(Some(b"before")), png(Some(b"after")));
        let input = file_commits(&[
            ("add", &[("image.png", &before)]),
            ("change", &[("image.png", &after)]),
        ]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let both = app.read_with(cx, |app, _| match app.content.as_deref() {
            Some(Content::Images { old, new }) => {
                old.system_preview.is_some() && new.system_preview.is_some()
            }
            _ => false,
        });
        assert!(both, "both sides of the changed PNG offer the action");
        for (control, caption) in [
            ("image-system-preview-0", "image-side-caption-0"),
            ("image-system-preview-1", "image-side-caption-1"),
        ] {
            let control = cx.debug_bounds(control).expect("the side offers it");
            let caption = cx.debug_bounds(caption).expect("the side's caption");
            assert!(
                holds_with_ring(caption, control),
                "{control:?} and its ring lie in its side's caption {caption:?}"
            );
        }
        assert!(
            cx.debug_bounds("image-captured-actions").is_none(),
            "no row of actions above the modes"
        );
    }

    /// A changed image's captions keep each side's open control where it can
    /// be seen and leave both sides still. In a 461 × 490 window the modes
    /// (233 px) and zooms (191 px) ran past the 177 px column on two rows, the
    /// scale notice took four lines, and each caption wrapped its control to a
    /// third line below the preview's clip: offered, never drawn. Now the
    /// groups become one-control menus where they do not fit, the scale
    /// notice follows the image with the guidance, one line each, and the
    /// control follows the side's name, so in the narrow window, at 13 and
    /// at 18 pt, each control
    /// and its ring lie in what the preview shows of its caption, and the two
    /// names share their top (Before once sat 8 px below After, centred
    /// beside a wrapped control). The notes keep their one line where the
    /// column holds it and otherwise leave. When a click opens nothing, the
    /// line saying why takes its own room beneath that side's caption, so
    /// neither caption nor name moves (a two-line refusal once grew the
    /// captions by 42.5 px and dropped Before's caption 20.5 px) and the image
    /// gives way below it.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn image_captions_keep_their_controls_in_view_and_still(cx: &mut TestAppContext) {
        use crate::system_preview::{
            DISMISSED, NotOpened,
            tests::{latest_note, png, queue},
        };
        let (before, after) = (png(Some(b"before")), png(Some(b"after")));
        let input = file_commits(&[
            ("add", &[("image.png", &before)]),
            ("change", &[("image.png", &after)]),
        ]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let bounds = |selector: &'static str, cx: &mut VisualTestContext| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let sides = [
            ("image-system-preview-0", "image-side-caption-0"),
            ("image-system-preview-1", "image-side-caption-1"),
        ];
        let mut one_line = Vec::new();
        for (text, width, height) in [
            (13, 1480., 980.),
            (13, 461., 490.),
            (18, 1480., 980.),
            (18, 461., 490.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let content = bounds("preview-content", cx);
            for (control, caption) in sides {
                let (control, caption) = (bounds(control, cx), bounds(caption, cx));
                let visible = caption.intersect(&content);
                assert!(
                    holds_with_ring(visible, control),
                    "{at}: {control:?} and its ring lie in the visible part {visible:?} of its side's caption {caption:?}"
                );
            }
            let names = ["image-side-name-0", "image-side-name-1"].map(|name| bounds(name, cx));
            assert!(
                (names[0].top() - names[1].top()).abs() <= px(0.5),
                "{at}: the side names share their top: {names:?}"
            );
            // The modes and zooms, labelled or as their menus, lie in the
            // column with their focus rings.
            let narrow = width < 1000.;
            let (shown, hidden) = (
                ["image-modes", "image-zooms"],
                ["image-mode-menu", "image-zoom-menu"],
            );
            let (shown, hidden) = if narrow {
                (hidden, shown)
            } else {
                (shown, hidden)
            };
            for selector in shown {
                let group = bounds(selector, cx);
                assert!(
                    holds_with_ring(content, group),
                    "{at}: {selector} {group:?} lies in the column {content:?}"
                );
            }
            for selector in hidden {
                assert!(cx.debug_bounds(selector).is_none(), "{at}: no {selector}");
            }
            if narrow {
                assert_eq!(
                    ["image-mode-menu", "image-zoom-menu"].map(|selector| {
                        COMPACT_LABELS.with(|names| names.borrow().get(selector).cloned())
                    }),
                    [
                        Some("Comparison mode: Side by side".into()),
                        Some("Zoom: Fit".into())
                    ],
                    "{at}: each menu is named by its group and current choice"
                );
            }
            // The notes keep the one line they take in a full window where
            // the column holds it, and otherwise leave rather than lie under
            // the preview's clip.
            let lines = ["image-source-scale", "image-interaction-guidance"]
                .map(|note| cx.debug_bounds(note));
            for note in lines.iter().flatten() {
                assert!(
                    note.bottom() <= content.bottom() + px(0.5),
                    "{at}: {note:?} lies in the column {content:?}"
                );
            }
            let lines = lines.map(|note| note.map(|note| note.size.height));
            if narrow {
                let full = one_line.pop().unwrap();
                for (line, full) in lines.into_iter().zip(full) {
                    assert!(
                        line.is_none_or(|line| Some(line) == full),
                        "{at}: one line each"
                    );
                }
            } else {
                assert!(
                    lines.iter().all(Option::is_some),
                    "{at}: both notes are drawn"
                );
                one_line.push(lines);
            }
        }

        // A click that opens nothing moves neither caption.
        let refusal = "The desktop would open this copy as an SVG document (image/svg+xml) rather than as a PNG image, so nothing was opened.";
        for (width, height) in [(1480., 980.), (461., 490.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let still = |cx: &mut VisualTestContext| {
                [
                    "image-side-caption-0",
                    "image-side-caption-1",
                    "image-side-name-0",
                    "image-side-name-1",
                ]
                .map(|selector| bounds(selector, cx))
            };
            let rest = still(cx);
            for (outcome, expected) in [
                (
                    Err(NotOpened::Failed(refusal.into())),
                    (format!("Could not open After. {refusal}"), true),
                ),
                (Err(NotOpened::Dismissed), (DISMISSED.to_owned(), false)),
            ] {
                queue(outcome);
                let control = bounds("image-system-preview-1", cx);
                cx.simulate_click(control.center(), Modifiers::default());
                cx.run_until_parked();
                draw(cx);
                let at = format!("{width} × {height}, {:?}", expected.0);
                assert_eq!(cx.read(latest_note), Some(expected.clone()), "{at}");
                assert_eq!(still(cx), rest, "{at}: the captions and names stay put");
                let caption = bounds("image-side-caption-1", cx);
                let note = bounds("image-side-note-1", cx);
                assert!(
                    note.top() >= caption.bottom() - px(0.5)
                        && note.left() >= caption.left() - px(0.5)
                        && note.right() <= caption.right() + px(0.5),
                    "{at}: the line {note:?} lies beneath After's caption {caption:?}"
                );
                assert!(cx.debug_bounds("image-system-preview-1/note").is_some());
                // In its own room: the image gives way rather than lying
                // beneath it.
                let image = bounds("image-canvas-area", cx);
                assert!(
                    image.top() >= note.bottom() - px(0.5),
                    "{at}: the image {image:?} starts below the line {note:?}"
                );
            }
        }
    }

    /// The line saying why a click on an image side's Open in default app
    /// opened nothing takes its own room beneath its side's caption, in the
    /// column's flow: laid over the image, a refusal in a 461 × 490 window,
    /// where the image had 27 px at 13 pt and none at 18 pt, painted over both
    /// of the comparison's notes and ran past the column. In that window and
    /// in a taller and a wider one, at 13 and 18 pt, the line lies beneath
    /// After's caption and inside the room the captions leave, neither side's
    /// name moves, the image starts below the line, and each note lies below
    /// it inside the column or, where that room holds no line for it, has
    /// left, its words kept in the image's accessible description. Where the
    /// room holds them, the line shows at least two of its lines.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_refused_open_takes_its_own_room_and_covers_no_note(cx: &mut TestAppContext) {
        use crate::system_preview::{
            NotOpened,
            tests::{latest_note, png, queue},
        };
        let input = file_commits(&[("add", &[("polyglot.png", &png(Some(b"after")))])]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let bounds = |selector: &'static str, cx: &mut VisualTestContext| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let refusal = "The desktop would open this copy as an SVG document (image/svg+xml) rather than as a PNG image, so nothing was opened.";
        let slack = px(0.5);
        for (text, width, height) in [
            (13, 461., 490.),
            (18, 461., 490.),
            (13, 461., 980.),
            (18, 461., 980.),
            (13, 640., 680.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let names = ["image-side-name-0", "image-side-name-1"].map(|name| bounds(name, cx));
            queue(Err(NotOpened::Failed(refusal.into())));
            let control = bounds("image-system-preview-1", cx);
            cx.simulate_click(control.center(), Modifiers::default());
            cx.run_until_parked();
            draw(cx);
            draw(cx);
            assert_eq!(
                cx.read(latest_note),
                Some((format!("Could not open After. {refusal}"), true)),
                "{at}"
            );
            assert_eq!(
                ["image-side-name-0", "image-side-name-1"].map(|name| bounds(name, cx)),
                names,
                "{at}: neither side's name moves"
            );
            let column = bounds("preview-content", cx);
            let caption = bounds("image-side-caption-1", cx);
            let room = bounds("image-stage-room", cx);
            let band = bounds("image-side-note-1", cx);
            let line = bounds("image-system-preview-1/note", cx);
            assert!(
                band.top() >= caption.bottom() - slack
                    && band.left() >= caption.left() - slack
                    && band.right() <= caption.right() + slack,
                "{at}: the line's band {band:?} lies beneath After's caption {caption:?}"
            );
            assert!(
                band.bottom() <= room.bottom() + slack && line.bottom() <= room.bottom() + slack,
                "{at}: the line {line:?} keeps to the room {room:?} beneath the captions"
            );
            let image = bounds("image-canvas-area", cx);
            assert!(
                image.top() >= band.bottom() - slack,
                "{at}: the image {image:?} starts below the line's band {band:?}"
            );
            let mut drawn = 0;
            for note in ["image-source-scale", "image-interaction-guidance"] {
                let Some(note) = cx.debug_bounds(note) else {
                    continue;
                };
                drawn += 1;
                assert!(
                    note.top() >= band.bottom() - slack,
                    "{at}: the note {note:?} lies below the line's band {band:?}"
                );
                assert!(
                    note.top() >= column.top() - slack
                        && note.bottom() <= column.bottom() + slack
                        && note.left() >= column.left() - slack
                        && note.right() <= column.right() + slack,
                    "{at}: the note {note:?} lies inside the column {column:?}"
                );
            }
            if drawn == 0 {
                let description = COMPACT_LABELS
                    .with(|labels| labels.borrow().get("image-canvas-area").cloned())
                    .unwrap_or_default();
                assert!(
                    description.contains("100% =")
                        && description.contains("An absent or unavailable side"),
                    "{at}: the notes' words stay in the image's description: {description:?}"
                );
            }
            let line_height = cx.update(|window, _| {
                window
                    .text_style()
                    .line_height
                    .to_pixels(appearance::ui_text(11.).into(), window.rem_size())
            });
            if room.size.height >= line_height * 6. {
                assert!(
                    line.size.height >= line_height * 2. - slack,
                    "{at}: the line {line:?} shows at least two of its lines"
                );
            }
        }
    }

    /// A changed image's notes beneath it, the scale notice and the
    /// interaction guidance, wrap to a second line where the column leaves the
    /// image its usable minimum beneath them: at 13 pt in a 640 × 680 window
    /// each read one line ending in an ellipsis above an image 270 px tall.
    /// There each takes two lines, inside the column and below the image,
    /// which keeps its scaled 96 px, and both sides' names keep their shared
    /// top. In a 461 × 490 window, at 13 and 18 pt, and in a 640 × 490 one,
    /// where one line each would fit but leave the image less than that, the
    /// notes leave and the image takes the whole room beneath the captions,
    /// the notes' words kept in its accessible description.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn image_notes_take_two_lines_where_the_image_keeps_its_room(cx: &mut TestAppContext) {
        use crate::system_preview::tests::png;
        let (before, after) = (png(Some(b"before")), png(Some(b"after")));
        let input = file_commits(&[
            ("add", &[("image.png", &before)]),
            ("change", &[("image.png", &after)]),
        ]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let bounds = |selector: &'static str, cx: &mut VisualTestContext| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let notes = ["image-source-scale", "image-interaction-guidance"];
        let slack = px(0.5);

        // One line each where the whole note fits, in a full window.
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let one_line = notes.map(|note| bounds(note, cx).size.height);

        cx.simulate_resize(size(px(640.), px(680.)));
        draw(cx);
        draw(cx);
        let at = "13 pt in 640 × 680";
        let column = bounds("preview-content", cx);
        let image = bounds("image-canvas-area", cx);
        let line = cx.update(|window, _| {
            window
                .text_style()
                .line_height
                .to_pixels(appearance::ui_text(10.).into(), window.rem_size())
        });
        for (note, one_line) in notes.into_iter().zip(one_line) {
            let drawn = bounds(note, cx);
            assert!(
                (drawn.size.height - (one_line + line)).abs() <= slack,
                "{at}: {note} {drawn:?} takes two lines, one line {one_line:?} and another {line:?}"
            );
            assert!(
                drawn.top() >= image.bottom() - slack
                    && drawn.bottom() <= column.bottom() + slack
                    && drawn.left() >= column.left() - slack
                    && drawn.right() <= column.right() + slack,
                "{at}: {note} {drawn:?} lies below the image {image:?} inside the column {column:?}"
            );
        }
        assert!(
            image.size.height >= appearance::ui_size(96.) - slack,
            "{at}: the image {image:?} keeps its usable minimum"
        );
        let names = ["image-side-name-0", "image-side-name-1"].map(|name| bounds(name, cx));
        assert!(
            (names[0].top() - names[1].top()).abs() <= slack,
            "{at}: the side names share their top: {names:?}"
        );

        for (text, width) in [(13, 461.), (18, 461.), (13, 640.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(490.)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × 490");
            for note in notes {
                assert!(cx.debug_bounds(note).is_none(), "{at}: {note} has left");
            }
            let (room, image) = (
                bounds("image-stage-room", cx),
                bounds("image-canvas-area", cx),
            );
            assert!(
                (room.size.height - image.size.height).abs() <= slack,
                "{at}: the image {image:?} takes the room {room:?} beneath the captions"
            );
            let description = COMPACT_LABELS
                .with(|labels| labels.borrow().get("image-canvas-area").cloned())
                .unwrap_or_default();
            assert!(
                description.contains("100% =") && description.contains("Linked zoom and pan"),
                "{at}: the notes' words stay in the image's description: {description:?}"
            );
        }
    }

    /// A changed image's captions give way before the image beneath them, as
    /// labels give way before content: each side's dimensions stay beneath
    /// its name only where the image keeps its usable minimum, a scaled
    /// 96 px, below the captions, and otherwise leave, their words staying in
    /// the name's accessible name and tooltip. In a 461 × 490 window, before
    /// any click, the image had 83 px at 13 pt and 60 at 18 pt beneath a
    /// line of dimensions (two at 18 pt). The test fonts' glyphs are wider
    /// than the interface font's, so in a 461 px window the toolbar's menus
    /// take two rows and Before's control wraps below its name, and that room
    /// falls at 461 × 560 at 13 pt and 461 × 590 at 18 pt: there the image now
    /// keeps at least 96 px. At every height of a 461 px window from 490 to
    /// 980, the dimensions are drawn only above an image that keeps it, and
    /// neither side's name moves when they leave; a full window keeps them.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn image_dimensions_give_way_before_the_image(cx: &mut TestAppContext) {
        use crate::system_preview::tests::png;
        let (before, after) = (png(Some(b"before")), png(Some(b"after")));
        let input = file_commits(&[
            ("add", &[("image.png", &before)]),
            ("change", &[("image.png", &after)]),
        ]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let bounds = |selector: &'static str, cx: &mut VisualTestContext| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let slack = px(0.5);
        let details = ["image-side-details-0", "image-side-details-1"];
        let names = ["image-side-name-0", "image-side-name-1"];
        let mut gave_way = [false; 2];
        for (index, text) in [13, 18].into_iter().enumerate() {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            let minimum = cx.update(|_, _| appearance::ui_size(96.));
            for height in (490..=980).step_by(10) {
                cx.simulate_resize(size(px(461.), px(height as f32)));
                draw(cx);
                let at = format!("{text} pt in 461 × {height}");
                let image = bounds("image-canvas-area", cx);
                let drawn = details.map(|selector| cx.debug_bounds(selector).is_some());
                assert_eq!(drawn[0], drawn[1], "{at}: both sides keep or leave them");
                if drawn[0] {
                    assert!(
                        image.size.height >= minimum - slack,
                        "{at}: the dimensions stay only above an image {image:?} that keeps its minimum"
                    );
                } else {
                    gave_way[index] = true;
                }
            }
            // Where the test fonts reproduce the 461 × 490 window's room, the
            // image keeps at least 96 px (the scaled minimum at 13 pt), and
            // the names stay where they were with the dimensions beneath them.
            let tall = if text == 13 { 560. } else { 590. };
            cx.simulate_resize(size(px(461.), px(980.)));
            draw(cx);
            let kept =
                names.map(|name| bounds(name, cx).top() - bounds("preview-content", cx).top());
            cx.simulate_resize(size(px(461.), px(tall)));
            draw(cx);
            let at = format!("{text} pt in 461 × {tall}");
            let image = bounds("image-canvas-area", cx);
            assert!(
                image.size.height >= px(96.) - slack,
                "{at}: the image {image:?} keeps at least 96 px"
            );
            for detail in details {
                assert!(cx.debug_bounds(detail).is_none(), "{at}: {detail} gave way");
            }
            assert_eq!(
                names.map(|name| bounds(name, cx).top() - bounds("preview-content", cx).top()),
                kept,
                "{at}: neither name moves"
            );
        }
        assert_eq!(
            gave_way, [true; 2],
            "the narrow window's dimensions give way"
        );
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(1480.), px(980.)));
            draw(cx);
            for detail in details {
                assert!(
                    cx.debug_bounds(detail).is_some(),
                    "{text} pt in 1480 × 980: {detail} stays"
                );
            }
        }
    }

    /// A document's two side headers keep one height, their first line as
    /// tall as the scaled 28 px open control, so Before and After align with
    /// or without it: an added PDF's After header, which offers the control,
    /// stood 8.5 px taller than Before's. In a 461 × 490 window After's
    /// control wraps beneath "After PDF" while Before's "Absent" wraps
    /// beneath its name, a line of text shorter than the control's, and
    /// Before's header ended 10 px above After's at 13 pt (14.5 at 18 pt).
    /// An added and a changed PDF, at the default size and at 18 pt, in a
    /// full and a half-tile window.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn side_headers_align_with_or_without_the_open_control(cx: &mut TestAppContext) {
        let pdf = |text: &str| format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\n%%EOF\n");
        let (first, changed, added) = (pdf("first"), pdf("changed"), pdf("added"));
        let input = file_commits(&[
            ("first", &[("changed.pdf", first.as_bytes())]),
            (
                "second",
                &[
                    ("added.pdf", added.as_bytes()),
                    ("changed.pdf", changed.as_bytes()),
                ],
            ),
        ]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        for (text, width, height) in [
            (13, 1480., 980.),
            (13, 461., 490.),
            (18, 1480., 980.),
            (18, 461., 490.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            for (path, offers) in [("added.pdf", [false, true]), ("changed.pdf", [true, true])] {
                let index = app.read_with(cx, |app, _| {
                    app.files
                        .iter()
                        .position(|file| file.path() == std::path::Path::new(path))
                        .unwrap()
                });
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.select_file(index, window, cx))
                });
                settle(&app, cx).await;
                let at = format!("{path} at {text} pt in {width} × {height}");
                assert_eq!(
                    [
                        cx.debug_bounds("system-preview-0").is_some(),
                        cx.debug_bounds("system-preview-1").is_some(),
                    ],
                    offers,
                    "{at}: the sides that offer the control"
                );
                let before = cx.debug_bounds("rich-side-header-0").unwrap();
                let after = cx.debug_bounds("rich-side-header-1").unwrap();
                assert!(
                    (before.top() - after.top()).abs() <= px(0.5)
                        && (before.size.height - after.size.height).abs() <= px(0.5),
                    "{at}: Before's header {before:?} aligns with After's {after:?}"
                );
                // The shorter header's lines keep to its top.
                let headings = ["rich-side-heading-0", "rich-side-heading-1"]
                    .map(|heading| cx.debug_bounds(heading).unwrap());
                assert!(
                    (headings[0].top() - headings[1].top()).abs() <= px(0.5),
                    "{at}: the side names share their top: {headings:?}"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Open in default app answers on its own control, as a copy does, and
    /// never in the window-wide notice, which once said "Opened a read-only
    /// copy of the captured bytes in the default app." with Dismiss and kept
    /// saying it eleven files later. Opened, the control reads Opened in its
    /// own bounds for 1.5 s and one polite announcement names no application.
    /// A refusal is a line beneath the header's controls in the warning color
    /// fitted to the page, and a closed chooser a muted line saying nothing
    /// opened, in a dark and a light palette; the line goes once another file
    /// is shown, even though the worker's cache shows the same preview again.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn open_in_default_app_answers_on_its_control(cx: &mut TestAppContext) {
        use crate::{
            copy_feedback::{
                RESTORE_AFTER,
                tests::{announced, wait},
            },
            system_preview::{
                DISMISSED, NotOpened, OPENED,
                tests::{latest_note, queue},
            },
        };
        let pdf = "%PDF-1.4\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n";
        let input = format!(
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 5\nmedia\nM 100644 inline launcher.desktop\ndata {}\n{pdf}\nM 100644 inline notes.txt\ndata 6\nnotes\n\ndone\n",
            pdf.len(),
        );
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        let open = |file: usize, cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(file, window, cx)));
        };
        let click = |cx: &mut VisualTestContext| {
            let control = cx
                .debug_bounds("system-preview-1")
                .expect("After offers it");
            cx.simulate_click(control.center(), Modifiers::default());
            cx.run_until_parked();
            draw(cx);
        };
        let notice = |cx: &mut VisualTestContext| {
            let notice = app.read_with(cx, |app, _| app.operation_notice.clone());
            (
                notice,
                cx.debug_bounds("operation-notice-summary").is_some(),
            )
        };
        let captured = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| match app.content.as_deref() {
                Some(Content::Rich(preview)) => preview.new.captured.clone(),
                _ => None,
            })
        };
        open(0, cx);
        settle(&app, cx).await;
        let rest = cx
            .debug_bounds("system-preview-1")
            .expect("After offers it");

        queue(Ok(()));
        click(cx);
        assert!(cx.debug_bounds("system-preview-1/opened").is_some());
        assert!(cx.debug_bounds("system-preview-1/rest").is_none());
        assert_eq!(cx.debug_bounds("system-preview-1"), Some(rest));
        assert_eq!(announced(cx).map(|(_, name)| name).as_deref(), Some(OPENED));
        assert_eq!(notice(cx), (None, false), "no window-wide notice");
        assert!(cx.debug_bounds("system-preview-1/note").is_none());
        wait(cx, RESTORE_AFTER);
        draw(cx);
        assert!(cx.debug_bounds("system-preview-1/rest").is_some());
        assert_eq!(cx.debug_bounds("system-preview-1"), Some(rest));
        assert_eq!(announced(cx), None);

        let refusal = "The desktop would open this copy as an SVG document (image/svg+xml) rather than as a PDF document, so nothing was opened.";
        for theme in [
            appearance::ThemeChoice::Midnight,
            appearance::ThemeChoice::Porcelain,
        ] {
            cx.update(|window, cx| theme.apply(Some(window), cx));
            draw(cx);
            let (warning, accent, muted) = cx.read(|cx| {
                let colors = palette(cx);
                (appearance::labels(cx).warning, colors.accent, colors.muted)
            });
            assert_ne!(warning, accent, "{theme:?}: unlike a confirmation");

            queue(Err(NotOpened::Failed(refusal.into())));
            click(cx);
            let control = cx.debug_bounds("system-preview-1").unwrap();
            let line = cx
                .debug_bounds("system-preview-1/note")
                .expect("the refusal stays beside its control");
            assert!(
                line.top() >= control.bottom() && line.left() <= control.left(),
                "{theme:?}: {line:?} is a line beneath {control:?}"
            );
            assert_eq!(
                cx.read(latest_note),
                Some((format!("Could not open After. {refusal}"), true))
            );
            assert_eq!(
                appearance::drawn_color("system-preview-1/note"),
                Some(rgb(warning).into()),
                "{theme:?}: the warning color"
            );
            assert!(cx.debug_bounds("system-preview-1/rest").is_some());
            assert_eq!(notice(cx), (None, false), "no window-wide notice");

            queue(Err(NotOpened::Dismissed));
            click(cx);
            assert!(cx.debug_bounds("system-preview-1/note").is_some());
            // The portal answers a closed chooser with no application to
            // offer as it answers one the person closed, so the line names
            // no cause.
            assert_eq!(DISMISSED, "Nothing opened.");
            assert_eq!(cx.read(latest_note), Some((DISMISSED.into(), false)));
            assert_eq!(
                appearance::drawn_color("system-preview-1/note"),
                Some(rgb(muted).into()),
                "{theme:?}: a closed chooser is not a failure"
            );
            assert_eq!(notice(cx), (None, false));
        }

        queue(Err(NotOpened::Failed(refusal.into())));
        click(cx);
        assert!(cx.debug_bounds("system-preview-1/note").is_some());
        let shown = app.read_with(cx, |app, _| app.content.clone());
        open(1, cx);
        settle(&app, cx).await;
        assert!(cx.debug_bounds("system-preview-1/note").is_none());
        open(0, cx);
        settle(&app, cx).await;
        assert!(cx.debug_bounds("system-preview-1").is_some());
        assert!(
            cx.debug_bounds("system-preview-1/note").is_none(),
            "gone once another file was shown"
        );
        // The worker's cache can hand back the very preview, captured bytes
        // and all, as a restored inspection does: still a new showing.
        open(1, cx);
        settle(&app, cx).await;
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.clear_preview();
                app.selected_file = Some(0);
                app.content = shown;
                cx.notify();
            })
        });
        draw(cx);
        assert!(captured(cx).is_some());
        assert!(cx.debug_bounds("system-preview-1").is_some());
        assert!(
            cx.debug_bounds("system-preview-1/note").is_none(),
            "the same preview shown again"
        );
        assert_eq!(notice(cx), (None, false));
    }

    /// [`history_window`] on the repository `input` imports.
    pub(crate) async fn history_window_from(
        cx: &mut TestAppContext,
        input: impl AsRef<[u8]>,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        styled_history_window(cx, input.as_ref(), |root| root).await
    }

    /// Readies a test to build `GitTurtle`: from empty settings stores, then
    /// with the kit and the renderer's image lifetime installed. Every test
    /// that builds the app starts here, before it writes any store of its
    /// own. A `#[gpui::test]` runs every seed on one thread, which keeps one
    /// test settings directory, so without the reset a seed's app read what
    /// the last seed saved: its recent projects, its GitHub reply drafts, the
    /// preference file a later seed expected to create.
    pub(crate) fn init_app(cx: &mut TestAppContext) {
        preferences::fresh_test_settings_directory();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
    }

    /// A seed releases the app it built once it ends, and with it the
    /// window and the preference, operation and reader threads the app
    /// started. The window context `add_window_view` hands out holds the app
    /// until the test context quits, and the test macro's teardown once quit
    /// only the app, which the test platform ignores: every seed's app lived
    /// on, about 15 MB a seed, and the app suite at 20 iterations grew past
    /// 20 GB until the kernel killed it (`vendor/gpui-pre-macros`). Each
    /// seed finds the app of the seed before it gone.
    #[gpui::test(iterations = 2)]
    async fn each_seed_releases_the_app_it_built(cx: &mut TestAppContext) {
        thread_local! {
            /// The app the previous seed on this test's thread built.
            static EARLIER: std::cell::RefCell<Option<std::rc::Weak<gpui::AppCell>>> =
                const { std::cell::RefCell::new(None) };
        }
        let earlier =
            EARLIER.with(|earlier| earlier.replace(Some(std::rc::Rc::downgrade(&cx.app))));
        if let Some(earlier) = earlier {
            assert_eq!(
                earlier.strong_count(),
                0,
                "the previous seed's app is still held"
            );
        }
        let (_fixture, _app, _cx) = history_window(cx).await;
    }

    /// A seed's app quits as a real one does, waiting for what its quit
    /// observers queue on its own threads, even in a test that never let the
    /// scheduler park: here the repository session save and the preferences
    /// barrier answer from the preferences thread while the shutdown waits.
    /// The test macro's teardown quit with parking forbidden, so such a reply
    /// tripped GPUI's check that no other thread wakes a deterministic test,
    /// and on a loaded host any seed could fail with "Detected activity on
    /// thread gitturtle-preferences" (`vendor/gpui-pre-macros`). The thread is
    /// held until the shutdown has polled the app's observers, so the replies
    /// land then every time; the check runs as the seed ends.
    #[gpui::test]
    fn a_seed_quits_while_its_saves_answer_from_their_thread(cx: &mut TestAppContext) {
        let commits = vec![crate::commit_message::tests::commit("a", "Title", "Body.")];
        let (app, cx) = crate::copy_feedback::tests::history(cx, commits);
        let mut held = Some(app.read_with(cx, |app, _| app.preferences_writer.hold()));
        let app = app.downgrade();
        cx.update(|_, cx| {
            // Registered after the app's own, so the shutdown calls this once
            // they have queued their saves, and polls it after theirs.
            cx.on_app_quit(move |cx| {
                let (done, answered) = std::sync::mpsc::channel();
                let marker = app.update(cx, |app, _| {
                    app.preferences_writer.submit(move || {
                        let _ = done.send(());
                        Ok(())
                    })
                });
                let held = held.take();
                async move {
                    drop(held);
                    if marker.is_ok() {
                        answered.recv().expect("the preferences thread answered");
                    }
                }
            })
            .detach();
        });
    }

    /// [`history_window_from`] on the repository the `input` bytes import,
    /// with the window's root styled by `style`, as the width walk marks
    /// every text run (`width_walk`).
    pub(crate) async fn styled_history_window<'a>(
        cx: &'a mut TestAppContext,
        input: &[u8],
        style: impl FnOnce(gpui_kit::component::Root) -> gpui_kit::component::Root + 'static,
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
        init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let repo = fixture_repository(fixture.path().join("narrow"));
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
        import.stdin.take().unwrap().write_all(input).unwrap();
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
            style(Root::new(app, window, cx))
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(680.)));
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app
                .profile
                .as_ref()
                .map(|profile| (profile.name.clone(), profile.email.clone()))),
            Some(("Fixture".to_owned(), "fixture@example.invalid".to_owned())),
            "the app reads the fixture's own identity, never the caller's"
        );
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
            // Blame, or the File actions menu that holds it where the whole
            // path needs the room, is the row's last control.
            let blame = cx
                .debug_bounds("open-blame")
                .or_else(|| cx.debug_bounds("file-actions-menu"))
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
    /// and the toolbar, its copy controls and the commit's Actions, and the
    /// files stay in the window.
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
        for id in ["copy-message", "copy-commit", "commit-recovery-menu"] {
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
    /// the window pins at 18 pt, and its label whole or not at all: in a
    /// 461 × 490 half tile, beside a count of 12 or of 3, in History and
    /// Compare, the label leaves rather than reading "Change…", the icon and
    /// count staying with "Changed files · N" as the count's accessible name,
    /// and as the window narrows the label leaves, then the count, never
    /// clipping either; wider, the label returns whole.
    #[gpui::test]
    async fn narrow_large_text_keeps_the_changed_files_count_whole(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &files_script(3, 12)).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        let (gap, inset, whole) = cx.update(|window, _| {
            let rem = window.rem_size();
            (rem * 0.5, rem * 0.75, files_label_width(window))
        });
        let check = |cx: &mut VisualTestContext, width: f32, state: &str| {
            COMPACT_LABELS.with(|labels| labels.borrow_mut().clear());
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
                    label.size.width >= whole && label.right() + gap <= count.left(),
                    "{state} at {width}: label {label:?} whole ({whole:?}) beside {count:?}"
                );
            }
            // Without its label the count carries the label's words.
            let named =
                COMPACT_LABELS.with(|labels| labels.borrow().get("changed-files-count").cloned());
            let files = cx.read(|cx| app.read(cx).files.len());
            assert_eq!(
                named.map(|name| name.to_string()),
                (count.is_some() && label.is_none()).then(|| format!("Changed files · {files}")),
                "{state} at {width}"
            );
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
                (false, true),
                "Compare, {count} files"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
            settle(&app, cx).await;
            assert_eq!(
                check(cx, 461., "History"),
                (false, true),
                "History, {count} files"
            );
            let mut shown = Vec::new();
            for width in (341..=721).rev().step_by(4) {
                shown.push(check(cx, width as f32, "History"));
            }
            assert!(
                shown
                    .windows(2)
                    .all(|pair| pair[0].0 >= pair[1].0 && pair[0].1 >= pair[1].1),
                "{count} files: the label and count only leave as the window narrows: {shown:?}"
            );
            assert!(
                shown.contains(&(true, true))
                    && shown.contains(&(false, true))
                    && shown.contains(&(false, false)),
                "{count} files: the label leaves before the count: {shown:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// At the default interface size the narrow half tiles keep origin/main's
    /// layout: these are the bounds its fixed minimums gave (the rail, a
    /// 177 px column and a 240 px inspector at 461 px; a 209 px column at
    /// 493 px), measured on origin/main at f695dce. Only the message toolbar
    /// differs: it holds the commit's Actions too, which the labelled Copy
    /// message and Hash (as wide as its copied state, the check icon and
    /// "Copied") leave no room for on their row, so the three keep only their
    /// icons on that one row, Actions at its trailing edge, rather than take a
    /// row of the 80 px message area.
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
                ("copy-message", bounds(inspector + 9.5, 210., 28., 28.)),
                ("copy-commit", bounds(inspector + 40.5, 210., 28., 28.)),
                (
                    "commit-recovery-menu",
                    bounds(inspector + 201.5, 210., 28., 28.),
                ),
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
                    // The 240 px inspector's message toolbar holds the
                    // commit's Actions beside its copy controls as icons.
                    ("copy-message", "Copy full commit message"),
                    ("copy-commit", "Copy full commit hash"),
                    ("commit-recovery-menu", "Selected commit actions"),
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
                    // The whole path does not fit beside the actions' icons,
                    // so Copy path, File history and Blame fold into one.
                    ("file-actions-menu", "File actions"),
                    ("copy-message", "Copy full commit message"),
                    ("copy-commit", "Copy full commit hash"),
                    ("commit-recovery-menu", "Selected commit actions"),
                ]
            )
        );
        // Labelled, the controls name themselves: from 1160 px the review
        // row holds its buttons and "Showing the original Git diff" whole.
        // Only the commit's Actions keeps its icon in the default 320 px
        // inspector, where the unit tests' text system leaves its label and
        // caret 10.5 px short beside the labelled copy controls (IBM Plex
        // Sans fits them, with 14.5 px to spare).
        cx.simulate_resize(size(px(1160.), px(680.)));
        assert_eq!(
            drawn(cx),
            [("commit-recovery-menu", "Selected commit actions")].into()
        );
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

    /// Compare's text, its editor focused from the keyboard, rings its own
    /// edge with the window's 1 px accent ring, whole on every side, in each
    /// text mode: the patch, each of Split's sides, Before and After. The
    /// ring lies over the text, which moves by nothing for it, and goes when
    /// focus leaves the text.
    #[gpui::test]
    async fn compare_text_rings_its_edge_while_focused(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        crate::focus_ring::sweep::prepare(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        let list = app.read_with(cx, |app, _| app.file_focus.clone());
        for (mode, name) in [
            (TextMode::Unified, "Diff"),
            (TextMode::Split, "Split"),
            (TextMode::Before, "Before"),
            (TextMode::After, "After"),
        ] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.text_mode = mode;
                    app.ensure_editor(window, cx);
                    cx.notify();
                })
            });
            settle(&app, cx).await;
            let panes: Vec<(&str, Entity<EditorState>)> = app.read_with(cx, |app, cx| {
                let shown = |editor: &Option<Entity<EditorState>>| editor.clone().unwrap();
                match mode {
                    TextMode::Unified => vec![("preview-content", shown(&app.patch_editor))],
                    TextMode::Before => vec![("preview-content", shown(&app.before_editor))],
                    TextMode::After => vec![("preview-content", shown(&app.after_editor))],
                    _ => {
                        let split = app.split_view.as_ref().unwrap().read(cx);
                        let [before, after] = split.editors().clone();
                        vec![("split-text-0", before), ("split-text-1", after)]
                    }
                }
            });
            for (selector, editor) in &panes {
                let at = format!("{selector} in {name}");
                let focus = editor.read_with(cx, |editor, cx| editor.focus_handle(cx));
                // Focus held by the file list leaves the text unringed.
                cx.update(|window, cx| window.focus(&list, cx));
                cx.simulate_keystrokes("down");
                draw(cx);
                let pane = cx.debug_bounds(selector).unwrap();
                let text = editor.read_with(cx, |editor, _| editor.text_bounds());
                let ring = |cx: &mut VisualTestContext, pane: Bounds<Pixels>| {
                    crate::focus_ring::painted_outlines(cx, accent)
                        .into_iter()
                        .filter(|outline| {
                            [
                                (outline.bounds.left(), pane.left()),
                                (outline.bounds.top(), pane.top()),
                                (outline.bounds.right(), pane.right()),
                                (outline.bounds.bottom(), pane.bottom()),
                            ]
                            .into_iter()
                            .all(|(drawn, edge)| (drawn - edge).abs() <= px(0.5))
                        })
                        .collect::<Vec<_>>()
                };
                assert!(ring(cx, pane).is_empty(), "{at}: ringed while unfocused");
                // A key with the text focused shows its ring.
                cx.update(|window, cx| window.focus(&focus, cx));
                cx.simulate_keystrokes("right");
                draw(cx);
                let [edge] = ring(cx, pane)[..] else {
                    panic!("{at}: one ring on the pane's edge, {pane:?}");
                };
                assert_eq!(edge.width, px(1.), "{at}: {edge:?}");
                assert!(edge.whole, "{at}: a clip cuts the ring: {edge:?}");
                assert_eq!(
                    cx.debug_bounds(selector),
                    Some(pane),
                    "{at}: the pane moved"
                );
                assert_eq!(
                    editor.read_with(cx, |editor, _| editor.text_bounds()),
                    text,
                    "{at}: the text moved"
                );
                // Only the focused side of Split rings its edge.
                for (other, _) in panes.iter().filter(|(other, _)| other != selector) {
                    let other_pane = cx.debug_bounds(other).unwrap();
                    assert!(
                        ring(cx, other_pane).is_empty(),
                        "{other} ringed while {selector} holds focus"
                    );
                }
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

    /// In the full header History, Changes and Pull requests keep one place
    /// in Changes, History and Compare: the return action's slot is as wide
    /// as the labelled Projects in every mode, so the Back arrow outside
    /// History's root leaves the controls after it where they were, History
    /// keeps the slot Projects takes elsewhere, so the name takes the same
    /// share of the row, and a click where Changes was never lands on Pull
    /// requests. At the default size and 18 pt, in 1000 × 680 and 1480 × 800
    /// windows, and at 800 × 718, where the other modes' header wraps.
    #[gpui::test]
    async fn full_header_keeps_its_toggles_in_place_across_pages(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        let controls = |cx: &mut VisualTestContext| {
            ["history-tab", "changes-tab", "pull-requests"].map(|selector| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector}"))
            })
        };
        for (text, width, height) in [
            (13, 1000., 680.),
            (13, 1480., 800.),
            (18, 1000., 680.),
            (18, 1480., 800.),
            (13, 800., 718.),
        ] {
            let at = format!("{text} pt at {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
            settle(&app, cx).await;
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            assert!(
                matches!(predicted_chrome(&app, cx).1, Chrome::Full { .. }),
                "{at}"
            );
            let changes = controls(cx);
            let back = cx.debug_bounds("page-back").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
            settle(&app, cx).await;
            assert_eq!(
                app.read_with(cx, |app, _| app.back_label()),
                "Back to Projects"
            );
            assert_eq!(controls(cx), changes, "History, {at}");
            assert_eq!(
                cx.debug_bounds("page-back").unwrap().left(),
                back.left(),
                "the return action keeps its edge, {at}"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
            settle(&app, cx).await;
            assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
            assert_eq!(controls(cx), changes, "Compare, {at}");
            // A click where Changes was lands on Changes.
            cx.simulate_click(changes[1].center(), Modifiers::default());
            settle(&app, cx).await;
            assert!(
                app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working),
                "{at}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Where the full header's trailing controls wrap onto a row of their
    /// own, that row starts at the header's left edge in Changes, History
    /// and Compare: the profile leads it below the return action, with no
    /// empty slot before it at History's root, the profile and Settings keep
    /// one place in every mode, and Projects follows Settings where it
    /// shows. Both such rows are checked, at the default size and 18 pt: one
    /// where the spacer stays on the row above, and one where it would have
    /// started the row and pushed it to the trailing edge. Where they share
    /// a row, at 1000 px, Projects still comes before the profile.
    #[gpui::test]
    async fn full_header_starts_its_trailing_row_at_the_left_edge(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        let trailing = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let app = app.read(cx);
                let narrow =
                    window.viewport_size().width < appearance::ui_size(COMPACT_HEADER_WIDTH);
                app.trailing(narrow, &app.chrome_widths(window), window)
            })
        };
        let controls = |cx: &mut VisualTestContext| {
            (
                cx.debug_bounds("page-back").expect("the return action"),
                cx.debug_bounds("profile-button").expect("the profile"),
                cx.debug_bounds("settings").expect("Settings"),
                cx.debug_bounds("projects"),
            )
        };
        let mut checked = Vec::new();
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            // The narrowest windows, 2 px apart, whose header takes each own
            // row, as the prediction decides it before layout.
            let mut sizes = vec![(1000., Trailing::Shared)];
            for spacer in [true, false] {
                let width = (461..1000)
                    .step_by(2)
                    .map(|width| width as f32)
                    .find(|&width| {
                        cx.update(|window, cx| {
                            let app = app.read(cx);
                            let narrow = px(width) < appearance::ui_size(COMPACT_HEADER_WIDTH);
                            let widths = app.chrome_widths(window);
                            app.full_header_items(px(width), narrow, true, &widths, window)
                                .1
                                == Trailing::OwnRow { spacer }
                        })
                    });
                sizes.extend(width.map(|width| (width, Trailing::OwnRow { spacer })));
            }
            for (width, expected) in sizes {
                let at = format!("{text} pt at {width} px, {expected:?}");
                cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
                settle(&app, cx).await;
                // Tall enough for the full chrome at either size.
                cx.simulate_resize(size(px(width), px(1400.)));
                draw(cx);
                assert!(
                    matches!(predicted_chrome(&app, cx).1, Chrome::Full { .. }),
                    "{at}"
                );
                assert_eq!(trailing(cx), expected, "{at}");
                checked.push(expected);
                let mut placed = None;
                for mode in ["Changes", "History", "Compare"] {
                    match mode {
                        "History" => cx.update(|window, cx| {
                            app.update(cx, |app, cx| app.show_history(window, cx))
                        }),
                        "Compare" => cx.update(|window, cx| {
                            app.update(cx, |app, cx| app.select_file(0, window, cx))
                        }),
                        _ => {}
                    }
                    settle(&app, cx).await;
                    let (back, profile, settings, projects) = controls(cx);
                    assert_eq!(
                        projects.is_some(),
                        mode != "History",
                        "{mode}, {at}: Projects outside History's root"
                    );
                    if expected == Trailing::Shared {
                        assert!(
                            profile.top() < back.bottom()
                                && projects
                                    .is_none_or(|projects| projects.right() <= profile.left()),
                            "{mode}, {at}: one row, Projects {projects:?} before {profile:?}"
                        );
                        continue;
                    }
                    assert!(
                        profile.top() >= back.bottom(),
                        "{mode}, {at}: the profile {profile:?} below {back:?}"
                    );
                    assert!(
                        (profile.left() - back.left()).abs() < px(0.5),
                        "{mode}, {at}: the row starts at the left edge, {profile:?} under {back:?}"
                    );
                    if let Some(projects) = projects {
                        assert!(
                            projects.left() >= settings.right()
                                && (projects.top() - settings.top()).abs() < px(0.5),
                            "{mode}, {at}: Projects {projects:?} after Settings {settings:?}"
                        );
                    }
                    match placed {
                        None => placed = Some((profile, settings)),
                        Some(placed) => assert_eq!(
                            (profile, settings),
                            placed,
                            "{mode}, {at}: the profile and Settings keep their place"
                        ),
                    }
                }
            }
        }
        for spacer in [true, false] {
            assert!(
                checked.contains(&Trailing::OwnRow { spacer }),
                "an own row with the spacer {}: {checked:?}",
                if spacer { "above" } else { "left out" }
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Outside History's root the full header's return action, Back, fills
    /// the slot the labelled Projects takes at History's root: its bounds,
    /// and with them its hit area, hover and pressed fill and focus ring,
    /// are that slot's, so no empty room before the turtle reads as a
    /// missing control, and the name and the toggles keep their place.
    /// Hovered at the slot's trailing end it shows its hover fill, and a
    /// click there goes back. In Changes and Compare, in 1000 × 680 and
    /// 1480 × 800 windows, at the default size and 18 pt, where the 1000 px
    /// window takes the narrow form and its 28 px icon slot.
    #[gpui::test]
    async fn full_header_back_fills_its_slot(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let hover = cx.update(|_, cx| {
            let p = palette(cx);
            gpui_kit::Background::from(gpui_kit::Hsla::from(p.control_fill(p.hover)))
        });
        let placed = |cx: &mut VisualTestContext| {
            [
                "page-back",
                "repository-heading",
                "history-tab",
                "changes-tab",
            ]
            .map(|selector| cx.debug_bounds(selector).expect(selector))
        };
        for (text, width, height) in [
            (13, 1000., 680.),
            (13, 1480., 800.),
            (18, 1000., 680.),
            (18, 1480., 800.),
        ] {
            let at = format!("{text} pt at {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
            settle(&app, cx).await;
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            // At 18 pt the 1000 px window takes the narrow form, whose slot
            // is the 28 px icon control in every mode.
            assert_eq!(
                predicted_chrome(&app, cx).1,
                Chrome::Full {
                    narrow: text == 18 && width < 1100.
                },
                "{at}"
            );
            let root = placed(cx);
            for (name, mode) in [
                ("Changes", WorkspaceMode::Working),
                ("Compare", WorkspaceMode::Compare),
            ] {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| match mode {
                        WorkspaceMode::Working => app.show_working(window, cx),
                        _ => app.select_file(0, window, cx),
                    })
                });
                settle(&app, cx).await;
                assert!(app.read_with(cx, |app, _| app.mode == mode), "{name}, {at}");
                assert_eq!(
                    placed(cx),
                    root,
                    "{name}, {at}: Back fills the slot, and the name and toggles stay"
                );
                let back = root[0];
                // The labelled Back's arrow and label keep the leading edge
                // Projects' folder and label take: the slot's spare room
                // trails them, up to the Button's inset.
                let room = cx.debug_bounds("page-back-room");
                if back.size.width > appearance::ui_size(28.) + px(0.5) {
                    let room = room.unwrap_or_else(|| panic!("{name}, {at}: no trailing room"));
                    assert!(
                        room.size.width > px(0.)
                            && (room.right() - (back.right() - appearance::ui_size(10.))).abs()
                                < px(0.5),
                        "{name}, {at}: the room {room:?} trails Back's content in {back:?}"
                    );
                } else {
                    assert_eq!(room, None, "{name}, {at}: the narrow form's icon");
                }
                // The slot's trailing end, past the arrow and its label.
                let end = point(back.right() - px(4.), back.center().y);
                cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
                draw(cx);
                assert_eq!(
                    painted_fills(cx, "page-back"),
                    vec![],
                    "{name}, {at}: at rest"
                );
                cx.simulate_mouse_move(end, None, Modifiers::default());
                draw(cx);
                assert_eq!(
                    painted_fills(cx, "page-back"),
                    vec![hover],
                    "{name}, {at}: hovered at the slot's end"
                );
                cx.simulate_click(end, Modifiers::default());
                settle(&app, cx).await;
                assert!(
                    app.read_with(cx, |app, _| app.mode == WorkspaceMode::History),
                    "{name}, {at}: a click at the slot's end goes back"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// What [`changes_badge_ring`] reads from the compact Changes count.
    #[derive(Debug)]
    struct BadgeRingProbe {
        /// The fills of the quads at the count's outer bounds: its cut-out.
        cutouts: Vec<gpui_kit::Background>,
        /// How many quads at those bounds draw a border, whose fill GPUI
        /// paints out to the outer edge beneath it.
        borders: usize,
        /// Each quad filled with the accent that meets the count's bounds, as
        /// its inset from them on the left, top, right and bottom.
        pills: Vec<[f32; 4]>,
        /// Whether every such quad is painted after the cut-out, inside it.
        pills_above: bool,
        /// The colour the quads painted before the count composite to just
        /// inside the cut-out's band at its left, mid-height: what the
        /// cut-out covers.
        beneath: u32,
    }

    /// The compact Changes count as the last frame painted it, with its pill
    /// in the palette's `accent`.
    fn changes_badge_ring(cx: &mut VisualTestContext, accent: u32) -> BadgeRingProbe {
        let badge = cx.debug_bounds("changes-count-badge").expect("the count");
        let ring = appearance::ui_size(CHANGES_BADGE_RING);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = |logical: Pixels| logical.as_f32() * scale;
            let near = |device: gpui_kit::ScaledPixels, logical: Pixels| {
                (device.as_f32() - logical.as_f32() * scale).abs() < 0.5
            };
            let quads = window.painted_quads();
            let at_badge = |quad: &&gpui_kit::Quad| {
                near(quad.bounds.left(), badge.left())
                    && near(quad.bounds.top(), badge.top())
                    && near(quad.bounds.right(), badge.right())
                    && near(quad.bounds.bottom(), badge.bottom())
            };
            let cutouts = quads
                .iter()
                .filter(at_badge)
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| quad.background)
                .collect::<Vec<_>>();
            let borders = quads
                .iter()
                .filter(at_badge)
                .filter(|quad| {
                    let widths = quad.border_widths;
                    quad.border_color.a > 0.
                        && [widths.left, widths.top, widths.right, widths.bottom]
                            .into_iter()
                            .any(|width| width.as_f32() > 0.)
                })
                .count();
            let accent = gpui_kit::Background::from(rgb(accent));
            let meets = |quad: &gpui_kit::Quad| {
                quad.background == accent
                    && quad.bounds.left().as_f32() < device(badge.right())
                    && quad.bounds.right().as_f32() > device(badge.left())
                    && quad.bounds.top().as_f32() < device(badge.bottom())
                    && quad.bounds.bottom().as_f32() > device(badge.top())
            };
            let pills = quads
                .iter()
                .filter(|quad| meets(quad))
                .map(|quad| {
                    [
                        quad.bounds.left().as_f32() - device(badge.left()),
                        quad.bounds.top().as_f32() - device(badge.top()),
                        device(badge.right()) - quad.bounds.right().as_f32(),
                        device(badge.bottom()) - quad.bounds.bottom().as_f32(),
                    ]
                    .map(|inset| inset / scale)
                })
                .collect::<Vec<_>>();
            let first = quads
                .iter()
                .position(|quad| at_badge(&quad))
                .expect("the count's quads");
            let pills_above = quads[..=first].iter().all(|quad| !meets(quad));
            let (x, y) = (
                (badge.left() + ring * 0.5).as_f32() * scale,
                badge.center().y.as_f32() * scale,
            );
            let contains = |area: gpui_kit::Bounds<gpui_kit::ScaledPixels>| {
                x >= area.left().as_f32()
                    && x < area.right().as_f32()
                    && y >= area.top().as_f32()
                    && y < area.bottom().as_f32()
            };
            let beneath = quads[..first]
                .iter()
                .filter(|quad| contains(quad.bounds) && contains(quad.content_mask.bounds))
                .fold(0, |surface, quad| {
                    let (area, widths) = (quad.bounds, quad.border_widths);
                    let in_border = x < area.left().as_f32() + widths.left.as_f32()
                        || x >= area.right().as_f32() - widths.right.as_f32()
                        || y < area.top().as_f32() + widths.top.as_f32()
                        || y >= area.bottom().as_f32() - widths.bottom.as_f32();
                    let layer = if in_border && quad.border_color.a > 0. {
                        quad.border_color
                    } else if let Some(fill) = quad.background.as_solid() {
                        fill
                    } else {
                        return surface;
                    };
                    appearance::composite(layer.to_rgb(), surface)
                });
            BadgeRingProbe {
                cutouts,
                borders,
                pills,
                pills_above,
                beneath,
            }
        })
    }

    /// Checks the compact Changes count as the last frame painted it: inside
    /// its toggle, a cut-out of the colour beneath it, to a channel step for
    /// the blend, is the outermost shape at its bounds, with no border whose
    /// fill would reach past it, and the accent pill is painted after it,
    /// its 14 px whole and inset from the cut-out's edge by the ring's 1–2
    /// px on every side, so nothing of the pill's antialiased edge shows
    /// past the cut-out. Returns the cut-out's colour.
    fn assert_badge_cut_out(cx: &mut VisualTestContext, at: &str, p: appearance::Palette) -> u32 {
        let same = |a: u32, b: u32| {
            [16, 8, 0]
                .into_iter()
                .all(|shift| ((a >> shift) & 0xff).abs_diff((b >> shift) & 0xff) <= 1)
        };
        let badge = cx.debug_bounds("changes-count-badge").expect("the count");
        let toggle = cx.debug_bounds("changes-tab").unwrap();
        assert!(
            badge.left() >= toggle.left()
                && badge.right() <= toggle.right()
                && badge.top() >= toggle.top()
                && badge.bottom() <= toggle.bottom(),
            "{at}: badge {badge:?} inside {toggle:?}"
        );
        let probe = changes_badge_ring(cx, p.accent);
        let [cutout] = probe.cutouts.as_slice() else {
            panic!("{at}: one cut-out at the count's bounds: {probe:?}");
        };
        let color = appearance::packed(cutout.as_solid().expect("a solid cut-out"));
        assert!(
            color != p.accent && same(color, probe.beneath),
            "{at}: the outermost shape {color:06x} is the fill beneath it, {:06x}, not the \
             accent pill: {probe:?}",
            probe.beneath
        );
        assert_eq!(probe.borders, 0, "{at}: no bordered quad: {probe:?}");
        let [insets] = probe.pills.as_slice() else {
            panic!("{at}: one accent pill: {probe:?}");
        };
        assert!(
            probe.pills_above && insets.iter().all(|inset| (1. ..=2.).contains(inset)),
            "{at}: the pill lies inside the cut-out, 1–2 px from its edge all round: {probe:?}"
        );
        let ring = appearance::ui_size(CHANGES_BADGE_RING);
        assert!(
            badge.size.height - ring * 2. >= appearance::ui_size(14.) - px(0.5),
            "{at}: the pill keeps its 14 px, to layout's half-pixel snapping, inside the \
             cut-out: {badge:?}"
        );
        color
    }

    /// The compact Changes count is cut out of the glyph it overlaps: its
    /// accent pill keeps its 14 px inside a cut-out that stays inside the
    /// toggle, filled with the colour the count sits on, so it reads as a cut
    /// in that fill rather than a halo around the pill
    /// ([`assert_badge_cut_out`]). In every palette with the toggle selected
    /// in Changes, and in Midnight and Daylight also selected and hovered,
    /// and in History on the toggle group's fill at rest, hovered and
    /// pressed.
    #[gpui::test]
    async fn compact_changes_badge_is_ringed_in_the_fill_beneath(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        let check = assert_badge_cut_out;

        let away = |cx: &mut VisualTestContext| {
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            draw(cx);
        };
        // On the toggle, clear of the count at its trailing corner.
        let on_toggle = |cx: &mut VisualTestContext| {
            let toggle = cx.debug_bounds("changes-tab").unwrap();
            point(toggle.left() + px(3.), toggle.center().y)
        };
        for (name, palette) in appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            away(cx);
            let ring = check(cx, &format!("{name}, selected"), palette);
            assert_eq!(ring, palette.selected, "{name}: the selected toggle's fill");
        }
        for theme in [
            crate::appearance::ThemeChoice::Midnight,
            crate::appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|_, cx| theme.apply(None, cx));
            let p = cx.update(|_, cx| palette(cx));
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
            settle(&app, cx).await;
            away(cx);
            assert_eq!(check(cx, &format!("{theme:?}, selected"), p), p.selected);
            let point = on_toggle(cx);
            cx.simulate_mouse_move(point, None, Modifiers::default());
            draw(cx);
            assert_eq!(
                check(cx, &format!("{theme:?}, selected and hovered"), p),
                p.row_hover(true)
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
            settle(&app, cx).await;
            away(cx);
            let rest = check(cx, &format!("{theme:?}, at rest"), p);
            assert_eq!(rest, p.subtle, "{theme:?}: the toggle group's fill");
            cx.simulate_mouse_move(point, None, Modifiers::default());
            draw(cx);
            let hovered = check(cx, &format!("{theme:?}, hovered"), p);
            assert_ne!(
                hovered, rest,
                "{theme:?}: the hover lifts the ring with the toggle"
            );
            cx.simulate_mouse_down(point, gpui_kit::MouseButton::Left, Modifiers::default());
            draw(cx);
            let pressed = check(cx, &format!("{theme:?}, pressed"), p);
            assert_ne!(
                pressed, hovered,
                "{theme:?}: the press moves the ring with the toggle"
            );
            cx.simulate_mouse_up(point, gpui_kit::MouseButton::Left, Modifiers::default());
            settle(&app, cx).await;
            assert!(
                app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working),
                "{theme:?}: the press selects Changes"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Nothing of the compact Changes count's accent pill shows past its
    /// cut-out, where a half-pixel fringe of it once did (1.23:1 against the
    /// toggle group's fill in Braden, 1.43:1 in Midnight): in Midnight and
    /// Braden, in Changes, where the toggle is selected, and in History, on
    /// the group's fill, the cut-out is the outermost shape
    /// ([`assert_badge_cut_out`]) wherever the chrome is compact, as at 18
    /// pt in a 461 × 490 window. The toggle keeps its name with the count,
    /// "Changes · 3". Where the full header stays, as at the default size in
    /// 461 × 490 and 640 × 680 windows, its Changes tab reads the count and
    /// no badge is drawn.
    #[gpui::test]
    async fn compact_changes_badge_shows_nothing_past_its_cut_out(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        for theme in [
            crate::appearance::ThemeChoice::Midnight,
            crate::appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|_, cx| theme.apply(None, cx));
            let p = cx.update(|_, cx| palette(cx));
            for (text, width, height) in [
                (13, 461., 490.),
                (13, 640., 680.),
                (18, 461., 490.),
                (18, 640., 680.),
            ] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                for history in [false, true] {
                    cx.update(|window, cx| {
                        app.update(cx, |app, cx| {
                            if history {
                                app.show_history(window, cx)
                            } else {
                                app.show_working(window, cx)
                            }
                        })
                    });
                    settle(&app, cx).await;
                    cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
                    COMPACT_LABELS.with(|names| names.borrow_mut().clear());
                    draw(cx);
                    let at = format!(
                        "{theme:?} in {} at {text} pt in {width} × {height}",
                        if history { "History" } else { "Changes" }
                    );
                    let compact = cx.update(|window, cx| app.read(cx).chrome(window, cx));
                    assert!(
                        compact.is_compact() || (text, width) != (18, 461.),
                        "{at}: the compact chrome"
                    );
                    if !compact.is_compact() {
                        assert!(
                            cx.debug_bounds("changes-count-badge").is_none(),
                            "{at}: no badge beside the full tab"
                        );
                        continue;
                    }
                    let cutout = assert_badge_cut_out(cx, &at, p);
                    let fill = if history { p.subtle } else { p.selected };
                    assert_eq!(cutout, fill, "{at}: the fill the count sits on");
                    assert_eq!(
                        COMPACT_LABELS.with(|names| names.borrow().get("changes-tab").cloned()),
                        Some("Changes · 3".into()),
                        "{at}: the toggle's name"
                    );
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The Working Changes header's count leaves with its "Changes" label:
    /// alone it would repeat the count the Changes tab reads above it. At
    /// 18 pt in a 461 × 1000 window the inspector has room for neither, and
    /// the header holds only Actions and Refresh, where it showed a lone
    /// "3"; at 560 × 1000 and at the default size the count follows its
    /// label.
    #[gpui::test]
    async fn working_header_count_leaves_with_its_label(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        for (text, width, height) in [(18, 461., 1000.), (18, 560., 1000.), (13, 1000., 680.)] {
            let at = format!("{text} pt at {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            assert!(cx.debug_bounds("working-header").is_some(), "{at}");
            let label = cx.debug_bounds("working-header-label");
            let count = cx.debug_bounds("working-header-count");
            assert_eq!(
                label.is_some(),
                count.is_some(),
                "{at}: {label:?}, {count:?}"
            );
            if let (Some(label), Some(count)) = (label, count) {
                assert!(
                    count.left() >= label.right(),
                    "{at}: the count follows its label"
                );
            }
            if width == 461. {
                assert!(count.is_none(), "{at}: no lone count");
                assert!(cx.debug_bounds("changes-tab").is_some());
                assert_eq!(
                    app.read_with(cx, |app, _| app.changes_label()),
                    "Changes · 3"
                );
            } else {
                assert!(count.is_some(), "{at}: the count beside its label");
            }
        }
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

    /// Blame keeps its own artwork beside the profile's person: at 18 pt in a
    /// 461 × 490 window Compare shows Blame as an icon and the compact
    /// header the profile as one, and the two draw different icons, which
    /// the app's assets hold as different artwork.
    #[gpui::test]
    async fn blame_and_the_profile_draw_different_icons(cx: &mut TestAppContext) {
        let script = history_script(2, 40).replace("notes.txt", "note.txt");
        let (_fixture, app, cx) = history_window_from(cx, &script).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        CONTROL_ICONS.with(|icons| icons.borrow_mut().clear());
        draw(cx);
        let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
        for selector in ["open-blame", "profile-button"] {
            let bounds = cx.debug_bounds(selector).expect(selector);
            assert_eq!(bounds.size.width, control, "{selector} is an icon");
        }
        let icons = CONTROL_ICONS.with(|icons| icons.borrow().clone());
        let (blame, profile) = (
            icons.get("open-blame").expect("Blame's icon").clone(),
            icons.get("profile").expect("the profile's icon").clone(),
        );
        assert_eq!(
            profile.as_ref(),
            "icons/user.svg",
            "the profile keeps the person"
        );
        assert_ne!(blame, profile, "Blame and the profile share an icon");
        let artwork = |path: &SharedString| {
            crate::Assets
                .load(path)
                .unwrap()
                .unwrap_or_else(|| panic!("{path} is an app asset"))
                .into_owned()
        };
        assert_ne!(
            artwork(&blame),
            artwork(&profile),
            "{blame} repeats {profile}"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The text modes' menu, on a row of its own below the path, starts its
    /// label at the path's left edge: at 18 pt in a 461 × 490 window "Diff"
    /// stood 14 px right of the path, the transparent button's padding. The
    /// menu gives that inset back to the toolbar's padding, and its focus
    /// ring keeps its room inside the column.
    #[gpui::test]
    async fn narrow_mode_menu_label_starts_with_the_path(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        COMPACT_LABELS.with(|names| names.borrow_mut().clear());
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert_eq!(
            COMPACT_LABELS.with(|names| names.borrow().get("text-mode-menu").cloned()),
            Some("Comparison mode: Diff".into())
        );
        let menu = cx.debug_bounds("text-mode-menu").expect("the modes' menu");
        let path = cx.debug_bounds("preview-file-path").unwrap();
        let toolbar = cx.debug_bounds("compare-toolbar").unwrap();
        assert!(
            menu.top() >= path.bottom(),
            "a row of its own: {menu:?} below {path:?}"
        );
        let label = menu.left() + appearance::ui_size(TEXT_MODE_MENU_INSET);
        assert!(
            (label - path.left()).abs() < px(0.5),
            "Diff starts at {label:?}, the path at {:?}",
            path.left()
        );
        let room = cx.read(appearance::button_ring_room);
        assert!(
            menu.left() - room >= toolbar.left(),
            "the ring's room {room:?} left of {menu:?} inside {toolbar:?}"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Compare's path keeps its room before its actions keep theirs: in the
    /// 177 px column of a 461 × 490 window "notes.txt" does not fit beside
    /// Copy path, File history and Blame as icons, where it read "notes.t…",
    /// so the three fold into one 28 px File actions control showing the
    /// more-actions dots and the path is whole. Its menu lists them; Copy
    /// file path copies the path and confirms on that control, the check
    /// taking the dots' place, and Blame opens the file's attribution. In a
    /// 1,000 px window the actions stand beside the path, Blame labelled.
    #[gpui::test]
    async fn compare_folds_its_actions_so_the_path_stays_whole(cx: &mut TestAppContext) {
        const COPY: &str = "menu-item: Copy file path · MenuItem";
        const BLAME: &str = "menu-item: Blame · MenuItem";
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let icon = |selector: &str| {
            CONTROL_ICONS.with(|icons| icons.borrow().get(selector).map(ToString::to_string))
        };
        cx.simulate_resize(size(px(461.), px(490.)));
        CONTROL_ICONS.with(|icons| icons.borrow_mut().clear());
        COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        draw(cx);
        let whole = cx.update(|window, _| {
            shaped_width(
                "notes.txt",
                appearance::ui_text(12.),
                FontWeight::MEDIUM,
                window,
            )
        });
        let path = cx.debug_bounds("preview-file-path").unwrap();
        assert!(path.size.width >= whole, "path {path:?}, whole {whole:?}");
        assert!(cx.debug_bounds("copy-path").is_none(), "Copy path folds");
        assert!(cx.debug_bounds("open-blame").is_none(), "Blame folds");
        assert_eq!(icon("file-history"), None, "File history folds");
        let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
        let trigger = cx.debug_bounds("file-actions-menu").expect("File actions");
        assert_eq!(trigger.size, size(control, control), "{trigger:?}");
        assert!(
            trigger.left() >= path.right(),
            "after the path: {trigger:?}"
        );
        assert_eq!(icon("file-actions-menu").as_deref(), Some("icons/more.svg"));
        assert_eq!(
            COMPACT_NAMES.with(|names| names.borrow().get("file-actions-menu").copied()),
            Some("File actions")
        );

        // Its menu copies the path, and the control confirms it.
        cx.simulate_click(trigger.center(), Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        for item in [COPY, "menu-item: File history… · MenuItem", BLAME] {
            assert!(cx.debug_bounds(item).is_some(), "the menu offers {item}");
        }
        let copy = cx.debug_bounds(COPY).unwrap();
        cx.simulate_click(copy.center(), Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        assert_eq!(
            cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text())),
            Some("notes.txt".to_owned())
        );
        assert_eq!(
            icon("file-actions-menu").as_deref(),
            Some("icons/check.svg"),
            "the control confirms the copy"
        );
        // Blame from the menu opens the file's attribution.
        let trigger = cx.debug_bounds("file-actions-menu").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        let blame = cx.debug_bounds(BLAME).expect("Blame in the menu");
        cx.simulate_click(blame.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            app.read_with(cx, |app, _| app.blame.is_visible()),
            "Blame opens from File actions"
        );
        let closed = cx.update(|window, cx| app.update(cx, |app, cx| app.close_blame(window, cx)));
        assert!(closed, "Blame closes");
        settle(&app, cx).await;

        // Wide enough, the actions stand beside the path, Blame labelled.
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        assert!(cx.debug_bounds("file-actions-menu").is_none(), "no menu");
        assert!(cx.debug_bounds("copy-path").is_some());
        let blame = cx.debug_bounds("open-blame").expect("Blame");
        assert!(blame.size.width > control, "labelled: {blame:?}");
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
                // The header starts at the shared inset, or, its icon and
                // label with their count gone, holds only Actions and Refresh.
                if let Some(first_in_header) = cx
                    .debug_bounds("working-header-icon")
                    .or_else(|| cx.debug_bounds("working-header-label"))
                {
                    assert!(
                        (first_in_header.left() - selection.left() - inset).abs() < px(0.5),
                        "{at}: the header starts at {first_in_header:?}"
                    );
                }
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

    /// Labelled, the Changes header's "Actions" with its caret is the shared
    /// 28 px compact control, as tall as Refresh beside it and centred on
    /// the same line in the header, in a 640 × 680, a 1000 × 680 and a 1480 ×
    /// 980 window at the default size and in a 1480 × 980 one at 18 pt: it
    /// was the kit's small Button, 1.5 rem tall (19.5 px at the default
    /// size, which AT-SPI read as 39 physical px), beside Refresh's 28 px.
    #[gpui::test]
    async fn labelled_actions_matches_refresh_beside_it(cx: &mut TestAppContext) {
        let (_fixture, _app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height) in [
            (13, 640., 680.),
            (13, 1000., 680.),
            (13, 1480., 980.),
            (18, 1480., 980.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let header = bounds(cx, "working-header");
            let actions = bounds(cx, "working-recovery-menu");
            let refresh = bounds(cx, "refresh-working");
            // Labelled: wider than the icon-only square, without its dots.
            assert!(
                actions.size.width > control && cx.debug_bounds("working-recovery-more").is_none(),
                "{at}: the labelled form {actions:?}"
            );
            assert_eq!(actions.size.height, control, "{at}: {actions:?}");
            assert_eq!(refresh.size.height, control, "{at}: {refresh:?}");
            let slack = px(0.5);
            assert!(
                (actions.center().y - refresh.center().y).abs() <= slack
                    && (actions.center().y - header.center().y).abs() <= slack,
                "{at}: {actions:?} beside {refresh:?} in {header:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The message toolbar is laid out for the width the inspector is drawn
    /// at, which can be narrower than its saved width: the panel group keeps
    /// the size a drag gave the inspector and rescales it with the window,
    /// as when a tiling window manager narrows a window after it opens,
    /// while the saved width stays what the drag left. Dragged 440 px wide
    /// in a 1,480 px window, the inspector is drawn about 297 px wide in a
    /// 1,000 px one at the default size; dragged 470 px wide at 18 pt, about
    /// 394 px at 1,240 px and its 388 px minimum at 1,000 px. Dragged 480 px
    /// wide in a 1,000 px window, it is drawn within the 380 px the content
    /// column's minimum leaves it at 900 px, ending at the window's edge. In
    /// each, Copy message, Hash and Actions share the toolbar's one row,
    /// Actions at its trailing edge, inside the inspector and the window.
    /// Laid out for the saved width, Actions kept its label and caret, which
    /// the drawn inspector has no room for beside the copy controls, and
    /// wrapped onto a row of its own below them; at 900 px the inspector ran
    /// 52 px past the window, Actions with it.
    #[gpui::test]
    async fn the_message_toolbar_fits_the_inspector_as_drawn(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        // Drags the divider before the inspector so that it is `wide` px
        // wide, as the user does.
        let drag = |cx: &mut VisualTestContext, wide: f32| {
            let inspector = cx.debug_bounds("commit-inspector").unwrap();
            let target = point(inspector.right() - px(wide), inspector.center().y);
            let from = point(inspector.left() - px(2.), inspector.center().y);
            cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
            cx.simulate_mouse_move(
                from - point(px(10.), px(0.)),
                Some(MouseButton::Left),
                Modifiers::default(),
            );
            cx.simulate_mouse_move(target, Some(MouseButton::Left), Modifiers::default());
            cx.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
            draw(cx);
        };
        let saved =
            |cx: &mut VisualTestContext| cx.read(|cx| px(app.read(cx).settings.inspector_width));
        // The inspector as drawn, its toolbar's controls on one row.
        let one_row = |cx: &mut VisualTestContext, at: &str| {
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let inspector = bounds(cx, "commit-inspector");
            let toolbar = bounds(cx, "commit-message-toolbar");
            let copy = bounds(cx, "copy-message");
            let hash = bounds(cx, "copy-commit");
            let actions = bounds(cx, "commit-recovery-menu");
            let (window, rem) = cx.update(|window, _| {
                (
                    Bounds::new(point(px(0.), px(0.)), window.viewport_size()),
                    window.rem_size(),
                )
            });
            assert!(
                copy.top() == hash.top() && hash.top() == actions.top(),
                "{at}: one row: {copy:?} {hash:?} {actions:?} in {inspector:?}"
            );
            assert!(
                copy.right() < hash.left() && hash.right() < actions.left(),
                "{at}: in order: {copy:?} {hash:?} {actions:?}"
            );
            // Its `py_1` either side, to within the half pixel layout snaps.
            assert!(
                (toolbar.size.height - (actions.size.height + rem * 0.5)).abs() < px(1.),
                "{at}: the toolbar is that row: {toolbar:?}"
            );
            assert!(
                (actions.right() - (toolbar.right() - rem * 0.75)).abs() < px(0.5),
                "{at}: Actions at the trailing edge: {actions:?} in {toolbar:?}"
            );
            assert!(
                within(actions, toolbar) && within(toolbar, inspector) && within(inspector, window),
                "{at}: {actions:?} in {toolbar:?} in {inspector:?} in {window:?}"
            );
            inspector.size.width
        };
        for (text, dragged, from, to) in [
            (13, 440., 1480., &[1000.][..]),
            (18, 470., 1480., &[1240., 1000.][..]),
            (13, 480., 1000., &[900.][..]),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(from), px(680.)));
            draw(cx);
            drag(cx, dragged);
            let at = format!("{text} pt dragged {dragged} px wide in {from} px");
            assert!((saved(cx) - px(dragged)).abs() < px(0.5), "{at}: saved");
            let wide = one_row(cx, &at);
            assert!((wide - px(dragged)).abs() < px(0.5), "{at}: drawn {wide:?}");
            for &width in to {
                cx.simulate_resize(size(px(width), px(680.)));
                draw(cx);
                let at = format!("{at}, then in {width} px");
                let drawn = one_row(cx, &at);
                assert!(
                    drawn < px(dragged) - px(40.),
                    "{at}: drawn {drawn:?} narrower than the saved width"
                );
                assert!(
                    (saved(cx) - px(dragged)).abs() < px(0.5),
                    "{at}: the saved width stays"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The selected commit's Actions stands in the message toolbar, outside
    /// the scrolling message and metadata, so it stays in view at every size:
    /// at the trailing edge of the copy controls' last row, on Hash's row, as
    /// tall as the copy controls, inside the inspector and above the message,
    /// where scrolling the message to its end leaves it. Labelled where its
    /// label and caret fit on that row, as in a 440 px inspector at the
    /// default size and a 480 px one at 18 pt, it is as wide as its label and
    /// caret in the shared 28 px compact control; otherwise it keeps only its
    /// more-actions icon in a 28 px square named "Selected commit actions",
    /// as in the 280 px inspector of a 1,000 × 680 window, the 388 px one at
    /// 18 pt and the half tiles, where Copy message and Hash keep only their
    /// icons too rather than leave Actions a row of the message's. It was
    /// the scrolling metadata's last row, below its fold at 18 pt in
    /// 1,000 × 680. At rest it paints no fill; hovered and pressed it takes
    /// the palette's hover and pressed layers, in a dark and a light palette.
    #[gpui::test]
    async fn commit_actions_stay_in_view_in_the_message_toolbar(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let saved = cx.read(|cx| app.read(cx).settings.inspector_width);
        for (text, width, height, inspector, labelled, compact_copy) in [
            (13, 1000., 680., None, false, false),
            (13, 1480., 980., Some(440.), true, false),
            (13, 461., 490., None, false, true),
            (18, 1000., 680., None, false, false),
            (18, 1480., 800., None, false, false),
            (18, 1480., 980., Some(480.), true, false),
            (18, 461., 490., None, false, true),
        ] {
            // The panel group first measures the inspector in this window, as
            // in a window opened at its size: measured in the last one, it
            // rescales the inspector with the window, so at 18 pt in
            // 1,480 × 800 it was drawn 480 px wide rather than at its 388 px
            // minimum (`the_message_toolbar_fits_the_inspector_as_drawn`).
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    app.settings.inspector_width = inspector.unwrap_or(saved);
                    app.content_panels = cx.new(|_| ResizableState::default());
                    cx.notify();
                });
            });
            COMPACT_NAMES.with(|names| names.borrow_mut().clear());
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let actions = bounds(cx, "commit-recovery-menu");
            let copy = bounds(cx, "copy-message");
            let hash = bounds(cx, "copy-commit");
            let toolbar = bounds(cx, "commit-message-toolbar");
            let viewport = bounds(cx, "commit-message-viewport");
            let panel = bounds(cx, "commit-inspector");
            let (control, inset, whole) = cx.update(|window, _| {
                let rem = window.rem_size();
                let label = shaped_width(
                    "Actions",
                    appearance::ui_text(12.),
                    window.text_style().font_weight,
                    window,
                );
                (
                    window.pixel_snap(appearance::ui_size(28.)),
                    rem * 0.75,
                    // The helper's padding either side, its label, its 6 px
                    // gap and the caret, 0.875 rem square.
                    label + appearance::ui_size(10.) * 2. + appearance::ui_size(6.) + rem * 0.875,
                )
            });
            let slack = px(0.5);
            assert_eq!(actions.size.height, control, "{at}: {actions:?}");
            assert_eq!(
                actions.top(),
                hash.top(),
                "{at}: on Hash's row: {actions:?}"
            );
            assert!(
                hash.right() < actions.left()
                    && (actions.right() - (toolbar.right() - inset)).abs() <= slack,
                "{at}: {actions:?} at the trailing edge of {toolbar:?}"
            );
            assert!(
                within(actions, toolbar) && within(actions, panel),
                "{at}: {actions:?} inside {toolbar:?} and the inspector {panel:?}"
            );
            assert!(
                actions.bottom() <= viewport.top(),
                "{at}: {actions:?} above the scrolling {viewport:?}"
            );
            assert_eq!(
                (copy.size.width == control, hash.size.width == control),
                (compact_copy, compact_copy),
                "{at}: Copy message {copy:?}, Hash {hash:?}"
            );
            let named =
                COMPACT_NAMES.with(|names| names.borrow().get("commit-recovery-menu").copied());
            if labelled {
                assert!(
                    (actions.size.width - whole).abs() <= px(1.),
                    "{at}: {actions:?} is as wide as its label and caret, {whole:?}"
                );
                assert_eq!(named, None, "{at}");
                assert!(cx.debug_bounds("commit-recovery-more").is_none(), "{at}");
            } else {
                assert_eq!(actions.size.width, control, "{at}: {actions:?}");
                assert_eq!(named, Some("Selected commit actions"), "{at}");
                assert!(cx.debug_bounds("commit-recovery-more").is_some(), "{at}");
            }
            // Scrolled to the message's end, Actions has not moved.
            cx.simulate_click(viewport.center(), Modifiers::default());
            cx.simulate_keystrokes("end");
            draw(cx);
            assert_eq!(
                bounds(cx, "commit-recovery-menu"),
                actions,
                "{at}: scrolled"
            );
            cx.simulate_keystrokes("home");
            draw(cx);
        }
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.settings.inspector_width = saved;
                app.content_panels = cx.new(|_| ResizableState::default());
            })
        });
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        for theme in [
            crate::appearance::ThemeChoice::Midnight,
            crate::appearance::ThemeChoice::Daylight,
        ] {
            cx.update(|_, cx| theme.apply(None, cx));
            let (hover, pressed) = cx.update(|_, cx| {
                let p = palette(cx);
                let fill =
                    |color| gpui_kit::Background::from(gpui_kit::Hsla::from(p.control_fill(color)));
                (fill(p.hover), fill(p.selected))
            });
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            draw(cx);
            assert_eq!(
                painted_fills(cx, "commit-recovery-menu"),
                vec![],
                "{theme:?} at rest"
            );
            let center = cx.debug_bounds("commit-recovery-menu").unwrap().center();
            cx.simulate_mouse_move(center, None, Modifiers::default());
            draw(cx);
            assert_eq!(
                painted_fills(cx, "commit-recovery-menu"),
                vec![hover],
                "{theme:?} hovered"
            );
            cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
            draw(cx);
            assert_eq!(
                painted_fills(cx, "commit-recovery-menu"),
                vec![pressed],
                "{theme:?} pressed"
            );
            cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
            draw(cx);
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
    }

    /// The selected commit's message and metadata draw no scrollbar where
    /// they fit, as the compact fixture's one-line commits do at 18 pt in a
    /// 1,480 × 800 and a 1,000 × 680 window and at the default size in 961
    /// and 1,240 px wide tiles 520 px tall: the metadata ends inside the
    /// message's viewport and the list has nothing to scroll, so its
    /// scrollbar draws nothing. With Actions as the metadata's last row they
    /// ran 7 px past it at 18 pt, its row's gap and the metadata's padding,
    /// and 31 px in the tiles; with the metadata's bottom padding they ran
    /// 13 px past it at 18 pt in 1,000 × 680.
    #[gpui::test]
    async fn commit_details_that_fit_draw_no_scrollbar(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        for (text, width, height) in [
            (18, 1480., 800.),
            (18, 1000., 680.),
            (13, 961., 520.),
            (13, 1240., 520.),
            (13, 1480., 980.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
            let metadata = cx.debug_bounds("commit-metadata").unwrap();
            assert!(
                metadata.bottom() <= viewport.bottom(),
                "{at}: {metadata:?} past {viewport:?}"
            );
            assert_eq!(
                cx.read(|cx| app.read(cx).inspector_message.overflow()),
                Some(px(0.)),
                "{at}: nothing to scroll"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A parent's control fits the 156 px inspector of an 18 pt half tile,
    /// 461 × 980: "P1 · ‹hash›", 148 px wide where the inspector leaves
    /// 128.5 px, widened the metadata past the inspector and the date line,
    /// wrapped at that width, ran past its edge. There it reads "P1", named
    /// for its parent's full hash as its tooltip is, and the metadata stays
    /// inside the viewport; in a 1,000 px window it keeps the hash.
    #[gpui::test]
    async fn parent_controls_fit_a_half_tile_inspector(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(2, 40)).await;
        let parent = app.read_with(cx, |app, _| {
            app.selected_commit
                .and_then(|index| app.commits.get(index))
                .and_then(|commit| commit.parents.first().cloned())
                .expect("the selected commit's parent")
        });
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        COMPACT_LABELS.with(|names| names.borrow_mut().clear());
        cx.simulate_resize(size(px(461.), px(980.)));
        draw(cx);
        let viewport = cx.debug_bounds("commit-message-viewport").unwrap();
        let metadata = cx.debug_bounds("commit-metadata").unwrap();
        let control = cx.debug_bounds("commit-parent-0").unwrap();
        let inset = cx.update(|window, _| window.rem_size() * 0.75);
        // A list item is laid out as a root, which rounds its 155.5 px to
        // 156: the half pixel is the metadata's padding, beyond its text.
        assert!(
            metadata.right() <= viewport.right() + px(0.5),
            "{metadata:?} inside {viewport:?}"
        );
        assert!(
            control.right() <= viewport.right() - inset,
            "{control:?} inside {viewport:?}"
        );
        assert_eq!(
            COMPACT_LABELS.with(|names| names.borrow().get("commit-parent-0").cloned()),
            Some(format!("Compare against parent 1 · {parent}").into()),
            "P1, named for its parent's hash"
        );
        let short = cx.update(|window, _| {
            line_widths(&[("P1", appearance::ui_text(12.))], window) + appearance::ui_size(20.)
        });
        assert!(
            control.size.width <= short.max(appearance::ui_size(28.)) + px(0.5),
            "{control:?} reads P1"
        );
        // Wide, it keeps the hash.
        COMPACT_LABELS.with(|names| names.borrow_mut().clear());
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let wide = cx.debug_bounds("commit-parent-0").unwrap();
        assert!(
            wide.size.width > control.size.width,
            "{wide:?} keeps the hash"
        );
        assert_eq!(
            COMPACT_LABELS.with(|names| names.borrow().get("commit-parent-0").cloned()),
            None
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// In a 461 × 490 window, at the default size and at 18 pt, the compact
    /// Actions trigger beside the filter is a 28 px control in Refresh's
    /// frame showing the more-actions dots centred in it, not a bare caret;
    /// it is a button named "Working changes and recovery actions", and Tab
    /// from the filter and Enter open its menu, which Escape closes.
    #[gpui::test]
    async fn compact_actions_is_a_named_more_actions_menu(cx: &mut TestAppContext) {
        const NAME: &str = "Working changes and recovery actions";
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let directories = "menu-item: Directories · MenuItemCheckBox False";
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let at = format!("{text} pt");
            assert!(cx.debug_bounds("working-toolbar").is_some(), "{at}: tight");
            let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
            let trigger = cx.debug_bounds("working-recovery-menu").unwrap();
            assert_eq!(
                (trigger.size.width, trigger.size.height),
                (control, control),
                "{at}: {trigger:?}"
            );
            if let Some(refresh) = cx.debug_bounds("refresh-working") {
                assert_eq!(refresh.size, trigger.size, "{at}: beside {refresh:?}");
                assert_eq!(refresh.top(), trigger.top(), "{at}: beside {refresh:?}");
            }
            // The dots, centred: no caret beside them.
            let dots = cx
                .debug_bounds("working-recovery-more")
                .unwrap_or_else(|| panic!("{at}: the more-actions dots"));
            let slack = px(0.5);
            assert!(
                (dots.center().x - trigger.center().x).abs() <= slack
                    && (dots.center().y - trigger.center().y).abs() <= slack,
                "{at}: {dots:?} centred in {trigger:?}"
            );
            assert_eq!(
                COMPACT_NAMES.with(|names| names.borrow().get("working-recovery-menu").copied()),
                Some(NAME),
                "{at}"
            );

            // Tab from the filter reaches it, and Enter opens its menu.
            let filter = app.read_with(cx, |app, cx| app.working_filter.read(cx).focus_handle(cx));
            cx.update(|window, cx| window.focus(&filter, cx));
            draw(cx);
            cx.simulate_keystrokes("tab");
            draw(cx);
            assert!(cx.debug_bounds(directories).is_none(), "{at}: closed");
            cx.simulate_keystrokes("enter");
            draw(cx);
            assert!(
                cx.debug_bounds(directories).is_some(),
                "{at}: Enter opens the menu"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(cx.debug_bounds(directories).is_none(), "{at}: Escape");
        }
        // It is a button with its name.
        let nodes = crate::worktrees::ring_room_tests::row_nodes(
            cx,
            vec![crate::recovery::working_actions_trigger(true, true)],
        );
        let [node] = nodes.as_slice() else {
            panic!("one node, not {nodes:?}");
        };
        assert_eq!(node.role(), gpui::accesskit::Role::Button);
        assert_eq!(node.label(), Some(NAME));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
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
    /// its text, as the Unstaged heading was at 640 × 680. What is left
    /// below its last whole row lies between it and the short composer.
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

    /// The Staged and Unstaged headings keep their texts and their trailing
    /// Unstage all and Stage all inside the file list's inset, in a 461 × 490
    /// window at the default size and at 18 pt and in a 1480 × 980 one. At
    /// 18 pt in the 156 px inspector each action keeps its icon as a 28 px
    /// control named "Unstage all" or "Stage all", and the group's name
    /// leaves with the glyph kept beside the count; at the default size and
    /// in the wide window each heading keeps its labelled form.
    #[gpui::test]
    async fn section_headings_keep_their_actions_inside_the_list(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let unstaged = app.read_with(cx, |app, _| {
            app.working_rows
                .iter()
                .position(|row| {
                    matches!(
                        row,
                        crate::workspace::WorkingRow::Heading(
                            gitturtle_core::ChangeArea::Unstaged,
                            _
                        )
                    )
                })
                .unwrap()
        });
        let headings = [
            (
                0,
                "unstage-all",
                "Unstage all",
                [
                    "staged-heading-icon",
                    "staged-heading-label",
                    "staged-heading-count",
                ],
            ),
            (
                unstaged,
                "stage-all",
                "Stage all",
                [
                    "unstaged-heading-icon",
                    "unstaged-heading-label",
                    "unstaged-heading-count",
                ],
            ),
        ];
        for (text, width, height) in [(13, 461., 490.), (18, 461., 490.), (13, 1480., 980.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt at {width} × {height}");
            let (inset, control) = cx.update(|window, _| {
                (
                    window.pixel_snap(window.rem_size() * 0.75),
                    window.pixel_snap(appearance::ui_size(28.)),
                )
            });
            let narrow = text == 18;
            for (row, selector, name, [glyph, label, count]) in headings {
                app.update(cx, |app, _| {
                    app.working_scroll.scroll_to_item(row, ScrollStrategy::Top)
                });
                draw(cx);
                // Only the names the settled layout gives: a frame laid out
                // at the previous width can have named a compact form.
                COMPACT_NAMES.with(|names| names.borrow_mut().clear());
                app.update(cx, |_, cx| cx.notify());
                draw(cx);
                let pane = cx.debug_bounds("working-pane").unwrap();
                // Inside the list's 1 px border, at the rows' leading inset.
                let (start, end) = (
                    pane.left() + px(1.) + inset - px(0.5),
                    pane.right() - px(1.) - inset + px(0.5),
                );
                let action = cx
                    .debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {at}"));
                let count = cx
                    .debug_bounds(count)
                    .unwrap_or_else(|| panic!("{count} at {at}"));
                let mut parts = vec![(selector, action)];
                for part in [glyph, label] {
                    parts.extend(cx.debug_bounds(part).map(|bounds| (part, bounds)));
                }
                parts.push(("count", count));
                for (part, bounds) in &parts {
                    assert!(
                        bounds.left() >= start && bounds.right() <= end,
                        "{at}: {part} {bounds:?} outside {start:?}..{end:?}"
                    );
                }
                assert!(
                    count.right() <= action.left(),
                    "{at}: {count:?} before {action:?}"
                );
                // The glyph tells the groups apart wherever the name has left.
                assert!(cx.debug_bounds(glyph).is_some(), "{at}: {glyph}");
                assert_eq!(cx.debug_bounds(label).is_some(), !narrow, "{at}: {label}");
                let names = COMPACT_NAMES.with(|names| names.borrow().clone());
                if narrow {
                    assert_eq!(action.size.width, control, "{at}: {selector} {action:?}");
                    assert_eq!(names.get(selector), Some(&name), "{at}: {selector}");
                } else {
                    assert!(action.size.width > control, "{at}: {selector} {action:?}");
                    assert_eq!(names.get(selector), None, "{at}: {selector}");
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The composer's title line, fields and commit action stay inside its
    /// column in a 461 × 490 window at the default size and at 18 pt, in a
    /// 461 × 1400 one at 18 pt and in a 1480 × 980 one. At 18 pt in the 156 px
    /// inspector the title counter reads "0/72", named "0/72 suggested";
    /// the title's placeholder reads its short whole word, "Summarize", where
    /// it once read "Summarize t…", and the description's, where the composer
    /// draws it, ends with an ellipsis at a character boundary inside its
    /// field; and the commit action reads "Commit", named with its whole
    /// label, such as "Write a commit title". Wherever they fit, the counter,
    /// the title's placeholder and the label stay whole.
    #[gpui::test]
    async fn narrow_composer_keeps_its_texts_inside_the_column(cx: &mut TestAppContext) {
        use crate::workspace::{
            DESCRIPTION_PLACEHOLDER, TITLE_PLACEHOLDER, TITLE_SHORT_PLACEHOLDER,
        };
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let sizes = [
            (13, 461., 490.),
            (18, 461., 490.),
            (18, 461., 1400.),
            (13, 1480., 980.),
        ];
        for (text, width, height) in sizes {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            // Only the names the settled layout gives: a frame laid out at
            // the previous width can have named a compact form.
            COMPACT_LABELS.with(|names| names.borrow_mut().clear());
            app.update(cx, |_, cx| cx.notify());
            draw(cx);
            let at = format!("{text} pt at {width} × {height}");
            let narrow = text == 18;
            let (rem, inset) = cx.update(|window, _| {
                (
                    window.rem_size(),
                    window.pixel_snap(window.rem_size() * 0.75),
                )
            });
            let width_of = |cx: &mut VisualTestContext, text: &str, size: Pixels| {
                cx.update(|window, _| {
                    shaped_width(text, size, window.text_style().font_weight, window)
                })
            };
            let composer = cx.debug_bounds("commit-composer").unwrap();
            let (start, end) = (
                composer.left() + inset - px(0.5),
                composer.right() - inset + px(0.5),
            );
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{selector} at {at}"))
            };
            for selector in [
                "commit-title-name",
                "commit-title-counter",
                "commit-title",
                "commit-staged",
            ] {
                let bounds = bounds(cx, selector);
                assert!(
                    bounds.left() >= start && bounds.right() <= end,
                    "{at}: {selector} {bounds:?} outside {start:?}..{end:?}"
                );
            }
            let labels = COMPACT_LABELS.with(|names| names.borrow().clone());
            let name = |selector| labels.get(selector).map(|name| name.to_string());
            // The counter draws "suggested" where it fits, else names it.
            let counter = bounds(cx, "commit-title-counter");
            let shown = if narrow { "0/72" } else { "0/72 suggested" };
            assert_eq!(
                counter.size.width,
                width_of(cx, shown, appearance::ui_text(11.)).ceil(),
                "{at}: the counter reads {shown:?}"
            );
            assert_eq!(
                name("commit-title-counter").as_deref(),
                narrow.then_some("0/72 suggested"),
                "{at}"
            );
            // The commit action's label fits beside its icon, inside the edge
            // a disabled action keeps: its whole label, or "Commit" named
            // with it. The label depends on the Git identity the fixture
            // finds ("Write a commit title" with one), so it is read back.
            let commit = bounds(cx, "commit-staged");
            let room = commit.size.width - rem * 0.625 * 2. - px(16.) - rem * 0.5 - px(2.);
            let whole = app.read_with(cx, |app, cx| app.commit_action_label(cx));
            let compact = width_of(cx, &whole, rem) > room;
            assert!(compact || !narrow, "{at}: {whole:?} in {room:?}");
            assert!(!compact || width < 1000., "{at}: {whole:?} in {room:?}");
            let shown = if compact { "Commit" } else { whole.as_str() };
            assert!(
                width_of(cx, shown, rem) <= room,
                "{at}: {shown:?} in {room:?}"
            );
            assert_eq!(
                name("commit-staged"),
                compact.then(|| whole.clone()),
                "{at}"
            );
            // Each placeholder fits the field it painted in: whole, the
            // title's in its short whole word, or cut before an ellipsis.
            let placeholders = app.read_with(cx, |app, cx| {
                [
                    (
                        TITLE_PLACEHOLDER,
                        app.commit_title.read(cx).text_bounds(),
                        app.commit_title
                            .read(cx)
                            .presentation()
                            .placeholder()
                            .clone(),
                    ),
                    (
                        DESCRIPTION_PLACEHOLDER,
                        app.commit_message.read(cx).text_bounds(),
                        app.commit_message
                            .read(cx)
                            .presentation()
                            .placeholder()
                            .clone(),
                    ),
                ]
            });
            let description = cx.debug_bounds("commit-description").is_some();
            assert_eq!(description, height > 490., "{at}: the description");
            for (whole, field, shown) in placeholders {
                if whole == DESCRIPTION_PLACEHOLDER && !description {
                    continue;
                }
                let field = field.unwrap_or_else(|| panic!("{at}: {whole:?} unpainted"));
                assert!(
                    width_of(cx, &shown, appearance::ui_text(12.)) <= field.size.width,
                    "{at}: {shown:?} in {field:?}"
                );
                // Its whole-word forms, from the longest; only the shortest
                // is ever cut.
                let forms: &[&str] = if whole == TITLE_PLACEHOLDER {
                    &[TITLE_PLACEHOLDER, TITLE_SHORT_PLACEHOLDER]
                } else {
                    &[whole]
                };
                let shortest = forms[forms.len() - 1];
                let stem = shown.trim_end_matches('…');
                assert!(
                    forms.contains(&shown.as_ref())
                        || (shown.ends_with('…') && !stem.is_empty() && shortest.starts_with(stem)),
                    "{at}: {shown:?} for {whole:?}"
                );
                if whole == TITLE_PLACEHOLDER {
                    // Where the whole title placeholder does not fit, it reads
                    // the short "Summarize", not "Summarize t…".
                    let expected = if narrow {
                        TITLE_SHORT_PLACEHOLDER
                    } else {
                        TITLE_PLACEHOLDER
                    };
                    assert_eq!(shown.as_ref(), expected, "{at}");
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The accessible name the last frame gave `field`: what AccessKit reads
    /// from its editing element, which the kit names after the field's
    /// `aria_label`, or without one after the placeholder it shows.
    fn field_name(cx: &mut VisualTestContext, field: &Entity<InputState>) -> Option<String> {
        input_node(cx, field).label().map(str::to_owned)
    }

    /// The accessibility node `field`'s editing element writes, built inside
    /// a draw. Outside one, GPUI allocates the elements a render makes in a
    /// thread-wide arena that nothing clears, so they held the field after
    /// the test's app was gone, which GPUI's leak check reports when the
    /// app drops; a draw allocates them in the app's own arena and clears
    /// it. The draw paints nothing.
    pub(crate) fn input_node<S: Render>(
        cx: &mut VisualTestContext,
        field: &Entity<S>,
    ) -> gpui::accesskit::Node {
        use gpui::Element as _;
        let mut written = None;
        let viewport = cx.update(|window, _| window.viewport_size());
        cx.draw(point(px(0.), px(0.)), viewport, |window, cx| {
            field.update(cx, |state, cx| {
                let element = Render::render(state, window, cx).into_element();
                let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
                element.write_a11y_info(&mut node);
                written = Some(node);
            });
            div()
        });
        written.expect("the field's element was built")
    }

    /// The Changes filter's placeholder fits its field in whole words. In
    /// the tight form's one row at 18 pt in a 461 × 490 window, where the
    /// field is about 86 px, it reads the short "Filter", whole, where it
    /// once read "Filte…", a cut word; in its own row in a 1480 × 980 window,
    /// at 13 and at 18 pt, it is the whole "Filter working paths…". Only a
    /// field too narrow for "Filter" cuts it before an ellipsis. Whichever
    /// form shows, the field keeps its whole name, "Filter working paths",
    /// where it was once announced as its fitted placeholder ("Filter…").
    #[gpui::test]
    async fn working_filter_placeholder_fits_its_field(cx: &mut TestAppContext) {
        use crate::workspace::{FILTER_NAME, FILTER_PLACEHOLDER, FILTER_SHORT_PLACEHOLDER};
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height, tight) in [
            (18, 461., 490., true),
            (13, 1480., 980., false),
            (18, 1480., 980., false),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            assert_eq!(
                cx.debug_bounds("working-toolbar").is_some(),
                tight,
                "{at}: the one-row form"
            );
            let (field, shown) = app.read_with(cx, |app, cx| {
                let filter = app.working_filter.read(cx);
                (
                    filter.text_bounds(),
                    filter.presentation().placeholder().clone(),
                )
            });
            let field = field.unwrap_or_else(|| panic!("{at}: the filter unpainted"));
            let drawn = cx.update(|window, _| {
                shaped_width(
                    &shown,
                    appearance::ui_text(12.),
                    window.text_style().font_weight,
                    window,
                )
            });
            assert!(drawn <= field.size.width, "{at}: {shown:?} in {field:?}");
            let expected = if tight {
                FILTER_SHORT_PLACEHOLDER
            } else {
                FILTER_PLACEHOLDER
            };
            assert_eq!(shown.as_ref(), expected, "{at}: in {field:?}");
            let filter = app.read_with(cx, |app, _| app.working_filter.clone());
            assert_eq!(
                field_name(cx, &filter).as_deref(),
                Some(FILTER_NAME),
                "{at}: the name"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The History inspector's changed-file filter fits its placeholder in
    /// whole words as the Working filter does: in the 156 px inspector at 18
    /// pt in a 461 × 490 window it reads the short "Filter", where it once
    /// showed "Filter changed" cut at the field's edge, and in a 1480 × 980
    /// window the whole "Filter changed paths…". Either way the field keeps
    /// its whole name, "Filter changed paths".
    #[gpui::test]
    async fn changed_file_filter_fits_in_whole_words_and_keeps_its_name(cx: &mut TestAppContext) {
        use crate::workspace::FILTER_SHORT_PLACEHOLDER;
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height, short) in [
            (18, 461., 490., true),
            (13, 1480., 980., false),
            (18, 1480., 980., false),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let filter = app.read_with(cx, |app, _| app.file_filter.clone());
            let (field, shown) = filter.read_with(cx, |filter, _| {
                (
                    filter.text_bounds(),
                    filter.presentation().placeholder().clone(),
                )
            });
            let field = field.unwrap_or_else(|| panic!("{at}: the filter unpainted"));
            let drawn = cx.update(|window, _| {
                shaped_width(
                    &shown,
                    appearance::ui_text(12.),
                    window.text_style().font_weight,
                    window,
                )
            });
            assert!(drawn <= field.size.width, "{at}: {shown:?} in {field:?}");
            let expected = if short {
                FILTER_SHORT_PLACEHOLDER
            } else {
                CHANGED_FILTER_PLACEHOLDER
            };
            assert_eq!(shown.as_ref(), expected, "{at}: in {field:?}");
            assert_eq!(
                field_name(cx, &filter).as_deref(),
                Some(CHANGED_FILTER_NAME),
                "{at}: the name"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// History's search field fits its placeholder in whole phrases and keeps
    /// its name. At 13 and 18 pt in 461 × 490 and 640 × 680 windows, and in
    /// a 1480 × 980 one, the placeholder is the first of its forms that fits
    /// the width the field draws its text in, and lies whole inside it: at
    /// 18 pt it once showed "…history, a" cut at the field's edge in the 640
    /// px window and a dangling comma in the 461 px one, where it now reads
    /// a shorter form; in the 1480 px window it is the whole "Search
    /// repository history, author, hash…". Whichever form shows, the field
    /// is named "Search repository history", where it was once announced as
    /// the placeholder it showed.
    #[gpui::test]
    async fn history_search_fits_its_placeholder_and_keeps_its_name(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height) in [
            (13, 461., 490.),
            (13, 640., 680.),
            (18, 461., 490.),
            (18, 640., 680.),
            (13, 1480., 980.),
            (18, 1480., 980.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let search = app.read_with(cx, |app, _| app.search.clone());
            let (field, shown) = search.read_with(cx, |search, _| {
                (
                    search.text_bounds(),
                    search.presentation().placeholder().clone(),
                )
            });
            let field = field.unwrap_or_else(|| panic!("{at}: the search field unpainted"));
            let measure = |cx: &mut VisualTestContext, text: &str| {
                cx.update(|window, _| {
                    shaped_width(
                        text,
                        appearance::ui_text(12.),
                        window.text_style().font_weight,
                        window,
                    )
                })
            };
            let drawn = measure(cx, &shown);
            assert!(drawn <= field.size.width, "{at}: {shown:?} in {field:?}");
            let form = SEARCH_PLACEHOLDERS
                .iter()
                .position(|form| *form == shown.as_ref())
                .unwrap_or_else(|| panic!("{at}: {shown:?} is one of the whole forms"));
            for longer in &SEARCH_PLACEHOLDERS[..form] {
                assert!(
                    measure(cx, longer) > field.size.width,
                    "{at}: {longer:?} fits {field:?}, yet {shown:?} shows"
                );
            }
            if width > 1000. {
                assert_eq!(form, 0, "{at}: the whole placeholder in {field:?}");
            } else if text == 18 {
                assert!(form > 0, "{at}: a shorter form in {field:?}");
            }
            assert_eq!(
                field_name(cx, &search).as_deref(),
                Some(SEARCH_NAME),
                "{at}: the name"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The navigation gives way in the one responsive order at the width it
    /// is drawn at. In 360 px at 13 pt Local, Remote and Worktrees keep their
    /// labels on one row, the filter reads its whole placeholder and the
    /// keyboard hint shows. At its 180 px minimum at 18 pt, where "Worktrees"
    /// once wrapped under the others, the hint wrapped inside its phrases and
    /// the filter read "Filter branches & worktre" cut at its edge, each
    /// mode is its 28 px glyph on that row, named after the mode, the hint
    /// gives way whole and the filter reads a shorter whole form. At every
    /// size the modes share one row inside the navigation, the placeholder
    /// lies inside its field, the hint shows exactly where the form keeps
    /// it, and the field is named "Filter branches and worktrees".
    #[gpui::test]
    async fn the_navigation_folds_its_modes_and_hint_before_they_wrap(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.simulate_resize(size(px(1480.), px(800.)));
        draw(cx);
        let panels = app.read_with(cx, |app, _| app.history_panels.clone());
        let search = app.read_with(cx, |app, _| app.nav_search.clone());
        for (text, navigation) in [(13, 360.), (13, 180.), (18, 360.), (18, 180.)] {
            let at = format!("{text} pt in a {navigation} px navigation");
            cx.update(|window, cx| {
                appearance::apply_text_sizes(text, 12, window, cx);
                panels.update(cx, |panels, cx| {
                    panels.resize_panel(0, px(navigation), window, cx)
                });
            });
            draw(cx);
            // Only what this layout names.
            COMPACT_NAMES.with(|names| names.borrow_mut().clear());
            CONTROL_ICONS.with(|icons| icons.borrow_mut().clear());
            app.update(cx, |_, cx| cx.notify());
            draw(cx);
            let sidebar = cx
                .debug_bounds("history-sidebar")
                .unwrap_or_else(|| panic!("{at}: the navigation drawn"));
            // The window's 1,480 px leave History its minimum beside 327.5
            // px of navigation at 18 pt.
            if navigation == 180. {
                assert_eq!(sidebar.size.width, px(navigation), "{at}");
            } else {
                assert!(sidebar.size.width > px(300.), "{at}: {sidebar:?}");
            }
            let at = format!("{at}, drawn {:?} wide", sidebar.size.width);
            let form = cx.update(|window, cx| app.read(cx).sidebar_form(window, cx));
            let (named, icons) = (
                COMPACT_NAMES.with(|names| names.borrow().clone()),
                CONTROL_ICONS.with(|icons| icons.borrow().clone()),
            );
            let row = cx.debug_bounds("nav-mode-local").unwrap();
            for (_, name, symbol, selector) in NAV_MODES {
                let mode = cx
                    .debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {name} drawn"));
                assert_eq!(mode.top(), row.top(), "{at}: {name} on the modes' row");
                assert!(
                    mode.right() <= sidebar.right(),
                    "{at}: {name} {mode:?} inside {sidebar:?}"
                );
                if form.labelled_modes {
                    assert_eq!(named.get(selector), None, "{at}: {name} labelled");
                } else {
                    assert_eq!(named.get(selector), Some(&name), "{at}: {name} named");
                    assert_eq!(
                        icons.get(name).map(SharedString::as_ref),
                        Some(format!("icons/{symbol}.svg").as_str()),
                        "{at}: {name}'s glyph"
                    );
                    assert_eq!(
                        mode.size.width,
                        appearance::ui_size(28.).ceil(),
                        "{at}: {name} is its glyph"
                    );
                }
            }
            assert_eq!(
                cx.debug_bounds("navigation-hint").is_some(),
                form.hint,
                "{at}: the hint where its lines fit whole"
            );
            let (field, shown) = search.read_with(cx, |search, _| {
                (
                    search.text_bounds(),
                    search.presentation().placeholder().clone(),
                )
            });
            let field = field.unwrap_or_else(|| panic!("{at}: the filter unpainted"));
            let drawn = cx.update(|window, _| {
                shaped_width(
                    &shown,
                    appearance::ui_text(11.),
                    window.text_style().font_weight,
                    window,
                )
            });
            assert!(drawn <= field.size.width, "{at}: {shown:?} in {field:?}");
            assert_eq!(
                field_name(cx, &search).as_deref(),
                Some(NAV_FILTER_NAME),
                "{at}: the filter's name"
            );
            match (text, navigation) {
                (13, 360.) => {
                    assert!(form.labelled_modes && form.hint, "{at}: {form:?}");
                    assert_eq!(shown.as_ref(), NAV_FILTER_PLACEHOLDERS[0], "{at}");
                }
                (18, 180.) => {
                    assert!(!form.labelled_modes && !form.hint, "{at}: {form:?}");
                    assert!(
                        NAV_FILTER_PLACEHOLDERS[1..].contains(&shown.as_ref()),
                        "{at}: a shorter whole form, not {shown:?}"
                    );
                }
                _ => {}
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// As glyphs, the selected navigation mode draws a 2 px accent bar under
    /// its glyph, inside its toggle, at 3:1 or more against the toggle's
    /// `selected` fill in Midnight and Daylight; it was told apart only by
    /// that fill, 1.40:1 against the panel in Midnight. The bar follows the
    /// selection, and no other mode draws one.
    #[gpui::test]
    async fn the_selected_mode_glyph_draws_an_accent_bar(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.simulate_resize(size(px(1480.), px(800.)));
        let panels = app.read_with(cx, |app, _| app.history_panels.clone());
        cx.update(|window, cx| {
            appearance::apply_text_sizes(18, 12, window, cx);
            panels.update(cx, |panels, cx| {
                panels.resize_panel(0, px(180.), window, cx)
            });
        });
        draw(cx);
        for (theme, mode, selector) in [
            (
                appearance::ThemeChoice::Midnight,
                NavMode::Local,
                "nav-mode-local",
            ),
            (
                appearance::ThemeChoice::Daylight,
                NavMode::Local,
                "nav-mode-local",
            ),
            (
                appearance::ThemeChoice::Midnight,
                NavMode::Worktrees,
                "nav-mode-worktrees",
            ),
            (
                appearance::ThemeChoice::Daylight,
                NavMode::Remote,
                "nav-mode-remote",
            ),
        ] {
            let at = format!("{} with {selector} selected", theme.label());
            cx.update(|window, cx| theme.apply(Some(window), cx));
            app.update(cx, |app, cx| {
                app.nav_mode = mode;
                cx.notify();
            });
            draw(cx);
            let form = cx.update(|window, cx| app.read(cx).sidebar_form(window, cx));
            assert!(!form.labelled_modes, "{at}: the modes are glyphs");
            let toggle = cx.debug_bounds(selector).expect("the toggle is drawn");
            let bar = cx
                .debug_bounds("nav-mode-selected-bar")
                .unwrap_or_else(|| panic!("{at}: the selected glyph draws its bar"));
            assert!(
                toggle.contains(&bar.origin) && toggle.contains(&bar.bottom_right()),
                "{at}: the bar {bar:?} lies inside its toggle {toggle:?}"
            );
            assert!(bar.size.height >= px(2.), "{at}: {bar:?} is 2 px tall");
            let p = cx.update(|_, cx| palette(cx));
            let (fills, _) = appearance::painted_row(cx, bar);
            let accent = Background::from(Hsla::from(rgb(p.accent)));
            assert_eq!(fills, [accent], "{at}: the bar paints the accent");
            let selected = Background::from(Hsla::from(rgb(p.selected)));
            assert!(
                appearance::painted_row(cx, toggle).0.contains(&selected),
                "{at}: the toggle keeps its selected fill"
            );
            let ratio = appearance::custom::contrast(p.accent, p.selected);
            assert!(
                ratio >= 3.,
                "{at}: the bar #{:06x} on the fill #{:06x} reads {ratio:.2}:1",
                p.accent,
                p.selected
            );
            for (_, _, _, other) in NAV_MODES.into_iter().filter(|mode| mode.3 != selector) {
                let other = cx.debug_bounds(other).expect("the toggle is drawn");
                assert!(
                    !other.intersects(&bar),
                    "{at}: the bar marks only the selected mode"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The selected commit's author is one line in the inspector, whole or
    /// ending in an ellipsis inside its box, with the whole name in its
    /// tooltip and accessible name: "GitTurtle QA" was cut by its box at 18
    /// pt. A name longer than the inspector, at 13 and 18 pt in a 461 × 490
    /// window and at 18 pt in a 1480 × 800 one, keeps one line, lies inside
    /// the metadata's edge, and every run of it ends inside its box and its
    /// clip; hovering it shows its tooltip, and its node is a label whose
    /// value, the name adapters announce, is the whole name.
    #[gpui::test]
    async fn the_inspector_ends_a_long_author_inside_its_box(cx: &mut TestAppContext) {
        use gpui::Element as _;
        use gpui_kit::component::Theme;
        /// gpui-base's tooltip show delay.
        const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
        const AUTHOR: &str = "GitTurtle QA Reviewer, whose name runs past a narrow inspector";
        let script = format!(
            "commit refs/heads/main\nauthor {AUTHOR} <qa@example.invalid> 1700000000 +0000\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline notes.txt\ndata 6\nnotes\n\ndone\n"
        );
        let (_fixture, _app, cx) = styled_history_window(cx, script.as_bytes(), |root| {
            root.text_bg(crate::width_walk::text_mark())
        })
        .await;
        cx.update(|_, cx| {
            crate::appearance::ThemeChoice::Midnight.apply(None, cx);
            cx.set_reduce_motion(true);
        });
        let popover = cx.read(|cx| Background::from(Theme::global(cx).tokens.popover));
        let popups = move |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover)
                    .count()
            })
        };
        let mark = Background::from(crate::width_walk::text_mark());
        for (text, width, height) in [(18, 461., 490.), (18, 1480., 800.), (13, 461., 490.)] {
            let at = format!("{text} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let author = cx
                .debug_bounds("commit-author")
                .unwrap_or_else(|| panic!("{at}: the author drawn"));
            let metadata = cx.debug_bounds("commit-metadata").unwrap();
            let inspector = cx.debug_bounds("commit-inspector").unwrap();
            assert!(
                author.right() <= metadata.right() && metadata.right() <= inspector.right(),
                "{at}: {author:?} inside {metadata:?} inside {inspector:?}"
            );
            assert!(
                author.size.height < appearance::ui_text(11.) * 2.,
                "{at}: one line, not {author:?}"
            );
            let (scale, quads) =
                cx.update(|window, _| (window.scale_factor(), window.painted_quads()));
            let logical = |value: gpui_kit::ScaledPixels| px(value.0 / scale);
            let runs: Vec<(Pixels, Pixels)> = quads
                .iter()
                .filter(|quad| quad.background == mark)
                .filter(|quad| {
                    let middle = point(
                        logical(quad.bounds.left() + quad.bounds.size.width / 2.),
                        logical(quad.bounds.top() + quad.bounds.size.height / 2.),
                    );
                    author.contains(&middle)
                })
                .map(|quad| {
                    (
                        logical(quad.bounds.right()),
                        logical(quad.content_mask.bounds.right()),
                    )
                })
                .collect();
            assert!(!runs.is_empty(), "{at}: the author's text painted");
            for (right, clip) in runs {
                assert!(
                    right <= author.right() + px(0.5) && right <= clip + px(0.5),
                    "{at}: a run ends at {right:?}, its box at {:?}, its clip at {clip:?}",
                    author.right()
                );
            }
            let resting = popups(cx);
            cx.simulate_mouse_move(author.center(), None, Modifiers::default());
            cx.executor().advance_clock(SHOW_DELAY);
            draw(cx);
            assert_eq!(popups(cx), resting + 1, "{at}: the author's tooltip");
            cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
            cx.executor().advance_clock(SHOW_DELAY);
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        let line = author_line(AUTHOR.into(), div()).into_element();
        let mut node = gpui::accesskit::Node::new(line.a11y_role().expect("a role"));
        line.write_a11y_info(&mut node);
        assert_eq!(node.role(), gpui::accesskit::Role::Label);
        assert_eq!(node.value(), Some(AUTHOR), "the name adapters announce");
        assert_eq!(node.label(), Some(AUTHOR));
    }

    /// With "README.md" staged first, as the native review's fixture has,
    /// each file row keeps its name whole in every window from 430 to 480
    /// px wide at 18 pt, around the 156 px inspector of a 461 px window:
    /// where the name does not fit beside the 26 px status tile, the tile
    /// gives way to its bare glyph, left of the name, before the name is
    /// cut. At the default size in a 461 × 490 window, and at 18 pt in a
    /// 1480 × 980 one, the rows keep their tiles.
    #[gpui::test]
    async fn narrow_file_rows_give_the_status_tile_up_before_the_name(cx: &mut TestAppContext) {
        let (fixture, app, cx) = history_window_from(cx, &history_script(40, 3)).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        std::fs::write(root.join("README.md"), "readme\n").unwrap();
        std::fs::write(root.join("alpha.txt"), "alpha\n").unwrap();
        fixture_git(&root, &["add", "README.md", "alpha.txt"]);
        std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let files = app.read_with(cx, |app, _| {
            let entries = &app.work_status.as_ref().unwrap().entries;
            app.working_rows
                .iter()
                .enumerate()
                .filter_map(|(position, row)| match row {
                    crate::workspace::WorkingRow::File(index, _) => Some((
                        position,
                        entries[*index].path.to_string_lossy().into_owned(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(
            files.first().map(|(_, name)| name.as_str()),
            Some("README.md")
        );
        let mut sizes = vec![(13, 461., 490.), (18, 1480., 980.)];
        sizes.extend((430..=480).step_by(2).map(|width| (18, width as f32, 490.)));
        let mut forms = std::collections::BTreeSet::new();
        for (text, width, height) in sizes {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt at {width} × {height}");
            let inset = cx.update(|window, _| window.pixel_snap(window.rem_size() * 0.75));
            for (position, name) in &files {
                app.update(cx, |app, _| {
                    app.working_scroll
                        .scroll_to_item(*position, ScrollStrategy::Top)
                });
                draw(cx);
                let pane = cx.debug_bounds("working-pane").unwrap();
                let end = pane.right() - px(1.) - inset + px(0.5);
                let selector =
                    |part: &str| -> &'static str { format!("working-{part}-{position}").leak() };
                let shown = cx
                    .debug_bounds(selector("name"))
                    .unwrap_or_else(|| panic!("{at}: {name}"));
                let whole = cx.update(|window, _| {
                    shaped_width(name, appearance::ui_text(12.), FontWeight::MEDIUM, window)
                });
                assert!(
                    shown.size.width >= whole && shown.right() <= end,
                    "{at}: {name:?} ({whole:?}) in {shown:?}, the list ending at {end:?}"
                );
                let tile = cx.debug_bounds(selector("tile"));
                let glyph = cx.debug_bounds(selector("glyph"));
                assert!(
                    tile.is_none() || glyph.is_none(),
                    "{at}: {name:?} draws its tile or its glyph"
                );
                if text == 13 || width > 1000. {
                    assert!(tile.is_some(), "{at}: {name:?}'s tile");
                }
                if let Some(status) = tile.or(glyph) {
                    assert!(
                        status.right() <= shown.left() && status.left() >= pane.left(),
                        "{at}: {name:?}'s status {status:?} before {shown:?}"
                    );
                }
                forms.insert((tile.is_some(), glyph.is_some()));
            }
        }
        // The walk reaches both forms: the tile, and its glyph in its place.
        assert!(forms.contains(&(true, false)), "{forms:?}");
        assert!(forms.contains(&(false, true)), "{forms:?}");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A file row keeps its name whole, at least a short one such as
    /// "README.md": at 18 pt in the 156 px inspector of a 461 × 490 window
    /// every row gives up its status word and then its Stage or Unstage
    /// control, which its menu then leads with, and the fixture's names
    /// ("alpha.txt", "beta.txt", "notes.txt") are whole inside the list. At
    /// the default size and in a 1480 × 980 window the rows keep both.
    #[gpui::test]
    async fn narrow_file_rows_keep_their_names_whole(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let files = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                let entries = &app.work_status.as_ref().unwrap().entries;
                app.working_rows
                    .iter()
                    .enumerate()
                    .filter_map(|(position, row)| match row {
                        crate::workspace::WorkingRow::File(index, area) => Some((
                            position,
                            *area,
                            entries[*index].path.to_string_lossy().into_owned(),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(files(cx).len(), 3);
        for (text, width, height) in [(13, 461., 490.), (18, 461., 490.), (13, 1480., 980.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt at {width} × {height}");
            let narrow = text == 18;
            let inset = cx.update(|window, _| window.pixel_snap(window.rem_size() * 0.75));
            for (position, _, name) in files(cx) {
                app.update(cx, |app, _| {
                    app.working_scroll
                        .scroll_to_item(position, ScrollStrategy::Top)
                });
                draw(cx);
                let pane = cx.debug_bounds("working-pane").unwrap();
                let end = pane.right() - px(1.) - inset + px(0.5);
                let selector =
                    |part: &str| -> &'static str { format!("working-{part}-{position}").leak() };
                let shown = cx
                    .debug_bounds(selector("name"))
                    .unwrap_or_else(|| panic!("{at}: {name}"));
                let whole = cx.update(|window, _| {
                    shaped_width(&name, appearance::ui_text(12.), FontWeight::MEDIUM, window)
                });
                assert!(
                    shown.size.width >= whole && shown.right() <= end,
                    "{at}: {name:?} ({whole:?}) in {shown:?}, the list ending at {end:?}"
                );
                for part in ["status", "stage"] {
                    let bounds = cx.debug_bounds(selector(part));
                    assert_eq!(bounds.is_some(), !narrow, "{at}: {name:?}'s {part}");
                    if let Some(bounds) = bounds {
                        assert!(
                            bounds.left() >= shown.right() && bounds.right() <= end,
                            "{at}: {name:?}'s {part} {bounds:?}"
                        );
                    }
                }
            }
        }
        // At 18 pt in the narrow window, the unstaged row's menu leads with
        // Stage.
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let (position, _, name) = files(cx)
            .into_iter()
            .find(|(_, area, _)| *area == gitturtle_core::ChangeArea::Unstaged)
            .unwrap();
        app.update(cx, |app, _| {
            app.working_scroll
                .scroll_to_item(position, ScrollStrategy::Top)
        });
        draw(cx);
        let selector: &'static str = format!("working-row-{position}").leak();
        let row = cx.debug_bounds(selector).unwrap();
        cx.simulate_mouse_move(row.center(), None, Modifiers::default());
        cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
        // The kit builds the menu in a deferred callback.
        cx.run_until_parked();
        draw(cx);
        let stage = cx
            .debug_bounds("menu-item: Stage · MenuItem")
            .expect("the row's menu offers Stage");
        cx.simulate_click(stage.center(), Modifiers::default());
        wait_until(cx, &app, "the menu's Stage", |app| {
            app.operation_busy.is_none() && app.working_paths.staged_count == 3
        });
        assert!(
            files(cx)
                .iter()
                .any(|(_, area, path)| *path == name && *area == gitturtle_core::ChangeArea::Staged),
            "{name:?} staged from its menu"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The Menu key where the platform reports one (Linux), else Shift-F10.
    const MENU_KEY: &str = if cfg!(target_os = "macos") {
        "shift-f10"
    } else {
        "menu"
    };

    /// The surface of the open menu that holds `item`: the last fill of the
    /// popover colour painted around it, over any page surface of that
    /// colour.
    fn menu_surface(cx: &mut VisualTestContext, item: Bounds<Pixels>) -> Bounds<Pixels> {
        cx.update(|window, cx| {
            let fill = Background::from(gpui_kit::component::Theme::global(cx).popover);
            let scale = window.scale_factor();
            let logical = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.background == fill)
                .map(|quad| {
                    let bounds = Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    );
                    (quad.order, bounds)
                })
                .filter(|(_, surface)| surface.contains(&item.center()))
                .max_by_key(|(order, _)| *order)
                .map(|(_, surface)| surface)
                .expect("the menu's surface")
        })
    }

    /// At 461 × 490, at 13 and 18 pt, the Working Actions menu ("Working
    /// changes and recovery actions") opens below its button and ends 8 px
    /// above the status bar, its items scrolling past that: at 18 pt it ran
    /// over the bar. Its items stay reachable by the keyboard.
    #[gpui::test]
    async fn the_working_actions_menu_stays_above_the_status_bar(cx: &mut TestAppContext) {
        const BROWSE: &str = "menu-item: Browse stashes… · MenuItem";
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|window, cx| {
            appearance::ThemeChoice::Midnight.apply(Some(window), cx);
            app.update(cx, |app, cx| app.show_working(window, cx));
        });
        settle(&app, cx).await;
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let at = format!("{text} pt in 461 × 490");
            let trigger = cx
                .debug_bounds("working-recovery-menu")
                .unwrap_or_else(|| panic!("{at}: the Actions button"));
            cx.simulate_click(trigger.center(), Modifiers::default());
            draw(cx);
            let browse = cx
                .debug_bounds(BROWSE)
                .unwrap_or_else(|| panic!("{at}: the menu"));
            let surface = menu_surface(cx, browse);
            let status = crate::theme_editor::tests::status_bar_top(cx);
            assert!(
                surface.top() >= trigger.bottom(),
                "{at}: the menu {surface:?} opens below its button {trigger:?}"
            );
            assert!(
                surface.bottom() <= status - px(8.) + px(0.25),
                "{at}: the menu {surface:?} ends 8 px above the status bar at {status:?}"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(cx.debug_bounds(BROWSE).is_none(), "{at}: Escape closed it");
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The open menu's rows labelled `labels`, in order, where the last frame
    /// drew them, inside or past the menu's fold: an item's whatever its
    /// state, or a label's.
    pub(crate) fn menu_rows(
        cx: &mut VisualTestContext,
        labels: &[&str],
    ) -> Vec<(String, Bounds<Pixels>)> {
        const ROLES: [&str; 5] = [
            "MenuItem",
            "MenuItem disabled",
            "MenuItemCheckBox True",
            "MenuItemCheckBox False",
            "Label",
        ];
        labels
            .iter()
            .filter_map(|label| {
                ROLES
                    .iter()
                    .find_map(|role| {
                        let selector = format!("menu-item: {label} · {role}");
                        cx.debug_bounds(Box::leak(selector.into_boxed_str()))
                    })
                    .map(|row| (label.to_string(), row))
            })
            .collect()
    }

    /// Asserts that the open menu, whose rows are `rows` (`at` names the
    /// case), keeps them as the same menu draws them where they fit,
    /// `uncapped`: each row both draw is as tall as there, and each two rows
    /// that follow one another in both are as far apart, the gap between
    /// groups included. The menu ends 8 px or more above the status bar and
    /// cuts no row at its foot; with rows past that fold it always shows its
    /// scrollbar, track and thumb, beside its rows rather than over them, and
    /// with none it draws no scrollbar. Returns the menu's surface and
    /// whether rows lie past its fold.
    pub(crate) fn assert_menu_keeps_its_rows(
        cx: &mut VisualTestContext,
        at: &str,
        rows: &[(String, Bounds<Pixels>)],
        uncapped: &[(String, Bounds<Pixels>)],
    ) -> (Bounds<Pixels>, bool) {
        let near = |a: Pixels, b: Pixels| (a - b).abs() < px(0.01);
        let surface = menu_surface(cx, rows[0].1);
        let status = crate::theme_editor::tests::status_bar_top(cx);
        assert!(
            surface.bottom() <= status - px(8.) + px(0.25),
            "{at}: the menu {surface:?} ends 8 px above the status bar at {status:?}"
        );
        let reference = |label: &str| uncapped.iter().position(|(name, _)| name == label);
        for (label, row) in rows {
            if let Some(ix) = reference(label) {
                let fits = uncapped[ix].1;
                assert!(
                    near(row.size.height, fits.size.height),
                    "{at}: {label:?} is {row:?}, {fits:?} where the menu fits"
                );
            }
            assert!(
                row.bottom() <= surface.bottom() + px(0.01)
                    || row.top() >= surface.bottom() - px(0.01),
                "{at}: the menu {surface:?} cuts {label:?} at {row:?}"
            );
        }
        for pair in rows.windows(2) {
            let ((above, first), (below, second)) = (&pair[0], &pair[1]);
            if let (Some(i), Some(j)) = (reference(above), reference(below))
                && j == i + 1
            {
                let fits = uncapped[j].1.top() - uncapped[i].1.top();
                assert!(
                    near(second.top() - first.top(), fits),
                    "{at}: {below:?} at {second:?} lies {:?} below {above:?} at {first:?}, \
                     {fits:?} where the menu fits",
                    second.top() - first.top()
                );
            }
        }
        let folded = rows
            .iter()
            .any(|(_, row)| row.top() >= surface.bottom() - px(0.01));
        let scrollbar = cx.debug_bounds("popup-menu-scrollbar");
        if !folded {
            assert!(
                scrollbar.is_none(),
                "{at}: a menu whose rows fit draws no scrollbar"
            );
            return (surface, folded);
        }
        assert!(scrollbar.is_some(), "{at}: the menu has its scrollbar");
        // The scrollbar as the last frame painted it: the opaque fills other
        // than the menu's own in the track along its right edge, its thumb
        // narrower than the track.
        let width = gpui_kit::base::Scrollbar::width();
        let track = surface.right() - width;
        let bar: Vec<Bounds<Pixels>> = cx.update(|window, cx| {
            let fill = Background::from(gpui_kit::component::Theme::global(cx).popover);
            let scale = window.scale_factor();
            let logical = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.background != fill && !quad.background.is_transparent())
                .map(|quad| {
                    Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    )
                })
                .filter(|quad| {
                    quad.left() >= track - px(0.01)
                        && quad.right() <= surface.right() + px(0.01)
                        && quad.top() >= surface.top() - px(0.01)
                        && quad.bottom() <= surface.bottom() + px(0.01)
                })
                .collect()
        });
        assert!(
            bar.iter().any(|quad| quad.size.width < width - px(0.01)),
            "{at}: the menu shows its scrollbar's thumb while rows lie past its fold: {bar:?}"
        );
        for quad in &bar {
            for (label, row) in rows {
                assert!(
                    row.right() <= quad.left() + px(0.01),
                    "{at}: the scrollbar's {quad:?} covers {label:?} at {row:?}"
                );
            }
        }
        (surface, folded)
    }

    /// The Working Actions menu, opened with a click in a window of `window`.
    fn open_working_actions(cx: &mut VisualTestContext, window: Size<Pixels>) {
        cx.simulate_resize(window);
        draw(cx);
        let trigger = cx
            .debug_bounds("working-recovery-menu")
            .expect("the Actions button");
        cx.simulate_click(trigger.center(), Modifiers::default());
        draw(cx);
    }

    /// At 461 × 490, at 13 and 18 pt, the Working Actions menu is shorter
    /// than its rows. Each row keeps the height and spacing it has where the
    /// menu fits, at 1280 × 800, and the menu ends after its last whole row,
    /// above the status bar, scrolling the rest with its scrollbar shown; Up
    /// brings its last row whole into view. Capped at the room above the
    /// bar, the menu once squeezed its rows, 24.5 px apart instead of 27.5
    /// at 13 pt, and at 18 pt cut its last row with no scrollbar.
    #[gpui::test]
    async fn a_capped_working_actions_menu_keeps_its_rows_and_scrolls(cx: &mut TestAppContext) {
        const SHARED: [&str; 5] = [
            "Ignore selected untracked file…",
            "Save changes to stash…",
            "Browse stashes…",
            "Amend last commit…",
            "Undo last local commit…",
        ];
        // In an inspector too short for its selection controls, the menu
        // leads with them, and at 18 pt with Refresh.
        const LEADING: [&str; 4] = [
            "Directories",
            "Stage 0 selected",
            "Unstage 0 selected",
            "Refresh working changes",
        ];
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|window, cx| {
            appearance::ThemeChoice::Midnight.apply(Some(window), cx);
            app.update(cx, |app, cx| app.show_working(window, cx));
        });
        settle(&app, cx).await;
        let top = |rows: &[(String, Bounds<Pixels>)], label: &str| {
            rows.iter()
                .find(|(name, _)| name == label)
                .map(|(_, row)| row.top())
                .unwrap_or_else(|| panic!("{label:?} in {rows:?}"))
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            let at = format!("{text} pt in 1280 × 800");
            open_working_actions(cx, size(px(1280.), px(800.)));
            let uncapped = menu_rows(cx, &SHARED);
            assert_eq!(uncapped.len(), SHARED.len(), "{at}: {uncapped:?}");
            let (surface, folded) = assert_menu_keeps_its_rows(cx, &at, &uncapped, &uncapped);
            assert!(!folded, "{at}: every row fits");
            // A wheel over a menu whose rows fit moves none of them.
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: surface.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-40.))),
                ..Default::default()
            });
            draw(cx);
            assert_eq!(menu_rows(cx, &SHARED), uncapped, "{at}: after a wheel");
            cx.simulate_keystrokes("escape");
            draw(cx);

            let at = format!("{text} pt in 461 × 490");
            open_working_actions(cx, size(px(461.), px(490.)));
            let rows = menu_rows(cx, &[&LEADING[..], &SHARED[..]].concat());
            let lead = rows.len() - SHARED.len();
            assert!(
                lead >= 3,
                "{at}: the menu leads with the selection controls {rows:?}"
            );
            let (surface, folded) = assert_menu_keeps_its_rows(cx, &at, &rows, &uncapped);
            assert!(folded, "{at}: rows lie past the menu's fold at {surface:?}");
            // The leading rows lie as far apart as Ignore… and Save…, and the
            // last of them as far above Ignore… as Browse… lies above Amend….
            let item = top(&uncapped, SHARED[1]) - top(&uncapped, SHARED[0]);
            let group = top(&uncapped, SHARED[3]) - top(&uncapped, SHARED[2]);
            for pair in rows[..lead].windows(2) {
                let apart = top(&rows, &pair[1].0) - top(&rows, &pair[0].0);
                assert_eq!(apart, item, "{at}: {pair:?}");
            }
            let apart = top(&rows, SHARED[0]) - top(&rows, &rows[lead - 1].0);
            assert_eq!(
                apart,
                group,
                "{at}: {:?} and {:?}",
                rows[lead - 1],
                rows[lead]
            );
            cx.simulate_keystrokes("up");
            draw(cx);
            let last = menu_rows(cx, &[SHARED[4]])[0].1;
            assert!(
                last.top() >= surface.top() - px(0.01)
                    && last.bottom() <= surface.bottom() + px(0.01),
                "{at}: Up brings {:?} at {last:?} whole into the menu {surface:?}",
                SHARED[4]
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// At 461 × 490 the Linux Main Menu fits above the status bar at 13 pt
    /// and is taller than the room there at 18 pt. Either way its rows keep
    /// the height and spacing they have in 1280 × 800, and at 18 pt it ends
    /// after its last whole row, scrolling the rest with its scrollbar shown.
    /// A capped menu once squeezed its rows.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_capped_main_menu_keeps_its_rows(cx: &mut TestAppContext) {
        use crate::command_palette::{COMMANDS, CommandId};
        let labels: Vec<&str> = [
            CommandId::Open,
            CommandId::Projects,
            CommandId::History,
            CommandId::Changes,
            CommandId::QuickOpen,
            CommandId::Compare,
            CommandId::Editor,
            CommandId::Terminal,
            CommandId::Reveal,
            CommandId::Palette,
            CommandId::Activity,
            CommandId::Settings,
            CommandId::Help,
            CommandId::About,
        ]
        .into_iter()
        .map(|id| COMMANDS.iter().find(|spec| spec.id == id).unwrap().label)
        .collect();
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
        settle(&app, cx).await;
        let open = |cx: &mut VisualTestContext, viewport: Size<Pixels>| {
            cx.simulate_resize(viewport);
            draw(cx);
            let trigger = cx.debug_bounds("main-menu").expect("the Main Menu button");
            cx.simulate_click(trigger.center(), Modifiers::default());
            draw(cx);
            menu_rows(cx, &labels)
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            let at = format!("{text} pt in 1280 × 800");
            let uncapped = open(cx, size(px(1280.), px(800.)));
            assert_eq!(uncapped.len(), labels.len(), "{at}: {uncapped:?}");
            let (_, folded) = assert_menu_keeps_its_rows(cx, &at, &uncapped, &uncapped);
            assert!(!folded, "{at}: every row fits");
            cx.simulate_keystrokes("escape");
            draw(cx);

            let at = format!("{text} pt in 461 × 490");
            let rows = open(cx, size(px(461.), px(490.)));
            assert_eq!(rows.len(), labels.len(), "{at}: {rows:?}");
            let (surface, folded) = assert_menu_keeps_its_rows(cx, &at, &rows, &uncapped);
            assert_eq!(
                folded,
                text == 18,
                "{at}: rows past the fold at {surface:?}"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Shift-F10 and the Menu key open the Working Changes cursor row's
    /// menu, the one a right-click opens, below the row: Ignore and Discard
    /// beside the row's Stage control, and at 18 pt in a 461 × 490 window,
    /// where the row has no room for the control, led by Stage. Escape
    /// closes it and returns focus to the list; it opens on its first
    /// action, so Enter alone stages the file. The menu, and with it a
    /// narrow row's Stage, once opened only on a right-click.
    #[gpui::test]
    async fn working_row_menu_opens_from_the_keyboard(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| {
            shortcuts::bind_keys(cx);
            native_accessibility::bind_keys(cx);
        });
        let notes = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .position(|entry| entry.path == std::path::Path::new("notes.txt"))
                .unwrap()
        });
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window))
        };
        const STAGE: &str = "menu-item: Stage · MenuItem";
        const IGNORE: &str = "menu-item: Ignore…, untracked files only · MenuItem disabled";
        const DISCARD: &str = "menu-item: Discard changes… · MenuItem";
        for (text, width, height, narrow) in [(13, 1480., 980., false), (18, 461., 490., true)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.choose_working_selection(
                        notes,
                        gitturtle_core::ChangeArea::Unstaged,
                        false,
                        false,
                        window,
                        cx,
                    );
                    window.focus(&app.file_focus, cx);
                })
            });
            draw(cx);
            let position = app.read_with(cx, |app, _| {
                app.working_rows
                    .iter()
                    .position(|row| {
                        matches!(row, crate::workspace::WorkingRow::File(index, area)
                            if *index == notes && *area == gitturtle_core::ChangeArea::Unstaged)
                    })
                    .unwrap()
            });
            let row = cx
                .debug_bounds(format!("working-row-{position}").leak())
                .unwrap_or_else(|| panic!("{at}: the cursor row"));
            assert!(cx.debug_bounds(DISCARD).is_none(), "{at}: no menu yet");
            cx.simulate_keystrokes("shift-f10");
            draw(cx);
            for (item, shown) in [(STAGE, narrow), (IGNORE, true), (DISCARD, true)] {
                assert_eq!(cx.debug_bounds(item).is_some(), shown, "{at}: {item}");
            }
            // A tracked file's Ignore reads as the command, its condition
            // apart from it at the row's end in secondary text: it read as
            // one phrase, "Ignore applies to untracked files".
            let muted = cx.update(|_, cx| {
                gpui::Rgba::from(gpui_kit::component::Theme::global(cx).muted_foreground)
            });
            let (command, detail, ignore) = (
                cx.debug_bounds("menu-command: Ignore…"),
                cx.debug_bounds(format!("menu-detail: Untracked files only · {muted:?}").leak()),
                cx.debug_bounds(IGNORE).unwrap(),
            );
            let (Some(command), Some(detail)) = (command, detail) else {
                panic!("{at}: Ignore… {command:?} and its condition {detail:?}");
            };
            assert!(
                command.right() < detail.left()
                    && (detail.right() - ignore.right()).abs() <= px(12.),
                "{at}: the condition {detail:?} after the command {command:?} at the end of {ignore:?}"
            );
            assert!(!focused(cx), "{at}: the menu takes the keys");
            // Below the cursor row, from its leading edge where the menu
            // fits there, and kept inside the window where it does not, as
            // beside the 156 px inspector at 18 pt.
            let first = cx
                .debug_bounds(if narrow { STAGE } else { IGNORE })
                .unwrap();
            assert!(
                first.top() >= row.bottom()
                    && first.top() < row.bottom() + row.size.height
                    && first.left() >= px(0.)
                    && first.right() <= px(width)
                    && (narrow || first.left() >= row.left()),
                "{at}: the menu's first item {first:?} below the row {row:?}"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(cx.debug_bounds(DISCARD).is_none(), "{at}: Escape closed it");
            assert!(focused(cx), "{at}: Escape returned focus to the list");
        }
        // The menu opens on Stage, the narrow row's first action.
        cx.simulate_keystrokes(MENU_KEY);
        draw(cx);
        assert!(
            cx.debug_bounds(STAGE).is_some(),
            "{MENU_KEY} opened the menu"
        );
        cx.simulate_keystrokes("enter");
        wait_until(cx, &app, "the menu's Stage", |app| {
            app.operation_busy.is_none() && app.working_paths.staged_count == 3
        });
        draw(cx);
        assert!(cx.debug_bounds(STAGE).is_none(), "the menu closed");
        assert!(focused(cx), "focus is back on the list");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Shift-F10 and the Menu key open the highlighted changed file's menu
    /// in History's inspector, the one a right-click opens, below its row.
    /// Escape closes it and returns focus to the list; it opens on File
    /// history…, so Enter alone opens it.
    #[gpui::test]
    async fn changed_file_menu_opens_from_the_keyboard(cx: &mut TestAppContext) {
        const HISTORY: &str = "menu-item: File history… · MenuItem";
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|_, cx| {
            shortcuts::bind_keys(cx);
            native_accessibility::bind_keys(cx);
        });
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window))
        };
        // A highlighted file, as Back from Compare leaves it.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(!app.files.is_empty(), "the commit's changed files");
                app.selected_file = Some(0);
                app.pane = Pane::Files;
                window.focus(&app.file_focus, cx);
                cx.notify();
            })
        });
        draw(cx);
        let row = cx
            .debug_bounds("selected-changed-file")
            .expect("the highlighted row");
        cx.simulate_keystrokes("shift-f10");
        draw(cx);
        let item = cx.debug_bounds(HISTORY).expect("the row's menu");
        assert!(
            item.top() >= row.bottom()
                && item.top() < row.bottom() + row.size.height
                && item.left() >= row.left(),
            "the menu's item {item:?} below the row {row:?}"
        );
        assert!(!focused(cx), "the menu takes the keys");
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(cx.debug_bounds(HISTORY).is_none(), "Escape closed it");
        assert!(focused(cx), "Escape returned focus to the list");
        cx.simulate_keystrokes(MENU_KEY);
        draw(cx);
        assert!(cx.debug_bounds(HISTORY).is_some(), "{MENU_KEY} opened it");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.file_history.is_active()),
            "File history… opened"
        );
    }

    /// Shift-F10 and the Menu key open History's selected commit's menu,
    /// the one a right-click on its row opens, below the row: Revert and
    /// Cherry-pick, and for HEAD Amend and Undo. Escape closes it and
    /// returns focus to the list. The commit list had no keyboard menu, so
    /// its actions were the pointer's alone.
    #[gpui::test]
    async fn commit_menu_opens_from_the_keyboard(cx: &mut TestAppContext) {
        const REVERT: &str = "menu-item: Revert this commit… · MenuItem";
        const UNDO: &str = "menu-item: Undo last local commit… · MenuItem";
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|_, cx| {
            shortcuts::bind_keys(cx);
            native_accessibility::bind_keys(cx);
        });
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).focus.is_focused(window))
        };
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_commit(0, window, cx);
                app.pane = Pane::History;
                window.focus(&app.focus, cx);
            })
        });
        settle(&app, cx).await;
        let index = app.read_with(cx, |app, _| app.selected_commit.expect("a selected commit"));
        let row = cx
            .debug_bounds(format!("history-row-{index}").leak())
            .expect("the selected row");
        let window = cx.update(|window, _| window.viewport_size());
        for key in ["shift-f10", MENU_KEY] {
            assert!(cx.debug_bounds(REVERT).is_none(), "{key}: no menu yet");
            cx.simulate_keystrokes(key);
            draw(cx);
            let item = cx
                .debug_bounds(REVERT)
                .unwrap_or_else(|| panic!("{key} opened the commit's menu"));
            assert!(cx.debug_bounds(UNDO).is_some(), "{key}: HEAD's actions");
            assert!(
                item.top() >= row.bottom()
                    && item.top() < row.bottom() + row.size.height
                    && item.left() >= row.left()
                    && item.right() <= window.width,
                "{key}: the menu's item {item:?} below the row {row:?}"
            );
            assert!(!focused(cx), "{key}: the menu takes the keys");
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{key}: nothing opened past the menu"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(cx.debug_bounds(REVERT).is_none(), "{key}: Escape closed it");
            assert!(focused(cx), "{key}: Escape returned focus to the list");
        }
    }

    /// At 18 pt in a 461 × 490 window, Shift-F10 on any of History's rows
    /// opens the commit's menu ending at least 8 px above the status bar,
    /// moved up where the row leaves it less: on a low row, whose menu's
    /// least height reached past the bar but not past the window's foot, it
    /// ended 2 px above the bar, as the anchor moves a menu only once it
    /// runs past the window. A press on the bar below the menu still closes
    /// it and returns focus to the list, as one anywhere outside it does.
    #[gpui::test]
    async fn keyboard_commit_menu_stays_above_the_status_bar(cx: &mut TestAppContext) {
        const REVERT: &str = "menu-item: Revert this commit… · MenuItem";
        const COMMITS: usize = 12;
        let (_fixture, app, cx) = history_window_from(cx, &history_script(COMMITS, 1)).await;
        cx.update(|window, cx| {
            shortcuts::bind_keys(cx);
            native_accessibility::bind_keys(cx);
            appearance::ThemeChoice::Midnight.apply(Some(window), cx);
            appearance::apply_text_sizes(18, 12, window, cx);
        });
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let status = crate::theme_editor::tests::status_bar_top(cx);
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).focus.is_focused(window))
        };
        let open = |index: usize, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.select_commit(index, window, cx);
                    app.pane = Pane::History;
                    window.focus(&app.focus, cx);
                })
            });
        };
        // Each whole row of the list, selected in place as a click selects
        // it, from the top down to the list's foot.
        let mut lowest = None;
        for index in 0..COMMITS {
            open(index, cx);
            settle(&app, cx).await;
            let list = app.read_with(cx, |app, _| {
                app.history_scroll.0.borrow().base_handle.bounds()
            });
            let Some(row) = cx
                .debug_bounds(format!("history-row-{index}").leak())
                .filter(|row| row.bottom() <= list.bottom())
            else {
                break;
            };
            cx.simulate_keystrokes("shift-f10");
            draw(cx);
            let item = cx
                .debug_bounds(REVERT)
                .unwrap_or_else(|| panic!("row {index} {row:?}: the commit's menu"));
            let surface = menu_surface(cx, item);
            assert!(
                surface.bottom() <= status - px(8.) + px(0.25) && surface.top() >= px(0.),
                "row {index} {row:?}: the menu {surface:?} ends 8 px above the status bar at {status:?}"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                cx.debug_bounds(REVERT).is_none(),
                "row {index}: Escape closed it"
            );
            lowest = Some(index);
        }
        let lowest = lowest.expect("the list's whole rows");
        assert!(lowest >= 1, "the rows down to a low one, {lowest}");
        // The lowest row's menu moves up to the bar's margin, the room it
        // keeps below lying over the bar: a press on the bar there, outside
        // the menu, closes it.
        open(lowest, cx);
        settle(&app, cx).await;
        cx.simulate_keystrokes("shift-f10");
        draw(cx);
        let item = cx.debug_bounds(REVERT).expect("the lowest row's menu");
        let surface = menu_surface(cx, item);
        assert!(
            (status - px(8.) - surface.bottom()).abs() <= px(0.5),
            "row {lowest}: the menu {surface:?} moved up to 8 px above the bar at {status:?}"
        );
        cx.simulate_click(
            point(surface.center().x, status + px(4.)),
            Modifiers::default(),
        );
        draw(cx);
        assert!(
            cx.debug_bounds(REVERT).is_none(),
            "a press on the bar closed the menu"
        );
        assert!(focused(cx), "the press returned focus to the list");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Shift-F10 and the Menu key open the navigator's cursor row's menu,
    /// the one a right-click opens, below the row: a branch's name and
    /// Branch actions…. Escape closes it and returns focus to the tree.
    /// Shift-F10 used to skip the menu for the branch's dialog, and the
    /// Menu key did nothing.
    #[gpui::test]
    async fn navigator_row_menu_opens_from_the_keyboard(cx: &mut TestAppContext) {
        const ACTIONS: &str = "menu-item: Branch actions… · MenuItem";
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|_, cx| {
            shortcuts::bind_keys(cx);
            native_accessibility::bind_keys(cx);
        });
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).nav_focus.is_focused(window))
        };
        let branch = app.read_with(cx, |app, _| {
            app.nav_rows
                .iter()
                .position(|row| matches!(row, NavRow::Branch(..)))
                .expect("a branch row")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.nav_cursor = Some(branch);
                window.focus(&app.nav_focus, cx);
                cx.notify();
            })
        });
        draw(cx);
        let row = cx
            .debug_bounds(format!("nav-row-{branch}").leak())
            .expect("the cursor row");
        for key in ["shift-f10", MENU_KEY] {
            assert!(cx.debug_bounds(ACTIONS).is_none(), "{key}: no menu yet");
            cx.simulate_keystrokes(key);
            draw(cx);
            let item = cx
                .debug_bounds(ACTIONS)
                .unwrap_or_else(|| panic!("{key} opened the row's menu"));
            assert!(
                cx.debug_bounds("menu-item: main · Label").is_some(),
                "{key}: the menu names its branch"
            );
            assert!(
                item.top() >= row.bottom() && item.left() >= row.left(),
                "{key}: the menu's item {item:?} below the row {row:?}"
            );
            assert!(!focused(cx), "{key}: the menu takes the keys");
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{key}: the menu, not the branch's dialog"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                cx.debug_bounds(ACTIONS).is_none(),
                "{key}: Escape closed it"
            );
            assert!(focused(cx), "{key}: Escape returned focus to the tree");
        }
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

    /// With a Git identity, as the native review's fixture has, a short
    /// composer keeps its scaled 4 px between the title's label and field
    /// and between its last row (the title's field, or the description's
    /// hint where the section shows) and the commit action, in a 461 × 490
    /// window at the default size and at 18 pt and in a 640 × 680 one at
    /// both: the height its share leaves over once gathered between the
    /// field and the action (10 px at the default size and 16.5 px at 18 pt
    /// in 640 × 680). Below the action is only the composer's padding, and
    /// the composer ends at the body's foot above the status bar; the rest,
    /// less than a row, lies between the list's last whole row and the
    /// composer, where it once lay below the action.
    #[gpui::test]
    async fn short_composer_keeps_its_rhythm_down_to_the_commit_action(cx: &mut TestAppContext) {
        let (fixture, app, cx) = working_window(cx).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["config", "user.name", "Ada"]);
        fixture_git(&root, &["config", "user.email", "ada@example.com"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height, description) in [
            (13, 461., 490., false),
            (18, 461., 490., false),
            (13, 640., 680., true),
            (18, 640., 680., false),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let (gap, padding, row, status) = cx.update(|window, cx| {
                (
                    window.pixel_snap(appearance::ui_size(4.)),
                    window.pixel_snap(window.rem_size() * 0.5),
                    window.pixel_snap(px(app.read(cx).settings.density.file_row_height())),
                    window.pixel_snap(appearance::ui_size(26.)),
                )
            });
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let composer = bounds(cx, "commit-composer");
            let label = bounds(cx, "commit-title-label");
            let title = bounds(cx, "commit-title");
            let commit = bounds(cx, "commit-staged");
            let hint = cx.debug_bounds("commit-description-hint");
            assert_eq!(hint.is_some(), description, "{at}: the description");
            let last = hint.unwrap_or(title);
            let slack = px(0.5);
            assert!(
                (title.top() - label.bottom() - gap).abs() <= slack,
                "{at}: {label:?} then {title:?}, {gap:?} apart"
            );
            assert!(
                (commit.top() - last.bottom() - gap).abs() <= slack,
                "{at}: {last:?} then {commit:?}, {gap:?} apart"
            );
            assert!(
                (composer.bottom() - commit.bottom() - padding).abs() <= slack,
                "{at}: {commit:?} then the composer's end {composer:?}"
            );
            // The list, whole rows, ends at the composer or less than a row
            // above it, and the composer ends at the body's foot: the rest
            // once lay below the commit action, a band of the inspector's
            // background above the status bar (6 px at the default size and
            // 3.5 px at 18 pt in 461 × 490, 11 and 14 px in 640 × 680).
            let list = bounds(cx, "working-pane");
            let body = bounds(cx, "working-body");
            assert!(
                list.bottom() <= composer.top() + slack && composer.top() - list.bottom() < row,
                "{at}: the list {list:?} above {composer:?}"
            );
            assert!(
                (body.bottom() - composer.bottom()).abs() <= slack
                    && (px(height) - status - body.bottom()).abs() <= slack,
                "{at}: {composer:?} ends at the body's foot {body:?}, above the status bar"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// With a Git identity, a taller composer keeps its fields' 0.5 rem
    /// rhythm down to the commit action, as a short one keeps its scaled
    /// 4 px: the title's label and field, the Description section's label,
    /// field and hint, and the action, each a gap apart, with the composer's
    /// padding below the action and the composer ending at the body's foot.
    /// The height its share leaves over goes to the description's field,
    /// which grows past its preference to take it, so nothing gathers above
    /// the action: at the default size in a 1480 × 980 window the hint and
    /// the action stood 32 px apart, where the rhythm gives 6.5 px, the
    /// composer's floor and its padding between
    /// them. The hint wraps inside the column, as the rows are predicted: at
    /// 18 pt in a 461 × 1400 window it ran on one line past the column, and
    /// the three lines predicted for it lay empty above the action. In
    /// 1480 × 980 and 1000 × 900 windows at the default size, and in
    /// 1480 × 1300 and 461 × 1400 ones at 18 pt (at 18 pt a 1480 × 980
    /// window's composer is the short one).
    #[gpui::test]
    async fn tall_composer_keeps_its_rhythm_down_to_the_commit_action(cx: &mut TestAppContext) {
        let (fixture, app, cx) = working_window(cx).await;
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["config", "user.name", "Ada"]);
        fixture_git(&root, &["config", "user.email", "ada@example.com"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height) in [
            (13, 1480., 980.),
            (13, 1000., 900.),
            (18, 1480., 1300.),
            (18, 461., 1400.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let (gap, padding, status) = cx.update(|window, _| {
                let rem = window.rem_size();
                (
                    window.pixel_snap(rem * 0.5),
                    window.pixel_snap(rem * 0.75),
                    window.pixel_snap(appearance::ui_size(26.)),
                )
            });
            let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
                cx.debug_bounds(selector)
                    .unwrap_or_else(|| panic!("{at}: {selector}"))
            };
            let composer = bounds(cx, "commit-composer");
            let fields = bounds(cx, "commit-composer-fields");
            let commit = bounds(cx, "commit-staged");
            let rows = [
                "commit-title-label",
                "commit-title",
                "commit-description-label",
                "commit-description",
                "commit-description-hint",
            ]
            .map(|selector| bounds(cx, selector));
            let slack = px(0.5);
            for pair in rows.windows(2) {
                assert!(
                    (pair[1].top() - pair[0].bottom() - gap).abs() <= slack,
                    "{at}: {:?} then {:?}, {gap:?} apart",
                    pair[0],
                    pair[1]
                );
            }
            let hint = rows[4];
            assert!(
                (commit.top() - hint.bottom() - gap).abs() <= slack
                    && hint.bottom() <= fields.bottom() + slack,
                "{at}: the hint {hint:?} then {commit:?}, {gap:?} apart, in {fields:?}"
            );
            assert!(
                (composer.bottom() - commit.bottom() - padding).abs() <= slack,
                "{at}: {commit:?} then the composer's end {composer:?}"
            );
            // The composer ends at the body's foot, above the status bar.
            let body = bounds(cx, "working-body");
            assert!(
                (body.bottom() - composer.bottom()).abs() <= slack
                    && (px(height) - status - body.bottom()).abs() <= slack,
                "{at}: {composer:?} ends at the body's foot {body:?}, above the status bar"
            );
            // The hint wraps inside the column, as the composer's rows are
            // predicted: at 18 pt in a 461 px window it ran on one 308 px
            // line past the 129.5 px column.
            let (line, whole) = cx.update(|window, cx| {
                let size = appearance::ui_text(10.);
                let hint = if app.read(cx).working_paths.staged_count == 0 {
                    "Stage files to include in your commit"
                } else {
                    "Only staged changes will be committed"
                };
                (
                    window
                        .text_style()
                        .line_height
                        .to_pixels(size.into(), window.rem_size()),
                    shaped_width(hint, size, window.text_style().font_weight, window),
                )
            });
            let lines = (whole / hint.size.width).ceil();
            assert!(
                hint.size.height + slack >= line * lines,
                "{at}: the hint {hint:?}, {whole:?} on one line, in {lines} lines of {line:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The composer's conflict note, "Resolve conflicted files before
    /// committing.", wraps inside the composer's fields as their rows are
    /// predicted, as the description's hint has since e3fc123: its text,
    /// unable to shrink below its one line, ran past the column at 18 pt in
    /// a 461 px window. At the default size in 1480 × 980 and 461 × 1400
    /// windows, and at 18 pt in a 461 × 1400 one.
    #[gpui::test]
    async fn composer_conflict_note_wraps_inside_its_column(cx: &mut TestAppContext) {
        const NOTE: &str = "Resolve conflicted files before committing.";
        let (_fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        for (text, width, height) in [(13, 1480., 980.), (13, 461., 1400.), (18, 461., 1400.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    app.working_paths.conflicted = true;
                    cx.notify();
                })
            });
            draw(cx);
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let fields = cx
                .debug_bounds("commit-composer-fields")
                .unwrap_or_else(|| panic!("{at}: the composer's fields"));
            let note = cx
                .debug_bounds("commit-conflict-note")
                .unwrap_or_else(|| panic!("{at}: the conflict note"));
            let slack = px(0.5);
            assert!(
                note.left() >= fields.left() - slack && note.right() <= fields.right() + slack,
                "{at}: the note {note:?} lies across the fields {fields:?}"
            );
            let (line, whole) = cx.update(|window, _| {
                let size = appearance::ui_text(11.);
                (
                    window
                        .text_style()
                        .line_height
                        .to_pixels(size.into(), window.rem_size()),
                    shaped_width(NOTE, size, window.text_style().font_weight, window),
                )
            });
            let lines = (whole / note.size.width).ceil();
            assert!(
                note.size.height + slack >= line * lines,
                "{at}: the note {note:?}, {whole:?} on one line, in {lines} lines of {line:?}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The composer's Description section (its label, field and hint) is
    /// drawn whole inside the scrolling fields, above the commit action, or
    /// not at all, with and without a Git identity (whose absence adds the
    /// "Set your commit identity" guidance below it). In a 461 × 490 window
    /// the composer is too short for it, so no part of "Description ·
    /// optional" shows below the title, and Tab from the title passes no
    /// undrawn field; in a 640 × 680 window with an identity it keeps a
    /// one-line field; in a 1480 × 980 window it shows at the default size
    /// and at 18 pt, and in a 461 × 1400 one at 18 pt, where its label drops
    /// "· optional". Every text and field the composer draws lies inside its
    /// fields' box and the window, at the default size and at 18 pt, in
    /// 461 × 490, 640 × 680, 461 × 980, 461 × 1400 and 1480 × 980 windows,
    /// and its label keeps one line. A focused description that leaves hands
    /// focus to the title.
    #[gpui::test]
    async fn composer_description_is_whole_or_absent(cx: &mut TestAppContext) {
        let (fixture, app, cx) = working_window(cx).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        const SECTION: [&str; 3] = [
            "commit-description-label",
            "commit-description",
            "commit-description-hint",
        ];
        // Each size, and whether the section shows there without and with
        // an identity (`None` where either is whole).
        let sizes = [
            (13, 461., 490., Some((false, false))),
            (18, 461., 490., Some((false, false))),
            (13, 640., 680., Some((false, true))),
            (18, 640., 680., None),
            (18, 461., 980., None),
            (18, 461., 1400., Some((true, true))),
            (13, 1480., 980., Some((true, true))),
            (18, 1480., 980., Some((true, true))),
        ];
        for identity in [false, true] {
            // The repository's own identity, which the app reads back. The app
            // reads global configuration too, so without one the repository
            // sets empty values, which win over a developer's own identity.
            let (name, email) = if identity {
                ("Ada", "ada@example.com")
            } else {
                ("", "")
            };
            let root = fixture.path().join("narrow");
            fixture_git(&root, &["config", "user.name", name]);
            fixture_git(&root, &["config", "user.email", email]);
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            settle(&app, cx).await;
            assert_eq!(
                app.read_with(cx, |app, _| app
                    .profile
                    .as_ref()
                    .map(|p| (p.name.clone(), p.email.clone()))),
                Some((name.to_owned(), email.to_owned())),
                "the fixture's identity"
            );
            for (text, width, height, expected) in sizes {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{text} pt at {width} × {height}, identity {identity}");
                // A new size and text size can take a few measured passes:
                // read the layout once it holds still.
                let geometry = |cx: &mut VisualTestContext| {
                    ["commit-composer", "commit-composer-fields", SECTION[1]]
                        .map(|selector| cx.debug_bounds(selector))
                };
                for _ in 0..4 {
                    let before = geometry(cx);
                    draw(cx);
                    if geometry(cx) == before {
                        break;
                    }
                }
                let composer = cx.debug_bounds("commit-composer").unwrap();
                let fields = cx.debug_bounds("commit-composer-fields").unwrap();
                let commit = cx.debug_bounds("commit-staged").unwrap();
                assert!(
                    composer.bottom() <= px(height) && commit.bottom() <= composer.bottom(),
                    "{at}: {commit:?} in {composer:?}"
                );
                // The whole section, or none of it.
                let section = SECTION.map(|selector| cx.debug_bounds(selector));
                let shown = section[0].is_some();
                assert!(
                    section.iter().all(|part| part.is_some() == shown),
                    "{at}: {section:?}"
                );
                if let Some((without, with)) = expected {
                    assert_eq!(shown, if identity { with } else { without }, "{at}");
                }
                // Every text and field inside the scrolling fields is whole
                // in their box, which ends above the commit action.
                let slack = px(0.5);
                for selector in ["commit-title-label", "commit-title"]
                    .into_iter()
                    .chain(SECTION)
                {
                    let Some(part) = cx.debug_bounds(selector) else {
                        continue;
                    };
                    assert!(
                        part.top() + slack >= fields.top()
                            && part.bottom() <= fields.bottom() + slack
                            && part.bottom() <= commit.top(),
                        "{at}: {selector} {part:?} outside {fields:?}, above {commit:?}"
                    );
                    assert!(
                        part.left() >= composer.left() && part.right() <= composer.right() + slack,
                        "{at}: {selector} {part:?} beside {composer:?}"
                    );
                }
                // The label keeps one line.
                if let Some(label) = section[0] {
                    let line = cx.update(|window, _| {
                        window.pixel_snap(
                            window
                                .text_style()
                                .line_height
                                .to_pixels(appearance::ui_text(11.).into(), window.rem_size()),
                        )
                    });
                    assert!(
                        label.size.height <= line + slack,
                        "{at}: the label {label:?} keeps one line of {line:?}"
                    );
                    // "Description" and, where it fits, "· optional", each
                    // drawn whole inside the label's line.
                    let parts = [
                        ("commit-description-name", "Description", FontWeight::MEDIUM),
                        (
                            "commit-description-optional",
                            "· optional",
                            FontWeight::NORMAL,
                        ),
                    ];
                    for (selector, text, weight) in parts {
                        let Some(part) = cx.debug_bounds(selector) else {
                            assert!(width < 1000., "{at}: {selector}");
                            continue;
                        };
                        let whole = cx.update(|window, _| {
                            shaped_width(text, appearance::ui_text(11.), weight, window)
                        });
                        assert!(
                            part.size.width + slack >= whole
                                && part.right() <= label.right() + slack
                                && part.size.height <= line + slack,
                            "{at}: {text:?} ({whole:?}) in {part:?}, the label {label:?}"
                        );
                    }
                }
            }
        }

        // Tab from the title in the short window passes no undrawn field.
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let (title, description) = app.read_with(cx, |app, cx| {
            (
                app.commit_title.read(cx).focus_handle(cx),
                app.commit_message.read(cx).focus_handle(cx),
            )
        });
        cx.update(|window, cx| window.focus(&title, cx));
        draw(cx);
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            !cx.update(|window, _| description.is_focused(window)),
            "Tab from the title reaches the undrawn description"
        );

        // A focused description that leaves gives its focus to the title.
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        assert!(cx.debug_bounds("commit-description").is_some());
        cx.update(|window, cx| window.focus(&description, cx));
        cx.simulate_input("Why this change");
        draw(cx);
        assert!(cx.update(|window, _| description.is_focused(window)));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(cx.debug_bounds("commit-description").is_none());
        assert!(
            cx.update(|window, _| title.is_focused(window)),
            "the title takes the leaving description's focus"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Tab and Shift-Tab leave the commit description for the next and the
    /// previous stop, as on every other stop, and add nothing to its text:
    /// the kit's multi-line field indents with them, so focus never left it.
    #[gpui::test]
    async fn tab_and_shift_tab_leave_the_commit_description(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = working_window(cx).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        assert!(cx.debug_bounds("commit-description").is_some());
        let (title, description) = app.read_with(cx, |app, cx| {
            (
                app.commit_title.read(cx).focus_handle(cx),
                app.commit_message.read(cx).focus_handle(cx),
            )
        });
        let text = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, cx| {
                app.commit_message.read(cx).value().to_string()
            })
        };
        cx.update(|window, cx| window.focus(&description, cx));
        cx.simulate_input("Why this change");
        draw(cx);
        cx.simulate_keystrokes("tab");
        draw(cx);
        let next = cx
            .update(|window, cx| window.focused(cx))
            .expect("Tab leaves focus on a stop");
        assert_ne!(next, description, "Tab leaves the description");
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the frame draws the stop after the description"
        );
        assert_eq!(text(cx), "Why this change", "Tab adds nothing");
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert!(
            cx.update(|window, _| description.is_focused(window)),
            "Shift-Tab from the next stop returns to the description"
        );
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert!(
            cx.update(|window, _| title.is_focused(window)),
            "Shift-Tab leaves the description for the title"
        );
        assert_eq!(text(cx), "Why this change", "Shift-Tab removes nothing");
    }

    /// Typed into `field`, Tab moves focus on to a drawn stop and Shift-Tab
    /// comes back and then leaves it backwards, neither changing its text.
    fn assert_tab_leaves(cx: &mut VisualTestContext, field: &Entity<TextareaState>, name: &str) {
        let focus = cx.read(|cx| field.read(cx).focus_handle(cx));
        let text = |cx: &mut VisualTestContext| cx.read(|cx| field.read(cx).value().to_string());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, _| focus.is_focused(window));
        let drawn =
            |cx: &mut VisualTestContext| !cx.update(|window, _| window.context_stack().is_empty());
        cx.update(|window, cx| window.focus(&focus, cx));
        draw(cx);
        assert!(focused(cx) && drawn(cx), "{name} takes focus");
        cx.simulate_input("Why");
        draw(cx);
        let typed = text(cx);
        assert!(typed.contains("Why"), "{name} takes typing: {typed:?}");
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(!focused(cx), "Tab leaves {name}");
        assert!(drawn(cx), "Tab lands on a drawn stop after {name}");
        assert_eq!(text(cx), typed, "Tab adds nothing to {name}");
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert!(focused(cx), "Shift-Tab comes back to {name}");
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert!(!focused(cx), "Shift-Tab leaves {name} backwards");
        assert!(drawn(cx), "Shift-Tab lands on a drawn stop before {name}");
        assert_eq!(text(cx), typed, "Shift-Tab removes nothing from {name}");
    }

    /// Every multi-line prose field lets Tab and Shift-Tab leave it, as the
    /// commit description does, and keeps its text: the tag annotation, the
    /// amended description, the remote form's fetch URLs, push URLs and fetch
    /// refspecs, and the rebase message. The kit's multi-line field indented
    /// with them, so focus never left. The GitHub panel's growing composers
    /// never indented (`github_view::conversations::tests`).
    #[gpui::test]
    async fn tab_and_shift_tab_leave_every_prose_field(cx: &mut TestAppContext) {
        use gitturtle_core::{InteractiveRebaseCommand, RebaseAction, WriteCommand};
        use gpui_kit::component::WindowExt;
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 1)).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        // Reads that open a form answer on the operation thread; settle the
        // replies before the form is used.
        let drain = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.operations.drain();
                app.preferences_writer.drain();
            });
            cx.executor().run_until_parked();
            draw(cx);
        };
        let close = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.close_all_dialogs(cx));
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(400));
            draw(cx);
        };

        let field = cx.update(|window, cx| {
            app.update(cx, |app, cx| tags::open_tag_annotation(app, window, cx))
        });
        draw(cx);
        assert_tab_leaves(cx, &field, "the tag annotation");
        close(cx);

        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| recovery::prepare_amend(app, head, window, cx))
        });
        drain(cx);
        let field = app
            .read_with(cx, recovery::amend_description)
            .expect("Amend last commit opens");
        assert_tab_leaves(cx, &field, "the amended description");
        close(cx);

        let lists = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                branch_actions::open_remote_lists(app, window, cx)
            })
        });
        draw(cx);
        for (name, field) in &lists {
            assert_tab_leaves(cx, field, name);
        }
        close(cx);

        // Last, as it leaves the repository mid-rebase: a reword paused for
        // its message, which Continue rebase edits.
        // The imported fixture has no checkout, which the rebase would take
        // for working changes.
        let root = fixture.path().join("narrow");
        for args in [
            ["config", "user.name", "Fixture"],
            ["config", "user.email", "fixture@example.invalid"],
            ["config", "commit.gpgsign", "false"],
            ["reset", "--hard", "--quiet"],
        ] {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .status()
                .unwrap();
            assert!(status.success());
        }
        let repo = app.read_with(cx, |app, _| app.repository.clone().unwrap());
        let plan = repo.interactive_rebase_plan("HEAD~1").unwrap();
        let mut steps = plan.steps();
        steps[0].action = RebaseAction::Reword;
        repo.execute(&WriteCommand::InteractiveRebase(Arc::new(
            InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published: false,
            },
        )))
        .unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.prepare_interactive_rebase_continue(window, cx)
            })
        });
        drain(cx);
        let field = app
            .read_with(cx, interactive_rebase::rebase_message)
            .expect("Continue rebase opens with Git's message");
        assert_tab_leaves(cx, &field, "the rebase message");
        close(cx);
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
        // Opened from the keyboard, the menu starts on its first enabled
        // item, so one Down reaches More context.
        choose(cx, 1);
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
        painted_sides_of(cx, bounds)
    }

    /// [`painted_sides`] for `bounds`.
    fn painted_sides_of(
        cx: &mut VisualTestContext,
        bounds: Bounds<Pixels>,
    ) -> [Option<gpui_kit::Background>; 4] {
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

    /// History's commit list draws its ring whole, on all four sides of the
    /// part of the list the window shows, where the table runs sideways past
    /// that part, as narrow History's Author, Date and SHA do under the
    /// inspector: at 461 × 490 and 640 × 680, at 13 and 18 pt, with the
    /// table at its start and scrolled to its end. The list's own edge ran
    /// with the table, so its right side lay beyond the viewport, and its
    /// left side too once the table scrolled.
    #[gpui::test]
    async fn narrow_history_draws_its_ring_whole(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let (ring, edge) = cx.update(|_, cx| {
            let p = palette(cx);
            let solid = |color| Some(gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(color))));
            ([solid(p.accent); 4], [solid(p.border); 4])
        });
        let (focus, elsewhere) =
            app.read_with(cx, |app, _| (app.focus.clone(), app.app_focus.clone()));
        let scroll = app.read_with(cx, |app, _| app.history_horizontal.clone());
        // The part of the list inside History's column.
        let shown = |cx: &mut VisualTestContext| {
            let list = cx.debug_bounds("history-pane").unwrap();
            let view = cx.debug_bounds("history-scope-toolbar").unwrap();
            (
                Bounds::from_corners(
                    point(list.left().max(view.left()), list.top()),
                    point(list.right().min(view.right()), list.bottom()),
                ),
                list,
            )
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for (width, height) in [(461., 490.), (640., 680.)] {
                cx.simulate_resize(size(px(width), px(height)));
                cx.update(|window, cx| window.focus(&elsewhere, cx));
                draw(cx);
                let at = format!("{width} × {height} at {text} pt");
                assert!(
                    app.read_with(cx, |app, _| app.history_column_layout().narrow),
                    "narrow {at}"
                );
                let (bounds, list) = shown(cx);
                assert!(
                    list.right() > bounds.right() + px(100.),
                    "the table runs past the viewport {at}: {list:?}"
                );
                assert_eq!(painted_sides_of(cx, bounds), edge, "at rest {at}");
                cx.update(|window, cx| window.focus(&focus, cx));
                cx.simulate_keystrokes("down");
                draw(cx);
                assert_eq!(painted_sides_of(cx, bounds), ring, "focused by a key {at}");

                scroll.set_offset(point(-scroll.max_offset().x, px(0.)));
                draw(cx);
                let (scrolled, list) = shown(cx);
                assert!(
                    list.left() < scrolled.left() - px(100.),
                    "the table scrolled to its end {at}: {list:?}"
                );
                assert_eq!(scrolled, bounds, "{at}");
                assert_eq!(
                    painted_sides_of(cx, scrolled),
                    ring,
                    "scrolled to the table's end {at}"
                );
                scroll.set_offset(point(px(0.), px(0.)));
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
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

    /// Disabled, the toolbar's Fetch, Pull and Push rest on the neutral
    /// `subtle`, which stands only 1.046:1 off Midnight's toolbar (#131a25 on
    /// #171e2b), so there they drew only their dim labels. In every palette,
    /// full and compact, a disabled action's fill, or else a 1 px edge 1.4:1
    /// or more off the toolbar, stands off it; an available one draws no
    /// edge, and none moves between the two, the edge taking its pixel from
    /// its padding.
    #[gpui::test]
    async fn disabled_git_actions_keep_their_shape_on_the_toolbar(cx: &mut TestAppContext) {
        use crate::appearance::custom::contrast;
        const ACTIONS: [&str; 3] = ["fetch", "pull", "push"];
        let (fixture, app, cx) = history_window_from(cx, &history_script(3, 3)).await;
        track_a_local_origin(fixture.path(), &app, cx).await;
        let solid = |fill: gpui_kit::Background| {
            let color = fill.as_solid().expect("a solid fill").to_rgb();
            let channel = |value: f32| (value * 255.).round() as u32;
            (channel(color.r) << 16) | (channel(color.g) << 8) | channel(color.b)
        };
        let busy = |cx: &mut VisualTestContext, busy: bool| {
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    app.operation_busy = busy.then_some("Fetching remote…");
                    cx.notify()
                })
            });
            draw(cx);
            // A write's busy look waits a moment.
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            draw(cx);
        };
        let mut failures = Vec::new();
        let (mut edged, mut plain) = (0, 0);
        for (window, compact) in [
            (size(px(1480.), px(980.)), false),
            (size(px(461.), px(400.)), true),
        ] {
            cx.simulate_resize(window);
            for (name, palette) in crate::appearance::test_palettes() {
                cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
                busy(cx, false);
                assert_eq!(predicted_chrome(&app, cx).1.is_compact(), compact);
                let at = format!("{name}, compact {compact}");
                let available = ACTIONS.map(|action| cx.debug_bounds(action).expect(action));
                for action in ACTIONS {
                    let edges = painted_edges(cx, action);
                    if !edges.is_empty() {
                        failures.push(format!("{at}: available {action} with edges {edges:?}"));
                    }
                }
                busy(cx, true);
                let [surface] = painted_fills(cx, "git-actions")[..] else {
                    panic!("{at}: the toolbar paints one fill");
                };
                let surface = solid(surface);
                for (action, place) in ACTIONS.into_iter().zip(available) {
                    assert_eq!(
                        cx.debug_bounds(action),
                        Some(place),
                        "{at}: disabled {action} moves"
                    );
                    let [fill] = painted_fills(cx, action)[..] else {
                        panic!("{at}: disabled {action} paints one fill");
                    };
                    let fill = solid(fill);
                    let edges = painted_edges(cx, action);
                    if contrast(fill, surface) < 1.05 {
                        edged += 1;
                        match edges[..] {
                            [edge] if contrast(solid(edge.into()), surface) >= 1.4 => {}
                            _ => failures.push(format!(
                                "{at}: {action} {fill:06x} on {surface:06x} with edges {edges:?}"
                            )),
                        }
                    } else {
                        plain += 1;
                        if !edges.is_empty() {
                            failures
                                .push(format!("{at}: {action} needs no edge but draws {edges:?}"));
                        }
                    }
                }
            }
        }
        assert!(edged > 0 && plain > 0, "edged {edged}, plain {plain}");
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
            // text in secondary text, not as a fourth, unavailable item, and
            // says what the review shows rather than titling an empty group.
            let caption = if checked {
                "Review filter active · exact source retained"
            } else {
                "Showing the original Git diff"
            };
            // It reads as a heading by the section labels' 11 px medium, not
            // by colour alone.
            let muted = cx.update(|_, cx| gpui_kit::component::Theme::global(cx).muted_foreground);
            for entry in [
                format!("menu-item: {caption} · Label"),
                format!("menu-label: {caption} · {:?}", gpui::Rgba::from(muted)),
                crate::native_accessibility::menu_tests::menu_label_style(
                    caption,
                    appearance::ui_text(appearance::SECTION_LABEL_TEXT),
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

    /// "Showing the original Git diff" and the other captions start where
    /// the buttons' labels do when they wrap to a line of their own.
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
        cx.simulate_resize(size(px(1160.), px(680.)));
        draw(cx);
        // The full row, its caption on a line of its own: the column holds
        // the row as it first appears, with "Showing the original Git diff",
        // but not with the longer caption.
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

    /// The collapsed row never shows a cut caption such as "Showing the
    /// orig…". From the column its whole text needs beside the arrows and
    /// Options, measured from the interface font, the caption shows whole
    /// after Options; one pixel narrower it leaves the row and closes the
    /// Options menu instead, announced by the same words. At 13 pt a 461 ×
    /// 490 tile is narrower than that.
    #[gpui::test]
    async fn narrow_review_caption_shows_whole_or_leaves_the_row(cx: &mut TestAppContext) {
        use crate::text_review::collapsed_caption_width;
        const CAPTION: &str = "Showing the original Git diff";
        const IN_MENU: &str = "menu-item: Showing the original Git diff · Label";
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

    /// Delivers only the frames the window asks for, as a platform's frame
    /// source does: the test platform draws a dirty window as each update
    /// ends, and each frame asked for with `on_next_frame`, as
    /// `request_animation_frame` asks, is delivered until none is left.
    /// Returns how many were delivered, and fails past `limit`: an idle
    /// window that keeps asking for frames redraws for ever.
    fn requested_frames(cx: &mut VisualTestContext, limit: usize) -> usize {
        let mut delivered = 0;
        loop {
            cx.executor().run_until_parked();
            if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
                return delivered;
            }
            delivered += 1;
            assert!(
                delivered <= limit,
                "the window still asks for a frame after {delivered}"
            );
        }
    }

    /// What [`history_geometry`] reads: [`history_columns`], then the commit
    /// list's and the inspector's bounds.
    type HistoryGeometry = (
        (f32, Vec<(Pixels, Pixels)>),
        Option<Bounds<Pixels>>,
        Option<Bounds<Pixels>>,
    );

    /// History's column headers, the width it recorded, and the commit list
    /// and inspector, the commit's or the empty one, as the last frame drew
    /// them.
    fn history_geometry(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> HistoryGeometry {
        (
            history_columns(app, cx),
            cx.debug_bounds("history-pane"),
            cx.debug_bounds("commit-inspector")
                .or_else(|| cx.debug_bounds("empty-state")),
        )
    }

    /// One resize, and only the frames it causes, lay History out for the
    /// new width: natively, after a single resize with no further input,
    /// the table kept the old width's columns for 2.5 s, until something
    /// else redrew, because History measures its width while drawing and a
    /// notify during a draw asks for no frame. Each resize below, wider and
    /// narrower, settles in the frames it asks for to the columns, list and
    /// inspector that further forced frames keep, and once settled the
    /// window asks for no frame at all, so an idle window draws nothing.
    /// With a commit selected, the list's probe reveals it after the
    /// viewport changes and asks for a frame of its own; without one, only
    /// History's new width can.
    #[gpui::test]
    async fn one_resize_lays_history_out_for_its_new_width(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window(cx).await;
        cx.simulate_resize(size(px(1480.), px(800.)));
        draw(cx);
        requested_frames(cx, 4);
        for (selected, width, height) in [
            (true, 1000., 800.),
            (true, 1480., 800.),
            (false, 1480., 800.),
            (false, 1000., 800.),
            (false, 1480., 800.),
            (false, 1213., 700.),
        ] {
            if !selected && app.read_with(cx, |app, _| app.selected_commit.is_some()) {
                app.update(cx, |app, cx| {
                    app.selected_commit = None;
                    app.files.clear();
                    cx.notify();
                });
                draw(cx);
                requested_frames(cx, 4);
                continue;
            }
            let at = format!("after one resize to {width} × {height}, selected {selected}");
            let before = app.read_with(cx, |app, _| app.history_width);
            cx.simulate_resize(size(px(width), px(height)));
            requested_frames(cx, 4);
            let shown = history_geometry(&app, cx);
            assert_ne!(shown.0.0, before, "{at}: History took the new width");
            assert_eq!(
                cx.update(|window, cx| window.simulate_next_frame(cx)),
                0,
                "{at}: settled, the window asks for nothing"
            );
            draw(cx);
            assert_eq!(
                history_geometry(&app, cx),
                shown,
                "{at}: the frames the resize asked for laid History out"
            );
            assert_eq!(requested_frames(cx, 0), 0, "{at}: still idle");
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
    /// Three branches from one root, `alpha`, `beta` and `main` a commit
    /// ahead of it each, and an octopus merge of the three on `main`: History
    /// draws three lanes side by side.
    fn lanes_script() -> String {
        let mut input = String::new();
        for (mark, reference, message, parents) in [
            (1, "main", "Root", &[][..]),
            (2, "alpha", "Alpha work", &[1][..]),
            (3, "beta", "Beta work", &[1][..]),
            (4, "main", "Main work", &[][..]),
            (5, "main", "Merge alpha and beta", &[2, 3][..]),
        ] {
            input.push_str(&format!(
                "commit refs/heads/{reference}\nmark :{mark}\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{message}\n",
                1_700_000_000 + mark,
                message.len(),
            ));
            for (position, parent) in parents.iter().enumerate() {
                if mark == 5 || position > 0 {
                    input.push_str(&format!("merge :{parent}\n"));
                } else {
                    input.push_str(&format!("from :{parent}\n"));
                }
            }
            input.push_str(&format!("M 100644 inline notes.txt\ndata 2\n{mark}\n\n"));
        }
        input + "done\n"
    }

    /// Narrow History's graph draws a lane whole or not at all, and keeps
    /// every lane the subject's floor leaves room for: at 461 × 490 and
    /// 640 × 680, at 13 and 18 pt, the graph column holds each lane whose
    /// node and half a pitch past it fit, the next lane's would not fit even
    /// with the subject at its floor, the column is no wider than its lanes'
    /// full margin, every node drawn lies whole inside its row's graph cell,
    /// unclipped, and a node is drawn for every row whose lane the column
    /// holds and for no other. The graph was first given whatever the floor
    /// left, about 29 px at 461 × 490 and 13 pt, and drew the next lanes'
    /// nodes cut at its edge; then it kept only lanes with their full 10 px
    /// margin, so at 18 pt in a 640 px window on Linux the third lane, whose
    /// node fitted with half a pitch to spare, lost its nodes for about a
    /// character of subject. The unit tests' text system leaves other room
    /// than IBM Plex Sans, so the lane spacings of 14 and 16 px, beside the
    /// default 20, put such a lane in the room at 461 and 640 px.
    #[gpui::test]
    async fn narrow_history_draws_whole_lanes_only(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &lanes_script()).await;
        assert_eq!(app.read_with(cx, |app, _| app.graph_lanes), 3);
        let rows = app.read_with(cx, |app, _| {
            app.visible
                .iter()
                .map(|&index| (index, app.graph[index].lane))
                .collect::<Vec<_>>()
        });
        assert!(rows.iter().any(|(_, lane)| *lane == 2), "{rows:?}");
        // Lanes kept with less than their full margin, by half a pitch.
        let mut short = Vec::new();
        let cases = [20, 14, 16]
            .into_iter()
            .flat_map(|spacing| [13, 18].map(|text| (spacing, text)));
        for (spacing, text) in cases {
            app.update(cx, |app, cx| {
                app.settings.graph_spacing = spacing;
                cx.notify();
            });
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for (width, height) in [(461., 490.), (640., 680.)] {
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{width} × {height} at {text} pt, {spacing} px lanes");
                let (layout, offset) = cx.update(|_, cx| {
                    let app = app.read(cx);
                    let rows = app.history_rows(cx);
                    (app.history_column_layout(), rows.lane_offset)
                });
                assert!(layout.narrow, "{at}");
                let pitch = graph::compact_spacing(app.read_with(cx, |app, _| app.lane_spacing()));
                let (floor, pill) = app.read_with(cx, |app, _| {
                    let metrics = app.history_metrics.get();
                    (metrics.subject_floor, metrics.pill_room())
                });
                let width_of = |id| {
                    layout
                        .columns
                        .iter()
                        .find(|column| column.id == id)
                        .map(|column| column.width)
                        .unwrap()
                };
                let graph_width = width_of(ColumnId::Graph);
                let column = cx.debug_bounds("history-column-1").unwrap();
                assert!(
                    // Layout rounds the column to half a device pixel.
                    (f32::from(column.size.width) - graph_width).abs() <= 0.5,
                    "the graph column of {graph_width} px {at}: {column:?}"
                );
                // A lane's node, its 10 px margin and a pitch per lane in,
                // and what the cell needs past it: half a pitch, and at the
                // narrowest pitch the selected node's 5 px radius.
                let node = |lane: usize| 10. + lane as f32 * pitch;
                let reach = (pitch / 2.).max(5.);
                let held = (0..3)
                    .take_while(|&lane| node(lane) + reach <= graph_width + 1e-3)
                    .count()
                    .max(1);
                if node(held - 1) + 10. > graph_width + 1e-3 {
                    short.push(at.clone());
                }
                // The next lane's node and half a pitch would not fit even
                // with the subject at its floor beside the pill.
                let room = graph_width + width_of(ColumnId::Subject) - floor - pill;
                if offset + held < 3 {
                    assert!(
                        node(held) + reach > room + 1e-3,
                        "lane {} fits whole in {room} px beside the subject's floor, \
                         yet the graph holds {held} in {graph_width} px {at}",
                        offset + held
                    );
                }
                assert!(
                    graph_width <= graph::required_width(held, pitch) + 1e-3,
                    "the room short of lane {} is the subject's: {graph_width} px for {held} \
                     lanes {at}",
                    offset + held
                );
                if (width, text) == (461., 13) {
                    // The third lane, which the graph once cut in half here.
                    assert!(offset + held < 3, "{held} lanes from {offset} {at}");
                }
                let list = cx.debug_bounds("history-pane").unwrap();
                let mut beyond = 0;
                for &(row, lane) in &rows {
                    // The rows the list shows whole.
                    let Some(cell) = bounds_of(
                        cx,
                        format!("history-cell-{row}-{}", ColumnId::Graph as usize),
                    )
                    .filter(|cell| cell.top() >= list.top() && cell.bottom() <= list.bottom()) else {
                        continue;
                    };
                    beyond += usize::from(lane >= offset + held);
                    let nodes: Vec<_> = cx.update(|window, _| {
                        let scale = window.scale_factor();
                        let logical = |device: gpui_kit::ScaledPixels| device.as_f32() / scale;
                        window
                            .painted_quads()
                            .into_iter()
                            .filter(|quad| {
                                let (w, h) = (
                                    logical(quad.bounds.size.width),
                                    logical(quad.bounds.size.height),
                                );
                                let y = logical(quad.bounds.top()) + h / 2.;
                                let x = logical(quad.bounds.left()) + w / 2.;
                                (w - h).abs() < 0.6
                                    && (7.5..=10.1).contains(&w)
                                    && (logical(quad.corner_radii.top_left) - w / 2.).abs() < 0.6
                                    && f32::from(cell.top()) <= y
                                    && y < f32::from(cell.bottom())
                                    && f32::from(cell.left()) - 10. <= x
                                    && x <= f32::from(cell.right()) + 10.
                            })
                            .map(|quad| (quad.bounds, quad.content_mask.bounds, scale))
                            .collect()
                    });
                    let shown = (offset..offset + held).contains(&lane);
                    assert_eq!(
                        nodes.len(),
                        usize::from(shown),
                        "row {row} on lane {lane}, lanes {offset}..{} held {at}: {nodes:?}",
                        offset + held
                    );
                    for (node, mask, scale) in nodes {
                        let left = node.left().as_f32() / scale;
                        let right = node.right().as_f32() / scale;
                        assert!(
                            left >= f32::from(cell.left()) - 0.01
                                && right <= f32::from(cell.right()) + 0.01
                                && node.left() >= mask.left()
                                && node.right() <= mask.right(),
                            "row {row}'s node {left}–{right} whole in {cell:?} {at}"
                        );
                    }
                }
                if held < 3 {
                    assert!(
                        beyond > 0,
                        "a shown row on a lane the column cannot hold {at}"
                    );
                }
            }
        }
        assert!(
            short.len() >= 3,
            "lanes kept short of their full margin, by half a pitch: {short:?}"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

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
    pub(crate) fn bounds_of(
        cx: &mut VisualTestContext,
        selector: String,
    ) -> Option<Bounds<Pixels>> {
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

    /// The bounds `bounds` takes as an AccessKit node: physical window pixels.
    fn node_bounds(cx: &mut VisualTestContext, bounds: Bounds<Pixels>) -> gpui::accesskit::Rect {
        let scale = cx.update(|window, _| f64::from(window.scale_factor()));
        gpui::accesskit::Rect {
            x0: f64::from(f32::from(bounds.left())) * scale,
            y0: f64::from(f32::from(bounds.top())) * scale,
            x1: f64::from(f32::from(bounds.right())) * scale,
            y1: f64::from(f32::from(bounds.bottom())) * scale,
        }
    }

    /// The scrolling container whose element ID and debug selector are `id`
    /// is a node of `role` that clips its children, at the part `visible` of
    /// it that the window draws.
    fn assert_clipping(
        cx: &mut VisualTestContext,
        id: &'static str,
        role: Role,
        visible: Bounds<Pixels>,
        at: &str,
    ) {
        let node = crate::native_accessibility::clipping_node(id)
            .unwrap_or_else(|| panic!("{at}: {id} is no clipping node"));
        assert_eq!(node.role(), role, "{at}: {id}");
        assert!(node.clips_children(), "{at}: {id} clips its children");
        let expected = node_bounds(cx, visible);
        let bounds = node.bounds().expect("bounds");
        let close = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(
            close(bounds.x0, expected.x0)
                && close(bounds.y0, expected.y0)
                && close(bounds.x1, expected.x1)
                && close(bounds.y1, expected.y1),
            "{at}: {id} reports {bounds:?}, the window draws {expected:?}"
        );
    }

    /// History's commit list and changed-file list are nodes that clip their
    /// rows, at the part of themselves the window draws. The commit table
    /// runs sideways past its viewport, under the inspector and past the
    /// window's edge, and the list reported all of it: its extents ran
    /// 82.5 px past a 461 × 490 window and 42.5 px past a 640 × 680 one.
    /// Every row the list renders lies at least partly in what it reports,
    /// so each stays reachable; both lists keep their role.
    #[gpui::test]
    async fn history_lists_report_the_part_the_window_draws(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(40, 3)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        for (width, height) in [(461., 490.), (640., 680.), (1480., 800.)] {
            crate::native_accessibility::forget_clipping_nodes();
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{width} × {height}");
            let window = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
            let pane = cx.debug_bounds("history-pane").unwrap();
            // The list's edge is drawn over the part of it the viewport shows.
            let shown = pane
                .intersect(&cx.debug_bounds("history-list-edge").unwrap())
                .intersect(&window);
            if width < 1000. {
                assert!(
                    pane.right() > window.right(),
                    "{at}: the table {pane:?} runs past the window"
                );
            }
            assert_clipping(cx, "history-pane", Role::ListBox, shown, &at);
            let rows: Vec<_> = (0..40)
                .filter_map(|index| bounds_of(cx, format!("history-row-{index}")))
                .collect();
            assert!(rows.len() > 2, "{at}: rows render");
            for row in rows {
                assert!(
                    row.intersects(&shown),
                    "{at}: the row {row:?} is in {shown:?}"
                );
            }
            let files = cx.debug_bounds("files-pane").unwrap();
            assert_clipping(
                cx,
                "files-pane",
                Role::ListBox,
                files.intersect(&window),
                &at,
            );
        }
    }

    /// No control in the inspector's commit details is reachable wholly
    /// outside the part of the details the window draws. At 13 pt in a
    /// 461 × 490 window, in History and Compare, AT-SPI reached "Compare
    /// against parent 1" and "Selected commit actions" at y 346 and 380.5,
    /// below the details' visible bottom at 322.5, where the window draws the
    /// Changed files header: their group, "Commit author and comparison
    /// parents", was partly drawn and clipped nothing, and AccessKit compares
    /// a child only with its nearest clipping parent. Actions has since moved
    /// to the message's toolbar; the parents remain in the group.
    #[gpui::test]
    async fn commit_details_reach_only_the_controls_drawn(cx: &mut TestAppContext) {
        use crate::native_accessibility::{reachable, written};
        use gpui::accesskit::{Node, Rect};

        let (_fixture, app, cx) = history_window_from(cx, &history_script(40, 3)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        let meets = |a: Rect, b: Rect| !a.intersect(b).is_empty();
        // Actions now sits in the message's toolbar, which stays in view;
        // the group holds the parents.
        let controls = [("commit-parent-0", "Compare against parent 1")];
        for page in ["History", "Compare"] {
            if page == "Compare" {
                cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
                settle(&app, cx).await;
            }
            for (text, width, height) in [(13, 461., 490.), (18, 461., 490.), (13, 640., 490.)] {
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                written::forget();
                draw(cx);
                let at = format!("{page} at {text} pt in {width} × {height}");
                let window = node_bounds(
                    cx,
                    Bounds::new(point(px(0.), px(0.)), size(px(width), px(height))),
                );
                let viewport = cx
                    .debug_bounds("commit-message-viewport")
                    .unwrap_or_else(|| panic!("{at}: the details are drawn"));
                // The part of the details the window draws, from layout alone.
                let drawn = node_bounds(cx, viewport).intersect(window);
                let Some(group) = cx.debug_bounds("commit-metadata") else {
                    // A message long enough keeps the metadata out of view.
                    continue;
                };
                // The tree as GPUI builds it: the details' document, which
                // clips nothing, holding the group, which clips where it is
                // wrapped and is otherwise the node GPUI writes from layout.
                let mut document = Node::new(Role::Document);
                document.set_bounds(node_bounds(cx, viewport));
                let mut tree = written::Assembly::default()
                    .node("commit-message-viewport", document, None)
                    .written("commit-metadata", Some("commit-message-viewport"));
                if !tree.has("commit-metadata") {
                    let mut node = Node::new(Role::Group);
                    node.set_label("Commit author and comparison parents");
                    node.set_bounds(node_bounds(cx, group));
                    tree = tree.node("commit-metadata", node, Some("commit-message-viewport"));
                }
                let mut placed = Vec::new();
                for (selector, label) in controls {
                    let laid_out = cx
                        .debug_bounds(selector)
                        .unwrap_or_else(|| panic!("{at}: {label} is laid out"));
                    let bounds = node_bounds(cx, laid_out);
                    let mut control = Node::new(Role::Button);
                    control.set_label(label);
                    control.set_bounds(bounds);
                    tree = tree.node(selector, control, Some("commit-metadata"));
                    placed.push((label, bounds));
                }
                if text == 13 && width == 461. {
                    assert!(
                        placed.iter().all(|(_, bounds)| bounds.y0 >= drawn.y1),
                        "{at}: the controls lie below the details' visible bottom at {}: \
                         {placed:?}",
                        drawn.y1
                    );
                }
                let found = reachable(tree.tree(window, None));
                for (index, (label, bounds)) in placed.iter().enumerate() {
                    if meets(*bounds, drawn)
                        || !found
                            .iter()
                            .any(|node| node.label.as_deref() == Some(*label))
                    {
                        continue;
                    }
                    // AccessKit keeps the first control past each edge.
                    let neighbour = [index.checked_sub(1), Some(index + 1)]
                        .into_iter()
                        .flatten()
                        .filter_map(|other| placed.get(other))
                        .any(|(_, other)| meets(*other, drawn));
                    assert!(
                        neighbour,
                        "{at}: {label} at {bounds:?} is reached, wholly outside the part of \
                         the details the window draws, {drawn:?}"
                    );
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Each History row's node reports the part of the row the list draws,
    /// inside the list's own node. A row is as wide as the commit table,
    /// which runs sideways under the inspector and past the window's edge,
    /// and AT-SPI reported each row to x 542.5 in a 461 px window at 13 pt,
    /// 630 at 18 pt and 681.5 in a 640 px window, across the inspector.
    #[gpui::test]
    async fn history_rows_report_the_part_the_list_draws(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &history_script(40, 3)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        for (text, width, height) in [(13, 461., 490.), (18, 461., 490.), (13, 640., 680.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            crate::native_accessibility::forget_clipping_nodes();
            draw(cx);
            let at = format!("{text} pt at {width} × {height}");
            let window = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
            let list = crate::native_accessibility::clipping_node("history-pane")
                .unwrap_or_else(|| panic!("{at}: the list is a clipping node"))
                .bounds()
                .expect("bounds");
            let inside = |row: gpui::accesskit::Rect| {
                row.x0 >= list.x0 - 0.01
                    && row.y0 >= list.y0 - 0.01
                    && row.x1 <= list.x1 + 0.01
                    && row.y1 <= list.y1 + 0.01
            };
            let mut rows = 0;
            for index in 0..40 {
                let Some(layout) = bounds_of(cx, format!("history-row-{index}")) else {
                    continue;
                };
                rows += 1;
                assert!(
                    layout.right() > window.right(),
                    "{at}: row {index} at {layout:?} runs past the window"
                );
                let node = crate::native_accessibility::clipping_node(&format!("commit-{index}"))
                    .unwrap_or_else(|| panic!("{at}: row {index} writes its node"));
                assert_eq!(node.role(), Role::ListBoxOption, "{at}: row {index}");
                let bounds = node.bounds().expect("bounds");
                assert!(
                    inside(bounds),
                    "{at}: row {index} reports {bounds:?}, outside the list's {list:?}"
                );
                // The row from its own left edge to where the list's visible
                // part ends, and no taller than itself: only the list cuts it.
                let whole = node_bounds(cx, layout);
                assert!(
                    (bounds.x0 - whole.x0).abs() < 0.01
                        && (bounds.x1 - list.x1).abs() < 0.01
                        && bounds.y0 >= whole.y0 - 0.01
                        && bounds.y1 <= whole.y1 + 0.01
                        && bounds.y1 > bounds.y0,
                    "{at}: row {index} reports {bounds:?}, the row is {whole:?} in the list's \
                     {list:?}"
                );
            }
            assert!(rows > 2, "{at}: rows render");
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Each rendered row of the inspector's Changed files list reports the
    /// part of itself the list draws, inside the list's own node and the
    /// window. The list renders the row its foot cuts, and AT-SPI reported
    /// that row whole past the list (a list at y 782–928 with a row at
    /// 872–960, at 13 pt in a 461 px window) and, at 18 pt, past the foot of
    /// a 980 px window.
    #[gpui::test]
    async fn changed_file_rows_report_the_part_the_list_draws(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &files_script(3, 30)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        let row_height = app.read_with(cx, |app, _| px(app.settings.density.file_row_height()));
        let mut cut = 0;
        // History's inspector, then Compare's, which keeps the list beside
        // the file it compares, at the window sizes the native reviews read.
        for (compare, text, width, height) in [
            (false, 13, 461., 980.),
            (false, 18, 461., 980.),
            (false, 13, 461., 490.),
            (false, 18, 461., 490.),
            (false, 18, 1000., 680.),
            (true, 13, 461., 490.),
            (true, 18, 461., 490.),
            (true, 18, 1000., 680.),
        ] {
            if compare && app.read_with(cx, |app, _| app.mode != WorkspaceMode::Compare) {
                cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
                settle(&app, cx).await;
            }
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            crate::native_accessibility::forget_clipping_nodes();
            draw(cx);
            let page = if compare { "Compare" } else { "History" };
            let at = format!("{page} at {text} pt in {width} × {height}");
            let window = node_bounds(
                cx,
                Bounds::new(point(px(0.), px(0.)), size(px(width), px(height))),
            );
            let list = crate::native_accessibility::clipping_node("files-pane")
                .unwrap_or_else(|| panic!("{at}: the list is a clipping node"))
                .bounds()
                .expect("bounds");
            let inside = |row: gpui::accesskit::Rect, outer: gpui::accesskit::Rect| {
                row.x0 >= outer.x0 - 0.01
                    && row.y0 >= outer.y0 - 0.01
                    && row.x1 <= outer.x1 + 0.01
                    && row.y1 <= outer.y1 + 0.01
            };
            let scale = cx.update(|window, _| f64::from(window.scale_factor()));
            let mut rows = 0;
            for index in 0..30 {
                let Some(node) =
                    crate::native_accessibility::written::get(&format!("file-{index}"))
                else {
                    continue;
                };
                rows += 1;
                let node = node.node;
                assert!(
                    node.clips_children(),
                    "{at}: row {index} clips what it holds"
                );
                assert_eq!(node.role(), Role::ListBoxOption, "{at}: row {index}");
                let bounds = node.bounds().expect("bounds");
                assert!(
                    inside(bounds, list) && inside(bounds, window),
                    "{at}: row {index} reports {bounds:?}, outside the list's {list:?} or the \
                     window"
                );
                if bounds.y1 - bounds.y0 < f64::from(f32::from(row_height)) * scale - 0.5 {
                    cut += 1;
                }
            }
            assert!(rows > 1, "{at}: the rows write nodes that clip");
        }
        assert!(cut > 0, "the list cuts a row at some size");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Each piece of the inspector's commit message reports the part of
    /// itself the message's viewport draws. The message is a document read
    /// as a whole, which clips nothing, and its title reported its layout
    /// bounds: AT-SPI gave it y 372–614 in a viewport ending at 558, over
    /// the "Changed files" heading, at 18 pt in a 461 px window.
    #[gpui::test]
    async fn message_pieces_report_the_part_the_details_draw(cx: &mut TestAppContext) {
        let subject = "Update the layout notes so the inspector's title wraps over several \
                       lines in a half tile";
        let body = "The first paragraph of the body explains the change.\n\n\
                    The second paragraph lies below the fold.";
        let message = format!("{subject}\n\n{body}");
        let input = format!(
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> \
             1700000000 +0000\ndata {}\n{message}\nM 100644 inline notes.txt\ndata 6\nnotes\n\
             done\n",
            message.len()
        );
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        let mut cut = 0;
        for (text, width, height) in [(13, 461., 490.), (18, 461., 490.), (13, 640., 490.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            crate::native_accessibility::forget_clipping_nodes();
            draw(cx);
            let at = format!("{text} pt in {width} × {height}");
            let window = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
            let viewport = cx
                .debug_bounds("commit-message-viewport")
                .unwrap_or_else(|| panic!("{at}: the message is drawn"));
            let drawn = node_bounds(cx, viewport.intersect(&window));
            let mut pieces = 0;
            for index in 0..8 {
                let Some(layout) = bounds_of(cx, format!("commit-message-piece-{index}")) else {
                    continue;
                };
                pieces += 1;
                let node = crate::native_accessibility::written::get(&format!(
                    "commit-message-piece-{index}"
                ))
                .unwrap_or_else(|| panic!("{at}: piece {index} writes its node"))
                .node;
                assert_eq!(node.role(), Role::Label, "{at}: piece {index}");
                assert!(
                    node.value().is_some(),
                    "{at}: piece {index} is read by its text"
                );
                let bounds = node.bounds().expect("bounds");
                assert!(
                    bounds.x0 >= drawn.x0 - 0.01
                        && bounds.y0 >= drawn.y0 - 0.01
                        && bounds.x1 <= drawn.x1 + 0.01
                        && bounds.y1 <= drawn.y1 + 0.01,
                    "{at}: piece {index} reports {bounds:?}, outside the part of the message \
                     the window draws, {drawn:?}"
                );
                if node_bounds(cx, layout).y1 > drawn.y1 + 0.5 {
                    cut += 1;
                }
            }
            assert!(pieces > 0, "{at}: the message draws its pieces");
        }
        assert!(cut > 0, "the viewport cuts a piece at some size");
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The Working inspector's file list, its composer's scrolling fields
    /// and, where it shows, its scrolling selection row are nodes that clip
    /// what they hold, at the part of themselves the window draws: a control
    /// one of them scrolls out of view is not reported where it is not
    /// drawn. The list keeps its role; the others are scroll views. A region
    /// the inspector leaves out writes no node.
    #[gpui::test]
    async fn working_regions_report_the_part_the_window_draws(cx: &mut TestAppContext) {
        let (_fixture, _app, cx) = working_window(cx).await;
        let regions = [
            ("working-pane", Role::ListBox),
            ("commit-composer-fields", Role::ScrollView),
            ("working-selection", Role::ScrollView),
        ];
        for (text, width, height) in [
            (13, 461., 490.),
            (18, 461., 490.),
            (13, 640., 680.),
            (13, 1480., 980.),
        ] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            // The inspector chooses its form from measured heights; read the
            // frame after it holds still.
            crate::native_accessibility::forget_clipping_nodes();
            draw(cx);
            let at = format!("{text} pt at {width} × {height}");
            let window = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
            for (id, role) in regions {
                match cx.debug_bounds(id) {
                    Some(bounds) => assert_clipping(cx, id, role, bounds.intersect(&window), &at),
                    None => assert!(
                        crate::native_accessibility::clipping_node(id).is_none(),
                        "{at}: {id} is not drawn, and writes no node"
                    ),
                }
            }
            for id in ["working-pane", "commit-composer-fields"] {
                assert!(cx.debug_bounds(id).is_some(), "{at}: {id} is drawn");
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// In a 461 × 490 half tile and a 640 × 680 window, at 13 and 18 pt,
    /// History is too narrow for the chosen References and Graph to leave
    /// the subject anything readable. The subject's cell keeps at least its
    /// floor, about 120 px scaled with the interface size, inside the
    /// viewport; References fold into one pill at its start, HEAD's branch
    /// and "+3" or a branch glyph and the count, in the badge's colours, with
    /// the whole list in the pill's tooltip and the row's accessible name;
    /// and the graph takes its compact pitch, its selected node on its lane
    /// and centred in its row.
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
                    let metrics = app.history_metrics.get();
                    (
                        metrics.subject_floor,
                        // The viewport from where the columns start, less
                        // the compact pill's room at the subject's start.
                        app.history_width - metrics.leading - metrics.pill_room(),
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

                // One pill at the subject's start, at most half its text: the
                // badge where half the cell's text room holds the text's
                // floor, as at 640 px and 13 pt, and otherwise the compact
                // pill, a branch glyph and the number of references.
                let compact = (width, text) != (640., 13);
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
                let (shown, count) = if compact {
                    ((None, Some("4".to_owned()), label), Some("1".to_owned()))
                } else {
                    (
                        (Some("main".to_owned()), Some("+3".to_owned()), label),
                        None,
                    )
                };
                assert_eq!(
                    HISTORY_PILLS.with(|pills| pills.borrow().get(&merge).cloned()),
                    Some(shown),
                    "HEAD's branch and the others' count, or the count {at}"
                );
                assert_eq!(
                    HISTORY_PILLS
                        .with(|pills| pills.borrow().get(&side).map(|pill| pill.1.clone())),
                    Some(count),
                    "a lone reference has no \"+N\" {at}"
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

                // The graph at half the pitch, within what the floor and the
                // pill leave.
                let compact = graph::compact_spacing(spacing);
                assert!(compact < spacing, "{at}");
                // In whole lanes: the room short of the next is the subject's.
                let expected = graph::fitted_width(lanes, available - floor, compact);
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
                // Where the graph holds one lane, narrowing revealed lane 1,
                // so the frame's lanes start there.
                let offset = cx.update(|_, cx| app.read(cx).history_rows(cx).lane_offset);
                assert!(
                    (x - (left + 10. + (1 - offset) as f32 * compact)).abs() <= 0.25
                        && x - 5. >= left
                        && x + 5. <= right,
                    "the selected node at x {x}, whole, on lane 1 of {left}–{right} from lane \
                     {offset} {at}"
                );
                assert!(
                    (y - f32::from(side_cell.center().y)).abs() < 0.5,
                    "the selected node at y {y} beside its subject {side_cell:?} {at}"
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The floor is on the subject's own text. At 461 × 490 and 640 × 680, at
    /// 13 and 18 pt, every row's subject text beside its pill is at least as
    /// wide as its header's whole title, inside the viewport. Where the badge
    /// would leave it less, the pill is compact: a branch glyph and the
    /// count, about 24 px at 13 pt, no wider than the room the layout keeps
    /// for it. The badge, in narrow History and in the References column
    /// alike, counts the others "+N" inside its chip.
    #[gpui::test]
    async fn narrow_history_folds_references_and_keeps_the_subject_floor(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        let merge = commit_index(&app, cx, MERGE_SUBJECT);
        let side = commit_index(&app, cx, SIDE_SUBJECT);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - px(0.5) && inner.right() <= outer.right() + px(0.5)
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            let scale = appearance::ui_scale();
            for (width, height) in [(461., 490.), (640., 680.)] {
                HISTORY_PILLS.with(|pills| pills.borrow_mut().clear());
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{width} × {height} at {text} pt");
                let metrics = app.read_with(cx, |app, _| app.history_metrics.get());
                let floor = metrics.text_floor();
                assert!(
                    (85. * scale..=100. * scale).contains(&floor),
                    "the title's width, about 93 px scaled {at}: {floor}"
                );
                let view = cx.debug_bounds("history-scope-toolbar").unwrap();
                let compact = (width, text) != (640., 13);
                for (row, count) in [(merge, "4"), (side, "1")] {
                    let subject = bounds_of(cx, format!("history-subject-text-{row}")).unwrap();
                    let pill = bounds_of(cx, format!("history-ref-pill-{row}")).unwrap();
                    assert!(
                        subject.size.width >= px(floor) - px(0.5) && inside(subject, view),
                        "row {row}'s subject text {subject:?} keeps {floor} in {view:?} {at}"
                    );
                    let shown = HISTORY_PILLS.with(|pills| pills.borrow().get(&row).cloned());
                    if compact {
                        assert_eq!(
                            shown.map(|pill| (pill.0, pill.1)),
                            Some((None, Some(count.to_owned()))),
                            "the glyph and the count, without the name {at}"
                        );
                        assert!(
                            pill.size.width <= px(metrics.compact_pill) + px(0.5)
                                && (20. * scale..=28. * scale)
                                    .contains(&f32::from(pill.size.width)),
                            "a compact pill of about 24 px scaled {at}: {pill:?}"
                        );
                    } else {
                        assert!(shown.is_some_and(|pill| pill.0.is_some()), "the badge {at}");
                    }
                }
                if !compact {
                    let pill = bounds_of(cx, format!("history-ref-pill-{merge}")).unwrap();
                    let more = bounds_of(cx, format!("history-ref-more-{merge}")).unwrap();
                    assert!(inside(more, pill), "\"+3\" inside {pill:?}: {more:?} {at}");
                }
            }
        }
        // The References column's badge counts the others inside its chip too.
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        assert!(!app.read_with(cx, |app, _| app.history_column_layout().narrow));
        let badge = bounds_of(cx, format!("history-ref-pill-{merge}")).unwrap();
        let more = bounds_of(cx, format!("history-ref-more-{merge}")).unwrap();
        assert!(inside(more, badge), "\"+3\" inside {badge:?}: {more:?}");
        assert!(inside(badge, cx.debug_bounds("history-column-0").unwrap()));
    }

    /// Every History row with references names them after its subject in
    /// one form, beside the References column's badge at 1480 × 980 and
    /// 1000 × 680 as beside narrow History's pill at 461 × 490, so assistive
    /// technology hears the references whatever the window's width; the
    /// badge's tooltip lists them too. A row without references names none.
    #[gpui::test]
    async fn every_history_row_names_its_references(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        let merge = commit_index(&app, cx, MERGE_SUBJECT);
        let side = commit_index(&app, cx, SIDE_SUBJECT);
        let plain = commit_index(&app, cx, "Main work");
        for (width, height, narrow) in [
            (1480., 980., false),
            (1000., 680., false),
            (461., 490., true),
        ] {
            HISTORY_ROW_NAMES.with(|names| names.borrow_mut().clear());
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{width} × {height}");
            assert_eq!(
                app.read_with(cx, |app, _| app.history_column_layout().narrow),
                narrow,
                "{at}"
            );
            assert_eq!(
                cx.debug_bounds("history-column-0").is_some(),
                !narrow,
                "the References column {at}"
            );
            let row = |row: usize| {
                HISTORY_ROW_NAMES
                    .with(|names| names.borrow().get(&row).cloned())
                    .unwrap()
            };
            let (name, tooltip) = row(merge);
            assert!(
                name.starts_with(&format!(
                    "{MERGE_SUBJECT} · References: main, feature/alpha, release, origin/main · Fixture · "
                )) && name.ends_with(" · 2 parents · merge commit"),
                "{name} {at}"
            );
            assert_eq!(
                tooltip.as_deref(),
                Some("main\nfeature/alpha\nrelease\norigin/main"),
                "{at}"
            );
            let (name, _) = row(side);
            assert!(
                name.starts_with(&format!("{SIDE_SUBJECT} · References: side · Fixture · ")),
                "{name} {at}"
            );
            let (name, tooltip) = row(plain);
            assert!(
                name.starts_with("Main work · Fixture · ") && tooltip.is_none(),
                "{name} {at}"
            );
        }
    }

    /// [`references_script`] with a lightweight tag, `v0.1-demo`, on the
    /// root, as the demo fixture has, and an annotated one, `v1.0`, on the
    /// merge.
    fn tagged_references_script() -> String {
        let script = references_script(2);
        let script = script.strip_suffix("done\n").unwrap();
        let merge = 2 + 3;
        format!(
            "{script}reset refs/tags/v0.1-demo\nfrom :1\n\ntag v1.0\nfrom :{merge}\ntagger Fixture <fixture@example.invalid> 1700000100 +0000\ndata 7\nRelease\ndone\n"
        )
    }

    /// History shows tags as references, after a commit's branches. A row
    /// with tags alone shows its first tag in the badge, led by the tag
    /// glyph, in the badge's lane tint and label colour; in narrow History's
    /// compact pill the tag glyph leads the count. A row with branches leads
    /// with HEAD's branch and counts its tags in "+N" and the compact count.
    /// The tooltip and the row's accessible name say "tag ‹name›". At
    /// 1000 × 680, and narrow at 461 and 640 px wide.
    #[gpui::test]
    async fn history_shows_tags_as_references(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &tagged_references_script()).await;
        cx.update(|_, cx| crate::appearance::ThemeChoice::Midnight.apply(None, cx));
        let merge = commit_index(&app, cx, MERGE_SUBJECT);
        let root = commit_index(&app, cx, "Linear 1");
        let colors = |cx: &mut VisualTestContext, row: usize| {
            cx.update(|_, cx| {
                let lane = app.read(cx).graph[row].color % 6;
                (
                    appearance::labels(cx).refs[lane],
                    gpui_kit::Background::from(gpui_kit::Hsla::from(rgba(
                        (graph::colors(cx)[lane] << 8) | crate::appearance::REF_TINT_ALPHA,
                    ))),
                )
            })
        };
        let (root_label, root_tint) = colors(cx, root);
        let (merge_label, _) = colors(cx, merge);
        for (width, height, narrow, compact) in [
            (1000., 680., false, false),
            (640., 900., true, false),
            (461., 900., true, true),
        ] {
            HISTORY_PILLS.with(|pills| pills.borrow_mut().clear());
            HISTORY_ROW_NAMES.with(|names| names.borrow_mut().clear());
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let at = format!("{width} × {height}");
            assert_eq!(
                app.read_with(cx, |app, _| app.history_column_layout().narrow),
                narrow,
                "{at}"
            );
            let pill = |row: usize| HISTORY_PILLS.with(|pills| pills.borrow().get(&row).cloned());
            let row = |row: usize| {
                HISTORY_ROW_NAMES
                    .with(|names| names.borrow().get(&row).cloned())
                    .unwrap()
            };
            if compact {
                assert_eq!(
                    pill(root),
                    Some((None, Some("1".into()), root_label)),
                    "{at}"
                );
                assert_eq!(
                    pill(merge),
                    Some((None, Some("5".into()), merge_label)),
                    "{at}"
                );
            } else {
                assert_eq!(
                    pill(root),
                    Some((Some("v0.1-demo".into()), None, root_label)),
                    "{at}"
                );
                assert_eq!(
                    pill(merge),
                    Some((Some("main".into()), Some("+4".into()), merge_label)),
                    "{at}"
                );
            }
            // The tag glyph leads the root's chip, inside it, and no other.
            let chip = bounds_of(cx, format!("history-ref-pill-{root}")).unwrap();
            let glyph = bounds_of(cx, format!("history-ref-tag-{root}"))
                .unwrap_or_else(|| panic!("the tag glyph {at}"));
            assert!(
                glyph.left() >= chip.left()
                    && glyph.right() <= chip.right()
                    && glyph.left() - chip.left() < px(8.),
                "the glyph {glyph:?} leads {chip:?} {at}"
            );
            assert!(
                bounds_of(cx, format!("history-ref-tag-{merge}")).is_none(),
                "{at}"
            );
            assert_eq!(
                painted_fills(cx, format!("history-ref-pill-{root}").leak()),
                vec![root_tint],
                "the badge's tint {at}"
            );
            let (name, tooltip) = row(root);
            assert!(
                name.starts_with("Linear 1 · References: tag v0.1-demo · Fixture · "),
                "{name} {at}"
            );
            assert_eq!(tooltip.as_deref(), Some("tag v0.1-demo"), "{at}");
            let (name, tooltip) = row(merge);
            assert!(
                name.starts_with(&format!(
                    "{MERGE_SUBJECT} · References: main, feature/alpha, release, origin/main, \
                     tag v1.0 · Fixture · "
                )),
                "{name} {at}"
            );
            assert_eq!(
                tooltip.as_deref(),
                Some("main\nfeature/alpha\nrelease\norigin/main\ntag v1.0"),
                "{at}"
            );
        }
    }

    /// Narrow History's tooltip listing a row's references belongs to its
    /// pill: hovering the subject beside the pill shows none, so no tooltip
    /// covers the rows being read, and hovering the pill shows the list.
    #[gpui::test]
    async fn narrow_history_shows_references_on_the_pill_only(cx: &mut TestAppContext) {
        use gpui_kit::component::Theme;
        /// gpui-base's tooltip show delay.
        const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.history_column_layout().narrow));
        let merge = commit_index(&app, cx, MERGE_SUBJECT);
        let popover = cx.read(|cx| Background::from(Theme::global(cx).tokens.popover));
        let popups = move |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover)
                    .count()
            })
        };
        // The popups shown while the pointer rests at `position`; then the
        // pointer leaves the window until any tooltip has gone.
        let hover = |cx: &mut VisualTestContext, position: Point<Pixels>| {
            cx.simulate_mouse_move(position, None, Modifiers::default());
            cx.executor().advance_clock(SHOW_DELAY);
            draw(cx);
            let shown = popups(cx);
            cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
            cx.executor().advance_clock(SHOW_DELAY);
            draw(cx);
            shown
        };
        let resting = popups(cx);
        let cell = bounds_of(cx, format!("history-subject-{merge}")).unwrap();
        let pill = bounds_of(cx, format!("history-ref-pill-{merge}")).unwrap();
        assert!(
            pill.right() < cell.right() - px(24.),
            "{pill:?} in {cell:?}"
        );
        assert_eq!(
            hover(cx, point(cell.right() - px(16.), cell.center().y)),
            resting,
            "the subject beside the pill shows no tooltip"
        );
        assert_eq!(
            hover(cx, pill.center()),
            resting + 1,
            "the pill shows its references"
        );
    }

    /// In narrow History the subject ends at the table's viewport, in the
    /// header and in every row, at 461 × 490 and 640 × 680 and at 13 and
    /// 18 pt: the layout's width counts the inset before the columns, so no
    /// sliver of Author, Date or SHA shows at the viewport's edge, and the
    /// header's columns start where the rows' do.
    #[gpui::test]
    async fn narrow_history_ends_the_subject_at_the_viewport(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(2)).await;
        let rows = [
            commit_index(&app, cx, MERGE_SUBJECT),
            commit_index(&app, cx, SIDE_SUBJECT),
        ];
        let header = |id: ColumnId| format!("history-column-{}", id as usize);
        let cell = |row: usize, id: ColumnId| {
            if id == ColumnId::Subject {
                format!("history-subject-{row}")
            } else {
                format!("history-cell-{row}-{}", id as usize)
            }
        };
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            for (width, height) in [(461., 490.), (640., 680.)] {
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{width} × {height} at {text} pt");
                assert!(
                    app.read_with(cx, |app, _| app.history_column_layout().narrow),
                    "narrow {at}"
                );
                let view = cx.debug_bounds("history-scope-toolbar").unwrap();
                let rows = rows.map(|row| (format!("row {row}"), Some(row)));
                for (name, row) in [("the header".to_owned(), None)].into_iter().chain(rows) {
                    let bounds = |cx: &mut VisualTestContext, id| {
                        bounds_of(cx, row.map_or_else(|| header(id), |row| cell(row, id)))
                    };
                    let graph = bounds(cx, ColumnId::Graph).unwrap();
                    let subject = bounds(cx, ColumnId::Subject).unwrap();
                    assert!(
                        (graph.left()
                            - view.left()
                            - px(app.read_with(cx, |app, _| app.history_metrics.get().leading)))
                        .abs()
                            <= px(0.5),
                        "{name}'s graph starts after the leading inset {at}: {graph:?} in {view:?}"
                    );
                    assert!(
                        (subject.right() - view.right()).abs() <= px(0.5),
                        "{name}'s subject ends at the viewport's edge {at}: {subject:?} in {view:?}"
                    );
                    for id in [ColumnId::Author, ColumnId::Date, ColumnId::Sha] {
                        let column = bounds(cx, id).unwrap();
                        assert!(
                            column.left() >= view.right() - px(0.5),
                            "{name}'s {id:?} is beyond the viewport {at}: {column:?} in {view:?}"
                        );
                    }
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Narrowing the window keeps the selected commit in view: after a
    /// resize from 1480 × 980 to 461 × 490 the selected row, its subject and
    /// its graph node lie inside the list, both for a row deep in the list,
    /// which the narrower list would have left below its end, and for a
    /// commit on the second lane, which the narrow graph's single lane would
    /// have left out of the drawn lanes.
    #[gpui::test]
    async fn narrowing_history_keeps_the_selected_row_and_lane_in_view(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = history_window_from(cx, &references_script(60)).await;
        let deep = app.read_with(cx, |app, _| app.visible[18]);
        let side = commit_index(&app, cx, SIDE_SUBJECT);
        assert_eq!(app.read_with(cx, |app, _| app.graph[side].lane), 1);
        for row in [side, deep] {
            cx.simulate_resize(size(px(1480.), px(980.)));
            draw(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(row, window, cx)));
            settle(&app, cx).await;
            let subject = bounds_of(cx, format!("history-subject-{row}"));
            assert!(subject.is_some(), "row {row} shows at 1480 × 980");

            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            assert!(app.read_with(cx, |app, _| app.history_column_layout().narrow));
            let list = cx.debug_bounds("history-pane").unwrap();
            let view = cx.debug_bounds("history-scope-toolbar").unwrap();
            let whole = bounds_of(cx, format!("history-row-{row}"))
                .unwrap_or_else(|| panic!("row {row} is drawn after narrowing"));
            assert!(
                whole.top() >= list.top() + px(1.) && whole.bottom() <= list.bottom() - px(1.),
                "row {row}'s {whole:?} lies inside the list's edge {list:?}"
            );
            let subject = bounds_of(cx, format!("history-subject-{row}")).unwrap();
            assert!(
                subject.left() >= view.left() && subject.left() < view.right() - px(80.),
                "row {row}'s subject starts inside the viewport {view:?}: {subject:?}"
            );
            // The selected node, 5 px in radius, inside the row's graph cell.
            let graph = bounds_of(
                cx,
                format!("history-cell-{row}-{}", ColumnId::Graph as usize),
            )
            .unwrap();
            let nodes: Vec<_> = cx.update(|window, _| {
                let scale = window.scale_factor();
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| {
                        (quad.bounds.size.width.as_f32() / scale - 10.).abs() < 0.01
                            && (quad.corner_radii.top_left.as_f32() / scale - 5.).abs() < 0.01
                    })
                    .map(|quad| {
                        let left = px(quad.bounds.left().as_f32() / scale);
                        let right = px(quad.bounds.right().as_f32() / scale);
                        let center = px((quad.bounds.top().as_f32()
                            + quad.bounds.bottom().as_f32())
                            / 2.
                            / scale);
                        (left, right, center)
                    })
                    .filter(|(_, _, y)| graph.top() <= *y && *y < graph.bottom())
                    .collect()
            });
            assert!(
                nodes.iter().any(|(left, right, _)| {
                    *left >= graph.left() - px(0.5) && *right <= graph.right() + px(0.5)
                }),
                "row {row}'s selected node lies inside its graph cell {graph:?}: {nodes:?}"
            );
        }
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
    pub(crate) fn fixture_git(repository: &std::path::Path, args: &[&str]) {
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

    /// Gives the fixture repository at `repository` a commit identity of its
    /// own, the committer its history names. The app reads the repository's
    /// effective identity, a developer's global configuration included, and
    /// the identity lays out the composer: with one, the composer keeps room
    /// for its description and the Changes list gets less. A fixture without
    /// its own identity therefore drew one way under the gate's empty global
    /// configuration and another on a machine whose configuration names its
    /// developer. A test that needs no identity sets empty values, which
    /// win over global ones too.
    pub(crate) fn fixture_identity(repository: &std::path::Path) {
        fixture_git(repository, &["config", "user.name", "Fixture"]);
        fixture_git(
            repository,
            &["config", "user.email", "fixture@example.invalid"],
        );
    }

    /// A new, empty repository at `path` on `main`, with its own commit
    /// identity ([`fixture_identity`]).
    pub(crate) fn fixture_repository(path: impl AsRef<std::path::Path>) -> GitRepository {
        let repository = GitRepository::init(path, "main").unwrap();
        fixture_identity(repository.path());
        repository
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
            assert!(
                app.history_column_layout().content_width + app.history_metrics.get().insets()
                    > app.history_width
            );
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
