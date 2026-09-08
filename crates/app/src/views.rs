use crate::*;
use gpui_kit::base::ElementExt;

impl GitTurtle {
    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h(px(48.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(icon("turtle", 24., MINT))
            .child(
                div()
                    .w(px(260.))
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(13.))
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
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(
                                self.path
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or("A clearer view of your code".into()),
                            ),
                    ),
            )
            .child(
                button("open", "Open", "folder", false).on_click(cx.listener(
                    |this, _, window, cx| this.choose_repository(&OpenRepository, window, cx),
                )),
            )
            .child(div().flex_1())
            .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(
                if self.mode == WorkspaceMode::Compare {
                    "File comparison"
                } else {
                    "Repository history"
                },
            ))
            .child(
                button("refresh", "Refresh", "refresh", false).on_click(
                    cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(MINT))
                    .px_2()
                    .py_1()
                    .bg(rgb(SELECTED))
                    .rounded(px(4.))
                    .child("READ ONLY"),
            )
            .into_any_element()
    }

    pub(super) fn render_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(44.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .py_3()
            .gap_3()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                button("rail-history", "", "commit", true)
                    .accessibility_label("Back to history")
                    .tooltip("Back to history · Escape")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.mode == WorkspaceMode::Compare {
                            this.back_to_history(window, cx);
                        } else {
                            this.sidebar = true;
                            this.history_sidebar = true;
                            cx.notify();
                        }
                    })),
            )
            .child(
                button("rail-open", "", "folder", false)
                    .accessibility_label("Open repository")
                    .tooltip("Open repository")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_repository(&OpenRepository, window, cx)
                    })),
            )
            .child(div().flex_1())
            .child(icon("turtle", 18., MUTED))
            .into_any_element()
    }

    pub(super) fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .flex()
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
                            button(name, name, "", self.nav_mode == mode).on_click(cx.listener(
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
                    .child(Input::new(&self.nav_search).text_size(px(11.))),
            )
            .child(
                uniform_list(
                    "navigation",
                    self.nav_rows.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|i| this.render_nav_row(i, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h_0()
                .track_scroll(&self.nav_scroll),
            )
            .child(
                div()
                    .p_3()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child("Remote branches reflect local refs.\nRefresh never fetches."),
            )
            .into_any_element()
    }

    pub(super) fn render_nav_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = self.nav_rows[index].clone();
        if let NavRow::Section(label, count) = &row {
            return div()
                .w_full()
                .h(px(30.))
                .px_3()
                .pt_2()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(10.))
                .text_color(rgb(MUTED))
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
            let key = key.clone();
            return div()
                .id(("nav-folder", index))
                .role(Role::Button)
                .aria_label(format!(
                    "{} {} · {} branches",
                    if *expanded { "Collapse" } else { "Expand" },
                    label,
                    count
                ))
                .w_full()
                .h(px(30.))
                .pl(px(12. + *depth as f32 * 12.))
                .pr_3()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(12.))
                .text_color(rgb(MUTED))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(HOVER)))
                .child(div().w(px(12.)).child(if *expanded { "⌄" } else { "›" }))
                .child(icon("folder", 14., MUTED))
                .child(div().flex_1().truncate().child(label.clone()))
                .child(div().text_size(px(10.)).child(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.expanded_folders.remove(&key) {
                        this.expanded_folders.insert(key.clone());
                    }
                    this.rebuild_navigation(cx);
                    cx.notify();
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
        div()
            .id(("nav", index))
            .role(Role::ListBoxOption)
            .aria_label(tooltip.clone())
            .aria_selected(active)
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .w_full()
            .h(px(30.))
            .pl(px(12. + depth as f32 * 12.))
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .overflow_hidden()
            .cursor_pointer()
            .bg(rgb(if active { SELECTED } else { PANEL }))
            .hover(|s| s.bg(rgb(HOVER)))
            .text_color(rgb(if active { MINT } else { TEXT }))
            .child(icon(symbol, 15., if active { MINT } else { MUTED }))
            .child(div().flex_1().truncate().child(name))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.limit = 500;
                match &row {
                    NavRow::All => {
                        if let Some(path) = this.path.clone() {
                            this.open(path, None, window, cx);
                        }
                    }
                    NavRow::Branch(i, _) => {
                        if let Some(path) = this.path.clone() {
                            let branch = &this.branches[*i];
                            this.open(
                                path,
                                Some((
                                    branch.name.clone(),
                                    worker::Scope::Branch {
                                        name: branch.name.clone(),
                                        remote: branch.remote,
                                    },
                                )),
                                window,
                                cx,
                            );
                        }
                    }
                    NavRow::Worktree(i) => {
                        let tree = &this.worktrees[*i];
                        let path = tree.path.clone();
                        let scope = Some((
                            tree.branch.clone().unwrap_or("Detached worktree".into()),
                            worker::Scope::Worktree { path: path.clone() },
                        ));
                        this.open(path, scope, window, cx);
                    }
                    _ => {}
                }
            }))
            .into_any_element()
    }

    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let scope = self
            .scope
            .as_ref()
            .map(|s| s.0.clone())
            .unwrap_or("All history".into());
        let columns = history_columns(self.history_width, self.graph_width);
        let history = if self.commits.is_empty() && self.error.is_some() {
            empty(
                "Could not open repository",
                self.error.as_deref().unwrap_or_default(),
            )
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
                if self.repository.is_none() {
                    "Open a Git repository to explore branches, worktrees and changes."
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
        let mut header = div()
            .h(px(28.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .px_3()
            .border_l_2()
            .border_color(rgb(CANVAS))
            .border_b_1()
            .text_size(px(10.))
            .text_color(rgb(MUTED))
            .child(
                div()
                    .w(px(columns.refs))
                    .flex_shrink_0()
                    .child("BRANCH / REF"),
            )
            .child(div().w(px(columns.graph)).flex_shrink_0().child("GRAPH"))
            .child(div().flex_1().min_w_0().child("COMMIT"));
        if columns.author > 0. {
            header = header.child(
                div()
                    .w(px(columns.author))
                    .flex_shrink_0()
                    .pl_3()
                    .child("AUTHOR"),
            );
        }
        if columns.date > 0. {
            header = header.child(div().w(px(columns.date)).flex_shrink_0().child("DATE"));
        }
        if columns.hash > 0. {
            header = header.child(div().w(px(columns.hash)).flex_shrink_0().child("SHA"));
        }
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
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
                    .h(px(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(icon("branch", 15., MINT))
                    .child(
                        div()
                            .max_w(px(190.))
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(scope),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(format!("{} commits", self.visible.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        button(
                            "load-more",
                            if self.limit >= 10000 {
                                "10,000 limit"
                            } else {
                                "Load more"
                            },
                            "chevron",
                            false,
                        )
                        .disabled(
                            self.repository.is_none()
                                || self.commits.len() < self.limit
                                || self.limit >= 10000,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.limit = (this.limit + 500).min(10000);
                            if let Some(path) = this.path.clone() {
                                this.open(path, this.scope.clone(), window, cx);
                            }
                        })),
                    ),
            )
            .child(
                div()
                    .h(px(38.))
                    .flex_shrink_0()
                    .px_3()
                    .pb_2()
                    .pt_1()
                    .child(Input::new(&self.search).text_size(px(12.))),
            )
            .children(self.graph_notice.as_ref().map(|notice| {
                div()
                    .px_3()
                    .py_1()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(notice.clone())
            }))
            .child(header)
            .child(
                div()
                    .id("history-pane")
                    .role(Role::ListBox)
                    .aria_label("Commit history")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.focus)
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
                    .child(history),
            )
            .into_any_element()
    }

    pub(super) fn render_commit_row(&self, position: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.visible[position];
        let commit = &self.commits[index];
        let active = self.selected_commit == Some(index);
        let columns = history_columns(self.history_width, self.graph_width);
        let color = graph::COLORS[self.graph[index].color % graph::COLORS.len()];
        let mut references = div()
            .w(px(columns.refs))
            .flex_shrink_0()
            .pr_2()
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden();
        if let Some(names) = self.refs.get(&commit.oid) {
            if let Some(name) = names.first() {
                let title = names.join("\n");
                references = references.child(
                    div()
                        .id(("refs", index))
                        .min_w_0()
                        .max_w(px(columns.refs - 12.))
                        .truncate()
                        .px_1()
                        .py_0p5()
                        .rounded(px(3.))
                        .text_size(px(10.))
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
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child(format!("+{}", names.len() - 1)),
                );
            }
        }
        let mut row = div()
            .id(("commit", index))
            .role(Role::ListBoxOption)
            .aria_label(format!(
                "{} · {} · {}",
                commit.subject,
                commit.author,
                short_oid(&commit.oid)
            ))
            .aria_selected(active)
            .w_full()
            .h(px(34.))
            .flex()
            .items_center()
            .px_3()
            .gap_0()
            .bg(rgb(if active { SELECTED } else { CANVAS }))
            .border_l_2()
            .border_color(rgb(if active { MINT } else { CANVAS }))
            .hover(|s| s.bg(rgb(HOVER)))
            .cursor_pointer()
            .child(references)
            .child(graph::render(
                self.graph[index].clone(),
                columns.graph,
                self.graph_lanes,
                active,
                commit.parents.len() > 1,
                self.visible.len() != self.commits.len() || self.graph_notice.is_some(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .truncate()
                    .text_size(px(13.))
                    .child(commit.subject.clone()),
            );
        if columns.author > 0. {
            row = row.child(
                div()
                    .w(px(columns.author))
                    .flex_shrink_0()
                    .pl_3()
                    .pr_2()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(commit.author.clone()),
            );
        }
        if columns.date > 0. {
            row = row.child(
                div()
                    .w(px(columns.date))
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(short_date(commit.timestamp)),
            );
        }
        if columns.hash > 0. {
            row = row.child(
                div()
                    .w(px(columns.hash))
                    .flex_shrink_0()
                    .font_family(mono())
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(short_oid(&commit.oid)),
            );
        }
        row.on_click(cx.listener(move |this, _, window, cx| this.select_commit(index, window, cx)))
            .into_any_element()
    }

    pub(super) fn render_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(commit) = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
        else {
            return div()
                .size_full()
                .bg(rgb(PANEL))
                .border_l_1()
                .border_color(rgb(BORDER))
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
                .on_click(
                    cx.listener(move |this, _, window, cx| this.change_parent(i, window, cx)),
                ),
            );
        }
        if commit.parents.len() > 128 {
            parents = parents.child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(format!("Showing 128 of {} parents", commit.parents.len())),
            );
        }
        if commit.parents.is_empty() {
            parents = parents.child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child("Root commit · empty-tree comparison"),
            );
        }
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .id("commit-metadata")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child("COMMIT DETAILS")
                            .child(
                                button("copy-commit", short_oid(&commit.oid), "copy", false)
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            oid.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(relative(1.35))
                            .font_weight(FontWeight::MEDIUM)
                            .child(commit.subject.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(commit.author.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(full_date(commit.timestamp)),
                    )
                    .child(parents)
                    .children((!commit.body.is_empty()).then(|| {
                        button("message", "Message", "", self.details).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.details = !this.details;
                                cx.notify();
                            },
                        ))
                    })),
            )
            .children(self.details.then(|| {
                div()
                    .id("commit-message")
                    .max_h(px(150.))
                    .overflow_y_scroll()
                    .p_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(12.))
                    .child(
                        button("copy-message", "Copy full message", "copy", false).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(message.clone()))
                            },
                        ),
                    )
                    .child(div().child(if commit.body.is_empty() {
                        "No extended commit message.".into()
                    } else {
                        commit.body.chars().take(8192).collect::<String>()
                    }))
            }))
            .child(div().flex_1().min_h_0().child(self.render_files(cx)))
            .into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(format!("CHANGED FILES   {}", self.files.len())),
            )
            .child(
                div()
                    .id("files-pane")
                    .role(Role::ListBox)
                    .aria_label("Changed files")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.file_focus)
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.pane = Pane::Files;
                            window.focus(&this.file_focus, cx);
                        }),
                    )
                    .child(if self.files.is_empty() {
                        empty(
                            self.loading.unwrap_or("No file changes"),
                            "Compared against the selected parent.",
                        )
                    } else {
                        uniform_list(
                            "files",
                            self.files.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|i| this.render_file_row(i, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.file_scroll)
                        .into_any_element()
                    }),
            )
            .into_any_element()
    }
    pub(super) fn render_file_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let file = &self.files[index];
        let active = self.selected_file == Some(index);
        let path = file.path();
        let color = match file.status.letter() {
            "A" => MINT,
            "D" => 0xf29aa2,
            "R" => 0x9cb9f2,
            _ => 0xe9c17e,
        };
        div()
            .id(("file", index))
            .role(Role::ListBoxOption)
            .aria_label(format!("{} · {}", path.display(), file.status.label()))
            .aria_selected(active)
            .w_full()
            .h(px(44.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(if active { SELECTED } else { PANEL }))
            .border_l_2()
            .border_color(rgb(if active { MINT } else { PANEL }))
            .hover(|s| s.bg(rgb(HOVER)))
            .cursor_pointer()
            .child(icon(
                if gitturtle_preview::is_image_path(path) {
                    "image"
                } else {
                    "code"
                },
                16.,
                MUTED,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().truncate().text_size(px(12.)).child(
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
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
                    .text_size(px(10.))
                    .text_color(rgb(color))
                    .child(file.status.letter()),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, window, cx)))
            .into_any_element()
    }
    pub(super) fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let file = self.selected_file.and_then(|index| self.files.get(index));
        let path = file
            .map(|file| file.path().to_string_lossy().into_owned())
            .unwrap_or("File comparison".into());
        let copy_path = path.clone();
        let mut toolbar = div()
            .h(px(42.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                button("back-history", "← History", "", false)
                    .accessibility_label("Back to history")
                    .on_click(cx.listener(|this, _, window, cx| this.back_to_history(window, cx))),
            )
            .child(div().h(px(18.)).w(px(1.)).bg(rgb(BORDER)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .child(path),
            )
            .children(file.map(|_| {
                button("copy-path", "", "copy", false).on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()))
                })
            }));
        let content = if let Some(error) = &self.error {
            empty("Preview unavailable", error)
        } else if file.is_none() {
            empty(
                self.loading.unwrap_or("Choose a file"),
                "Select a changed file in the right panel, or return to history.",
            )
        } else if let Some(content) = &self.content {
            match content.as_ref() {
                Content::Text { patch, .. } => {
                    for (mode, name) in [
                        (TextMode::Unified, "Diff"),
                        (TextMode::Before, "Before"),
                        (TextMode::After, "After"),
                    ] {
                        toolbar =
                            toolbar.child(button(name, name, "", self.text_mode == mode).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.text_mode = mode;
                                    this.ensure_editor(window, cx);
                                    cx.notify();
                                }),
                            ));
                    }
                    let editor = match self.text_mode {
                        TextMode::Unified => &self.patch_editor,
                        TextMode::Before => &self.before_editor,
                        TextMode::After => &self.after_editor,
                    };
                    if self.text_mode == TextMode::Unified && patch.is_empty() {
                        empty("Content unchanged", "Only the file mode or path changed.")
                    } else if self.text_mode == TextMode::Unified && self.patch_view.is_some() {
                        div()
                            .size_full()
                            .child(self.patch_view.as_ref().unwrap().clone())
                            .into_any_element()
                    } else if let Some(editor) = editor {
                        Editor::new(editor)
                            .h(relative(1.))
                            .readonly(true)
                            .bordered(false)
                            .aria_label("Read-only file comparison")
                            .text_size(px(12.))
                            .into_any_element()
                    } else {
                        empty("Loading text…", "")
                    }
                }
                Content::Images { old, new } => {
                    if self.zoom > 0. {
                        toolbar = toolbar.child(
                            div()
                                .text_size(px(10.))
                                .text_color(rgb(MUTED))
                                .child("Drag to pan"),
                        );
                    }
                    for (name, zoom) in [("Fit", 0.), ("50%", 0.5), ("100%", 1.), ("200%", 2.)] {
                        toolbar =
                            toolbar.child(button(name, name, "", self.zoom == zoom).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.zoom = zoom;
                                    cx.notify();
                                }),
                            ));
                    }
                    div()
                        .size_full()
                        .flex()
                        .child(self.render_image_side(0, old, cx))
                        .child(self.render_image_side(1, new, cx))
                        .into_any_element()
                }
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
            .bg(rgb(CANVAS))
            .child(toolbar)
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
            .children(file.map(|file| {
                div()
                    .h(px(24.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child(format!(
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
                    ))
            }))
            .into_any_element()
    }
    pub(super) fn render_image_side(
        &self,
        index: usize,
        side: &worker::ImageSide,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = if index == 0 { "BEFORE" } else { "AFTER" };
        let details = side
            .image
            .as_ref()
            .map(|i| {
                format!(
                    "{} × {} · {} · {}{}",
                    i.original_width,
                    i.original_height,
                    i.format,
                    format_bytes(side.bytes),
                    if i.width != i.original_width || i.height != i.original_height {
                        format!(" · preview {} × {}", i.width, i.height)
                    } else {
                        String::new()
                    }
                )
            })
            .unwrap_or_default();
        let view = if let Some(image) = &self.images[index] {
            let preview = side.image.as_ref().unwrap();
            if self.zoom == 0. {
                div()
                    .size_full()
                    .p_4()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(image.clone())
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            } else {
                div()
                    .id(("image-scroll", index))
                    .size_full()
                    .overflow_scroll()
                    .track_scroll(&self.image_scroll)
                    .cursor(if self.image_drag.is_some() {
                        CursorStyle::ClosedHand
                    } else {
                        CursorStyle::OpenHand
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.image_drag = Some((event.position, this.image_scroll.offset()));
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(self
                                .images
                                .iter()
                                .flatten()
                                .map(|i| i.size(0).width.0)
                                .max()
                                .unwrap_or(preview.width as i32)
                                as f32
                                * self.zoom))
                            .h(px(self
                                .images
                                .iter()
                                .flatten()
                                .map(|i| i.size(0).height.0)
                                .max()
                                .unwrap_or(preview.height as i32)
                                as f32
                                * self.zoom))
                            .child(
                                img(image.clone())
                                    .w(px(preview.width as f32 * self.zoom))
                                    .h(px(preview.height as f32 * self.zoom))
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .into_any_element()
            }
        } else {
            empty(
                if side.message.is_some() {
                    "Image unavailable"
                } else if index == 0 {
                    "Added image"
                } else {
                    "Deleted image"
                },
                side.message.as_deref().unwrap_or("This side has no image."),
            )
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(name)
                    .child(details),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(checkerboard())
                    .child(div().absolute().inset_0().child(view)),
            )
            .into_any_element()
    }
}

impl Render for GitTurtle {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let left = if self.mode == WorkspaceMode::Compare {
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
                .child(
                    resizable_panel()
                        .size(px(220.))
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
        // The inspector's parent and element identity are identical in both
        // modes, preserving its width and keeping file navigation in place.
        let workspace = h_resizable("content-columns")
            .child(
                resizable_panel()
                    .size_range(px(520.)..px(10000.))
                    .child(left),
            )
            .child(
                resizable_panel()
                    .size(px(320.))
                    .size_range(px(280.)..px(480.))
                    .flex_none()
                    .child(self.render_inspector(cx)),
            );
        div()
            .id("gitturtle")
            .key_context("GitTurtle")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            // Keep an active image drag continuous across the toolbar, inspector,
            // and either image viewport until the mouse is released.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let Some((start, offset)) = this.image_drag else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
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
                cx.listener(|this, _, _, cx| {
                    if this.image_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.image_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::choose_repository))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::search))
            .on_action(cx.listener(Self::clear_search))
            .on_action(
                cx.listener(|this, _: &BackHistory, window, cx| this.back_to_history(window, cx)),
            )
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
                if let Some(index) = this.selected_file {
                    this.select_file(index, window, cx);
                } else {
                    this.pane = Pane::Files;
                    window.focus(&this.file_focus, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                if this.mode == WorkspaceMode::Compare {
                    this.back_to_history(window, cx);
                } else {
                    this.sidebar = !this.sidebar;
                    this.history_sidebar = this.sidebar;
                    cx.notify();
                }
            }))
            .child(self.render_header(cx))
            .child(div().flex_1().min_h_0().child(workspace))
            .child(
                div()
                    .h(px(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(PANEL))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(
                        div()
                            .size(px(5.))
                            .rounded_full()
                            .bg(rgb(if self.error.is_some() { 0xf29aa2 } else { MINT })),
                    )
                    .child(
                        div().flex_1().truncate().child(
                            self.error
                                .as_deref()
                                .unwrap_or(self.loading.unwrap_or(&self.status))
                                .to_string(),
                        ),
                    )
                    .child(if self.mode == WorkspaceMode::Compare {
                        "Esc Back to history · ↑↓ Files".to_owned()
                    } else {
                        format!(
                            "{}F Search · ↑↓ Navigate · Return Open file",
                            primary_label()
                        )
                    }),
            )
    }
}

#[derive(Clone, Copy)]
struct HistoryColumns {
    refs: f32,
    graph: f32,
    author: f32,
    date: f32,
    hash: f32,
}

fn history_columns(width: f32, graph_width: f32) -> HistoryColumns {
    let width = width.max(320.);
    HistoryColumns {
        refs: if width < 620. {
            108.
        } else if width < 840. {
            140.
        } else {
            160.
        },
        graph: graph_width.min(if width < 620. {
            84.
        } else if width < 840. {
            112.
        } else {
            150.
        }),
        author: if width >= 780. { 110. } else { 0. },
        date: if width >= 640. { 66. } else { 0. },
        hash: if width >= 1020. { 64. } else { 0. },
    }
}
