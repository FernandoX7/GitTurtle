use crate::native_accessibility::LiveRegion as _;
use crate::*;
use columns::ColumnId;
use gpui_kit::base::{Scrollbar, ScrollbarMode};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder;
use std::cell::RefCell;

/// Window-level action host mounted as a `RootPlugin` so shortcuts that apply
/// while a dialog or sheet holds focus (`Refresh`, window management) reach
/// `GitTurtle` even though `Root` renders overlay plugins as siblings of the
/// application view.
pub(crate) struct WindowActionHost;

impl Render for WindowActionHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl gpui_kit::base::RootPlugin for WindowActionHost {
    fn decorate(
        &self,
        surface: AnyElement,
        root: &gpui_kit::base::Root,
        _: &mut Window,
        _: &mut App,
    ) -> impl IntoElement {
        let Ok(app) = root.view().clone().downcast::<GitTurtle>() else {
            return surface;
        };
        let app_close = app.clone();
        let app_refresh = app;
        div()
            .size_full()
            .key_context("GitTurtle")
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(move |_: &CloseWindow, window, cx| {
                if app_close.read(cx).operation_busy.is_none() {
                    window.remove_window();
                }
            })
            .on_action(|_: &ZoomWindow, window, _| window.zoom_window())
            .on_action(move |action: &Refresh, window, cx| {
                app_refresh.update(cx, |this, cx| {
                    if this.refresh_open_stashes(window, cx) {
                        return;
                    }
                    if !window.has_active_dialog(cx)
                        && !window.has_active_sheet(cx)
                        && this.page == AppPage::Repository
                    {
                        this.refresh(action, window, cx);
                    }
                });
            })
            .child(surface)
            .into_any_element()
    }
}

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
    // A scroll the list has yet to apply was asked for on purpose, as Center
    // for a file chosen with the keyboard or one that arrives, or Top for a
    // changed path filter, and the list applies only the last one asked
    // for. The changed files' probe reveals before the list in the frame,
    // so a reveal made now would replace that request with a Nearest one and
    // leave the selection where the reveal put it. The new size is recorded
    // either way: a later frame finds no change to reveal and leaves the
    // request's position alone.
    if scroll.0.borrow().deferred_scroll_to_item.is_some() {
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
    /// Watches a list's viewport and reveals its selected row when the
    /// viewport's size or the row height changes
    /// ([`reveal_resized_selection`]).
    ///
    /// The probe reveals the row as it is prepainted, so it goes before its
    /// list: the list applies the reveal as it lays its rows out in the same
    /// frame, and the frame that first draws the list at a new size, as the
    /// inspector or History after a width change, draws the selection where
    /// it settles and asks for no other. Revealed as it was painted, after
    /// the list, the row moved only in the next frame, so each reveal asked
    /// for one, and a width step drew two frames where the second, the
    /// selection still in view, redrew the first. History's selected lane,
    /// which a new width can leave out of the graph's lanes, is revealed as
    /// History takes that width, before the frame builds its header and rows
    /// ([`Self::take_history_width`]).
    fn list_viewport_probe(&self, history: bool, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        canvas(
            move |bounds, _, cx| {
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
                    // History's probe sits in its table, which is as wide as
                    // the columns and scrolls sideways inside the viewport:
                    // the viewport's width is the one History took in this
                    // frame.
                    let width = if history {
                        px(this.history_width)
                    } else {
                        bounds.size.width
                    };
                    let next = (size(width, bounds.size.height), row_height);
                    let previous = if history {
                        this.history_list_layout
                    } else {
                        this.file_list_layout
                    };
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
                        (
                            &mut this.workspace.history_list_layout,
                            &this.workspace.history_scroll,
                        )
                    } else {
                        (
                            &mut this.workspace.file_list_layout,
                            &this.workspace.file_scroll,
                        )
                    };
                    reveal_resized_selection(previous, next, selected, scroll);
                });
            },
            |_, _, _, _| {},
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
        /// Back's shortcut as its tooltip ends with it, formatted once.
        static SHORTCUT: std::sync::LazyLock<SharedString> =
            std::sync::LazyLock::new(|| format!("{}[", primary_label()).into());
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
        // History's Back opens Projects, which a running write refuses
        // (`projects_refusal`); once it looks refused, its tooltip says why.
        .map(|back| {
            self.refusing_with_reason(
                back,
                "page-back",
                self.projects_refusal().0 && self.mode == WorkspaceMode::History,
                |app| crate::workspace::WaitingTooltip {
                    name: app.back_label().into(),
                    shortcut: Some(SHORTCUT.clone()),
                    dimmed: app.projects_refusal().1 && app.mode == WorkspaceMode::History,
                },
                cx,
            )
        })
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

    /// The repository's path as the header's identity reads it, or the
    /// app's line before one opens.
    fn repository_path_line(&self) -> String {
        self.path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or("A clearer view of your code".into())
    }

    /// The repository's name and path as the header shows them in `room`,
    /// each line elided in its middle as macOS elides one, so the part that
    /// tells two tabs apart stays: the name keeping its start and its end
    /// around "…" ("sysp…mod", [`name_middle`]), and the path its top
    /// directory and its last component ("/tmp/…/compact"), never its last
    /// component alone ([`location_tail`]),
    /// each measured in its own font as it is laid out on the window's
    /// device pixels. Their whole text stays the heading's accessible name
    /// and its tooltip ([`Self::repository_heading`]). Kept for each text,
    /// room, font, size and scale factor drawn lately, so a steady frame
    /// shapes nothing; it finds what it kept by what the texts are made of,
    /// borrowed, so it allocates nothing either: built as owned strings to
    /// look up, the name and the path line were two allocations a frame.
    /// (Decided 2026-11-02: both ended in an ellipsis,
    /// "/tmp/gitturtle-evi…" and "sysprev…" for "sysprev-mod", cutting
    /// what told two tabs apart.)
    fn shown_identity(&self, room: Pixels, window: &Window) -> ShownIdentity {
        #[cfg(test)]
        let before = crate::test_support::allocations();
        let line = LineKey::current(window);
        let (scale, room_bits) = (window.scale_factor().to_bits(), f32::from(room).to_bits());
        let repository = self.repository.as_ref().map(|repo| repo.path());
        let named = repository
            .and_then(|path| self.project_names.get(path))
            .map(String::as_str);
        let path = self.path.as_deref();
        let shown = HEADER_IDENTITY.with_borrow_mut(|cache| {
            cache
                .get_where(
                    |kept| {
                        kept.line == line
                            && kept.scale == scale
                            && kept.room == room_bits
                            && kept.repository.as_deref() == repository
                            && kept.named.as_deref() == named
                            && kept.path.as_deref() == path
                    },
                    || IdentityKey {
                        line: line.clone(),
                        scale,
                        room: room_bits,
                        repository: repository.map(std::path::Path::to_path_buf),
                        named: named.map(str::to_owned),
                        path: path.map(std::path::Path::to_path_buf),
                    },
                    |_| {
                        #[cfg(test)]
                        IDENTITY_LOOKUPS.with(|counted| {
                            let (lookups, measured, allocations) = counted.get();
                            counted.set((lookups, measured + 1, allocations));
                        });
                        let width = |size: f32, weight: FontWeight| {
                            move |text: &str| {
                                laid_out_text_width(
                                    shaped_width(text, appearance::ui_text(size), weight, window),
                                    window,
                                )
                            }
                        };
                        let whole = self.repository_name();
                        let name = name_middle(&whole, room, &width(14., FontWeight::SEMIBOLD));
                        ShownIdentity {
                            name_elided: name != whole,
                            name: name.into(),
                            path: location_tail(
                                &self.repository_path_line(),
                                room,
                                width(11., FontWeight::NORMAL),
                            )
                            .into(),
                        }
                    },
                )
                .clone()
        });
        #[cfg(test)]
        IDENTITY_LOOKUPS.with(|counted| {
            let (lookups, measured, allocations) = counted.get();
            let made = crate::test_support::allocations() - before;
            counted.set((lookups + 1, measured, allocations + made));
        });
        shown
    }

    /// The header's repository identity, named with its path for assistive
    /// technology and in its tooltip, as the chrome's kept `labels` have
    /// them ([`Self::heading_text`]).
    fn repository_heading(
        &self,
        labels: &ChromeLabels,
    ) -> native_accessibility::StaticText<Stateful<Div>> {
        let identity = labels.heading_tooltip.clone();
        note_control_tooltip(&"repository-heading".into(), &identity);
        native_accessibility::static_text(
            div()
                .id("repository-heading")
                .debug_selector(|| "repository-heading".into()),
            labels.heading.clone(),
        )
        .tooltip(move |window, cx| Tooltip::new(identity.clone()).build(window, cx))
    }

    /// The repository heading's accessible name, the repository's name with
    /// the window's path, and its tooltip, those two on their own lines, or
    /// the app's name before a repository opens.
    fn heading_text(&self) -> (String, String) {
        #[cfg(test)]
        CHROME_LABELS_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
        let heading = format!(
            "Repository {}. {}",
            self.repository
                .as_ref()
                .map(|repo| self.project_name(repo.path()))
                .unwrap_or_default(),
            self.path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        );
        let tooltip = self
            .path
            .as_ref()
            .map(|path| format!("{}\n{}", self.project_name(path), path.display()))
            .unwrap_or_else(|| "GitTurtle".into());
        (heading, tooltip)
    }

    /// The Changes mode's name with its working-file count, as its tab and
    /// its compact toggle's accessible name read it; the header draws the
    /// copy the chrome keeps ([`ChromeLabels`]).
    pub(super) fn changes_label(&self) -> String {
        #[cfg(test)]
        CHROME_LABELS_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
        self.header_status()
            .map(|status| {
                if status.entries.is_empty() {
                    "Changes".into()
                } else {
                    format!("Changes · {}", status.entries.len())
                }
            })
            .unwrap_or_else(|| "Changes".into())
    }

    /// A running operation's progress, with its repository where `named`;
    /// its tooltip and accessible name always name the repository's path.
    /// In the header it is the 11 pt label within its limit; in the status
    /// bar (`bar`) it takes the status line's text and room.
    fn render_operation_progress(
        &self,
        label: &str,
        named: bool,
        bar: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .live_region(gpui::accesskit::Live::Off, false)
            .min_w_0()
            .when(!bar, |progress| {
                progress
                    .max_w(appearance::ui_size(OPERATION_PROGRESS_WIDTH))
                    .text_size(appearance::ui_text(11.))
            })
            .truncate()
            .text_color(rgb(p.accent))
            .child(if named {
                self.operation_text(&progress)
            } else {
                progress
            })
            .into_any_element()
    }

    /// The progress label's text: `progress`, and the write's repository
    /// where it has one.
    fn operation_text(&self, progress: &str) -> String {
        progress.to_owned()
            + &self
                .operation_repository
                .as_ref()
                .map_or(String::new(), |path| {
                    format!(" · {}", self.project_name(path))
                })
    }

    /// A running write's status in the header: its progress label and
    /// Cancel, drawn at `place` in its form, in the room
    /// [`GitTurtle::operation_status_size`] gives it, the label giving way to
    /// Cancel. In the spacer, the whole spacer up to the form's most, or the
    /// compact row's `width`, they end at its trailing edge, beside the
    /// trailing controls; after those controls the status takes the row's
    /// room from the form's starting width to its most.
    fn render_operation_status(
        &self,
        label: &str,
        layout: OperationLayout,
        width: Option<Pixels>,
        widths: &ChromeWidths,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let room = self.operation_status_size(widths, window);
        let form = layout.form;
        div()
            .debug_selector(|| "operation-status".into())
            .flex()
            .items_center()
            .gap_2()
            .h(room.height(form))
            .map(|status| match (layout.place, width) {
                (OperationPlace::Spacer, Some(width)) => {
                    status.flex_shrink_0().w(width).justify_end()
                }
                (OperationPlace::Spacer, None) => {
                    status.w_full().max_w(room.most(form)).justify_end()
                }
                (OperationPlace::After, _) => status
                    .flex_1()
                    .flex_basis(room.width(form))
                    .min_w_0()
                    .max_w(room.most(form)),
                // The status bar draws it there (`render_bar_operation`).
                (OperationPlace::StatusBar, _) => status,
            })
            .child(self.render_operation_progress(label, form.named, false, cx))
            .child(self.framed_header_control(self.render_operation_cancel(form.icon, window, cx)))
            .into_any_element()
    }

    fn render_projects_button(&self, cx: &mut Context<Self>) -> Button {
        /// Projects' shortcut as its tooltip ends with it, formatted once.
        static SHORTCUT: std::sync::LazyLock<SharedString> =
            std::sync::LazyLock::new(|| shortcuts::label(shortcuts::ShortcutId::Projects).into());
        button("projects", "", "folder", false)
            .debug_selector(|| "projects".into())
            .accessibility_label("Projects")
            // `show_projects` refuses while a write runs, since every action
            // Projects offers starts another operation (`projects_refusal`);
            // once the button looks refused, its tooltip says why.
            .map(|projects| {
                self.refusing_with_reason(
                    projects,
                    "projects",
                    self.projects_refusal().0,
                    |app| crate::workspace::WaitingTooltip {
                        name: "Open Projects".into(),
                        shortcut: Some(SHORTCUT.clone()),
                        dimmed: app.projects_refusal().1,
                    },
                    cx,
                )
            })
            .on_click(cx.listener(|this, _, window, cx| this.show_projects(window, cx)))
    }

    fn render_settings_button(&self, cx: &mut Context<Self>) -> Button {
        /// Settings' tooltip, with its shortcut, formatted once.
        static TOOLTIP: std::sync::LazyLock<SharedString> =
            std::sync::LazyLock::new(|| format!("Settings · {},", primary_label()).into());
        button("settings", "", "settings", false)
            .debug_selector(|| "settings".into())
            .accessibility_label("Settings")
            .tooltip(TOOLTIP.clone())
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
    /// rule: the action bar's compact row follows it on the same surface. A
    /// running write's status goes before the trailing controls where the
    /// name leaves it room, and to the status bar elsewhere, as the frame's
    /// `plan` has it ([`GitTurtle::compact_operation_place`]), so nothing on
    /// the row moves.
    fn render_compact_header(
        &self,
        turtle: bool,
        name: Pixels,
        plan: HeaderPlan,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        /// History's tooltip, with its shortcut, formatted once.
        static HISTORY_TOOLTIP: std::sync::LazyLock<SharedString> =
            std::sync::LazyLock::new(|| format!("History · {}1", primary_label()).into());
        let p = palette(cx);
        let (widths, labels) = self.kept_chrome(window, |widths, labels| (*widths, labels.clone()));
        let place = plan.operation;
        let before = self
            .operation_shown()
            .filter(|_| place.place == OperationPlace::Spacer)
            .map(|label| {
                self.render_operation_status(label, place, plan.width, &widths, window, cx)
            });
        let back_to_projects = self.back_label() == "Back to Projects";
        let ring = appearance::button_ring_room(cx);
        // The name alone, elided in its middle to its room, which it then
        // takes whole, as the end-elided name did, so the toggles after it
        // stand where they stood.
        let ShownIdentity {
            name: shown,
            name_elided: elided,
            ..
        } = self.shown_identity(name, window);
        #[cfg(test)]
        HEADER_SHOWN
            .with(|kept| *kept.borrow_mut() = Some((shown.clone(), SharedString::default())));
        let history = self.mode != WorkspaceMode::Working;
        let count = self
            .header_status()
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
            .child(self.framed_header_control(self.render_back_button(false, cx)))
            .when(turtle, |header| {
                header.child(app_icon(f32::from(appearance::ui_size(28.))))
            })
            .child(
                self.header_control(self.repository_heading(&labels))
                    .min_w_0()
                    .max_w(name)
                    .when(elided, |heading| heading.w(name))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .text_size(appearance::ui_text(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(shown),
            )
            .when(self.repository.is_some(), |header| {
                header
                    .child(
                        self.header_control(div())
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_0p5()
                            .rounded(appearance::ui_size(7.))
                            .bg(rgb(p.subtle))
                            .child(
                                mark_selected_mode(
                                    compact_control_name(
                                        button("history-tab", "", "commit", history),
                                        "history-tab",
                                        "History",
                                    ),
                                    history,
                                    crate::ICON_INSET,
                                    "mode-tab-selected-bar",
                                    p.accent,
                                )
                                .debug_selector(|| "history-tab".into())
                                .toggled(history)
                                .tooltip(HISTORY_TOOLTIP.clone())
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
                                        mark_selected_mode(
                                            compact_control_label(
                                                button("changes-tab", "", "changes", !history),
                                                "changes-tab",
                                                labels.changes.clone(),
                                            ),
                                            !history,
                                            crate::ICON_INSET,
                                            "mode-tab-selected-bar",
                                            p.accent,
                                        )
                                        .debug_selector(|| "changes-tab".into())
                                        .toggled(!history)
                                        .tooltip({
                                            note_control_tooltip(
                                                &"changes-tab".into(),
                                                &labels.changes_tooltip,
                                            );
                                            labels.changes_tooltip.clone()
                                        })
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
                                        toggle.child(changes_badge(
                                            labels.changes_badge.clone(),
                                            !history,
                                            p,
                                        ))
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
                        // Refused while the panel cannot open, saying why
                        // and still answering a press; a cancelled read
                        // still draining does not stop it
                        // (`GitTurtle::github_open_refusal`).
                        .map(|open| self.refusing_pull_requests(open, true, cx))
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
                    .children(before)
                    .when(!back_to_projects, |trailing| {
                        trailing.child(self.framed_header_control(self.render_projects_button(cx)))
                    })
                    .child(self.framed_header_control(self.render_profile_button(true, true, cx)))
                    .child(self.framed_header_control(self.render_settings_button(cx))),
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
    /// and Settings lead it in every mode and Projects follows them. A
    /// Repository frame brings the `plan` it worked out for its header
    /// ([`GitTurtle::header_plan`]).
    fn render_header(
        &self,
        chrome: Chrome,
        plan: Option<HeaderPlan>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let compact = match (chrome, plan) {
            (
                Chrome::Compact {
                    app_icon: turtle,
                    name,
                },
                Some(plan),
            ) if self.page == AppPage::Repository => {
                return self.render_compact_header(turtle, name, plan, window, cx);
            }
            (Chrome::Compact { .. }, _) => false,
            (Chrome::Full { narrow }, _) => narrow,
        };
        let p = palette(cx);
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
                    noted_tooltip(
                        button("page-back", "", "arrow-left", false)
                            .accessibility_label(back_label),
                        "page-back",
                        settings_back_tooltip(back_label),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.return_from_page(window, cx))),
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
        let (widths, labels) = self.kept_chrome(window, |widths, labels| (*widths, labels.clone()));
        let plan = plan.unwrap_or_else(|| self.header_plan(chrome, window));
        // Each line inside the identity, short of the pixel its own middle
        // elision keeps as a guard ([`ellipsis_middle`]).
        let ShownIdentity { name, path, .. } =
            self.shown_identity(plan.identity - px(ELLIPSIS_SLACK), window);
        #[cfg(test)]
        HEADER_SHOWN.with(|shown| *shown.borrow_mut() = Some((name.clone(), path.clone())));
        let (trailing, slot) = plan.full.unwrap_or((Trailing::Shared, false));
        let place = plan.operation;
        let own_row = trailing != Trailing::Shared;
        // A running write's status, in the spacer or after the trailing
        // controls, wherever it moves nothing; elsewhere the status bar
        // draws it.
        let status = self
            .operation_shown()
            .filter(|_| place.place != OperationPlace::StatusBar)
            .map(|label| self.render_operation_status(label, place, None, &widths, window, cx));
        let (spaced, after) = if place.place == OperationPlace::Spacer {
            (status, None)
        } else {
            (None, status)
        };
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
                self.header_control(div())
                    .flex_shrink_0()
                    .when(!compact, |slot| {
                        slot.w(self.labelled_width(widths.projects, true, window))
                    })
                    .child(self.render_back_button(!compact, cx)),
            )
            .child(app_icon(f32::from(appearance::ui_size(28.))))
            .child(
                self.header_control(self.repository_heading(&labels))
                    .flex_1()
                    .max_w(appearance::ui_size(200.))
                    .min_w(appearance::ui_size(100.))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        ellipsis_middle(div().debug_selector(|| "repository-name".into()))
                            .text_size(appearance::ui_text(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        ellipsis_middle(div().debug_selector(|| "repository-path".into()))
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            .child(path),
                    ),
            )
            .when(self.repository.is_some(), |header| {
                header
                    .child(
                        self.header_control(div())
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap_0p5()
                            .p_0p5()
                            .rounded(appearance::ui_size(9.))
                            .bg(rgb(p.subtle))
                            .child(
                                mark_selected_mode(
                                    button(
                                        "history-tab",
                                        "History",
                                        "",
                                        self.mode != WorkspaceMode::Working,
                                    ),
                                    self.mode != WorkspaceMode::Working,
                                    crate::LABEL_INSET,
                                    "mode-tab-selected-bar",
                                    p.accent,
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
                                mark_selected_mode(
                                    button(
                                        "changes-tab",
                                        labels.changes.clone(),
                                        "",
                                        self.mode == WorkspaceMode::Working,
                                    ),
                                    self.mode == WorkspaceMode::Working,
                                    crate::LABEL_INSET,
                                    "mode-tab-selected-bar",
                                    p.accent,
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
                            // Refused while the panel cannot open, saying
                            // why and still answering a press; a cancelled
                            // read still draining does not stop it
                            // (`GitTurtle::github_open_refusal`).
                            .map(|open| self.refusing_pull_requests(open, false, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_github_from_header(window, cx)
                            })),
                    )
            })
            // The spacer holds the status at its trailing end, and lays out
            // as if it were empty: it takes no room of its own.
            .when(trailing != (Trailing::OwnRow { spacer: false }), |header| {
                header.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .justify_end()
                        .children(spaced),
                )
            })
            // History's root leaves Projects to its return action but keeps
            // its slot where that adds no row, so the name shares the same
            // room in every mode. On a row of their own the trailing
            // controls start at the header's edge with the profile and
            // Settings, and Projects follows them where it shows.
            .when(slot, |header| {
                header.child(div().flex_shrink_0().size(appearance::ui_size(28.)))
            })
            .when(!back_to_projects && !own_row, |header| {
                header.child(self.framed_header_control(self.render_projects_button(cx)))
            })
            .child(self.framed_header_control(self.render_profile_button(compact, false, cx)))
            .child(self.framed_header_control(self.render_settings_button(cx)))
            .when(!back_to_projects && own_row, |header| {
                header.child(self.framed_header_control(self.render_projects_button(cx)))
            })
            .children(after)
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
                            .polite()
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
                    .debug_selector(|| "rail-open".into())
                    // A marked tab's way back where the rail stands in the
                    // navigation's place (`close_menus_for_the_mark`), which
                    // takes no press that began elsewhere
                    // (`rail_way_back_key_down`).
                    .track_focus(&self.mark_way_back)
                    .capture_key_down(cx.listener(Self::rail_way_back_key_down))
                    .capture_key_up(cx.listener(Self::way_back_key_up))
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
        // A marked tab's branches, worktrees and counts are those read
        // before the mark, of a repository the tab can no longer open: the
        // tree lists none and says what the mark says, as the header does
        // (`header_status`), read each frame, as a mark goes on and off
        // without a read. It listed them, "LOCAL BRANCHES 1" over "main",
        // and its keys browsed and acted on them.
        let marked = self.shown_tab_unavailable();
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
                        // Labelled or a glyph, the selected mode keeps its
                        // fill and draws the accent bar under its label or
                        // glyph (`mark_selected_mode`).
                        let control = if form.labelled_modes {
                            mark_selected_mode(
                                button(name, name, "", selected),
                                selected,
                                crate::LABEL_INSET,
                                "nav-mode-selected-bar",
                                colors.accent,
                            )
                        } else {
                            mark_selected_mode(
                                compact_control_name(button(name, "", symbol, selected), selector, name)
                                    .tooltip(name),
                                selected,
                                crate::ICON_INSET,
                                "nav-mode-selected-bar",
                                colors.accent,
                            )
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
                    // On a marked tab, the mode's line and its way back, as
                    // focus meets the tree: it was named "Branches and
                    // worktrees unavailable", the line and the way back left
                    // on static text inside a tree with no items.
                    .aria_label(if marked {
                        nav_unavailable_name(self.nav_mode)
                    } else {
                        "Branches and worktrees. Arrow keys browse; Left and Right collapse or expand; Enter activates; Shift F10 opens branch or worktree actions"
                    })
                    .key_context("GitTurtleNavigation")
                    .tab_stop(true)
                    .track_focus(&self.nav_focus)
                    .flex_1().min_h_0().relative()
                    // Its keys browse and act on the rows it lists, none on a
                    // marked tab.
                    .when(!marked, |tree| tree.on_action(cx.listener(|this, _: &native_accessibility::NextNavigation, window, cx| this.move_navigation(true, false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::PreviousNavigation, window, cx| this.move_navigation(false, false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::FirstNavigation, window, cx| this.move_navigation(false, true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::LastNavigation, window, cx| this.move_navigation(true, true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::ExpandNavigation, window, cx| this.expand_navigation(true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::CollapseNavigation, window, cx| this.expand_navigation(false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::ActivateNavigation, window, cx| { if let Some(index)=this.nav_cursor {this.activate_navigation(index, window, cx);} }))
                    .on_action(cx.listener(|this, _: &native_accessibility::ManageNavigation, window, cx| this.manage_navigation(window, cx))))
                    .map(|tree| if marked {
                        // Its way back below the line, Refresh, refused at
                        // once while a write runs, and for the whole of a
                        // press that began refused (`way_back_key_down`);
                        // the Refresh shortcut, unlike it, reads the
                        // repository once even then, a passive read found
                        // harmless (FU44-C). It says why as the
                        // other refused controls do, dimmed with the busy
                        // reason in its tooltip once the write looks busy, or
                        // at once as a press is refused. It was drawn
                        // enabled, its tooltip only its name and shortcut,
                        // and a press did nothing. Focus goes there from a
                        // menu the mark closed (`close_menus_for_the_mark`).
                        static SHORTCUT: std::sync::LazyLock<SharedString> = std::sync::LazyLock::new(|| {
                            shortcuts::label(shortcuts::ShortcutId::Refresh).into()
                        });
                        let refresh = crate::empty_state::action("navigation-refresh", "Refresh", "refresh")
                            .track_focus(&self.mark_way_back)
                            .capture_key_down(cx.listener(Self::way_back_key_down))
                            .capture_key_up(cx.listener(Self::way_back_key_up))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.way_back_pointer_down(cx)))
                            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.way_back_pointer_up()))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.way_back_pointer_up()))
                            .on_click(cx.listener(|this, event, window, cx| this.press_way_back(event, window, cx)));
                        tree.child(
                            EmptyState::new("navigation-unavailable", nav_unavailable_line(self.nav_mode))
                                .tooltip(crate::branch_actions::UNAVAILABLE_WAY_BACK)
                                .action(self.refusing_with_reason(
                                    refresh,
                                    "navigation-refresh",
                                    self.operation_busy.is_some(),
                                    |app| crate::workspace::WaitingTooltip {
                                        name: "Refresh".into(),
                                        shortcut: Some(SHORTCUT.clone()),
                                        dimmed: app.operation_shown().is_some(),
                                    },
                                    cx,
                                ))
                                .pane(cx),
                        )
                    } else {
                        tree.child(uniform_list("navigation", self.nav_rows.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range.map(|i| this.render_nav_row(i, cx)).collect::<Vec<_>>()
                        })).size_full().track_scroll(&self.nav_scroll))
                    })
                    // Its keyboard ring on its own edge, over the rows, so
                    // none of them moves.
                    .children(self.focus_ring.edge(&self.nav_focus, rgb(colors.accent)))
                    // The mark closes it as it lands
                    // (`close_menus_for_the_mark`), and a marked tree opens
                    // none.
                    .children(self.nav_menu.as_ref().map(|menu| {
                        menu.render("navigation-menu-overlay", Self::dismiss_nav_menu, cx)
                    }))),
            )
            // Each of its lines whole, or none of it: the tree's accessible
            // name keeps the keys. A marked tree takes none of them.
            .when(form.hint && !marked, |sidebar| {
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

    /// The navigator's row `index` where it is a folder: a tree item named
    /// for what activating it does, its name and its branches counted with
    /// their noun ([`folder_name`]).
    fn render_nav_folder(&self, index: usize, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let NavRow::Folder {
            label,
            depth,
            count,
            expanded,
            ..
        } = self.nav_rows.get(index)?.clone()
        else {
            return None;
        };
        let colors = palette(cx);
        let label = label.as_str();
        Some(
            div()
                .id(self.nav_row_key(index))
                .role(Role::TreeItem)
                .aria_level(depth + 1)
                .when(self.nav_cursor == Some(index), |row| {
                    row.aria_active_descendant()
                        .border_l_2()
                        .border_color(rgb(colors.accent))
                })
                .aria_expanded(expanded)
                .aria_label(folder_name(expanded, label, count))
                .w_full()
                .h(crate::appearance::ui_size(30.))
                .pl(nav_row_inset(depth, self.nav_cursor == Some(index)))
                .pr_3()
                .flex()
                .items_center()
                .gap_1()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(colors.muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(colors.hover)).text_color(rgb(colors.text)))
                .active(|s| s.bg(rgb(colors.selected)))
                .child(div().w(px(12.)).child(if expanded { "⌄" } else { "›" }))
                .child(icon("folder", 14., colors.muted))
                .child(div().flex_1().truncate().child(label.to_owned()))
                .child(
                    div()
                        .text_size(crate::appearance::ui_text(10.))
                        .child(count.to_string()),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_navigation(index, window, cx)
                })),
        )
    }

    pub(super) fn render_nav_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let row = self.nav_rows[index].clone();
        if let NavRow::Section(label, count) = row {
            return div()
                .debug_selector(move || format!("nav-section/{label}/{count}"))
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
                .child(label)
                .child(count.to_string())
                .into_any_element();
        }
        if let NavRow::SectionToggle {
            key: _,
            label,
            count,
            truncated,
            expanded,
        } = row
        {
            let count_text = if truncated {
                format!("{count}+")
            } else {
                count.to_string()
            };
            let spoken = format!(
                "{} {label} ({count_text})",
                if expanded { "Collapse" } else { "Expand" }
            );
            return native_accessibility::recorded(
                div()
                    .id(self.nav_row_key(index))
                    .debug_selector(move || format!("nav-row-{index}"))
                    .role(Role::TreeItem)
                    .aria_level(1)
                    .when(self.nav_cursor == Some(index), |row| {
                        row.aria_active_descendant()
                            .border_l_2()
                            .border_color(rgb(colors.accent))
                    })
                    .aria_expanded(expanded)
                    .aria_label(spoken)
                    .w_full()
                    .h(crate::appearance::ui_size(30.))
                    .pl(nav_row_inset(0, self.nav_cursor == Some(index)))
                    .pr_3()
                    .pt_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .text_size(crate::appearance::ui_text(
                        crate::appearance::SECTION_LABEL_TEXT,
                    ))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(colors.muted))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(colors.hover)).text_color(rgb(colors.text)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().w(px(12.)).child(if expanded { "⌄" } else { "›" }))
                            .child(label),
                    )
                    .child(count_text)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_navigation(index, window, cx)
                    })),
            )
            .into_any_element();
        }
        if let Some(folder) = self.render_nav_folder(index, cx) {
            // Its node recorded for tests, which read the name it is drawn
            // with; transparent otherwise.
            return native_accessibility::recorded(folder).into_any_element();
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
            NavRow::Tag(i) => {
                let tag = &self.tags[*i];
                (
                    tag.name.clone(),
                    "tag",
                    false,
                    1,
                    format!("{} · {}", tag.name, short_oid(&tag.target_oid)),
                )
            }
            NavRow::Stash(i) => {
                let stash = &self.stashes[*i];
                (
                    stash.name.clone(),
                    "clock",
                    false,
                    1,
                    format!("{} · {}", stash.selector, stash.name),
                )
            }
            NavRow::Submodule(i) => {
                let sub = &self.submodules[*i];
                let path_str = sub.path.to_string_lossy();
                (
                    format!("{path_str} · {}", sub.state.label()),
                    "folder",
                    false,
                    1,
                    format!(
                        "{} ({}) · {} · {}",
                        sub.name,
                        path_str,
                        sub.state.label(),
                        short_oid(&sub.index_oid)
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
        let detached_head = self
            .work_status
            .as_ref()
            .is_some_and(|status| status.branch.is_none());
        let contextual_branch = match &row {
            NavRow::Branch(index, _) => Some(BranchMenuTarget {
                name: self.branches[*index].name.clone(),
                remote: self.branches[*index].remote,
                current: self.branches[*index].current,
                detached_head,
            }),
            _ => None,
        };
        let contextual_tag = match &row {
            NavRow::Tag(index) => Some(self.tags[*index].clone()),
            _ => None,
        };
        let contextual_stash = match &row {
            NavRow::Stash(index) => Some(self.stashes[*index].clone()),
            _ => None,
        };
        let contextual_submodule = match &row {
            NavRow::Submodule(index) => Some(self.submodules[*index].clone()),
            _ => None,
        };
        let branch_owner = cx.entity().downgrade();
        let branch_repository = self.path.clone();
        let contextual_worktree = match &row {
            NavRow::Worktree(index) => Some(self.worktrees[*index].clone()),
            _ => None,
        };
        let element = div()
            .id(self.nav_row_key(index))
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
        if let Some(target) = contextual_branch {
            let built_key = target.clone();
            element
                .app_context_menu(move |menu, window, cx| {
                    let (gone, wait) = branch_owner
                        .upgrade()
                        .map_or((true, None), |owner| (false, owner.read(cx).menu_wait()));
                    branch_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        target.clone(),
                        (gone, wait),
                        window,
                        cx,
                    )
                })
                .built_from(built_key)
                // A write's start or end builds the open menu again.
                .follows_writes(cx.entity().downgrade())
                .into_any_element()
        } else if let Some(tag) = contextual_tag {
            let built_key = (tag.name.clone(), tag.oid.clone(), tag.target_oid.clone());
            element
                .app_context_menu(move |menu, _, cx| {
                    let wait = branch_owner
                        .upgrade()
                        .and_then(|owner| owner.read(cx).menu_wait());
                    tag_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        tag.clone(),
                        wait,
                    )
                })
                .built_from(built_key)
                .follows_writes(cx.entity().downgrade())
                .into_any_element()
        } else if let Some(stash) = contextual_stash {
            let built_key = (stash.selector.clone(), stash.oid.clone());
            element
                .app_context_menu(move |menu, _, cx| {
                    let wait = branch_owner
                        .upgrade()
                        .and_then(|owner| owner.read(cx).menu_wait());
                    stash_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        stash.clone(),
                        wait,
                    )
                })
                .built_from(built_key)
                .follows_writes(cx.entity().downgrade())
                .into_any_element()
        } else if let Some(submodule) = contextual_submodule {
            let built_key = (
                submodule.path.clone(),
                submodule.index_oid.clone(),
                submodule.state,
            );
            element
                .app_context_menu(move |menu, _, cx| {
                    let wait = branch_owner
                        .upgrade()
                        .and_then(|owner| owner.read(cx).menu_wait());
                    submodule_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        submodule.clone(),
                        wait,
                    )
                })
                .built_from(built_key)
                .follows_writes(cx.entity().downgrade())
                .into_any_element()
        } else if let Some(tree) = contextual_worktree {
            element
                .app_context_menu(move |menu, _, cx| {
                    let refusal = branch_owner.upgrade().map_or((true, None), |owner| {
                        let owner = owner.read(cx);
                        (
                            owner.path != branch_repository || owner.page != AppPage::Repository,
                            owner.menu_wait(),
                        )
                    });
                    worktree_row_menu(
                        menu,
                        branch_owner.clone(),
                        branch_repository.clone(),
                        tree.clone(),
                        refusal,
                    )
                })
                // A write's start or end builds the open menu again.
                .follows_writes(cx.entity().downgrade())
                .into_any_element()
        } else {
            element.into_any_element()
        }
    }

    /// Shift-F10, or on Linux the Menu key, in the navigator: opens the
    /// cursor row's menu below it, the one a right-click on a branch or a
    /// worktree opens, its actions unavailable while a write runs and
    /// built again in place as one starts or ends. It stays with its branch
    /// or worktree as rows come or go above it, drawn beside its row where
    /// that row now stands, as the cursor stays on it
    /// (`rebuild_navigation`), and closes once its branch or worktree leaves
    /// the tree, deleted or renamed.
    pub(super) fn manage_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav_menu.is_some() || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(index) = self.nav_cursor else {
            return;
        };
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        // The cursor row still shows the branch or worktree the menu opened
        // on, wherever rows that came or went above it moved that row.
        let held = self.nav_row_key(index);
        let shown = repository.clone();
        let holds = move |app: &GitTurtle| {
            app.path == shown
                && app.nav_cursor.and_then(|index| app.nav_ids.get(index)) == Some(&held)
        };
        type Build =
            Box<dyn Fn(PopupMenu, &GitTurtle, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;
        let (depth, build): (usize, Build) = match self.nav_rows.get(index) {
            Some(NavRow::Branch(branch, depth)) => {
                let branch = &self.branches[*branch];
                let (name, remote) = (branch.name.clone(), branch.remote);
                (
                    *depth,
                    Box::new(move |menu, app: &GitTurtle, window, cx| {
                        let current = app
                            .branches
                            .iter()
                            .any(|b| b.remote == remote && b.name == name && b.current);
                        let detached_head = app
                            .work_status
                            .as_ref()
                            .is_some_and(|status| status.branch.is_none());
                        branch_row_menu(
                            menu,
                            owner.clone(),
                            repository.clone(),
                            BranchMenuTarget {
                                name: name.clone(),
                                remote,
                                current,
                                detached_head,
                            },
                            (false, app.menu_wait()),
                            window,
                            cx,
                        )
                    }),
                )
            }
            Some(NavRow::Tag(tag)) => {
                let tag = self.tags[*tag].clone();
                (
                    1,
                    Box::new(move |menu, app: &GitTurtle, _, _| {
                        tag_row_menu(
                            menu,
                            owner.clone(),
                            repository.clone(),
                            tag.clone(),
                            app.menu_wait(),
                        )
                    }),
                )
            }
            Some(NavRow::Stash(stash)) => {
                let stash = self.stashes[*stash].clone();
                (
                    1,
                    Box::new(move |menu, app: &GitTurtle, _, _| {
                        stash_row_menu(
                            menu,
                            owner.clone(),
                            repository.clone(),
                            stash.clone(),
                            app.menu_wait(),
                        )
                    }),
                )
            }
            Some(NavRow::Submodule(submodule)) => {
                let submodule = self.submodules[*submodule].clone();
                (
                    1,
                    Box::new(move |menu, app: &GitTurtle, _, _| {
                        submodule_row_menu(
                            menu,
                            owner.clone(),
                            repository.clone(),
                            submodule.clone(),
                            app.menu_wait(),
                        )
                    }),
                )
            }
            Some(NavRow::Worktree(tree)) => {
                let tree = self.worktrees[*tree].clone();
                (
                    0,
                    Box::new(move |menu, app: &GitTurtle, _, _| {
                        let elsewhere = app.path != repository || app.page != AppPage::Repository;
                        worktree_row_menu(
                            menu,
                            owner.clone(),
                            repository.clone(),
                            tree.clone(),
                            (elsewhere, app.menu_wait()),
                        )
                    }),
                )
            }
            _ => return,
        };
        // Drawn beside the row where the list lays it out, which
        // `rebuild_navigation` moves as rows come or go above it.
        let anchor = native_accessibility::ListRow::new(
            &self.nav_scroll,
            index,
            crate::appearance::ui_size(30.),
            nav_row_inset(depth, true),
        );
        self.nav_menu = Some(native_accessibility::KeyboardMenu::following(
            anchor,
            self.nav_focus.clone(),
            build,
            holds,
            // A branch's or a worktree's items read nothing HEAD changes.
            None,
            Self::dismiss_nav_menu,
            self,
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

    /// What History's scope toolbar counts, as it draws it: the commits a
    /// search found, or the first and last rows of the page shown, formatted
    /// only when they change ([`SCOPE_COUNT_TEXT`]), so a steady render
    /// formats nothing. Every frame formatted it.
    fn history_scope_count(&self) -> SharedString {
        let count = if self.history_search_active() {
            ScopeCount::Found(self.visible.len())
        } else {
            ScopeCount::Rows(
                self.history_paging.offset + usize::from(!self.visible.is_empty()),
                self.history_paging.offset + self.visible.len(),
            )
        };
        SCOPE_COUNT_TEXT.with_borrow_mut(|texts| texts.get(count, |&count| count.text()).clone())
    }

    /// The name of the scope History shows, as its toolbar draws it, made
    /// again only when the scope changes ([`SCOPE_NAME_TEXT`]). Every frame
    /// copied it twice.
    fn history_scope_name(&self) -> SharedString {
        let name = self.scope.as_ref().map(|(name, _)| name.as_str());
        SCOPE_NAME_TEXT.with_borrow_mut(|texts| {
            texts
                .get_where(
                    |kept| kept.as_deref() == name,
                    || name.map(str::to_owned),
                    |name| scope_name_text(name.as_deref()),
                )
                .clone()
        })
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
        let count = self.history_scope_count();
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
        .track_focus(&self.columns_focus)
        .on_click(cx.listener(|this, _, window, cx| {
            // A press leaves focus where it is, and closing the panel takes
            // a control inside it away: the button takes focus instead.
            if this.column_menu && this.columns_panel_focus.contains_focused(window, cx) {
                this.columns_focus.focus(window, cx);
            }
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
                .polite()
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

    /// History, laid out for the width it has in the frame that draws it
    /// ([`crate::in_frame`]): it takes that width
    /// ([`Self::take_history_width`]) before anything in the frame lays its
    /// columns, toolbar or search controls out. Measured as History was
    /// prepainted, the width came after the columns were built for the one
    /// before, so each width step asked for a second frame that laid them
    /// out for the new one.
    pub(super) fn render_history(&self, _: &Window, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                crate::in_frame::in_frame(move |bounds, window, cx| {
                    owner
                        .update(cx, |this, cx| {
                            // Before anything in this frame lays the columns
                            // out, the lane its new width reveals and the
                            // rows included.
                            let metrics = HistoryMetrics::current(window, this.widest_references());
                            this.history_metrics.set(metrics);
                            this.take_history_width(bounds.size.width);
                            this.render_history_at(window, cx)
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .named("history"),
            )
            .into_any_element()
    }

    /// Takes the width History is laid out at in this frame. Layout snaps
    /// it to device pixels, so one window width always measures the same.
    /// It is kept exactly: a tolerance would keep a nearby stale width, and
    /// Subject takes the leftover, so the columns would land somewhere else
    /// after navigation is hidden and shown again.
    ///
    /// Where the list was last laid out at another width, it reveals the
    /// selected commit's lane, so a list narrowed to fewer lanes, as narrow
    /// History's one, shows that lane as well as its row, which the list's
    /// viewport probe reveals ([`Self::list_viewport_probe`]); the header's
    /// lane buttons and the rows built in this frame read it. The list's own
    /// record decides, not the window's width: a warm tab brings back the
    /// layout and lanes it was left with, at a width the window may have
    /// taken while another tab showed, and revealed only for a new window
    /// width, a tab left at 1,480 px and shown again in a half tile kept its
    /// selected commit's lane out of its one. It reveals nothing while a cold
    /// tab is restored or before the list was first laid out, which keep a
    /// saved view, as the probe's reveal does not.
    fn take_history_width(&mut self, width: Pixels) {
        let taken = f32::from(width);
        if self.history_width != taken {
            #[cfg(test)]
            HISTORY_WIDTHS.with(|count| count.set(count.get() + 1));
            self.history_width = taken;
        }
        if self.repository_tabs.restoring.is_none()
            && self
                .history_list_layout
                .is_some_and(|(list, _)| list.width != width)
            && let Some(index) = self.selected_commit
            && self.visible.contains(&index)
        {
            self.reveal_graph_lane(index);
        }
    }

    /// History as [`Self::render_history`] builds it, for the width it took.
    fn render_history_at(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        let scope = self.history_scope_name();
        let metrics = self.history_metrics.get();
        let columns = self.history_column_layout();
        #[cfg(test)]
        HISTORY_LANES.with_borrow_mut(|lanes| {
            if let Some(lanes) = lanes {
                let rows = self.history_rows(cx);
                lanes.push((rows.lane_offset, self.layout_lane_capacity(&rows.columns)));
            }
        });
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
                    !(narrow && id == columns::ColumnId::Graph) || graph_title_fits(width, metrics);
                div()
                    .debug_selector(move || format!("history-column-{}", id as usize))
                    .relative()
                    .w(px(width))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    // A title's insets, where it has one: narrow History's
                    // untitled compact graph, 20 px wide, has no room for them,
                    // and at 18 pt their 23.5 px widened the header's cell
                    // past the rows', so the subject's title stood 3.5 px
                    // right of its rows.
                    .when(title, |cell| {
                        cell.pl(px(10.))
                            .pr_3()
                            .child(div().truncate().child(id.label()))
                    })
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
            .child(self.render_history_scope_toolbar(scope, window, cx))
            .child(
                div()
                    .h(crate::appearance::ui_size(38.))
                    .flex_shrink_0()
                    .px_3()
                    .pb_2()
                    .pt_1()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
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
                    .child(self.render_history_filter_toggle(cx)),
            )
            .children(self.render_history_filter_bar(cx))
            .child(self.render_history_search_controls(window, cx))
            .children(self.graph_notice.as_ref().map(|notice| {
                div().px_3().py_1().child(
                    native_accessibility::text("history-graph-notice", notice.clone())
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(colors.muted)),
                )
            }))
            .when(self.column_menu, |el| {
                el.child(
                    div()
                        .track_focus(&self.columns_panel_focus)
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
                                    .aria_size_of_set(self.visible.len())
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
                                    // Before the list, which applies its reveal
                                    // in the same frame
                                    // ([`Self::list_viewport_probe`]).
                                    .child(self.list_viewport_probe(true, cx))
                                    .child(history)
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
        // The SHA column keeps seven characters whole, whatever width was
        // saved; the saved width itself is untouched.
        let mut chosen = self.settings.columns.clone();
        chosen.sha.width = chosen.sha.width.max(metrics.sha_cell);
        chosen.history_layout(
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
        let range_span = self.history_range.as_ref().and_then(|range| {
            let mut anchor_pos = None;
            let mut target_pos = None;
            for (pos, &index) in self.visible.iter().enumerate() {
                if let Some(commit) = self.commits.get(index) {
                    if commit.oid == range.anchor_oid {
                        anchor_pos = Some(pos);
                    }
                    if commit.oid == range.target_oid {
                        target_pos = Some(pos);
                    }
                    if anchor_pos.is_some() && target_pos.is_some() {
                        break;
                    }
                }
            }
            let (a, t) = (anchor_pos?, target_pos?);
            Some((a.min(t), a.max(t)))
        });
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
            range_span,
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
            // Inside its row's ID, which its commit keys.
            let chip = div()
                .id("refs")
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
            "{}{}{} · {} · {} · {}{}",
            subject,
            listing,
            listed,
            author,
            short_oid(&commit.oid),
            crate::counted(commit.parents.len(), "parent", "parents"),
            if commit.parents.len() > 1 {
                " · merge commit"
            } else {
                ""
            }
        );
        let compare_marked = self.selected_compare_commit.as_deref() == Some(commit.oid.as_str());
        let range_role = match self.history_range.as_ref() {
            Some(range) if range.before_oid == commit.oid => HistoryRangeRole::Before,
            Some(range) if range.after_oid == commit.oid => HistoryRangeRole::After,
            Some(_)
                if rows
                    .range_span
                    .is_some_and(|(lo, hi)| position > lo && position < hi) =>
            {
                HistoryRangeRole::Middle
            }
            _ => HistoryRangeRole::None,
        };
        let range_endpoint = matches!(
            range_role,
            HistoryRangeRole::Before | HistoryRangeRole::After
        );
        #[cfg(test)]
        HISTORY_ROW_NAMES.with(|names| {
            names
                .borrow_mut()
                .insert(index, (name.clone(), pill_tooltip))
        });
        let row_label: SharedString = if compare_marked {
            format!("{name} · marked for comparison").into()
        } else {
            match range_role {
                HistoryRangeRole::Before => format!("{name} · range start").into(),
                HistoryRangeRole::After => format!("{name} · range end").into(),
                HistoryRangeRole::Middle => format!("{name} · in comparison range").into(),
                HistoryRangeRole::None => name.into(),
            }
        };
        let row = div()
            .id(commit_row_id(&commit.oid))
            .debug_selector(move || format!("history-row-{index}"))
            .role(Role::ListBoxOption)
            .aria_label(row_label)
            .aria_selected(active || compare_marked || range_role != HistoryRangeRole::None)
            .when(active, |row| row.aria_active_descendant())
            .aria_position_in_set(position)
            .w_full()
            .h(px(self.settings.density.history_row_height()))
            .flex()
            .items_center()
            .px_3()
            .gap_0()
            .bg(rgb(if active || range_endpoint {
                colors.selected
            } else if compare_marked || range_role == HistoryRangeRole::Middle {
                colors.subtle
            } else {
                colors.canvas
            }))
            .border_l_2()
            .border_color(rgb(if active || compare_marked || range_endpoint {
                colors.accent
            } else if range_role == HistoryRangeRole::Middle {
                colors.border
            } else {
                colors.canvas
            }))
            .hover(|s| s.bg(rgb(colors.row_hover(active || range_endpoint))))
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
                                active: active || range_endpoint,
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
                    // A commit's identifier in the interface font, as every
                    // identifier the app draws (`sha_cell_width`).
                    ColumnId::Sha => cell
                        .pl(px(10.))
                        .pr_3()
                        .truncate()
                        .text_size(crate::appearance::ui_text(11.))
                        .text_color(rgb(colors.muted))
                        .children(font_probe("history-sha"))
                        .child(short_oid(&commit.oid))
                        .into_any_element(),
                }
            }));
        let recovery_owner = cx.entity().downgrade();
        let recovery_repository = self.path.clone();
        let recovery_commit = recovery::commit_target(commit);
        let head_of = recovery_commit.clone();
        // The row is as wide as the table, which runs sideways past the
        // list's viewport, under the inspector and past the window's edge;
        // its node reports the part of it the list draws, as the list's own
        // node does, rather than extents to x 542.5 in a 461 px window.
        let click_oid = commit.oid.clone();
        native_accessibility::clips_children(row.on_click(cx.listener(
            move |this, event: &ClickEvent, window, cx| {
                if event.modifiers().secondary(cx) {
                    if this.history_range.is_some() {
                        this.clear_history_range(window, cx);
                    }
                    if this.selected_compare_commit.as_deref() == Some(click_oid.as_str()) {
                        this.selected_compare_commit = None;
                    } else {
                        this.selected_compare_commit = Some(click_oid.clone());
                    }
                    cx.notify();
                } else if event.modifiers().shift {
                    this.select_history_range(index, window, cx);
                } else {
                    this.selected_compare_commit = None;
                    this.select_commit(index, window, cx);
                }
            },
        )))
        .app_context_menu(move |menu, window, cx| {
            GitTurtle::recovery_context_menu(
                menu,
                &recovery_owner,
                &recovery_repository,
                &recovery_commit,
                window,
                cx,
            )
        })
        // A write's start or end builds the open menu again, as does the
        // commit becoming or no longer being HEAD.
        .follows_writes(cx.entity().downgrade())
        .follows_head_of(head_of)
        .into_any_element()
    }

    /// The inspector, its "Changed files" header laid out as `files_header`,
    /// which the window measures where it fixes the inspector's width.
    pub(super) fn render_inspector(
        &self,
        files_header: FilesHeader,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.file_history.is_active() {
            return self.render_file_history(window, cx);
        }
        if self.revision_inspection.is_active() {
            return self.render_revision_inspector(cx);
        }
        if let Some(range) = self.history_range.as_ref() {
            return self.render_history_range_inspector(range, files_header, cx);
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
        let extent = self.message_extent(commit, None, window, cx);
        div()
            .debug_selector(|| "commit-inspector".into())
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_l_1()
            .border_color(rgb(colors.border))
            .child(self.render_commit_message(commit, false, extent, window, cx))
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

    fn render_history_range_inspector(
        &self,
        range: &HistoryRange,
        files_header: FilesHeader,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let before_short = short_oid(&range.before_oid);
        let after_short = short_oid(&range.after_oid);
        let heading = format!("Comparing {before_short}..{after_short}");
        let heading_tip = format!("Comparing {}..{}", range.before_oid, range.after_oid);
        let subtitle = if range.swapped {
            format!(
                "{} · reversed",
                counted(range.commit_count, "commit", "commits")
            )
        } else {
            counted(range.commit_count, "commit", "commits")
        };
        let from_line = format!("From {before_short} · {}", range.before_subject);
        let to_line = format!("To {after_short} · {}", range.after_subject);
        let from_tip = format!("Before: {} — {}", range.before_oid, range.before_subject);
        let to_tip = format!("After: {} — {}", range.after_oid, range.after_subject);
        let spoken_summary = format!("{heading}. {subtitle}. {from_tip}. {to_tip}");
        let clear_tip = if self.mode == WorkspaceMode::History {
            format!("Clear commit range ({})", shortcuts::ESCAPE)
        } else {
            "Clear commit range".to_owned()
        };
        let header = native_accessibility::clips_children(
            div()
                .id("history-range-header")
                .debug_selector(|| "history-range-header".into())
                .role(Role::Group)
                .aria_label(spoken_summary)
                .flex_shrink_0()
                .min_w_0()
                .px_3()
                .py_2p5()
                .border_b_1()
                .border_color(rgb(colors.border))
                .flex()
                .flex_col()
                .gap_1p5()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            native_accessibility::static_text(
                                div()
                                    .id("history-range-title")
                                    .debug_selector(|| "history-range-title".into()),
                                heading.clone(),
                            )
                            .min_w_0()
                            .truncate()
                            .text_size(appearance::ui_text(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(colors.text))
                            .tooltip(move |window, cx| {
                                Tooltip::new(heading_tip.clone()).build(window, cx)
                            })
                            .child(heading),
                        )
                        .child(
                            native_accessibility::static_text(
                                div()
                                    .id("history-range-count")
                                    .debug_selector(|| "history-range-count".into()),
                                subtitle.clone(),
                            )
                            .min_w_0()
                            .truncate()
                            .text_size(appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(subtitle),
                        ),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .text_size(appearance::ui_text(11.))
                        .text_color(rgb(colors.muted))
                        .child(
                            native_accessibility::static_text(
                                div().id("history-range-from"),
                                from_tip.clone(),
                            )
                            .min_w_0()
                            .truncate()
                            .tooltip(move |window, cx| {
                                Tooltip::new(from_tip.clone()).build(window, cx)
                            })
                            .child(from_line),
                        )
                        .child(
                            native_accessibility::static_text(
                                div().id("history-range-to"),
                                to_tip.clone(),
                            )
                            .min_w_0()
                            .truncate()
                            .tooltip(move |window, cx| {
                                Tooltip::new(to_tip.clone()).build(window, cx)
                            })
                            .child(to_line),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1p5()
                        .pt_0p5()
                        .child(
                            button("history-range-swap", "Swap", "", false)
                                .debug_selector(|| "history-range-swap".into())
                                .accessibility_label("Swap comparison direction")
                                .tooltip("Swap before and after revisions")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.swap_history_range(window, cx);
                                })),
                        )
                        .child(
                            button("history-range-clear", "Clear", "", false)
                                .debug_selector(|| "history-range-clear".into())
                                .accessibility_label("Clear commit range")
                                .tooltip(clear_tip)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.clear_history_range(window, cx);
                                })),
                        ),
                ),
        );
        div()
            .debug_selector(|| "history-range-inspector".into())
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_l_1()
            .border_color(rgb(colors.border))
            .child(header)
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
        // The list paints the metadata's whole row once its top is in view,
        // so Tab reaches a parent's control below the message's fold, where
        // the frame cuts it away: revealed whole with its ring's room as the
        // keyboard moves onto it, by the message's reveal
        // (`commit_message::State::parent_reveal`). Below what the list
        // draws at all, Tab meets a stand-in in the control's place, and the
        // control it brings in asks the list to show it as it prepaints
        // (`commit_message::ParentStops`).
        let reveal = self.inspector_message.parent_reveal();
        let stops = self.inspector_message.parent_stops();
        let room = appearance::button_ring_room(cx);
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
            let focus = self.inspector_message.parent_focus(i, cx);
            let control = control
                .track_focus(&focus)
                .debug_selector(move || format!("commit-parent-{i}"))
                .tooltip(name)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.change_parent(i, window, cx)),
                );
            parents = parents.child(match (&reveal, &stops) {
                (Some(reveal), Some(stops)) => {
                    let report = reveal.control(focus.clone(), room);
                    let stops = stops.clone();
                    gpui_kit::base::ElementExt::on_prepaint(control, move |bounds, window, cx| {
                        report(vec![bounds], window, cx);
                        stops.prepainted(&focus, bounds, room, window);
                    })
                    .into_any_element()
                }
                _ => control.into_any_element(),
            });
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
                // The commit's identifier ends the date's line, which an
                // element of its own holds, so a test reads the face the
                // identifier takes where it is drawn.
                .child(
                    div()
                        .child(date)
                        .children(font_probe("inspector-identifier")),
                )
                .child(parents)
                .children(
                    self.commit_signature
                        .as_ref()
                        .filter(|(oid, _)| oid == &commit.oid)
                        .map(|(_, sig)| {
                            let (badge_label, raw_color) = match sig.trust {
                                gitturtle_core::SignatureTrust::Good => {
                                    (format!("Verified {}", sig.format.label()), colors.accent)
                                }
                                gitturtle_core::SignatureTrust::GoodUntrusted
                                | gitturtle_core::SignatureTrust::ExpiredSignature
                                | gitturtle_core::SignatureTrust::ExpiredKey
                                | gitturtle_core::SignatureTrust::MissingKey
                                | gitturtle_core::SignatureTrust::Unverified => (
                                    format!("Signed ({} · unverified)", sig.format.label()),
                                    colors.warning,
                                ),
                                gitturtle_core::SignatureTrust::Bad
                                | gitturtle_core::SignatureTrust::RevokedKey => (
                                    format!("Invalid {} signature", sig.format.label()),
                                    colors.removed,
                                ),
                            };
                            let color = colors.small_label(raw_color, &[colors.panel]);
                            let mut tip_parts = vec![badge_label.clone()];
                            if let Some(signer) = &sig.signer {
                                tip_parts.push(format!("Signer: {signer}"));
                            }
                            if let Some(key) = &sig.key_id {
                                tip_parts.push(format!("Key: {key}"));
                            }
                            tip_parts.push(sig.detail.clone());
                            let tip = tip_parts.join("\n");
                            let spoken = tip_parts.join(". ");
                            native_accessibility::static_text(
                                div()
                                    .id("commit-signature-badge")
                                    .debug_selector(|| "commit-signature-badge".into()),
                                spoken,
                            )
                            .self_start()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(7.))
                            .border_1()
                            .border_color(rgb(color))
                            .text_size(appearance::ui_text(10.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(color))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                    .build(window, cx)
                            })
                            .child(badge_label)
                        }),
                ),
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
                    this.change_file_filter(window, cx);
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
                    // Before the list, which applies its reveal in the same
                    // frame ([`Self::list_viewport_probe`]).
                    .child(self.list_viewport_probe(false, cx))
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
                    } else if visible.is_empty()
                        && let Some(error) = &self.error
                        && self.history_range.is_some()
                    {
                        empty("Could not compare commits", error)
                    } else if visible.is_empty() && self.file_filter.read(cx).value().is_empty() {
                        EmptyState::new(
                            "files-empty",
                            if self.history_range.is_some() {
                                "No file changes in this commit range"
                            } else {
                                "No file changes against this parent"
                            },
                        )
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
                    .children(self.file_menu.as_ref().map(|menu| {
                        menu.render("changed-file-menu-overlay", Self::dismiss_file_menu, cx)
                    })),
            ))
            .into_any_element()
    }

    /// Opens the selected commit's menu beside its row, from Shift-F10 or the
    /// Menu key: the menu a right-click on the row opens. It ends at the
    /// list's trailing inset in the part of the list the viewport shows,
    /// where the columns scroll sideways, so it leaves the graph's lanes in
    /// view. It belongs to its commit, not to the row's place: where a
    /// quiet reload brings commits above it, it stays open beside the row
    /// that holds its commit now, and it closes only once no row does.
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
        let oid = commit.oid.clone();
        let commit = recovery::commit_target(commit);
        let inset = appearance::ui_size(appearance::PANE_INSET);
        // The list's right edge in the part the viewport shows, inside the
        // 1 px edge drawn there (`history_list_edge`).
        let list = self.history_scroll.0.borrow().base_handle.bounds().right();
        let shown = self.history_horizontal.bounds().right() - px(1.);
        // Its row lies inside that edge, over the list's 1 px padding: a
        // row whose foot lies under it opens its menu without moving the
        // list.
        let anchor = native_accessibility::ListRow::new(
            &self.history_scroll,
            row,
            window.pixel_snap(px(self.settings.density.history_row_height())),
            inset,
        )
        .trailing(list.min(shown) - inset)
        .framed(px(1.));
        let place = anchor.place();
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        // A row still holds the commit, and the menu is drawn beside it: an
        // Undo or an Amend takes the commit from History as the write's
        // quiet reload lands, and commits arriving above it move its row
        // down. The row it was last beside is asked first, so a check while
        // nothing moved reads one row.
        let shown = repository.clone();
        // The selected commit a reload took away stays listed, retained at
        // the list's end for its inspector, but no longer in History: its
        // menu goes, as one opened on HEAD before an Undo must, which
        // offered a Revert of the commit the Undo had taken away.
        let holds = move |app: &GitTurtle| {
            let holds = |row: usize| {
                app.visible
                    .get(row)
                    .filter(|index| app.automatic.retained_commit != Some(**index))
                    .and_then(|index| app.commits.get(*index))
                    .is_some_and(|commit| commit.oid == oid)
            };
            let row = Some(place.get())
                .filter(|row| holds(*row))
                .or_else(|| (0..app.visible.len()).find(|row| holds(*row)));
            if let Some(row) = row {
                place.set(row);
            }
            app.path == shown && row.is_some()
        };
        let head_of = commit.clone();
        let build = Self::keyboard_commit_menu(owner, repository, commit);
        self.commit_menu = Some(
            native_accessibility::KeyboardMenu::following(
                anchor,
                self.focus.clone(),
                build,
                holds,
                // Built again as the commit becomes or stops being HEAD,
                // which adds or takes away Amend and Undo.
                Some(head_of),
                Self::dismiss_commit_menu,
                self,
                window,
                cx,
            )
            .refuses_while_paused(Self::refuse_commit_menu_while_paused),
        );
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
    /// It belongs to its file, by path, not to the row's place: where the
    /// rows change under it, as a slow path filter's answer or the commit's
    /// files landing again replace them, it stays open beside the row that
    /// holds its file now, and it closes, focus back on the list, only once
    /// no row does or another commit's files are shown.
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
        let commit = self.changed_file_revision(&file);
        let range_pair = self
            .history_range
            .as_ref()
            .map(|range| (range.before_oid.clone(), range.after_oid.clone()));
        let anchor = native_accessibility::ListRow::new(
            &self.file_scroll,
            row,
            window.pixel_snap(px(self.settings.density.file_row_height())),
            appearance::ui_size(appearance::PANE_INSET),
        )
        // Inside the pane's 1 px border, as the commit list is in its edge.
        .framed(px(1.));
        let place = anchor.place();
        let (owner, repository) = (cx.entity().downgrade(), self.path.clone());
        let (shown, of) = (repository.clone(), commit.clone());
        // The entry by both its paths, so two entries sharing one path, as a
        // rename's source and a file added in its place, stay apart.
        let entry = (file.old_path.clone(), file.new_path.clone());
        let menu_file = file.clone();
        // A row still holds the file, and the menu is drawn beside it. The
        // row it was last beside is asked first, so a check while nothing
        // moved reads one row; the rows are the list's own, read again only
        // where they changed.
        let holds = move |app: &GitTurtle| {
            let current_range = app
                .history_range
                .as_ref()
                .map(|range| (range.before_oid.as_str(), range.after_oid.as_str()));
            let expected_range = range_pair
                .as_ref()
                .map(|(before, after)| (before.as_str(), after.as_str()));
            if app.path != shown
                || app.changed_file_revision(&menu_file) != of
                || current_range != expected_range
            {
                return false;
            }
            let is_entry = |file: &FileChange| file.old_path == entry.0 && file.new_path == entry.1;
            // While the filter's answer for the files as they now stand is
            // pending, as the commit's files landing again under a query
            // leave it, no row is known yet: the menu stays while the files
            // hold its entry, its place kept until the rows are known.
            let Some(rows) = app.answered_file_rows() else {
                return app.files.iter().any(&is_entry);
            };
            let holds = |row: usize| {
                rows.get(row)
                    .and_then(|index| app.files.get(*index))
                    .is_some_and(is_entry)
            };
            let row = Some(place.get())
                .filter(|row| holds(*row))
                .or_else(|| (0..rows.len()).find(|row| holds(*row)));
            if let Some(row) = row {
                place.set(row);
            }
            row.is_some()
        };
        let paused = self.integration_state.is_some();
        let wait = self.menu_wait();
        self.file_menu = Some(native_accessibility::KeyboardMenu::open_while(
            anchor,
            self.file_focus.clone(),
            move |menu, window, cx| {
                changed_file_menu(
                    menu, owner, repository, file, commit, paused, wait, window, cx,
                )
            },
            holds,
            Self::dismiss_file_menu,
            window,
            cx,
        ));
        cx.notify();
    }

    /// The text the comparison's pane shows this frame, whose keyboard focus
    /// draws a ring on its edge (`FocusRing::edge`): the editor of the patch,
    /// the Before or the After text, which the pane rings, or Split, whose
    /// sides ring their own. `None` where the pane shows anything else, as
    /// an error ("Preview unavailable"), an empty patch, Blame, a document or
    /// an image, though an editor kept from before may still hold focus. The
    /// preview's ring and the inspector's row marker both ask it, so the row
    /// gives up its accent only where the ring is drawn.
    pub(super) fn shown_text(&self) -> Option<ShownText<'_>> {
        if !self.shows_selected_file() {
            return None;
        }
        let Some(Content::Text { patch, .. }) = self.content.as_deref() else {
            return None;
        };
        match self.text_mode {
            TextMode::Split => self.split_view.as_ref().map(ShownText::Split),
            TextMode::Unified if patch.is_empty() => None,
            TextMode::Unified => self.patch_editor.as_ref().map(ShownText::Editor),
            TextMode::Before => self.before_editor.as_ref().map(ShownText::Editor),
            TextMode::After => self.after_editor.as_ref().map(ShownText::Editor),
            TextMode::Formatted => self.formatted_editor.as_ref().map(ShownText::Editor),
            TextMode::Diagrams | TextMode::Markdown | TextMode::Table => None,
        }
    }

    /// Whether the comparison's pane shows the selected file's content: out
    /// of History, with a file selected, and no error or Blame in its place.
    fn shows_selected_file(&self) -> bool {
        self.mode != WorkspaceMode::History
            && !self.blame.is_visible()
            && self.error.is_none()
            && self
                .selected_file
                .and_then(|index| self.files.get(index))
                .is_some()
    }

    /// Whether the comparison's text draws the keyboard ring on its pane's
    /// edge this frame ([`Self::shown_text`]), in Compare or a Working
    /// preview, or Blame's lines or the rendered Markdown, which ring their
    /// own, stand in its place. The inspector's selected row, whose leading
    /// marker stands a few pixels right of that edge, then leaves the accent
    /// to the ring.
    pub(crate) fn text_ring_shows(&self, cx: &App) -> bool {
        // Blame's lines, shown over the comparison, ring their own edge.
        if self.blame.is_visible() {
            return self.blame.ring_shows(&self.focus_ring);
        }
        let shows =
            |editor: &Entity<EditorState>| self.focus_ring.shows(&editor.read(cx).focus_handle(cx));
        match self.shown_text() {
            Some(ShownText::Editor(editor)) => shows(editor),
            Some(ShownText::Split(view)) => view.read(cx).editors().iter().any(shows),
            // The rendered Markdown, the selected file's, rings its own
            // edge.
            None => {
                self.text_mode == TextMode::Markdown
                    && self.shows_selected_file()
                    && self
                        .markdown_view
                        .as_ref()
                        .is_some_and(|view| view.read(cx).ring_shows())
            }
        }
    }

    /// The leading marker of the inspector's selected row: the accent, except
    /// while the comparison's text beside it shows its keyboard ring
    /// ([`Self::text_ring_shows`]), where the ring is the one accent stroke
    /// and the row's selected fill keeps the selection in view; at 13 pt in a
    /// 461 px window the two read as one doubled stroke. Transparent, the
    /// marker shows the row's own fill, hovered or not, and nothing moves.
    pub(crate) fn selected_row_marker(&self, cx: &App) -> Rgba {
        if self.text_ring_shows(cx) {
            rgba(0)
        } else {
            rgb(palette(cx).accent)
        }
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
        let history_commit = self.changed_file_revision(file);
        let (symbol, color, label) = match file.status.letter() {
            "A" => ("file-added", colors.added, "New"),
            "D" => ("file-deleted", colors.removed, "Deleted"),
            "R" => ("file-renamed", colors.renamed, "Renamed"),
            "T" => ("file-type", colors.modified, "Type"),
            _ => ("file-modified", colors.modified, "Modified"),
        };
        let row = div()
            .id(changed_file_id(file))
            .debug_selector(move || {
                if active {
                    "selected-changed-file".into()
                } else {
                    format!("changed-file-{index}")
                }
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
            // Its tile stands at the inspector's edge, where the header's
            // icon and the toolbar's frames do: the list's 1 px edge and the
            // row's 2 px marker come out of the pane's inset (DESIGN.md, "One
            // left edge"). It stood 3 px past them.
            .pl(crate::appearance::ui_size(crate::appearance::PANE_INSET) - px(3.))
            .pr_3()
            .bg(rgb(if active {
                colors.selected
            } else {
                colors.panel
            }))
            .border_l_2()
            .border_color(if active {
                self.selected_row_marker(cx)
            } else {
                rgb(colors.panel)
            })
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
                    // Each line fits whole or ends in an ellipsis inside the
                    // row, never cut by its side, as a Working row's do.
                    .child(
                        ellipsis_end(
                            div().debug_selector(move || format!("changed-file-name-{index}")),
                        )
                        .text_size(crate::appearance::ui_text(12.))
                        .child(
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )
                    .child(
                        ellipsis_end(
                            div().debug_selector(move || format!("changed-file-directory-{index}")),
                        )
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
            .app_context_menu(move |menu, window, cx| {
                let (paused, wait) = history_owner.upgrade().map_or((false, None), |owner| {
                    let owner = owner.read(cx);
                    (owner.integration_state.is_some(), owner.menu_wait())
                });
                changed_file_menu(
                    menu,
                    history_owner.clone(),
                    history_repository.clone(),
                    history_file.clone(),
                    history_commit.clone(),
                    paused,
                    wait,
                    window,
                    cx,
                )
            })
            .follows_writes(cx.entity().downgrade())
            .into_any_element()
    }
    /// `column_min` is the preview column's narrowest width; the path row keeps
    /// 220 px before the modes wrap below it, but never more than that column
    /// holds, and its buttons wrap below the path rather than clip.
    ///
    /// The preview is laid out for the column's width in the frame it is
    /// drawn ([`crate::in_frame`]): its toolbar's groups (the path's actions,
    /// the text modes, or an image's modes and zooms), the review options,
    /// the footer's whole or short line and what gives way beneath an image
    /// are chosen from that width, so a width change never draws a frame
    /// with the choice of the frame before.
    pub(super) fn render_preview(&self, column_min: Pixels, cx: &mut Context<Self>) -> AnyElement {
        if self.blame.is_visible() {
            // Laid out for the pane's size in the frame it is drawn, as the
            // preview below is: what Blame's header shows is chosen from it,
            // and a dragged inspector moves it, not the window.
            let owner = cx.entity().downgrade();
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    crate::in_frame::in_frame(move |bounds, window, cx| {
                        owner
                            .update(cx, |this, cx| this.render_blame(bounds.size, window, cx))
                            .unwrap_or_else(|_| div().into_any_element())
                    })
                    .named("blame"),
                )
                .into_any_element();
        }
        let owner = cx.entity().downgrade();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                crate::in_frame::in_frame(move |bounds, window, cx| {
                    owner
                        .update(cx, |this, cx| {
                            this.render_preview_at(column_min, bounds.size.width, window, cx)
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .named("compare-preview"),
            )
            .into_any_element()
    }

    /// The preview [`Self::render_preview`] draws, in a column `column` wide.
    fn render_preview_at(
        &self,
        column_min: Pixels,
        column: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let quick_source = self.is_quick_source();
        // A marked tab's Changes shows nothing from before: a working file
        // previewed before the mark is the last status's, so the column
        // draws no path, actions, modes or footer for it and says the mark
        // (below). It kept that file's preview and toolbar beside the
        // list's "Changes unavailable".
        let marked_working = self.mode == WorkspaceMode::Working && self.shown_tab_unavailable();
        let file = (self.selected_file)
            .and_then(|index| self.files.get(index))
            .filter(|_| !marked_working);
        let path = file
            .map(|file| file.path().to_string_lossy().into_owned())
            .unwrap_or("File comparison".into());
        let copy_path = path.clone();
        let history_index = self
            .selected_file
            .filter(|_| self.mode != WorkspaceMode::Working && !self.file_history.is_active());
        let text = !marked_working && matches!(self.content.as_deref(), Some(Content::Text { .. }));
        let blame_index = if self.mode == WorkspaceMode::Working {
            self.working_selected.map(|(index, _)| index)
        } else {
            self.selected_file
        }
        .filter(|_| text);
        // The path keeps its room before its actions keep theirs: Blame
        // gives up its label, then the actions fold into one menu.
        let row = file.map_or(PathRow::WHOLE, |file| {
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
                    .filter(|_| self.mode == WorkspaceMode::Working && !marked_working)
                    .map(|(_, area)| {
                        let staged = area == gitturtle_core::ChangeArea::Staged;
                        let labels = crate::appearance::labels(cx);
                        div()
                            .debug_selector(|| "preview-change-area".into())
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
            .child({
                // Where not even the actions' menu leaves it room, the path
                // is elided in its middle, its top directory and its file
                // name staying (`path_tail`), and where not even "…/" and the
                // name fit beside the actions, the name alone is elided in
                // its middle on their line, keeping its start and its
                // extension; the whole path is its tooltip and its
                // accessible name.
                let whole: SharedString = file
                    .map(|file| file.path().display().to_string())
                    .unwrap_or("File comparison".into())
                    .into();
                let shown = shown_compare_path(&whole, row.room, window);
                #[cfg(test)]
                COMPARE_PATH_SHOWN.with(|shown_path| {
                    *shown_path.borrow_mut() = Some((shown.clone(), whole.clone()))
                });
                // The line's own middle elision guards a measurement a
                // fraction of a pixel short.
                ellipsis_middle(
                    div()
                        .id("preview-file-path")
                        .debug_selector(|| "preview-file-path".into())
                        .flex_1()
                        .min_w_0(),
                )
                .role(Role::Label)
                .aria_label(whole.clone())
                .aria_value(whole)
                .text_size(crate::appearance::ui_text(12.))
                .font_weight(FontWeight::MEDIUM)
                .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                .child(shown)
            });
        if let Some(file) = file.filter(|_| row.actions == PathActions::Menu) {
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
                        .debug_selector(|| "open-file-history".into())
                        .accessibility_label("File history")
                        .tooltip("File history · follow renames along the first parent")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_file_history(index, window, cx)
                        }))
                }));
        }
        if let Some(index) = blame_index.filter(|_| row.actions != PathActions::Menu) {
            const DETAIL: &str =
                "Line attribution and history; working files include uncommitted lines";
            // Blame keeps its label where the whole path fits beside it;
            // otherwise its icon, so the path is not cut for a label.
            let labelled = row.actions == PathActions::Labelled;
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
            path_controls = path_controls.child(crate::in_frame::probed(
                blame.debug_selector(|| "open-blame".into()).on_click(
                    cx.listener(move |this, _, window, cx| this.open_blame(index, window, cx)),
                ),
                "open-blame",
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
        // Beneath the preview, the file's status, modes and object IDs on one
        // line. Narrower than its line, the footer drops the object IDs first
        // and then truncates with an ellipsis; its tooltip keeps the whole
        // line. An image comparison draws it at its own foot, where it gives
        // way before the image's minimum ([`CompareFooter`]). Quick source
        // draws none: its one file's mode leads the information's
        // identities ("Mode 100644"), so nothing beneath the pane changes
        // as a file lands there, and an image's minimum gives way only to
        // its own labels (drawn beneath the pane, its line outlasted the
        // image).
        let mut footer = file.filter(|_| !quick_source).map(|file| {
            let short = format!(
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
            let line = format!(
                "{short}   ·   {} → {}",
                file.old_oid.as_deref().map(short_oid).unwrap_or("—".into()),
                file.new_oid.as_deref().map(short_oid).unwrap_or("—".into())
            );
            let fits = cached_text_width(&FOOTER_LINE, &line, window, || {
                line_widths(&[(&line, appearance::ui_text(10.))], window)
            }) <= column - window.rem_size() * 1.5;
            let shown = if fits { line.clone() } else { short };
            let line = SharedString::from(line);
            let whole = line.clone();
            CompareFooter {
                element: div()
                    .id("compare-footer")
                    .debug_selector(|| "compare-footer".into())
                    .h(crate::appearance::ui_size(COMPARE_FOOTER_HEIGHT))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(colors.muted))
                    .border_t_1()
                    .border_color(rgb(colors.border))
                    .when(!fits, |footer| {
                        footer.tooltip(move |window, cx| {
                            Tooltip::new(whole.clone()).build(window, cx)
                        })
                    })
                    .child(crate::in_frame::probed(
                        div()
                            .debug_selector(|| "compare-footer-text".into())
                            .min_w_0()
                            .truncate()
                            .child(shown),
                        // Which line it shows, for tests.
                        if fits {
                            "compare-footer-line"
                        } else {
                            "compare-footer-short"
                        },
                    ))
                    .into_any_element(),
                line,
            }
        });
        // The editor the text pane shows, which draws no edge of its own, so
        // the pane rings its edge for it ([`Self::shown_text`]).
        // So does Quick source's pane while it holds focus with no text
        // (`GitTurtle::quick_source_holds_pane`), on the same edge.
        let text_focus = match self.shown_text() {
            Some(ShownText::Editor(editor)) => Some(editor.read(cx).focus_handle(cx)),
            _ => self
                .quick_source_holds_pane()
                .then(|| self.source_focus.clone()),
        };
        // Split's text, which shares its room with the footer
        // ([`split_room`]).
        let mut split = None;
        let content = if let Some(error) = &self.error {
            empty("Preview unavailable", error)
        } else if marked_working {
            // A marked tab's status is the last one read, beside the list's
            // "Changes unavailable": the preview says the mark and its way
            // back rather than "Working tree clean", "Choose a file" or the
            // file chosen before the mark.
            empty(
                crate::branch_actions::UNAVAILABLE_REASON,
                crate::branch_actions::UNAVAILABLE_WAY_BACK,
            )
        } else if file.is_none()
            && self.mode == WorkspaceMode::Working
            && self.loading.is_none()
            && self
                .header_status()
                .is_some_and(|status| status.entries.is_empty())
        {
            EmptyState::new(
                "working-preview-clean",
                empty_state::working_clean_line(self.header_status(), true),
            )
            .pane(cx)
        } else if file.is_none()
            && self.mode == WorkspaceMode::Working
            && self.working_status_unread()
        {
            // Beside a list that has no rows until the status is read, the
            // preview says the read as the list does, where it said "Choose
            // a file", or the Open's "Reading local history…", inviting a
            // choice from a list with none.
            EmptyState::new(
                "working-preview-reading",
                crate::workspace::READING_WORKING_TREE,
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
                    table,
                    structured,
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
                        (TextMode::Table, "Table"),
                        (TextMode::Formatted, "Formatted"),
                    ]
                    .into_iter()
                    .filter(|&(mode, _)| {
                        !((quick_source
                            && !matches!(
                                mode,
                                TextMode::After
                                    | TextMode::Diagrams
                                    | TextMode::Markdown
                                    | TextMode::Table
                                    | TextMode::Formatted
                            ))
                            || (mode == TextMode::Diagrams && diagrams.is_none())
                            || (mode == TextMode::Markdown && markdown.is_none())
                            || (mode == TextMode::Table && table.is_none())
                            || (mode == TextMode::Formatted && structured.is_none()))
                    })
                    .collect::<Vec<_>>();
                    let names = available.iter().map(|(_, name)| *name).collect::<Vec<_>>();
                    // Narrower than the labelled group, the modes keep one
                    // row as a menu named by the current mode, as the review
                    // options keep theirs.
                    let collapsed =
                        column - window.rem_size() * 1.5 < text_modes_width(&names, window);
                    // Transfer focus cleanly when resizing hides the mode
                    // button strip behind the collapsed menu or restores it,
                    // so keyboard focus is never stranded on an unrendered control.
                    if collapsed {
                        if self
                            .text_mode_focus
                            .iter()
                            .any(|handle| handle.is_focused(window))
                        {
                            let target = self.text_mode_menu_focus.clone();
                            window.defer(cx, move |window, cx| {
                                target.focus(window, cx);
                            });
                        }
                    } else if self.text_mode_menu_focus.is_focused(window)
                        || self
                            .text_mode_focus
                            .iter()
                            .enumerate()
                            .any(|(index, handle)| {
                                handle.is_focused(window)
                                    && !available.iter().any(|(mode, _)| mode.index() == index)
                            })
                    {
                        let active = available
                            .iter()
                            .find(|(mode, _)| *mode == self.text_mode)
                            .or_else(|| available.first())
                            .map(|(mode, _)| self.text_mode_focus[mode.index()].clone());
                        if let Some(target) = active {
                            window.defer(cx, move |window, cx| {
                                target.focus(window, cx);
                            });
                        }
                    }
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
                                .track_focus(&self.text_mode_focus[mode.index()])
                                .debug_selector(move || name.into())
                                .toggled(self.text_mode == mode)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.choose_text_mode(mode, window, cx);
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
                        let menu = crate::in_frame::probed(
                            compact_control_label(
                                button("text-mode-menu", label, "", false)
                                    .track_focus(&self.text_mode_menu_focus),
                                "text-mode-menu",
                                format!("Comparison mode: {label}"),
                            ),
                            "text-mode-menu",
                        )
                        .debug_selector(|| "text-mode-menu".into())
                        .px(appearance::ui_size(TEXT_MODE_MENU_INSET))
                        .when(own_row, |menu| {
                            menu.ml(-appearance::ui_size(TEXT_MODE_MENU_INSET))
                        })
                        .dropdown_caret(true)
                        .tooltip(format!("Comparison mode · {}", names.join(", ")))
                        // Where focus is as a press opens the menu, before
                        // the menu takes it, which a choice acts on.
                        .capture_any_mouse_down({
                            let owner = owner.clone();
                            move |event, window, cx| {
                                if event.button == MouseButton::Left
                                    && let Some(app) = owner.upgrade()
                                {
                                    app.read(cx).note_mode_menu_press(window, cx);
                                }
                            }
                        })
                        .app_menu(move |menu, window, cx| {
                            // Built when opened; a choice acts on the page
                            // as it is then, as a mode button does, and on
                            // focus where it was as the menu opened, since
                            // the menu holds it while the choice acts. The
                            // note is taken here, once: the menu must not be
                            // built again while open, or the note is lost.
                            // A key-opened menu acts on none, even one a
                            // press the menu never answered left.
                            let note = owner
                                .upgrade()
                                .and_then(|app| app.read(cx).take_mode_menu_note())
                                .filter(|_| !window.last_input_was_keyboard());
                            available.iter().fold(menu, |menu, &(mode, name)| {
                                let owner = owner.clone();
                                let note = note.clone();
                                menu.item(
                                    PopupMenuItem::new(name).checked(mode == current).on_click(
                                        move |_, window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                if this.page == AppPage::Repository {
                                                    // Noted in this
                                                    // inspection, not one
                                                    // shown since.
                                                    let held = note.as_ref().is_some_and(|note| {
                                                        this.mode_menu_note_holds(note)
                                                    });
                                                    this.choose_text_mode_held(
                                                        mode, held, window, cx,
                                                    );
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
                        toolbar.child(crate::in_frame::probed(modes, "text-modes"))
                    };
                    let editor = match self.text_mode {
                        TextMode::Split
                        | TextMode::Diagrams
                        | TextMode::Markdown
                        | TextMode::Table => &None,
                        TextMode::Unified => &self.patch_editor,
                        TextMode::Before => &self.before_editor,
                        TextMode::After => &self.after_editor,
                        TextMode::Formatted => &self.formatted_editor,
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
                    } else if self.text_mode == TextMode::Table {
                        table.as_ref().map_or_else(
                            || {
                                empty(
                                    "No tabular preview",
                                    "Use the source tabs to inspect this file.",
                                )
                            },
                            |comparison| {
                                crate::structured_view::render_tabular(
                                    comparison,
                                    quick_source,
                                    colors,
                                    &self.source_focus,
                                    &self.tabular_scroll,
                                    self.focus_ring.edge(&self.source_focus, rgb(colors.accent)),
                                )
                            },
                        )
                    } else if self.text_mode == TextMode::Formatted
                        && let Some(structured) = structured
                        && let Err(reason) = structured.display_text()
                    {
                        let title = format!("{} parse issue", structured.format.label());
                        let detail = format!(
                            "{reason}\n\nExact source and Git diff remain unchanged in the other tabs."
                        );
                        empty(&title, &detail)
                    } else if self.text_mode == TextMode::Split {
                        match &self.split_view {
                            Some(view) => {
                                split = Some(view.clone());
                                div().into_any_element()
                            }
                            None if self.loading.is_some() => {
                                empty("Loading split comparison…", "")
                            }
                            None => empty(
                                self.error
                                    .as_deref()
                                    .unwrap_or("Split comparison unavailable"),
                                "",
                            ),
                        }
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
                Content::Images { old, new } => {
                    self.render_image_comparison(old, new, column, footer.take(), window, cx)
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
        let content = self.quick_source_pane(content);
        // Beneath a failing Git LFS side's line the comparison keeps its
        // rows and the line gives way first. Beside a Download row, which
        // keeps its height and must stay pressable, it keeps one row.
        let download_offered = self.lfs_download_offered();
        let lfs_floor = self.lfs_failure_sentence().is_some().then(|| {
            comparison_floor(
                if download_offered {
                    1.
                } else {
                    SPLIT_FIND_ROWS
                },
                window,
            )
        });
        // Split's room also holds its sides' headings. It asks for them and
        // its rows with Compare's footer above, and gives way in one order,
        // with or without a Download row: the line first, to its one line;
        // then the footer (`split_room`); the headings and the rows stay.
        // Where the column cannot hold them beneath the line at its least
        // and a Download row, which keeps its height, the room takes what
        // the column leaves it and no more, its headings first, and its
        // text what remains: at 18 pt in a 461 × 490 window, with no floor
        // there, the line kept all its lines and left Split a strip 9
        // device pixels tall with no heading's words, and at 461 × 440, its
        // headings kept whole as its least, the room ran 4 px under the
        // status bar.
        let split_basis = lfs_floor.map(|floor| {
            let heading =
                window.pixel_snap(appearance::ui_size(crate::split_diff::SIDE_HEADING_HEIGHT));
            let footer_room = footer.as_ref().map_or(px(0.), |_| {
                window.pixel_snap(appearance::ui_size(COMPARE_FOOTER_HEIGHT))
            });
            floor + heading + footer_room
        });
        // The whole rows Split's room keeps beneath its headings before the
        // footer gives way: three beneath the line alone, and one beside a
        // Download row, whose sentence or the line gives way first
        // (`split_room`).
        let split_rows = (download_offered || lfs_floor.is_some()).then_some(if download_offered {
            1.
        } else {
            SPLIT_FIND_ROWS
        });
        let text_shown = matches!(self.content.as_deref(), Some(Content::Text { .. }))
            && !self.blame.is_visible()
            && !matches!(
                self.text_mode,
                TextMode::Diagrams | TextMode::Markdown | TextMode::Table | TextMode::Formatted
            );
        let review_shown = text_shown && !quick_source;
        // Why a working file's lines cannot be staged one by one, beneath
        // its text.
        let reason = self
            .content
            .as_ref()
            .and_then(|content| match content.as_ref() {
                Content::Text {
                    partial_unavailable,
                    ..
                } if self.mode == WorkspaceMode::Working => {
                    partial_unavailable.as_deref().map(SharedString::from)
                }
                _ => None,
            });
        let muted = colors.muted;
        // Beside a Download row, Diff, Before, After and Split lay the row,
        // the review options and the comparison out for the room the line
        // leaves them in the frame they are drawn ([`DownloadRegion`]), from
        // heights measured only where what they follow from changed
        // ([`Self::download_heights`]).
        let region = (text_shown && download_offered).then(|| {
            let heights =
                self.download_heights(column, review_shown, reason.as_ref(), muted, window, cx);
            let heading = if split.is_some() {
                window.pixel_snap(appearance::ui_size(crate::split_diff::SIDE_HEADING_HEIGHT))
            } else {
                px(0.)
            };
            DownloadRegion {
                owner: cx.entity().downgrade(),
                column,
                heights,
                review: review_shown.then(|| self.render_text_review(column, window, cx)),
                floor: comparison_floor(1., window) + heading,
                name: match self.text_mode {
                    TextMode::Before => "Before",
                    TextMode::After if quick_source => "Source",
                    TextMode::After => "After",
                    _ => "Diff",
                },
            }
        });
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(colors.canvas))
            .child(crate::in_frame::probed(toolbar, "compare-toolbar"))
            .children(self.render_lfs_failure_line(window, cx))
            .when(region.is_none(), |el| {
                el.child(self.render_lfs_download_actions(column, false, window, cx))
                    .when(review_shown, |el| {
                        el.child(self.render_text_review(column, window, cx))
                    })
            })
            .map(|column| {
                // The text holding keyboard focus rings the pane's own edge,
                // over its lines, so none of them moves.
                let edge = text_focus
                    .and_then(|focus| self.focus_ring.edge(&focus, rgb(colors.accent)))
                    .map(|edge| {
                        edge.debug_selector(|| "compare-text-ring".into())
                            .into_any_element()
                    });
                if let Some(region) = region {
                    return column
                        .child(region.render(content, split, edge, reason, muted, footer, window));
                }
                match split {
                    // Laid out for the room it shares with the footer in the
                    // frame it is drawn, where the footer gives way to Find.
                    Some(view) => {
                        let room = crate::in_frame::in_frame(move |bounds, window, cx| {
                            split_room(
                                view,
                                edge,
                                (reason, None),
                                muted,
                                footer,
                                split_rows,
                                false,
                                bounds,
                                window,
                                cx,
                            )
                        })
                        .named("compare-split-room");
                        match split_basis {
                            Some(basis) => column.child(
                                div()
                                    .flex_grow(1.)
                                    .flex_basis(basis)
                                    .flex_shrink(1.)
                                    .min_h(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(room),
                            ),
                            None => column.child(room),
                        }
                    }
                    None => column
                        .child(
                            preview_content(content, edge)
                                .when_some(lfs_floor, |pane, floor| pane.min_h(floor)),
                        )
                        .children(reason.map(|reason| partial_reason(reason, muted)))
                        .children(footer.map(|footer| footer.element)),
                }
            })
            .into_any_element()
    }
}

/// Compare's Download row, its review options and the comparison beneath
/// them in Diff, Before, After and Split, laid out for the room a failing
/// side's line above leaves them in the frame they are drawn, which give
/// way in one order: the row's sentence (`lfs_download::download_row`);
/// then Compare's footer, its words kept in the comparison's accessible
/// description; then the buttons' word "Download", so two buttons that
/// stack with it share a line without it, the footer coming back wherever
/// the room then holds it with a whole row; and only then do they stack,
/// the comparison keeping at least one whole row, beneath Split's headings
/// there. Beside two stacked buttons at 18 pt in a 461 px wide window the
/// footer took the comparison's last row below 480 px tall, leaving Diff
/// none at 440 px. Its parts' heights are measured only where what they
/// follow from changes ([`GitTurtle::download_heights`]), and it builds the
/// row once, in the form it draws.
struct DownloadRegion {
    /// The window, which builds the row in the form the region chooses.
    owner: WeakEntity<GitTurtle>,
    /// The column's width, which the row is built for.
    column: Pixels,
    /// What its parts take, as laid out in the column.
    heights: DownloadHeights,
    /// The review options, where they are drawn.
    review: Option<AnyElement>,
    /// What the comparison keeps: one whole row, beneath Split's headings.
    floor: Pixels,
    /// The mode the comparison is shown in, which names it once it carries
    /// the footer's words.
    name: &'static str,
}

/// The heights of a Download row's region's parts, as laid out in Compare's
/// column ([`GitTurtle::download_heights`]).
#[derive(Clone, Copy)]
struct DownloadHeights {
    /// The row with its buttons' word, and without it.
    rows: [Pixels; 2],
    /// The review options, where they are drawn, else none.
    review: Pixels,
    /// The reason the text's lines cannot be staged, where it is drawn,
    /// else none.
    reason: Pixels,
}

/// What a Download row's region's heights follow from
/// ([`GitTurtle::download_heights`]): the interface font and sizes, the
/// window's scale factor, the column's width, what the row is built from
/// (`lfs_download::CompareRow`), what the review options say, where they
/// are drawn, and the reason beneath the text. Never the room's height,
/// which only the region's arithmetic reads, in the frame it is drawn.
#[derive(Clone, PartialEq)]
struct DownloadHeightsKey {
    line: LineKey,
    scale: u32,
    column: u32,
    /// Its Quick source, which the row's labels need, `review` already
    /// tells apart here: the region draws the options exactly outside it.
    row: crate::lfs_download::CompareRow,
    /// The options and whether the review is unavailable, which choose its
    /// labels and caption.
    review: Option<(crate::text_review::Options, bool)>,
    reason: Option<SharedString>,
}

thread_local! {
    /// The heights Download rows' regions lay their parts out at, for each
    /// of the few keys drawn lately ([`DownloadHeightsKey`]).
    static DOWNLOAD_HEIGHTS: RefCell<Measured<DownloadHeightsKey, DownloadHeights>> =
        const { RefCell::new(Measured::new()) };
}

/// Empties [`DOWNLOAD_HEIGHTS`], for a test that counts what a Download
/// row's region measures: a GPUI test's seeds share the thread and its
/// cache.
#[cfg(test)]
pub(crate) fn forget_download_heights() {
    DOWNLOAD_HEIGHTS.with_borrow_mut(|kept| *kept = Measured::new());
}

impl GitTurtle {
    /// The heights a Download row's region lays its parts out at in a
    /// column `column` wide: the row with its buttons' word and without it,
    /// the review options where `review_shown`, and the `reason` the text's
    /// lines cannot be staged, its text `muted`. Each is laid out as a root
    /// to measure it, built for that alone, which beside the row once cost
    /// every Compare frame 142–177 µs of the UI thread's time, scrolls and
    /// resizes alike (release, 2026-11-10); so they are measured again only
    /// where what they follow from changes ([`DownloadHeightsKey`]), and a
    /// frame that changes none of it, as each of a scroll, measures nothing.
    /// The key is read from this frame's inputs and a new one is measured
    /// in this frame, so the form the region draws never follows a frame
    /// before's layout and asks for no other frame.
    fn download_heights(
        &self,
        column: Pixels,
        review_shown: bool,
        reason: Option<&SharedString>,
        muted: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DownloadHeights {
        let key = DownloadHeightsKey {
            line: LineKey::current(window),
            scale: window.scale_factor().to_bits(),
            column: f32::from(column).to_bits(),
            row: self.lfs_download_row(),
            review: review_shown.then(|| (self.review.options, self.review_unavailable())),
            reason: reason.cloned(),
        };
        DOWNLOAD_HEIGHTS.with_borrow_mut(|kept| {
            *kept.get(key, |_| {
                let mut row = |short| {
                    let row = self.render_lfs_download_actions(column, short, window, cx);
                    crate::in_frame::measured_height(row, column, window, cx)
                };
                let rows = [row(false), row(true)];
                let review = if review_shown {
                    let review = self.render_text_review(column, window, cx);
                    crate::in_frame::measured_height(review, column, window, cx)
                } else {
                    px(0.)
                };
                let reason = reason.map_or(px(0.), |reason| {
                    let reason = partial_reason(reason.clone(), muted);
                    crate::in_frame::measured_height(reason, column, window, cx)
                });
                DownloadHeights {
                    rows,
                    review,
                    reason,
                }
            })
        })
    }
}

impl DownloadRegion {
    /// The region holding `content`, or Split's `split` view, with the
    /// `reason` its lines cannot be staged and `footer`.
    #[expect(clippy::too_many_arguments)]
    fn render(
        self,
        content: AnyElement,
        split: Option<Entity<crate::split_diff::SplitView>>,
        edge: Option<AnyElement>,
        reason: Option<SharedString>,
        muted: u32,
        footer: Option<CompareFooter>,
        window: &Window,
    ) -> Div {
        let Self {
            owner,
            column,
            heights:
                DownloadHeights {
                    rows: [full_height, short_height],
                    review: review_height,
                    reason: reason_height,
                },
            review,
            floor,
            name,
        } = self;
        let footer_height = footer.as_ref().map_or(px(0.), |_| {
            window.pixel_snap(appearance::ui_size(COMPARE_FOOTER_HEIGHT))
        });
        let device = px(1. / window.scale_factor());
        #[cfg(test)]
        DOWNLOAD_ROWS.set(Some([full_height, short_height]));
        let region = crate::in_frame::in_frame(move |bounds, window, cx| {
            // Beside the row with its word, the room the comparison has,
            // less half a device pixel, as every length here stands on a
            // whole one.
            let room = bounds.size.height - full_height - review_height - reason_height;
            let holds = |room: Pixels, need: Pixels| room >= need - device / 2.;
            // The first of these that leaves the comparison its floor: the
            // row with its word and the footer; the row alone, the footer
            // given way; the row without its word, where its going lets
            // the buttons share a line, and the footer, which stays
            // wherever the room holds it with a whole row; and that row
            // alone. In a column that stacks the buttons without their word
            // too, as Compare's 243.5 px one at 18 pt in a 461 px window
            // does, the word stays. Once the footer had given way before
            // the word, it stayed away where the word's going left room
            // for it: at 18 pt, scale 1.2, in an 829 × 490 window, Split
            // left 122.5 px to its headings and a row, 109.67 px with the
            // footer.
            let shortens = short_height < full_height;
            let (short_shown, footer_stays) =
                if footer.is_some() && holds(room, floor + footer_height) {
                    (false, true)
                } else if !shortens || holds(room, floor) {
                    (false, false)
                } else {
                    let room = room + full_height - short_height;
                    (true, footer.is_some() && holds(room, floor + footer_height))
                };
            // The row in the form drawn, built once in the frame.
            let row = owner
                .update(cx, |this, cx| {
                    this.render_lfs_download_actions(column, short_shown, window, cx)
                })
                .unwrap_or_else(|_| div().into_any_element());
            let body = match split {
                Some(view) => div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        crate::in_frame::in_frame(move |bounds, window, cx| {
                            split_room(
                                view,
                                edge,
                                (reason, Some(reason_height)),
                                muted,
                                footer,
                                Some(1.),
                                !footer_stays,
                                bounds,
                                window,
                                cx,
                            )
                        })
                        .named("compare-split-room"),
                    )
                    .into_any_element(),
                None => {
                    let (footer, described) = match footer {
                        Some(footer) if !footer_stays => (None, Some(footer.line)),
                        footer => (footer.map(|footer| footer.element), None),
                    };
                    // A group in every layout, as Split's is, so its text
                    // keeps its place as the footer comes and goes.
                    let comparison = div()
                        .id("text-comparison")
                        .size_full()
                        .role(Role::Group)
                        .when_some(described, |group, line| {
                            note_compact_label("text-comparison", &line);
                            group
                                .aria_label(format!("{name} comparison"))
                                .aria_description(line)
                        })
                        .child(content)
                        .into_any_element();
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(preview_content(comparison, edge))
                        .children(reason.map(|reason| partial_reason(reason, muted)))
                        .children(footer)
                        .into_any_element()
                }
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(row)
                .children(review)
                .child(body)
                .into_any_element()
        })
        .named("compare-download-region");
        // It asks for the row with its word, the options, a whole row and
        // the footer, and gives that back only once the line above, which
        // gives way first (`GIVES_FIRST`), stands at its least, taking what
        // the column leaves it and no more.
        div()
            .flex_grow(1.)
            .flex_basis(full_height + review_height + reason_height + floor + footer_height)
            .flex_shrink(1.)
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(region)
    }
}

/// The pane Compare's preview draws `content` in, ringed by `edge` where
/// its text holds keyboard focus.
fn preview_content(content: AnyElement, edge: Option<AnyElement>) -> Div {
    div()
        .debug_selector(|| "preview-content".into())
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .child(content)
        .children(edge)
}

/// Why a working file's lines cannot be staged one by one, beneath its text.
fn partial_reason(reason: SharedString, muted: u32) -> Div {
    div()
        .px_3()
        .py_2()
        .text_size(crate::appearance::ui_text(11.))
        .text_color(rgb(muted))
        .child(reason)
}

/// The code editor's line height, as a multiple of its text size (the
/// kit's `EDITOR_LINE_HEIGHT`), which GPUI rounds to a whole pixel.
const CODE_LINE_HEIGHT: f32 = 1.5;

/// The code editor's padding above and below its lines, which no line is
/// drawn in (the kit's medium field's `input_py`, at every text size), and
/// which layout draws on its whole device pixel, halves toward zero.
const CODE_PADDING: Pixels = px(8.);

/// The fewest whole rows of Split's text Compare's footer leaves in view
/// beneath an open Find bar before it gives way.
const SPLIT_FIND_ROWS: f32 = 3.;

/// The shrink factor of a failing Git LFS side's line, which gives up its
/// height before Split's room beside it, whose factor is 1: large enough
/// that the line takes all of a shortage until it stands at its minimum,
/// and the room, its factor a whole 1, then takes the rest. With the room's
/// factor a millionth instead, layout gave the room only that share of what
/// remained once the line stood at its minimum (a flex container hands out
/// only the sum of its unfrozen factors' share of the free space where that
/// sum is below one), so at 18 pt in a 461 × 490 window beside a Download
/// row the room ran 21 px under the status bar with its footer.
pub(crate) const GIVES_FIRST: f32 = 1e6;

/// The height of [`SPLIT_FIND_ROWS`] whole rows of the comparison's text and
/// its padding: what a comparison keeps beneath a line above it that gives
/// way first, as a failing Git LFS side's does.
pub(crate) fn comparison_rows_floor(window: &Window) -> Pixels {
    comparison_floor(SPLIT_FIND_ROWS, window)
}

/// The height of `rows` whole rows of the comparison's text and its padding.
pub(crate) fn comparison_floor(rows: f32, window: &Window) -> Pixels {
    let line = (appearance::code_text() * CODE_LINE_HEIGHT).round();
    line * rows + window.pixel_snap(CODE_PADDING) * 2.
}

#[cfg(test)]
thread_local! {
    /// The open Find bar's height [`split_room`] last reckoned with, for a
    /// test.
    pub(crate) static SPLIT_FIND_BAR: std::cell::Cell<Option<Pixels>> =
        const { std::cell::Cell::new(None) };
    /// The heights of the Download row with its buttons' word and without
    /// it [`DownloadRegion`] last chose between, for a test.
    pub(crate) static DOWNLOAD_ROWS: std::cell::Cell<Option<[Pixels; 2]>> =
        const { std::cell::Cell::new(None) };
}

/// Split's text, `view`, with the reason its lines cannot be staged and
/// Compare's footer beneath it, laid out for the room they share in the
/// frame it is drawn, `bounds`. While Split's Find is open its bar stands
/// across both sides above their text, and the footer gives way wherever it
/// would leave fewer than [`SPLIT_FIND_ROWS`] whole rows of the text in
/// view, as an image comparison's footer gives way to the image's minimum
/// ([`CompareFooter`]): in a 461 × 490 window at 18 pt the bar's two lines
/// left two rows and the top of a third above it. Its words stay in the
/// accessible description of the comparison's group, which is named only
/// while it carries them, as an image comparison keeps them in the image's;
/// it comes back as Find closes or the room grows. `rows`, where it is
/// given, is how many whole rows the room keeps beneath its headings before
/// the footer gives way by the same reckoning, the headings in the bar's
/// place: three where a failing Git LFS side's line stands above the room
/// alone, which holds them first, once the line has given way to its one
/// line, and one beside a Download row, once its sentence, or the line
/// beside it, has given way. Kept, the footer ran past the column, 9 px
/// under the status bar, beneath the line, and beside two stacked Download
/// buttons it left the text no whole row, both at 18 pt in a 461 × 490
/// window. Where `given_way`, the footer has given way already, as a
/// Download row's region decides it ([`DownloadRegion`]), which passes the
/// `reason`'s height it keeps, `reason_height`, measured only where what it
/// follows from changes ([`GitTurtle::download_heights`]); without it the
/// reason is measured in the frame it is drawn.
#[expect(clippy::too_many_arguments)]
fn split_room(
    view: Entity<crate::split_diff::SplitView>,
    edge: Option<AnyElement>,
    (reason, reason_height): (Option<SharedString>, Option<Pixels>),
    muted: u32,
    footer: Option<CompareFooter>,
    rows: Option<f32>,
    given_way: bool,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let width = bounds.size.width;
    let editors = view.read(cx).editors().clone();
    let bar = footer
        .as_ref()
        .and_then(|_| crate::editor_find::shared_bar_height(&editors, width, window, cx));
    #[cfg(test)]
    SPLIT_FIND_BAR.set(bar);
    let device_pixel = px(1. / window.scale_factor());
    let footer_height = window.pixel_snap(appearance::ui_size(COMPARE_FOOTER_HEIGHT));
    let reason_height = reason_height.unwrap_or_else(|| {
        reason.clone().map_or(px(0.), |reason| {
            crate::in_frame::measured_height(partial_reason(reason, muted), width, window, cx)
        })
    });
    // What stands above the text, the rows the footer leaves it and how much
    // beyond them it needs: with the bar, a device pixel short of the rows,
    // as layout may draw the text's edges, takes the footer away; beneath
    // the headings every length is on a whole device pixel, so only a real
    // shortage, less half a pixel, does.
    let above = match bar {
        Some(bar) => Some((bar, SPLIT_FIND_ROWS, device_pixel)),
        None if footer.is_some() => rows.map(|rows| {
            (
                window.pixel_snap(appearance::ui_size(crate::split_diff::SIDE_HEADING_HEIGHT)),
                rows,
                -device_pixel / 2.,
            )
        }),
        None => None,
    };
    let gives_way = given_way
        || above.is_some_and(|(above, rows, spare)| {
            let line = (appearance::code_text() * CODE_LINE_HEIGHT).round();
            // The padding as drawn: reckoned at the raw 8 px, drawn on 10 device
            // pixels at scale 1.2, the footer stayed with 0.17 px to spare
            // beyond the rows, a fifth of the device pixel below.
            let padding = window.pixel_snap(CODE_PADDING);
            let text = bounds.size.height - reason_height - footer_height - above - padding * 2.;
            text < line * rows + spare
        });
    let (footer, described) = match footer {
        Some(footer) if gives_way => (None, Some(footer.line)),
        footer => (footer.map(|footer| footer.element), None),
    };
    // Labels give way before content (DESIGN.md, "One responsive order"):
    // where the room left to the sides holds a whole row but not their
    // headings with it, the headings give way, each side's accessible name
    // saying which side it is, and where it holds no whole row they take
    // its height, never cut. Kept whole, at 18 pt in a 461 × 440 window
    // beside a failing side's line and a Download row, the headings took
    // 41.5 px of a 37.5 px room, cut, and left the text none, though one
    // row is 34 px.
    let room =
        bounds.size.height - reason_height - footer.as_ref().map_or(px(0.), |_| footer_height);
    let heading = window.pixel_snap(appearance::ui_size(crate::split_diff::SIDE_HEADING_HEIGHT));
    let row = comparison_floor(1., window);
    let holds = |need: Pixels| room >= need - device_pixel / 2.;
    view.read(cx).fit_headings(if holds(heading + row) {
        crate::split_diff::Headings::Whole
    } else if holds(row) {
        crate::split_diff::Headings::GivenWay
    } else {
        crate::split_diff::Headings::Room(room.max(px(0.)))
    });
    // A group in every layout, so its texts keep their place in the
    // accessibility tree as the footer comes and goes.
    let comparison = div()
        .id("split-comparison")
        .size_full()
        .role(Role::Group)
        .when_some(described, |group, line| {
            note_compact_label("split-comparison", &line);
            group.aria_label("Split comparison").aria_description(line)
        })
        .child(view)
        .into_any_element();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(preview_content(comparison, edge))
        .children(reason.map(|reason| partial_reason(reason, muted)))
        .children(footer)
        .into_any_element()
}

/// Compare's footer beneath the preview, as [`GitTurtle::render_preview`]
/// builds it, and its whole line.
pub(crate) struct CompareFooter {
    pub(crate) element: AnyElement,
    /// What the footer says, whole, which an image comparison keeps in its
    /// accessible description once the footer has given way.
    pub(crate) line: SharedString,
}

/// The footer's height before scaling, its top rule included.
pub(crate) const COMPARE_FOOTER_HEIGHT: f32 = 24.;

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

#[derive(Clone, PartialEq, Eq)]
struct BranchMenuTarget {
    name: String,
    remote: bool,
    current: bool,
    detached_head: bool,
}

/// A navigator branch's menu, which a right-click on its row and Shift-F10
/// or the Menu key on the cursor row open alike: its name, then Branch
/// actions… and direct everyday branch commands, which act only while the
/// repository the menu was built for is still the one shown.
fn branch_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    target: BranchMenuTarget,
    (refused, wait): (bool, Option<native_accessibility::MenuWait>),
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let BranchMenuTarget {
        name,
        remote,
        current,
        detached_head,
    } = target;
    let mut menu = menu.label(name.clone()).item(
        native_accessibility::waiting_item("Branch actions…", refused, wait).on_click({
            let owner = owner.clone();
            let repository = repository.clone();
            let name = name.clone();
            move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.path == repository && this.page == AppPage::Repository {
                        this.open_contextual_branch(name.clone(), remote, window, cx);
                    }
                });
            }
        }),
    );
    if !remote {
        if !current {
            menu = menu.separator();
            let owner_sw = owner.clone();
            let repo_sw = repository.clone();
            let branch = name.clone();
            menu = menu.item(
                native_accessibility::waiting_item("Switch to branch", refused, wait).on_click(
                    move |_, window, cx| {
                        let _ = owner_sw.update(cx, |this, cx| {
                            if this.path == repo_sw && this.page == AppPage::Repository {
                                this.write(
                                    gitturtle_core::WriteCommand::Checkout {
                                        branch: branch.clone(),
                                    },
                                    "Switching branch…",
                                    window,
                                    cx,
                                );
                            }
                        });
                    },
                ),
            );
            if !detached_head {
                let owner_m = owner.clone();
                let repo_m = repository.clone();
                let branch_m = name.clone();
                menu = menu.item(
                    native_accessibility::waiting_item("Merge into current…", refused, wait)
                        .on_click(move |_, window, cx| {
                            let _ = owner_m.update(cx, |this, cx| {
                                if this.path == repo_m && this.page == AppPage::Repository {
                                    this.prepare_named_integration(
                                        branch_m.clone(),
                                        false,
                                        window,
                                        cx,
                                    );
                                }
                            });
                        }),
                );
                let owner_r = owner.clone();
                let repo_r = repository.clone();
                let branch_r = name.clone();
                menu = menu.item(
                    native_accessibility::waiting_item("Rebase current onto…", refused, wait)
                        .on_click(move |_, window, cx| {
                            let _ = owner_r.update(cx, |this, cx| {
                                if this.path == repo_r && this.page == AppPage::Repository {
                                    this.prepare_named_integration(
                                        branch_r.clone(),
                                        true,
                                        window,
                                        cx,
                                    );
                                }
                            });
                        }),
                );
            }
        }
    } else {
        menu = menu.separator();
        let owner_tr = owner.clone();
        let repo_tr = repository.clone();
        let branch_tr = name.clone();
        menu = menu.item(
            native_accessibility::waiting_item("Create tracking branch…", refused, wait).on_click(
                move |_, window, cx| {
                    let _ = owner_tr.update(cx, |this, cx| {
                        if this.path == repo_tr && this.page == AppPage::Repository {
                            this.open_remote_branch_track_named(branch_tr.clone(), window, cx);
                        }
                    });
                },
            ),
        );
    }
    menu.submenu("More", window, cx, move |mut sub, _, _| {
        if !remote {
            let owner_rn = owner.clone();
            let repo_rn = repository.clone();
            let branch_rn = name.clone();
            sub = sub.item(
                native_accessibility::waiting_item("Rename…", refused, wait).on_click(
                    move |_, window, cx| {
                        let _ = owner_rn.update(cx, |this, cx| {
                            if this.path == repo_rn && this.page == AppPage::Repository {
                                this.open_branch_rename_named(branch_rn.clone(), window, cx);
                            }
                        });
                    },
                ),
            );
            if !current {
                let owner_del = owner.clone();
                let repo_del = repository.clone();
                let branch_del = name.clone();
                sub = sub.item(
                    native_accessibility::waiting_item("Delete…", refused, wait).on_click(
                        move |_, window, cx| {
                            let _ = owner_del.update(cx, |this, cx| {
                                if this.path == repo_del && this.page == AppPage::Repository {
                                    this.open_branch_delete_named(
                                        branch_del.clone(),
                                        false,
                                        window,
                                        cx,
                                    );
                                }
                            });
                        },
                    ),
                );
                let owner_fdel = owner.clone();
                let repo_fdel = repository.clone();
                let branch_fdel = name.clone();
                sub = sub.item(
                    native_accessibility::waiting_item("Force delete…", refused, wait).on_click(
                        move |_, window, cx| {
                            let _ = owner_fdel.update(cx, |this, cx| {
                                if this.path == repo_fdel && this.page == AppPage::Repository {
                                    this.open_branch_delete_named(
                                        branch_fdel.clone(),
                                        true,
                                        window,
                                        cx,
                                    );
                                }
                            });
                        },
                    ),
                );
            }
        } else {
            let owner_dr = owner.clone();
            let repo_dr = repository.clone();
            let branch_dr = name.clone();
            sub = sub.item(
                native_accessibility::waiting_item("Delete remote branch…", refused, wait)
                    .on_click(move |_, window, cx| {
                        let _ = owner_dr.update(cx, |this, cx| {
                            if this.path == repo_dr
                                && this.page == AppPage::Repository
                                && let Some((remote_name, branch_name)) =
                                    this.split_remote_branch_ref(&branch_dr)
                            {
                                this.prepare_remote_branch_delete(
                                    remote_name,
                                    branch_name,
                                    window,
                                    cx,
                                );
                            }
                        });
                    }),
            );
        }
        sub = sub.separator();
        if !current {
            let owner_cmp = owner.clone();
            let repo_cmp = repository.clone();
            let ref_spec = if remote {
                format!("refs/remotes/{name}")
            } else {
                format!("refs/heads/{name}")
            };
            sub = sub.item(
                native_accessibility::waiting_item("Compare with HEAD", refused, wait).on_click(
                    move |_, window, cx| {
                        let _ = owner_cmp.update(cx, |this, cx| {
                            if this.path == repo_cmp && this.page == AppPage::Repository {
                                this.compare_revisions_direct(
                                    ref_spec.clone(),
                                    "HEAD".into(),
                                    gitturtle_core::ComparisonMode::Endpoints,
                                    window,
                                    cx,
                                );
                            }
                        });
                    },
                ),
            );
        }
        if !remote {
            let owner_wt = owner.clone();
            let repo_wt = repository.clone();
            sub = sub.item(
                native_accessibility::waiting_item("Create worktree…", refused, wait).on_click(
                    move |_, window, cx| {
                        let _ = owner_wt.update(cx, |this, cx| {
                            if this.path == repo_wt && this.page == AppPage::Repository {
                                this.open_worktree_manager(window, cx);
                            }
                        });
                    },
                ),
            );
        }
        let copy_name = name.clone();
        sub.item(
            PopupMenuItem::new("Copy branch name").on_click(move |_, window, cx| {
                copy_feedback::copy(window, cx, None, "branch name", copy_name.clone());
            }),
        )
    })
}

fn tag_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    tag: gitturtle_core::Tag,
    wait: Option<native_accessibility::MenuWait>,
) -> PopupMenu {
    let mut menu = menu.label(tag.name.clone());
    if tag.target_kind == "commit" {
        let owner_j = owner.clone();
        let repo_j = repository.clone();
        let oid = tag.target_oid.clone();
        menu = menu.item(
            PopupMenuItem::new("Jump to commit").on_click(move |_, window, cx| {
                let _ = owner_j.update(cx, |this, cx| {
                    if this.path == repo_j && this.page == AppPage::Repository {
                        this.jump_to_oid(oid.clone(), window, cx);
                    }
                });
            }),
        );
    }
    let owner_t = owner;
    let repo_t = repository;
    menu = menu.item(
        native_accessibility::waiting_item("Manage tags…", false, wait).on_click(
            move |_, window, cx| {
                let _ = owner_t.update(cx, |this, cx| {
                    if this.path == repo_t && this.page == AppPage::Repository {
                        this.open_tags(window, cx);
                    }
                });
            },
        ),
    );
    let copy_name = tag.name;
    menu.item(
        PopupMenuItem::new("Copy tag name").on_click(move |_, window, cx| {
            copy_feedback::copy(window, cx, None, "tag name", copy_name.clone());
        }),
    )
}

fn stash_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    stash: gitturtle_core::StashEntry,
    wait: Option<native_accessibility::MenuWait>,
) -> PopupMenu {
    let owner_s = owner;
    let repo_s = repository;
    let copy_name = stash.name;
    menu.label(stash.selector)
        .item(
            native_accessibility::waiting_item("Browse stashes…", false, wait).on_click(
                move |_, window, cx| {
                    let _ = owner_s.update(cx, |this, cx| {
                        if this.path == repo_s && this.page == AppPage::Repository {
                            this.open_stashes(window, cx);
                        }
                    });
                },
            ),
        )
        .item(
            PopupMenuItem::new("Copy stash name").on_click(move |_, window, cx| {
                copy_feedback::copy(window, cx, None, "stash name", copy_name.clone());
            }),
        )
}

fn submodule_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    submodule: gitturtle_core::SubmoduleSummary,
    wait: Option<native_accessibility::MenuWait>,
) -> PopupMenu {
    let label = format!("{} · {}", submodule.path.display(), submodule.state.label());
    let initialized = submodule.state != gitturtle_core::SubmoduleState::Uninitialized;
    let open_owner = owner.clone();
    let open_repo = repository.clone();
    let open_sub_path = submodule.path.clone();
    let init_owner = owner.clone();
    let init_repo = repository.clone();
    let init_sub_path = submodule.path.clone();
    let init_url = submodule.url.clone();
    let update_owner = owner;
    let update_repo = repository;
    let update_sub_path = submodule.path;
    let update_url = submodule.url;
    menu.label(label)
        .item(
            native_accessibility::waiting_item("Open in tab", !initialized, wait).on_click(
                move |_, window, cx| {
                    let _ = open_owner.update(cx, |this, cx| {
                        if this.path == open_repo
                            && let Some(root) = &this.path
                        {
                            let path = root.join(&open_sub_path);
                            this.open_repository_tab(path, window, cx);
                        }
                    });
                },
            ),
        )
        .item(
            native_accessibility::waiting_item("Initialize submodule…", false, wait).on_click(
                move |_, window, cx| {
                    let _ = init_owner.update(cx, |this, cx| {
                        if this.path == init_repo && this.page == AppPage::Repository {
                            let sub_display = init_sub_path.display().to_string();
                            let url_note = init_url
                                .as_deref()
                                .map(|u| {
                                    format!(
                                        "\n\nConfigured .gitmodules URL: {}",
                                        gitturtle_core::strip_bidi_controls(u)
                                    )
                                })
                                .unwrap_or_default();
                            this.confirm_git_write(
                                format!("Initialize submodule {sub_display}"),
                                format!(
                                    "Run `git submodule init -- {sub_display}` to register this submodule URL in local Git configuration.{url_note}"
                                ),
                                "Initialize submodule",
                                gitturtle_core::WriteCommand::Submodule(
                                    gitturtle_core::SubmoduleCommand::Init {
                                        path: Some(init_sub_path.clone()),
                                    }
                                    .into(),
                                ),
                                window,
                                cx,
                            );
                        }
                    });
                },
            ),
        )
        .item(
            native_accessibility::waiting_item("Update submodule…", false, wait).on_click(
                move |_, window, cx| {
                    let _ = update_owner.update(cx, |this, cx| {
                        if this.path == update_repo && this.page == AppPage::Repository {
                            let sub_display = update_sub_path.display().to_string();
                            let url_note = update_url
                                .as_deref()
                                .map(|u| {
                                    format!(
                                        "\n\nConfigured .gitmodules URL: {}",
                                        gitturtle_core::strip_bidi_controls(u)
                                    )
                                })
                                .unwrap_or_default();
                            this.confirm_git_write(
                                format!("Update submodule {sub_display}"),
                                format!(
                                    "Run `git submodule update --init -- {sub_display}` to initialize (if needed), fetch from its configured remote, and check out the recorded submodule revision.{url_note}"
                                ),
                                "Update submodule",
                                gitturtle_core::WriteCommand::Submodule(
                                    gitturtle_core::SubmoduleCommand::Update {
                                        path: Some(update_sub_path.clone()),
                                        init: true,
                                        recursive: false,
                                    }
                                    .into(),
                                ),
                                window,
                                cx,
                            );
                        }
                    });
                },
            ),
        )
}

/// A navigator worktree's menu, which a right-click on its row and
/// Shift-F10 or the Menu key on the cursor row open alike: its path, then
/// its actions and removals, which act only while the repository the menu
/// was built for is still the one shown. `refusal` is as for
/// [`branch_row_menu`].
fn worktree_row_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    tree: Worktree,
    (refused, wait): (bool, Option<native_accessibility::MenuWait>),
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
        menu = menu.item(
            native_accessibility::waiting_item(label, refused, wait).on_click(
                move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        if this.path == repository && this.page == AppPage::Repository {
                            this.open_worktree_actions(tree.clone(), removal, window, cx);
                        }
                    });
                },
            ),
        );
    }
    menu
}

/// A changed file's row ID: the entry it shows, by its paths, rather than
/// its place in the commit's list, so the window's accessibility tree keeps
/// one node for each file as another commit's files or the same commit's
/// read again replace the list, taking nodes away and adding others rather
/// than renaming them, as Keyboard Shortcuts' rows are kept
/// (`shortcuts::HelpRow::id`). The paths' bytes are hashed as Git gave them,
/// so the ID costs no allocation as each frame lays the rows out; its debug
/// selector is still its place.
fn changed_file_id(file: &FileChange) -> ElementId {
    use std::hash::{Hash, Hasher};
    fn bytes(path: &Option<std::path::PathBuf>) -> Option<&[u8]> {
        path.as_deref()
            .map(|path| path.as_os_str().as_encoded_bytes())
    }
    let mut paths = std::hash::DefaultHasher::new();
    (bytes(&file.old_path), bytes(&file.new_path)).hash(&mut paths);
    ElementId::NamedInteger("changed-file".into(), paths.finish())
}

/// A History changed file's menu, which a right-click on its row and
/// Shift-F10 or the Menu key on the highlighted row open alike. File history
/// opens only while the repository and commit the menu was built for are
/// still those shown and the file is still among their changed files.
#[expect(clippy::too_many_arguments)]
fn changed_file_menu(
    menu: PopupMenu,
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    file: FileChange,
    commit: Option<String>,
    paused: bool,
    wait: Option<native_accessibility::MenuWait>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = menu.item(PopupMenuItem::new("File history…").on_click({
        let owner = owner.clone();
        let repository = repository.clone();
        let file = file.clone();
        let commit = commit.clone();
        move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == repository
                    && this.changed_file_revision(&file) == commit
                    && let Some(index) = this.files.iter().position(|candidate| candidate == &file)
                {
                    this.open_file_history(index, window, cx);
                }
            });
        }
    }));
    menu.submenu("More", window, cx, move |mut sub, _, _| {
        sub = sub.item(PopupMenuItem::new("Blame…").on_click({
            let owner = owner.clone();
            let repository = repository.clone();
            let file = file.clone();
            let commit = commit.clone();
            move |_, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.path == repository
                        && this.changed_file_revision(&file) == commit
                        && let Some(index) =
                            this.files.iter().position(|candidate| candidate == &file)
                    {
                        this.open_blame(index, window, cx);
                    }
                });
            }
        }));
        sub = sub.separator();
        {
            let owner_e = owner.clone();
            let repo_e = repository.clone();
            let path_e = file.path().to_owned();
            sub = sub.item(
                PopupMenuItem::new("Open in editor").on_click(move |_, window, cx| {
                    let _ = owner_e.update(cx, |this, cx| {
                        if this.path == repo_e && this.page == AppPage::Repository {
                            this.open_repo_file_in_editor(path_e.clone(), None, window, cx);
                        }
                    });
                }),
            );
        }
        {
            let owner_r = owner.clone();
            let repo_r = repository.clone();
            let path_r = file.path().to_owned();
            let reveal_label = if cfg!(target_os = "macos") {
                "Reveal in Finder"
            } else {
                "Reveal in file manager"
            };
            sub = sub.item(
                PopupMenuItem::new(reveal_label).on_click(move |_, window, cx| {
                    let _ = owner_r.update(cx, |this, cx| {
                        if this.path == repo_r && this.page == AppPage::Repository {
                            this.reveal_repo_file_in_manager(path_r.clone(), window, cx);
                        }
                    });
                }),
            );
        }
        {
            let copy_path = file.path().to_string_lossy().into_owned();
            sub = sub.item(
                PopupMenuItem::new("Copy path").on_click(move |_, window, cx| {
                    copy_feedback::copy(window, cx, None, "file path", copy_path.clone());
                }),
            );
        }
        if let Some(oid) = &commit {
            let owner_rs = owner.clone();
            let repo_rs = repository.clone();
            let path_rs = file.path().to_owned();
            let oid = oid.clone();
            sub = sub.separator().item(
                native_accessibility::waiting_item("Restore file to this revision…", paused, wait)
                    .on_click(move |_, window, cx| {
                        let _ = owner_rs.update(cx, |this, cx| {
                            if this.path == repo_rs && this.page == AppPage::Repository {
                                this.open_restore_file(oid.clone(), path_rs.clone(), window, cx);
                            }
                        });
                    }),
            );
        }
        sub
    })
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

/// Below this width of the window's body ([`narrow_header`]), at the
/// interface text size, the header drops the Projects and profile labels so
/// it stays on one row.
const COMPACT_HEADER_WIDTH: f32 = 800.;
/// Pull requests' label in the header's compact row.
const COMPACT_PULL_REQUESTS: &str = "PRs";
/// The most a running write's progress label takes in the header, at the
/// interface size, before it ends in an ellipsis.
const OPERATION_PROGRESS_WIDTH: f32 = 180.;

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

/// How the Git action bar draws the ahead and behind counts, whole, and the
/// width the compact branch picker gives up to beside them
/// (`GitTurtle::sync_fit`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SyncFit {
    pub(crate) form: SyncForm,
    /// The compact picker's most width, its label ending in an ellipsis,
    /// where even the arrows alone would not fit beside its whole label.
    pub(crate) picker: Option<Pixels>,
}

/// The form the ahead and behind counts take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SyncForm {
    /// Each arrow beside its count and word: "↑ 0 ahead".
    Words,
    /// Each arrow beside its count alone: "↑ 0".
    Digits,
    /// The arrows alone, the counts in the tooltip and accessible name.
    Icons,
}

/// The text the full chrome lays out, other than its fixed labels, built
/// only when what it is made of changes ([`ChromeInputs`]): measured then,
/// and the part the header draws kept with its widths ([`ChromeLabels`]).
struct ChromeText {
    labels: ChromeLabels,
    profile: String,
    branch: String,
    ahead: String,
    behind: String,
    /// The ahead and behind counts' digits alone, as the compact bar draws
    /// them where their words would cut them ([`SyncForm::Digits`]).
    ahead_count: String,
    behind_count: String,
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
    /// The repository's name, as the compact header draws it.
    name: String,
    /// The running write's own label, which its status keeps whole, and
    /// that label with its repository, as the progress label starts.
    busy: String,
    busy_named: String,
}

/// The part of [`ChromeText`] the header draws, kept with the chrome's
/// widths for what it was made of (`GitTurtle::kept_chrome`), so a frame at
/// rest draws them as they were built, each cloned without allocating (a
/// short one held inline, a longer one shared), and formats none of them:
/// the header formatted the Changes label, the compact toggle's tooltip
/// and the repository heading's accessible name and tooltip again on every
/// frame, beside the copy the chrome measured.
#[derive(Clone)]
struct ChromeLabels {
    /// The Changes mode's name with its working-file count
    /// (`GitTurtle::changes_label`): the full tab's label and the compact
    /// toggle's accessible name.
    changes: SharedString,
    /// The compact Changes toggle's tooltip: that name, then its shortcut.
    changes_tooltip: SharedString,
    /// The count on the compact Changes toggle's badge
    /// ([`changes_badge_text`]), empty with no working file, where no badge
    /// is drawn.
    changes_badge: SharedString,
    /// The repository heading's accessible name and its tooltip
    /// (`GitTurtle::repository_heading`).
    heading: SharedString,
    heading_tooltip: SharedString,
}

/// What [`ChromeText`] is made of, borrowed from the window's state, so a
/// render compares it with what each kept [`ChromeWidths`] was measured
/// from without building the text: the chrome built its fifteen strings at
/// every lookup, ten a frame on the repository page, only to find them
/// unchanged. The text is built and measured again only when one of these
/// changes, or the font, the sizes or the scale do
/// (`GitTurtle::chrome_widths`). Each field is everything one of the text's
/// builders reads, so a change any of them would show changes this.
#[derive(PartialEq)]
struct ChromeInputs<'a> {
    /// The working status, once read: the count `changes_label` reads, the
    /// branch, HEAD and upstream the branch, its compact label and the
    /// edge read, the counts and the operation in progress. None on a marked
    /// tab, as `GitTurtle::header_status` reads it.
    status: Option<StatusInputs<'a>>,
    /// Whether the shown tab is marked, whose branch reads as the mark does.
    unavailable: bool,
    /// Whether the working status is not read yet, and the branch the
    /// accepted snapshot found checked out then, if any, which the branch
    /// picker names (`GitTurtle::working_status_unread`).
    reading: bool,
    unread_branch: Option<Cow<'a, str>>,
    /// Whether it is not read and no read of it runs, where the branch
    /// reads as the branch picker's does
    /// (`GitTurtle::working_status_not_read`).
    not_read: bool,
    /// Whether a remote is configured, which decides the edge with them
    /// (`RepositoryEdge::of`).
    remotes: bool,
    /// What the profile picker's label is made of
    /// (`GitTurtle::profile_label`).
    profile: crate::profiles::ProfileLabel<'a>,
    /// The repository and the running write's repository, each with the
    /// name it was given, if any (`GitTurtle::project_name`).
    repository: Option<NamedPath<'a>>,
    operation_repository: Option<NamedPath<'a>>,
    /// The window's path, with the name it was given, if any, which the
    /// repository heading's accessible name and tooltip read
    /// ([`ChromeLabels`]).
    path: Option<NamedPath<'a>>,
    /// The running write's own label.
    busy: Option<&'static str>,
}

/// The parts of the working status the chrome's text shows
/// ([`ChromeInputs`]).
#[derive(PartialEq)]
struct StatusInputs<'a> {
    entries: usize,
    branch: Option<Cow<'a, str>>,
    head: Option<Cow<'a, str>>,
    upstream: bool,
    ahead: u64,
    behind: u64,
    operation: Option<Cow<'a, str>>,
}

/// A repository's path and the name it was given, if any, of which
/// `GitTurtle::project_name` makes the name it shows.
#[derive(PartialEq)]
struct NamedPath<'a> {
    path: Cow<'a, std::path::Path>,
    named: Option<Cow<'a, str>>,
}

impl<'a> NamedPath<'a> {
    /// `path` with the name `names` gives it, if any.
    fn of(path: &'a std::path::Path, names: &'a HashMap<PathBuf, String>) -> Self {
        Self {
            named: names.get(path).map(|name| Cow::Borrowed(name.as_str())),
            path: Cow::Borrowed(path),
        }
    }
}

impl ChromeInputs<'_> {
    /// These inputs as a key a cache keeps, owning what they borrowed.
    fn owned(&self) -> ChromeInputs<'static> {
        let text = |text: &Cow<str>| Cow::Owned(String::from(&**text));
        let named = |named: &NamedPath| NamedPath {
            path: Cow::Owned(named.path.to_path_buf()),
            named: named.named.as_ref().map(text),
        };
        ChromeInputs {
            status: self.status.as_ref().map(|status| StatusInputs {
                entries: status.entries,
                branch: status.branch.as_ref().map(text),
                head: status.head.as_ref().map(text),
                upstream: status.upstream,
                ahead: status.ahead,
                behind: status.behind,
                operation: status.operation.as_ref().map(text),
            }),
            unavailable: self.unavailable,
            reading: self.reading,
            unread_branch: self.unread_branch.as_ref().map(text),
            not_read: self.not_read,
            remotes: self.remotes,
            profile: self.profile.owned(),
            repository: self.repository.as_ref().map(named),
            operation_repository: self.operation_repository.as_ref().map(named),
            path: self.path.as_ref().map(named),
            busy: self.busy,
        }
    }
}

/// The widths `ChromeText` and the chrome's fixed labels take, each as its
/// control lays it out: at its size and weight, rounded up to a whole pixel
/// and then to a whole device pixel ([`laid_out_text_width`]).
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
    ahead_count: Pixels,
    behind_count: Pixels,
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
    /// The wider of Cancel's two labels, which a running write's status
    /// reserves in the header ([`OperationPlace`]).
    cancel: Pixels,
    /// The repository's name in the compact header, whose room beside it a
    /// running write's status may take ([`GitTurtle::compact_operation_place`]).
    name: Pixels,
    /// The running write's own label, and the label with its repository.
    busy: Pixels,
    busy_named: Pixels,
}

impl ChromeWidths {
    fn measure(text: &ChromeText, window: &Window) -> Self {
        #[cfg(test)]
        CHROME_MEASURED.with(|measured| measured.set(measured.get() + 1));
        let laid_out = |text: &str, size: Pixels, weight: FontWeight| {
            laid_out_text_width(shaped_width(text, size, weight, window), window)
        };
        let weight = window.text_style().font_weight;
        let regular = |text: &str, size: Pixels| laid_out(text, size, weight);
        let medium = |text: &str| laid_out(text, appearance::ui_text(12.), FontWeight::MEDIUM);
        let label = appearance::ui_text(12.);
        let small = appearance::ui_text(11.);
        Self {
            projects: regular("Projects", label),
            history: regular("History", label),
            changes: regular(&text.labels.changes, label),
            pull_requests: regular("Pull requests", label),
            compact_pull_requests: regular(COMPACT_PULL_REQUESTS, label),
            // A medium kit Button's label is the rem size.
            profile: regular(&text.profile, window.rem_size()),
            branch: medium(&text.branch),
            ahead: regular(&text.ahead, small),
            behind: regular(&text.behind, small),
            ahead_count: regular(&text.ahead_count, small),
            behind_count: regular(&text.behind_count, small),
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
            // A small kit Button's label is 0.875 rem.
            cancel: crate::authentication::CANCEL_LABELS
                .map(|cancel| regular(cancel, window.rem_size() * 0.875))
                .into_iter()
                .fold(px(0.), Pixels::max),
            name: laid_out(&text.name, appearance::ui_text(14.), FontWeight::SEMIBOLD),
            busy: regular(&text.busy, small),
            busy_named: regular(&text.busy_named, small),
        }
    }
}

/// [`ChromeWidths`] and the [`ChromeLabels`] the header draws, by font,
/// sizes, scale factor and what the text is made of ([`ChromeInputs`]).
type ChromeWidthsCache =
    RefCell<Measured<(LineKey, u32, ChromeInputs<'static>), (ChromeWidths, ChromeLabels)>>;

thread_local! {
    /// [`ChromeWidths`] and [`ChromeLabels`] for what each chrome text was
    /// made of, and each font, sizes and scale factor, drawn lately, so the
    /// windows and text sizes drawn in turn keep their own; its widths snap
    /// to the window's device pixels.
    static CHROME_WIDTHS: ChromeWidthsCache = const { RefCell::new(Measured::new()) };
    /// The header's repository name and path as they show in their room
    /// (`GitTurtle::shown_identity`), for each text, room, font, size and
    /// scale factor drawn lately.
    static HEADER_IDENTITY: RefCell<Measured<IdentityKey, ShownIdentity>> =
        const { RefCell::new(Measured::new()) };
}

/// What the header's repository name and path were measured from
/// (`GitTurtle::shown_identity`): the line's font and size, the scale
/// factor and the room, with the repository, the name it was given, if
/// any, and the path line's path, of which the texts are made.
#[derive(PartialEq)]
struct IdentityKey {
    line: LineKey,
    scale: u32,
    room: u32,
    repository: Option<PathBuf>,
    named: Option<String>,
    path: Option<PathBuf>,
}

/// The header's repository name and path as they show in their room
/// (`GitTurtle::shown_identity`), and whether the name was elided there.
#[derive(Clone)]
struct ShownIdentity {
    name: SharedString,
    path: SharedString,
    name_elided: bool,
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

/// Where the header draws a running write's status, its progress label and
/// Cancel in one group ([`GitTurtle::render_operation_status`],
/// [`OperationStatus`]), so every control keeps its place when it appears
/// and goes: nothing it brings moves under the pointer (DESIGN.md,
/// "Selection, focus, and feedback").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationPlace {
    /// In the room the row leaves free before the trailing controls, at its
    /// trailing end: inside the full header's spacer, which lays out as if
    /// it were empty, where the spacer holds one of the group's forms
    /// ([`OperationForm`]); in the compact row, before Projects where the
    /// name at its whole width leaves the group that room.
    Spacer,
    /// After the trailing controls where no flexible control shares their
    /// line: on their own row ([`Trailing::OwnRow`]), or where Settings
    /// wrapped onto a line of its own past the spacer's.
    After,
    /// In the status bar, in the status line's place, where the header's
    /// rows hold not even the barest form beside their controls: the bar
    /// keeps Cancel's room beside its line wherever a write's status can
    /// come there ([`HeaderPlan::bar_room`]), so neither the header, the
    /// content nor the bar's other parts move.
    StatusBar,
}

/// Which of the repository tab strip's labelled actions keep their labels
/// ([`GitTurtle::tab_strip_labels`]); each gives way to its 28 px icon on
/// its own. Menu stands in the strip on Linux only; elsewhere `menu` stays
/// false.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TabStripLabels {
    pub(crate) menu: bool,
    pub(crate) workspaces: bool,
}

impl TabStripLabels {
    /// The forms the strip takes as its room narrows, each label giving way
    /// once, in turn: both labels, then Menu's alone, Workspaces' wider label
    /// going first, then neither. Off Linux, where no Menu stands in the
    /// strip, the second form is the third.
    pub(crate) const ORDER: [Self; 3] = [
        Self::labelled(true, true),
        Self::labelled(true, false),
        Self::labelled(false, false),
    ];

    const fn labelled(menu: bool, workspaces: bool) -> Self {
        Self {
            menu: menu && cfg!(target_os = "linux"),
            workspaces,
        }
    }

    /// Whether either keeps its label, as the tests ask.
    #[cfg(test)]
    pub(crate) fn any(self) -> bool {
        self.menu || self.workspaces
    }
}

/// The room `items`, wrapped as [`wrapped_rows`] wraps them, leave after the
/// last of them on its line.
fn last_line_room(items: &[(Pixels, Pixels)], room: Pixels, gap: Pixels) -> Pixels {
    let start = wrapped_rows(items, room, gap)
        .last()
        .map_or(0, |&(first, _)| first);
    let line = &items[start..];
    line.iter().fold(room, |left, &(width, _)| left - width) - gap * (line.len().max(1) - 1) as f32
}

/// The width layout gives the spacer at `spacer` among `items`, wrapped as
/// [`wrapped_rows`] wraps them: the room its line leaves, which it shares,
/// both growing alike from nothing, with the identity at `identity` where
/// that is on the line, the identity's half held within its `bounds`.
fn spacer_width(
    items: &[(Pixels, Pixels)],
    spacer: usize,
    identity: usize,
    bounds: (Pixels, Pixels),
    room: Pixels,
    gap: Pixels,
) -> Pixels {
    let firsts: Vec<usize> = wrapped_rows(items, room, gap)
        .into_iter()
        .map(|(first, _)| first)
        .collect();
    let start = firsts
        .iter()
        .copied()
        .filter(|&first| first <= spacer)
        .max()
        .unwrap_or(0);
    let end = firsts
        .iter()
        .copied()
        .find(|&first| first > spacer)
        .unwrap_or(items.len());
    let fixed = (start..end)
        .filter(|&item| item != spacer && item != identity)
        .fold(px(0.), |fixed, item| fixed + items[item].0);
    let free = room - fixed - gap * (end - start - 1) as f32;
    if (start..end).contains(&identity) {
        free - (free / 2.).max(bounds.0).min(bounds.1)
    } else {
        free
    }
}

/// The width the full header's identity, the repository's name above its
/// path, at `identity` among `items`, takes on its line as layout gives it:
/// it and the spacer, at `spacer` where it shares the line, grow alike from
/// nothing into the room the line's other items leave, the identity within
/// its `bounds`, as [`spacer_width`] gives the spacer the rest.
fn identity_width(
    items: &[(Pixels, Pixels)],
    identity: usize,
    spacer: Option<usize>,
    bounds: (Pixels, Pixels),
    room: Pixels,
    gap: Pixels,
) -> Pixels {
    let firsts: Vec<usize> = wrapped_rows(items, room, gap)
        .into_iter()
        .map(|(first, _)| first)
        .collect();
    let start = firsts
        .iter()
        .copied()
        .filter(|&first| first <= identity)
        .max()
        .unwrap_or(0);
    let end = firsts
        .iter()
        .copied()
        .find(|&first| first > identity)
        .unwrap_or(items.len());
    let shared = spacer.filter(|at| (start..end).contains(at));
    let fixed = (start..end)
        .filter(|&item| item != identity && Some(item) != shared)
        .fold(px(0.), |fixed, item| fixed + items[item].0);
    let free = room - fixed - gap * (end - start - 1) as f32;
    let share = if shared.is_some() { free / 2. } else { free };
    share.max(bounds.0).min(bounds.1)
}

/// The room a running write's status takes in the header
/// ([`GitTurtle::operation_status_size`]): the progress label as it starts,
/// the write's own label (`own`) or that label with its repository after it
/// (`named`), at most its `limit`, then Cancel, labelled (`labelled`) or its
/// 28 px icon (`icon`), each with the gap before it. Within its form the
/// label takes the room there is, a later, longer progress ending in an
/// ellipsis.
#[derive(Clone, Copy, Debug, PartialEq)]
struct OperationStatus {
    own: Pixels,
    named: Pixels,
    limit: Pixels,
    labelled: Pixels,
    icon: Pixels,
    /// The labelled Cancel's height and the 28 px control's, each at least
    /// the label's line.
    heights: (Pixels, Pixels),
}

impl OperationStatus {
    /// The richest form ([`OperationForm::ORDER`]) whose starting width fits
    /// `room`, Cancel labelled only where `labelled` allows it; none where
    /// even the barest does not.
    fn form(&self, room: Pixels, labelled: bool) -> Option<OperationForm> {
        OperationForm::ORDER
            .into_iter()
            .filter(|form| form.icon || labelled)
            .find(|&form| self.width(form) <= room)
    }

    /// The status's width as it starts in `form`: its label's text and
    /// Cancel.
    fn width(&self, form: OperationForm) -> Pixels {
        (if form.named { self.named } else { self.own }) + self.cancel(form)
    }

    /// The most the status takes in `form`: its label at its limit.
    fn most(&self, form: OperationForm) -> Pixels {
        self.limit + self.cancel(form)
    }

    fn height(&self, form: OperationForm) -> Pixels {
        if form.icon {
            self.heights.1
        } else {
            self.heights.0
        }
    }

    fn cancel(&self, form: OperationForm) -> Pixels {
        if form.icon { self.icon } else { self.labelled }
    }

    /// The status a write with the widest label brings, in every form at
    /// the label's limit: the labels are a finite set of `&'static str`, a
    /// GitHub request's (`github_view::request_labels`) among them, which
    /// already reaches the limit at 13 and 18 pt, so none starts wider
    /// (`tests::the_status_bar_keeps_cancels_room_only_where_a_write_can_take_it`).
    fn widest(self) -> Self {
        Self {
            own: self.limit,
            named: self.limit,
            ..self
        }
    }
}

/// The full header's labelled profile picker's width around a `label` this
/// wide, before its 230 px limit: a medium kit Button's `px_3`, the 16 px
/// icon, the label and the rem-sized caret, `gap_2` apart.
fn profile_width(label: Pixels, window: &Window) -> Pixels {
    let rem = window.rem_size();
    let snap = |length: Pixels| window.pixel_snap(length);
    snap(rem * 0.75) * 2. + px(16.) + snap(rem * 0.5) * 2. + label + snap(rem)
}

/// The labelled Cancel's width: a small kit Button's `px_2` around the wider
/// of its labels (`authentication::CANCEL_LABELS`), so its label changing as
/// it is pressed moves nothing.
fn labelled_cancel(widths: &ChromeWidths, window: &Window) -> Pixels {
    window.pixel_snap(window.rem_size() * 0.5) * 2. + widths.cancel
}

/// How a running write's status draws: Cancel as its icon or labelled, and
/// whether the progress label names the write's repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OperationForm {
    icon: bool,
    named: bool,
}

impl OperationForm {
    /// The status's forms, richest first, in the responsive order (DESIGN.md,
    /// "One responsive order"): Cancel labelled beside the label naming the
    /// write's repository; then Cancel as its icon, its label leading its
    /// tooltip; then the label without its repository, which its tooltip
    /// and accessible name keep.
    const ORDER: [Self; 3] = [
        Self {
            icon: false,
            named: true,
        },
        Self {
            icon: true,
            named: true,
        },
        Self {
            icon: true,
            named: false,
        },
    ];
}

/// Where a running write's status goes, and its form for the room it has
/// there ([`OperationStatus::form`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OperationLayout {
    place: OperationPlace,
    form: OperationForm,
}

/// How a Repository frame's header is arranged, worked out once per frame
/// before layout ([`GitTurtle::header_plan`]): the header, the status bar's
/// room and a running write's status all read it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HeaderPlan {
    /// The full header's trailing controls ([`Trailing`]) and whether
    /// History's root keeps the Projects slot among them; none for the
    /// compact row.
    full: Option<(Trailing, bool)>,
    /// Where a running write's status goes, for its label, and its form.
    operation: OperationLayout,
    /// The width the compact row gives the status where it goes there
    /// ([`GitTurtle::compact_operation_width`]).
    width: Option<Pixels>,
    /// The width the repository's name and path take: the full header's
    /// identity on its line ([`identity_width`]), or the most the compact
    /// row's name takes. Each line is elided in its middle to it
    /// ([`GitTurtle::shown_identity`]).
    identity: Pixels,
    /// Whether the status bar keeps Cancel's room beside its line
    /// ([`GitTurtle::fit_status_bar`]): wherever, at the window's width and
    /// text size, a write's status could come to the bar with any header
    /// contents, the widest label's barest form ([`OperationStatus::widest`])
    /// finding no room in the header's rows; decided from the width and
    /// text size alone ([`GitTurtle::full_bar_room`], and in every compact
    /// row, whose name can take all its room), so the bar's idle hints never
    /// change form as a label changes. Elsewhere every write's status stays
    /// in the header, and the bar's hints and Details… keep that room.
    bar_room: bool,
}

/// Which of the full header's controls that come and go a layout counts
/// ([`GitTurtle::header_layout`]): Projects or its slot among the trailing
/// controls, and History, Changes and Pull requests, which a repository
/// brings.
#[derive(Clone, Copy, Debug)]
struct HeaderControls {
    projects: bool,
    repository: bool,
}

/// The full header's controls laid out in a window as `render_header` lays
/// them out ([`GitTurtle::full_header_layout`]).
struct HeaderLayout {
    /// Each control's width and height, in order.
    items: Vec<(Pixels, Pixels)>,
    /// Where the spacer stands among `items`, where the header keeps one.
    spacer: Option<usize>,
    trailing: Trailing,
    /// Where a running write's status goes, whether or not one runs.
    operation: OperationLayout,
    /// Whether the widest label's barest form goes to the status bar with
    /// these controls and labels ([`GitTurtle::full_bar_room`]).
    widest_to_bar: bool,
    /// How much History and Changes, where a repository brings them, and
    /// the profile can each grow before the controls' arrangement changes
    /// ([`line_slack`]): Changes in the row and in the row the trailing
    /// controls' form is decided from, the profile in the row.
    changes_slack: Option<Pixels>,
    profile_slack: Pixels,
    /// The room the controls wrap in, `gap` apart.
    room: Pixels,
    gap: Pixels,
}

/// The room the line holding item `at` leaves after its last item, as
/// [`wrapped_rows`] wraps `items` in `room`, `gap` apart: how much the item
/// can grow before that line's last item wraps. Less than nothing where the
/// item starts its line wider than the room, which no growth of it changes.
fn line_slack(items: &[(Pixels, Pixels)], at: usize, room: Pixels, gap: Pixels) -> Pixels {
    let firsts: Vec<usize> = wrapped_rows(items, room, gap)
        .into_iter()
        .map(|(first, _)| first)
        .collect();
    let start = firsts
        .iter()
        .copied()
        .filter(|&first| first <= at)
        .max()
        .unwrap_or(0);
    let end = firsts
        .iter()
        .copied()
        .find(|&first| first > at)
        .unwrap_or(items.len());
    let line = &items[start..end];
    line.iter().fold(room, |left, &(width, _)| left - width) - gap * (line.len().max(1) - 1) as f32
}

/// Whether `to_bar` holds for some width from `least` to `most` of one
/// control in the full header's wrapping row, given, for a width, whether
/// the widest status goes to the status bar and how much the control can
/// grow before the controls' arrangement changes ([`line_slack`],
/// [`HeaderLayout::changes_slack`]). Until then the controls keep
/// their lines and every room the status could take only shrinks as the
/// control grows, so each arrangement is asked where it leaves the least,
/// where its line fills exactly, which still holds the line's last control
/// ([`wrapped_rows`] breaks only past the room), and a hair short of that
/// in case rounding wrapped the control there; and the next a hair past
/// it, where the line's last control has wrapped, until `most`.
fn to_bar_anywhere(
    least: Pixels,
    most: Pixels,
    mut to_bar: impl FnMut(Pixels) -> (bool, Pixels),
) -> bool {
    let hair = px(1. / 64.);
    let mut width = least;
    // Each step wraps a control onto a later line, so a row of a dozen
    // controls takes a few; the bound only guards against a stuck step.
    for _ in 0..32 {
        let (bar, slack) = to_bar(width);
        if bar {
            return true;
        }
        let full = width + slack;
        if slack < px(0.) || full >= most {
            return width < most && to_bar(most).0;
        }
        if slack > px(0.) && (to_bar(full).0 || (slack > hair && to_bar(full - hair).0)) {
            return true;
        }
        width = full + hair;
    }
    to_bar(most).0
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

/// The window's decorations and the toolkit's `window_paddings` for them,
/// which inset the window's body. A test window has only the platform's own
/// decorations, so a test that lays its window out in a copy of the
/// client-decorated frame sets that frame here ([`TEST_FRAME`]).
fn window_frame(window: &Window) -> (Decorations, Edges<Pixels>) {
    #[cfg(test)]
    if let Some(frame) = TEST_FRAME.with(std::cell::Cell::get) {
        return frame;
    }
    (
        window.window_decorations(),
        gpui_kit::component::window_paddings(window),
    )
}

/// The size the window's body is laid out at in this window
/// ([`laid_out_size`]). The header, the tab strip and the status bar span
/// it, so their forms and the chrome's, and every page's own breakpoints,
/// are decided from it, never from the window's width: inside a
/// client-decorated frame the body is the frame's insets narrower.
pub(crate) fn body_size(window: &Window) -> Size<Pixels> {
    body_bounds(window).size
}

/// Where the window's body is laid out in this window ([`laid_out_bounds`]):
/// the status bar ends at its foot.
pub(crate) fn body_bounds(window: &Window) -> Bounds<Pixels> {
    let (decorations, paddings) = window_frame(window);
    laid_out_bounds(
        window.viewport_size(),
        window.scale_factor(),
        decorations,
        paddings,
    )
}

/// Whether the full header drops the Projects and profile labels in this
/// window: below the scaled `COMPACT_HEADER_WIDTH` of the body it spans.
fn narrow_header(window: &Window) -> bool {
    body_size(window).width < appearance::ui_size(COMPACT_HEADER_WIDTH)
}

/// The width the window's body is laid out at in a window `viewport` wide,
/// for this window's scale factor and decorations ([`laid_out_size`]): the
/// width the project pane's group is fitted to.
fn body_width(viewport: Pixels, window: &Window) -> Pixels {
    let (decorations, paddings) = window_frame(window);
    laid_out_size(
        size(viewport, window.viewport_size().height),
        window.scale_factor(),
        decorations,
        paddings,
    )
    .width
}

/// The size the window's content is laid out at, as the layout rounds it:
/// on each axis the viewport, less, inside a client-decorated frame, the
/// toolkit's `window_paddings` and its 1 px border on each untiled side,
/// each rounded to device pixels as the layout rounds it (a padding to the
/// nearest, the border as a stroke, never under one), and the far edge at
/// the device pixel it is snapped to. At scale factor 1.5 an untiled
/// frame's 20 px padding is 30 device pixels and its border 1, not 1.5, so
/// in a 1,061 px window the body is laid out 1,529 device pixels wide where
/// the window less the insets unrounded, snapped, would be 1,528.
fn laid_out_size(
    viewport: Size<Pixels>,
    scale: f32,
    decorations: Decorations,
    paddings: Edges<Pixels>,
) -> Size<Pixels> {
    laid_out_bounds(viewport, scale, decorations, paddings).size
}

/// Where the window's content is laid out, as the layout rounds it: its
/// near edges at the frame's insets, each a whole number of device pixels,
/// and its size as [`laid_out_size`] gives it. At scale factor 1.5 an
/// untiled frame's foot is 31 device pixels, so in a 980 px tall window the
/// content ends at 959⅓ px, where the window less the frame's 21 px
/// unrounded ends at 959.
fn laid_out_bounds(
    viewport: Size<Pixels>,
    scale: f32,
    decorations: Decorations,
    paddings: Edges<Pixels>,
) -> Bounds<Pixels> {
    // To the nearest device pixel, halves toward zero, as the layout rounds.
    let device = |length: f32| (length.abs() - 0.5).ceil().copysign(length);
    // Each side's inset in device pixels: inside a client-decorated frame,
    // its padding and, unless the frame is tiled against that side, its
    // border.
    let insets = match decorations {
        Decorations::Server => Edges::default(),
        Decorations::Client { tiling } => {
            let inset = |padding: Pixels, tiled: bool| {
                let border = if tiled { 0. } else { device(scale).max(1.) };
                device(f32::from(padding) * scale) + border
            };
            Edges {
                top: inset(paddings.top, tiling.top),
                right: inset(paddings.right, tiling.right),
                bottom: inset(paddings.bottom, tiling.bottom),
                left: inset(paddings.left, tiling.left),
            }
        }
    };
    // Each inset is a whole number of device pixels, so the content's near
    // edge lies on one, and its far edge is snapped where the layout snaps
    // it.
    let length = |viewport: Pixels, near: f32, far: f32| {
        let far = device(f32::from(viewport) * scale - far);
        px((far - near).max(0.) / scale)
    };
    Bounds::new(
        point(px(insets.left / scale), px(insets.top / scale)),
        size(
            length(viewport.width, insets.left, insets.right),
            length(viewport.height, insets.top, insets.bottom),
        ),
    )
}

impl GitTurtle {
    /// The chrome a Repository frame draws, decided before layout so no
    /// frame shows the other form first. The full forms' heights are
    /// predicted for this window: each control's width from the interface
    /// font at the interface size and its fixed padding, icon and gaps, the
    /// rows its header and action bar wrap to, and each row's tallest
    /// control, every length snapped to device pixels as layout snaps it.
    /// Where the window's body as it is laid out ([`body_size`]) less the
    /// tab strip, the status bar and those heights leaves History less than
    /// `history_minimum`, the chrome is compact. Notices, banners and an
    /// expanded Targets take their room from the content in either form and
    /// do not decide it, so opening Targets never changes the form of the
    /// control that opened it; a running write's status takes room from
    /// neither ([`OperationPlace`]).
    pub(super) fn chrome(&self, window: &Window, cx: &App) -> Chrome {
        let narrow = narrow_header(window);
        if self.page != AppPage::Repository {
            return Chrome::Full { narrow };
        }
        let body = body_size(window);
        let widths = self.chrome_widths(window);
        let (header, actions) = self.full_chrome_heights(&widths, window, cx);
        let full = header + actions;
        let tabs = window.pixel_snap(repository_tabs::tab_strip_height(window, cx));
        let status = window.pixel_snap(appearance::ui_size(26.));
        let need = self
            .history_minimum(window)
            .max(self.compare_minimum(window, cx));
        if body.height - tabs - status - full >= need {
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
        let name = body.width - window.pixel_snap(rem * 0.75) * 2. - fixed - gap * (items - 1.);
        let app_icon = name - control - gap >= window.pixel_snap(appearance::ui_size(100.));
        Chrome::Compact {
            app_icon,
            name: (if app_icon { name - control - gap } else { name }).max(px(0.)),
        }
    }

    /// Which of the repository tab strip's labelled actions, Menu (on Linux)
    /// and Workspaces, keep their labels in this window: the first form in
    /// [`TabStripLabels::ORDER`] whose tab list holds every tab's whole
    /// label, to the tab's cap (`tabs_whole_width`). Content comes first
    /// (DESIGN.md, "One responsive order"), so a label gives way before any
    /// tab's name is cut, and only where the tabs need its room, one label
    /// at a time and each once as the window narrows: Workspaces' wider
    /// label first, then Menu's, so Menu, the primary control, keeps its
    /// label longest. At 18 pt in a 461 × 490 half tile Menu keeps its label
    /// beside Workspaces' icon, where both icons once left about 175 px of
    /// the list empty. The order is fixed rather than the form that labels
    /// the most room: ranked so, Menu's label went off, on and off again as
    /// the window narrowed, moving the tab list's left edge about 60 px each
    /// time. Like the chrome, it is decided before layout from the interface
    /// font ([`Self::tab_list_room`]).
    pub(super) fn tab_strip_labels(&self, window: &Window, cx: &App) -> TabStripLabels {
        let whole = self.tabs_whole_width(window);
        // Measured once, for every form.
        let (menu, workspaces) = self.tab_strip_labelled_actions(window);
        let icons = self.tab_list_room(TabStripLabels::default(), (menu, workspaces), window, cx);
        let control = window.pixel_snap(appearance::ui_size(28.));
        let labelled = |shown: bool, width: Pixels| {
            if shown { width - control } else { px(0.) }
        };
        TabStripLabels::ORDER
            .into_iter()
            .find(|labels| {
                let room = labelled(labels.menu, menu) + labelled(labels.workspaces, workspaces);
                whole <= icons - room
            })
            .unwrap_or_default()
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
    /// and Workspaces as `labels` has them, each labelled or its 28 px icon:
    /// the strip spans the window's body ([`body_size`]) inside its `px_2`,
    /// its controls a ring's room apart (`repository_tabs::tab_gap`), and
    /// holds the window controls
    /// (`window_chrome::strip_width`), Menu, the list, the 28 px New tab and
    /// Workspaces, every length snapped to device pixels as layout snaps
    /// it. Labels are measured as layout lays them out, rounded up to a
    /// whole pixel and then to a whole device pixel
    /// ([`laid_out_text_width`]), as the chrome's are, so the room is the
    /// room layout gives at a fractional scale factor too: the labels give
    /// way neither early nor late (rounded to whole pixels alone, at 1.5 a
    /// 71 px Menu was drawn 71⅓ px wide and a tab predicted whole was cut).
    /// Menu and Workspaces take the widths `labelled` gives them labelled
    /// ([`Self::tab_strip_labelled_actions`]), which the strip's choice
    /// measures once for every form.
    pub(super) fn tab_list_room(
        &self,
        labels: TabStripLabels,
        (labelled_menu, labelled_workspaces): (Pixels, Pixels),
        window: &Window,
        cx: &App,
    ) -> Pixels {
        let rem = window.rem_size();
        let gap = crate::repository_tabs::tab_gap(window);
        let control = window.pixel_snap(appearance::ui_size(28.));
        let menu = if labels.menu { labelled_menu } else { control };
        let workspaces = if labels.workspaces {
            labelled_workspaces
        } else {
            control
        };
        #[cfg(target_os = "linux")]
        let (menu, decorations) = (menu + gap, crate::window_chrome::strip_width(window, cx));
        #[cfg(not(target_os = "linux"))]
        let (menu, decorations) = {
            let _ = (cx, menu);
            (px(0.), px(0.))
        };
        body_size(window).width
            - crate::repository_tabs::titlebar_left_inset(window)
            - window.pixel_snap(rem * 0.5)
            - decorations
            - menu
            - (gap + control)
            - (gap + workspaces)
    }

    /// The full header's and action bar's heights in this window: the header
    /// spans the window's body ([`body_size`]), the action bar the repository
    /// page beside the project pane.
    fn full_chrome_heights(
        &self,
        widths: &ChromeWidths,
        window: &Window,
        cx: &App,
    ) -> (Pixels, Pixels) {
        let narrow = narrow_header(window);
        let page = self.full_actions_page(window, cx);
        (
            // A running write's status takes its room from the content and
            // never switches the form.
            self.full_header_height(
                body_size(window).width,
                narrow,
                self.back_label() != "Back to Projects",
                false,
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
    /// File actions menu. The path takes what they leave it on their one
    /// line whatever its length, so the toolbar keeps one height for every
    /// file and Compare's minimum counts every row it draws
    /// ([`Self::compare_toolbar_minimum`]): where "…/" and the file name did
    /// not fit beside File actions, the control wrapped onto a row of its own
    /// that the minimum left out.
    fn path_actions(
        &self,
        path: &str,
        history: bool,
        blame: bool,
        modes: bool,
        (column, column_min): (Pixels, Pixels),
        window: &Window,
    ) -> PathRow {
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
        // The path takes what its actions leave it on their one line, so
        // the row keeps one height whatever the file's name.
        let fit = |actions, beside: Pixels| PathRow {
            actions,
            room: beside,
        };
        if blame {
            let label = cached_width(&BLAME_LABEL, &LineKey::current(window), || {
                line_widths(&[("Blame", appearance::ui_text(12.))], window)
            }) + appearance::ui_size(20.);
            if room - (icons - control + label) >= needed {
                return fit(PathActions::Labelled, room - (icons - control + label));
            }
        }
        if room - icons >= needed || count < 2 {
            fit(PathActions::Icons, room - icons)
        } else {
            fit(PathActions::Menu, room - control - gap)
        }
    }

    /// Copy file path, File history and Blame folded into one 28 px control
    /// showing the overflow chevrons ("»"), where the whole path does not fit
    /// beside them as icons (`path_actions`): named "File actions", its
    /// tooltip leading with that name and listing them, its menu acting only
    /// on the file it was built for while Compare still shows it. The
    /// chevrons are macOS's toolbar overflow, which this fold is: the
    /// more-actions dots belong to the inspector's Actions in the same row,
    /// and two controls with the dots side by side read as one control drawn
    /// twice. A copy chosen there confirms on this control, as the branch
    /// picker confirms a switch: the check takes the chevrons' place and its
    /// tooltip reads Copied, with its name unchanged.
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
        let symbol = if copied { "check" } else { "overflow" };
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
    pub(super) fn compare_minimum(&self, window: &Window, cx: &App) -> Pixels {
        let rem = window.rem_size();
        let options = rem * 0.25 * 2. + rem * 1.5 + px(1.);
        let line = window.pixel_snap(appearance::code_text() * 1.5);
        self.compare_toolbar_minimum(window, cx)
            + options
            + px(8.) * 2.
            + line * 3.
            + window.pixel_snap(appearance::ui_size(COMPARE_FOOTER_HEIGHT))
    }

    /// Compare's toolbar height in the column it would have beside the rail
    /// and the inspector (`compare_toolbar_height`), which
    /// [`Self::compare_minimum`] counts: the path's one row and the text
    /// modes, on that row or the next.
    pub(super) fn compare_toolbar_minimum(&self, window: &Window, cx: &App) -> Pixels {
        let width = self.repository_width(window, cx);
        let narrow = width < appearance::ui_size(SIDEBAR_FIT_WIDTH);
        let rail = appearance::ui_size(44.);
        let columns = ColumnMinimums::new(width, rail, narrow);
        let column = width - rail - self.expected_inspector_width(width, &columns);
        compare_toolbar_height(column, columns.content - rail, window)
    }

    /// `ChromeWidths` for the chrome's current text ([`Self::kept_chrome`]).
    fn chrome_widths(&self, window: &Window) -> ChromeWidths {
        self.kept_chrome(window, |widths, _| *widths)
    }

    /// What `read` makes of the chrome's current widths and labels, built
    /// and measured again only when what the text is made of
    /// ([`ChromeInputs`]), the font, the sizes or the scale change, so a
    /// steady render builds none of its strings, asks the text system
    /// nothing and allocates nothing. Each window's text, and each text
    /// size, keeps its own ([`Measured`]): kept in one slot, two windows'
    /// repositories measured again on each other's frames.
    fn kept_chrome<R>(
        &self,
        window: &Window,
        read: impl FnOnce(&ChromeWidths, &ChromeLabels) -> R,
    ) -> R {
        #[cfg(test)]
        let before = crate::test_support::allocations();
        let line = LineKey::current(window);
        let scale = window.scale_factor().to_bits();
        let inputs = self.chrome_inputs();
        let value = CHROME_WIDTHS.with_borrow_mut(|cache| {
            let (widths, labels) = cache.get_where(
                |(kept_line, kept_scale, kept)| {
                    *kept_scale == scale && *kept_line == line && *kept == inputs
                },
                || (line.clone(), scale, inputs.owned()),
                |_| {
                    let text = self.chrome_text();
                    (ChromeWidths::measure(&text, window), text.labels)
                },
            );
            read(widths, labels)
        });
        #[cfg(test)]
        CHROME_LOOKUPS.with(|counted| {
            let (lookups, allocations) = counted.get();
            let made = crate::test_support::allocations() - before;
            counted.set((lookups + 1, allocations + made));
        });
        value
    }

    /// What the chrome's text is made of now, borrowed.
    fn chrome_inputs(&self) -> ChromeInputs<'_> {
        let names = &self.project_names;
        ChromeInputs {
            unavailable: self.shown_tab_unavailable(),
            reading: self.working_status_unread(),
            unread_branch: self.unread_branch().map(Cow::Borrowed),
            not_read: self.working_status_not_read(),
            status: self.header_status().map(|status| StatusInputs {
                entries: status.entries.len(),
                branch: status.branch.as_deref().map(Cow::Borrowed),
                head: status.head.as_deref().map(Cow::Borrowed),
                upstream: status.upstream.is_some(),
                ahead: status.ahead,
                behind: status.behind,
                operation: status.operation.as_deref().map(Cow::Borrowed),
            }),
            remotes: !self.remotes.is_empty(),
            profile: self.profile_label(),
            repository: (self.repository.as_ref()).map(|repo| NamedPath::of(repo.path(), names)),
            operation_repository: (self.operation_repository.as_deref())
                .map(|path| NamedPath::of(path, names)),
            path: (self.path.as_deref()).map(|path| NamedPath::of(path, names)),
            busy: self.operation_busy,
        }
    }

    /// The chrome's text, as [`ChromeInputs`] make it.
    fn chrome_text(&self) -> ChromeText {
        #[cfg(test)]
        CHROME_TEXT_BUILT.with(|built| built.set(built.get() + 1));
        let status = self.header_status();
        let edge = self.repository_edge();
        // The branch picker's own word ([`crate::branch_actions`]).
        let branch = if self.shown_tab_unavailable() {
            crate::branch_actions::UNAVAILABLE.into()
        } else if let Some(branch) = status.and_then(|status| status.branch.clone()) {
            branch
        } else if self.working_status_unread() {
            (self.unread_branch()).map_or(crate::branch_actions::READING.into(), str::to_owned)
        } else if self.working_status_not_read() {
            // No status read and none being read: neither a branch nor a
            // detached HEAD, which it said after a read that failed.
            crate::branch_actions::NOT_READ.into()
        } else {
            "Detached HEAD".into()
        };
        let changes = self.changes_label();
        let (heading, heading_tooltip) = self.heading_text();
        ChromeText {
            labels: ChromeLabels {
                changes_tooltip: format!("{changes} · {}2", primary_label()).into(),
                changes: changes.into(),
                changes_badge: changes_badge_text(status.map_or(0, |status| status.entries.len())),
                heading: heading.into(),
                heading_tooltip: heading_tooltip.into(),
            },
            profile: self.profile_title(),
            // The label beside which the line is measured
            // (`compact_edge_fits`).
            compact_branch: edge
                .as_ref()
                .and_then(|edge| edge.compact_branch(false))
                .unwrap_or_else(|| branch.clone()),
            branch,
            ahead: status.map_or_else(String::new, |s| format!("{} ahead", s.ahead)),
            behind: status.map_or_else(String::new, |s| format!("{} behind", s.behind)),
            ahead_count: status.map_or_else(String::new, |s| s.ahead.to_string()),
            behind_count: status.map_or_else(String::new, |s| s.behind.to_string()),
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
            name: self.repository_name(),
            busy: self.operation_busy.unwrap_or_default().to_owned(),
            busy_named: self
                .operation_busy
                .map_or_else(String::new, |label| self.operation_text(label)),
        }
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
    /// the other modes, in a full header `width` wide: wherever the slot
    /// adds no row to the header, so History lays out the controls the
    /// other modes do and the name takes the same share of the row. Where it
    /// would add a row, the other modes' row wraps and leaves the name its
    /// minimum, as History's tighter row does without the slot. On the
    /// trailing controls' own row ([`Trailing::OwnRow`]) the slot would only
    /// open that row with an empty space, so `render_header` asks only when
    /// they share a row.
    fn keeps_projects_slot(
        &self,
        width: Pixels,
        narrow: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> bool {
        self.full_header_height(width, narrow, true, false, widths, window)
            <= self.full_header_height(width, narrow, false, false, widths, window)
    }

    /// How a full header `width` wide, the body's as it is laid out
    /// ([`body_size`]) where it is drawn, lays out its trailing controls,
    /// decided from the other modes' controls, Projects among them, so
    /// History's root lays them out alike. A running write's status has its
    /// own place ([`OperationPlace`]), so it never changes this.
    fn trailing(
        &self,
        width: Pixels,
        narrow: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> Trailing {
        self.full_header_layout(width, narrow, true, false, widths, window)
            .trailing
    }

    /// How a full header `width` wide is arranged ([`HeaderPlan`]): its
    /// trailing controls ([`Self::trailing`]), whether History's root keeps
    /// the Projects slot among them ([`Self::keeps_projects_slot`], asked
    /// only where they share a row), where a running write's status goes
    /// beside the controls this mode draws ([`OperationPlace`]), for the
    /// write's label, and whether the widest label's status would go to the
    /// status bar. The controls' arrangement never depends on whether a
    /// write runs, and the status's place moves none of them, so the busy
    /// look's coming and going moves nothing.
    fn header_arrangement(
        &self,
        width: Pixels,
        narrow: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> HeaderPlan {
        let trailing = self.trailing(width, narrow, widths, window);
        let projects = self.back_label() != "Back to Projects";
        let slot = !projects
            && trailing == Trailing::Shared
            && self.keeps_projects_slot(width, narrow, widths, window);
        let layout =
            self.full_header_layout(width, narrow, projects || slot, false, widths, window);
        let bar_room = self.full_bar_room(width, narrow, widths, window);
        let scaled = |base: f32| window.pixel_snap(appearance::ui_size(base));
        let identity = identity_width(
            &layout.items,
            2,
            layout.spacer,
            (scaled(100.), scaled(200.)),
            layout.room,
            layout.gap,
        );
        // Every header's contents were asked, these among them.
        debug_assert!(
            bar_room || !layout.widest_to_bar,
            "the bar keeps Cancel's room wherever this header turns the widest status away"
        );
        HeaderPlan {
            full: Some((trailing, slot)),
            operation: layout.operation,
            width: None,
            identity,
            bar_room,
        }
    }

    /// Whether the status bar keeps Cancel's room beside its line in a
    /// frame whose full header is `width` wide ([`HeaderPlan::bar_room`]):
    /// wherever the widest label's barest form ([`OperationStatus::widest`])
    /// finds no room in the header's rows with some header's contents at
    /// this width and text size. Every header's contents are asked: with
    /// and without Projects or its slot, and a repository's toggles;
    /// Changes without a count or with one of up to six digits, or the
    /// longer one `widths` measures; and the profile's label from nothing to
    /// its control's 230 px limit. So the bar's idle hints change form only
    /// with the window's width or the text size, never as a branch, a
    /// count, the profile or the mode changes the header's labels, and a
    /// write whose status this header turns away finds the room kept. The
    /// widest contents alone would not do: a profile too wide for the
    /// spacer's line wraps with Settings onto a line of its own, which holds
    /// the status, where a narrower one fills that line and leaves the
    /// spacer too little, so each arrangement the contents can take is
    /// asked where it leaves the least ([`to_bar_anywhere`]). Kept for each
    /// width, text size and scale factor ([`Measured`]), since the header's
    /// insets and gaps snap to the window's device pixels; a resize asks
    /// again, and so does a window on a monitor of another scale.
    fn full_bar_room(
        &self,
        width: Pixels,
        narrow: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> bool {
        let line = LineKey::current(window);
        let (least, widest) = cached_width(&CHANGES_LABELS, &line, || {
            let label = |text: &str| line_widths(&[(text, appearance::ui_text(12.))], window);
            let widest = (0..10)
                .map(|digit| label(&format!("Changes · {}", digit.to_string().repeat(6))))
                .fold(px(0.), Pixels::max);
            (label("Changes"), widest)
        });
        let changes = (least.min(widths.changes), widest.max(widths.changes));
        let key = (
            line,
            window.scale_factor().to_bits(),
            f32::from(width).to_bits(),
            narrow,
            f32::from(changes.1).to_bits(),
        );
        BAR_ROOM.with_borrow_mut(|cache| {
            *cache.get(key, |_| {
                self.widest_to_bar_anywhere(width, narrow, changes, widths, window)
            })
        })
    }

    /// Whether some full header `width` wide sends the widest label's
    /// status to the bar ([`Self::full_bar_room`]): for each set of controls
    /// ([`HeaderControls`]), each arrangement Changes can take from its
    /// least to its most width (`changes`), and within it each arrangement
    /// the profile can take.
    fn widest_to_bar_anywhere(
        &self,
        width: Pixels,
        narrow: bool,
        changes: (Pixels, Pixels),
        widths: &ChromeWidths,
        window: &Window,
    ) -> bool {
        // The profile's label grows its control up to its limit; the narrow
        // header draws its icon alone.
        let profile = if narrow {
            px(0.)
        } else {
            (window.pixel_snap(px(230.)) - profile_width(px(0.), window)).max(px(0.))
        };
        let layout = |controls: HeaderControls, changes: Pixels, profile: Pixels| {
            let widths = ChromeWidths {
                changes,
                profile,
                ..*widths
            };
            self.header_layout(width, narrow, controls, false, &widths, window)
        };
        [true, false].into_iter().any(|repository| {
            [true, false].into_iter().any(|projects| {
                let controls = HeaderControls {
                    projects,
                    repository,
                };
                let profiles = |changes: Pixels| {
                    to_bar_anywhere(px(0.), profile, |profile| {
                        let layout = layout(controls, changes, profile);
                        (layout.widest_to_bar, layout.profile_slack)
                    })
                };
                if !repository {
                    return profiles(changes.0);
                }
                to_bar_anywhere(changes.0, changes.1, |changes| {
                    let layout = layout(controls, changes, px(0.));
                    let slack = layout.changes_slack.expect("a repository's toggles");
                    (profiles(changes), slack)
                })
            })
        })
    }

    /// How the compact header draws a running write's status
    /// ([`HeaderPlan`]): before the trailing controls where the room the
    /// name's row leaves it beside the name's whole width (`name` being the
    /// most the name takes, from `Chrome::Compact`) holds the status's
    /// barest form, and the status bar elsewhere
    /// ([`OperationPlace::StatusBar`]), so the row's controls keep their
    /// place and the name stays whole. The bar keeps Cancel's room in every
    /// compact frame: a repository's name can take all of `name`, leaving
    /// the status none, so the room, decided from the window's width and
    /// text size alone ([`HeaderPlan::bar_room`]), is kept whatever the
    /// name. It once followed the name, so the bar's hints changed form as
    /// a tab with a shorter name showed.
    fn compact_operation_place(
        &self,
        name: Pixels,
        widths: &ChromeWidths,
        window: &Window,
    ) -> HeaderPlan {
        let status = self.operation_status_size(widths, window);
        let fit = Self::compact_operation_width(status, name, widths, window);
        HeaderPlan {
            full: None,
            operation: match fit {
                Some((_, form)) => OperationLayout {
                    place: OperationPlace::Spacer,
                    form,
                },
                None => OperationLayout {
                    place: OperationPlace::StatusBar,
                    form: OperationForm::ORDER[2],
                },
            },
            width: fit.map(|(width, _)| width),
            identity: name,
            bar_room: true,
        }
    }

    /// The header's arrangement in a Repository frame drawn with `chrome`
    /// ([`Self::header_arrangement`] for the body's width,
    /// [`Self::compact_operation_place`]), for the write's label. The frame
    /// works it out once, before layout, for the header and the status bar.
    fn header_plan(&self, chrome: Chrome, window: &Window) -> HeaderPlan {
        let widths = self.chrome_widths(window);
        match chrome {
            Chrome::Compact { name, .. } => self.compact_operation_place(name, &widths, window),
            Chrome::Full { narrow } => {
                self.header_arrangement(body_size(window).width, narrow, &widths, window)
            }
        }
    }

    /// The width `status` takes before the compact row's trailing controls,
    /// and its form, if it goes there: the room the name leaves, a gap
    /// before Projects kept, up to the form's most, where that holds the
    /// barest form. Cancel is its 28 px icon there, as every control on the
    /// row is.
    fn compact_operation_width(
        status: OperationStatus,
        name: Pixels,
        widths: &ChromeWidths,
        window: &Window,
    ) -> Option<(Pixels, OperationForm)> {
        let gap = window.pixel_snap(window.rem_size() * 0.5);
        let room = (name - widths.name.min(name) - gap).floor();
        status
            .form(room, false)
            .map(|form| (room.min(status.most(form)), form))
    }

    /// The room a running write's status takes in the header
    /// ([`OperationStatus`]): the progress label, from its text as the write
    /// starts to its limit, and after it, `gap_2` apart, Cancel: labelled
    /// ([`labelled_cancel`]) or the shared 28 px control with its icon.
    fn operation_status_size(&self, widths: &ChromeWidths, window: &Window) -> OperationStatus {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let line = LineKey::line_height(window).to_pixels(appearance::ui_text(11.).into(), rem);
        let limit = snap(appearance::ui_size(OPERATION_PROGRESS_WIDTH));
        let gap = snap(rem * 0.5);
        let control = snap(appearance::ui_size(28.));
        OperationStatus {
            own: widths.busy.min(limit),
            named: widths.busy_named.min(limit),
            limit,
            labelled: gap + labelled_cancel(widths, window),
            icon: gap + control,
            heights: (snap((rem * 1.5).max(line)), snap(control.max(line))),
        }
    }

    /// The labelled Cancel's width ([`labelled_cancel`]), at which it is
    /// drawn whichever label it shows.
    pub(super) fn labelled_cancel_width(&self, window: &Window) -> Pixels {
        labelled_cancel(&self.chrome_widths(window), window)
    }

    /// The full header's height in a window `width` wide: `render_header`'s
    /// controls in order, wrapped as layout wraps them, with Projects or its
    /// slot among the trailing controls where `projects`, and a running
    /// write's status in its place where `operation`.
    fn full_header_height(
        &self,
        width: Pixels,
        narrow: bool,
        projects: bool,
        operation: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> Pixels {
        let HeaderLayout {
            items, room, gap, ..
        } = self.full_header_layout(width, narrow, projects, operation, widths, window);
        let snap = |length: Pixels| window.pixel_snap(length);
        let rem = window.rem_size();
        (wrapped_height(&items, room, gap) + snap(rem * 0.375) * 2. + snap(px(1.)))
            .max(snap(appearance::ui_size(48.)))
    }

    /// `render_header`'s full-form controls in a window `width` wide
    /// ([`HeaderLayout`]): in order, each its width and height, with
    /// Projects or its slot where `projects`, and a running write's status
    /// where `operation`; how the trailing controls are laid out
    /// ([`Trailing`]), which orders them and keeps or leaves out the spacer,
    /// decided without the status; where the status goes ([`OperationPlace`]);
    /// and the room and gap they wrap in.
    fn full_header_layout(
        &self,
        width: Pixels,
        narrow: bool,
        projects: bool,
        operation: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> HeaderLayout {
        let controls = HeaderControls {
            projects,
            repository: self.repository.is_some(),
        };
        self.header_layout(width, narrow, controls, operation, widths, window)
    }

    /// [`Self::full_header_layout`] with the `controls` given rather than
    /// this frame's, for a header with other contents
    /// ([`Self::full_bar_room`]).
    fn header_layout(
        &self,
        width: Pixels,
        narrow: bool,
        controls: HeaderControls,
        operation: bool,
        widths: &ChromeWidths,
        window: &Window,
    ) -> HeaderLayout {
        let HeaderControls {
            projects,
            repository,
        } = controls;
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let scaled = |base: f32| snap(appearance::ui_size(base));
        let line_height = LineKey::line_height(window);
        let line = |size: Pixels| snap(line_height.to_pixels(size.into(), rem));
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
        let changes_at = repository.then_some(items.len());
        if repository {
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
        let decided = [items.as_slice(), &[spacer, slot]].concat();
        let first = wrapped_rows(&decided, room, gap)
            .last()
            .map_or(0, |&(first, _)| first);
        let trailing = if first < items.len() {
            Trailing::Shared
        } else {
            Trailing::OwnRow {
                spacer: first > items.len(),
            }
        };
        // Within its 230 px limit, and as tall as the row's other controls.
        let profile = if narrow {
            control
        } else {
            profile_width(widths.profile, window).min(snap(px(230.)))
        };
        let spacer_at = (trailing != (Trailing::OwnRow { spacer: false })).then(|| {
            items.push(spacer);
            items.len() - 1
        });
        if projects && trailing == Trailing::Shared {
            items.push(slot);
        }
        let profile_at = items.len();
        items.push((profile, control));
        items.push((control, control));
        if projects && trailing != Trailing::Shared {
            items.push(slot);
        }
        // The status takes its richest form (`OperationForm::ORDER`) that
        // either place holds as it starts, the spacer first: in the spacer,
        // laid out empty, or after the trailing controls in the room their
        // line leaves, so it moves nothing; where neither holds even the
        // barest, the status bar draws it, which keeps Cancel's room
        // wherever any header's contents send the widest label's barest
        // there (`full_bar_room`).
        let status = self.operation_status_size(widths, window);
        let identity = (scaled(100.), scaled(200.));
        let spacer_room = spacer_at.map(|at| spacer_width(&items, at, 2, identity, room, gap));
        // After the trailing controls where their last line holds neither
        // the spacer nor the identity, which would give up the room.
        let last = wrapped_rows(&items, room, gap)
            .last()
            .map_or(0, |&(first, _)| first);
        let after = (last > 2 && spacer_at.is_none_or(|at| last > at))
            .then(|| last_line_room(&items, room, gap) - gap);
        let places = [
            (OperationPlace::Spacer, spacer_room),
            (OperationPlace::After, after),
        ];
        let place = |status: OperationStatus| {
            OperationForm::ORDER
                .into_iter()
                .find_map(|form| {
                    places
                        .iter()
                        .find(|(_, fits)| fits.is_some_and(|fits| status.width(form) <= fits))
                        .map(|&(place, _)| OperationLayout { place, form })
                })
                .unwrap_or(OperationLayout {
                    place: OperationPlace::StatusBar,
                    form: OperationForm::ORDER[2],
                })
        };
        let operation_layout = place(status);
        let widest_to_bar = place(status.widest()).place == OperationPlace::StatusBar;
        // Changes changes the arrangement where its line fills in either
        // row; where it starts its line too wide in one, it does in both.
        let changes_slack = changes_at
            .map(|at| line_slack(&decided, at, room, gap).min(line_slack(&items, at, room, gap)));
        let profile_slack = line_slack(&items, profile_at, room, gap);
        if operation && operation_layout.place == OperationPlace::After {
            let form = operation_layout.form;
            items.push((status.width(form), status.height(form)));
        }
        HeaderLayout {
            items,
            spacer: spacer_at,
            trailing,
            operation: operation_layout,
            widest_to_bar,
            changes_slack,
            profile_slack,
            room,
            gap,
        }
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
    /// its shortest whole form would be cut ([`Self::compact_edge_fits`]),
    /// a detached HEAD's branch picker saying that line's word instead.
    pub(super) fn edge_form(
        &self,
        compact: bool,
        window: &Window,
        cx: &App,
    ) -> empty_state::EdgeForm {
        let [.., barest] = empty_state::EdgeForm::ORDER;
        let Some(edge) = self.repository_edge() else {
            return barest;
        };
        let widths = self.chrome_widths(window);
        if compact {
            return empty_state::EdgeForm {
                glyph: !self.compact_edge_fits(&edge, &widths, window, cx),
                ..barest
            };
        }
        let page = self.full_actions_page(window, cx);
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
        cx: &App,
    ) -> bool {
        let actions = 4. + if edge.action().is_some() { 1. } else { 0. };
        window.pixel_snap(window.rem_size() * 0.5) * 2. + widths.edge_compact
            <= self.compact_line_room(actions, widths, window, cx)
    }

    /// The room the compact action bar's row leaves the line after the
    /// branch picker: the row's `actions` 28 px controls keep their room,
    /// and the picker its whole label (`render_git_actions`).
    fn compact_line_room(
        &self,
        actions: f32,
        widths: &ChromeWidths,
        window: &Window,
        cx: &App,
    ) -> Pixels {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        let control = snap(appearance::ui_size(28.));
        let gap = snap(rem * 0.5);
        let room =
            self.full_actions_page(window, cx) - snap(rem * 0.75) * 2. - (control + gap) * actions;
        room - self.compact_picker(widths, window) - gap
    }

    /// The compact branch picker's width with its whole label: a medium kit
    /// Button's `px_3`, the 16 px icon and the label `gap_2` apart, and the
    /// caret after it, within its 280 px limit.
    fn compact_picker(&self, widths: &ChromeWidths, window: &Window) -> Pixels {
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        (snap(rem * 0.75) * 2. + px(16.) + snap(rem * 0.5) * 2. + widths.compact_branch + snap(rem))
            .min(snap(px(280.)))
    }

    /// How the Git action bar draws the ahead and behind counts. The full
    /// bar, which wraps, draws their words; the compact bar, one row, the
    /// richest form whole beside the branch picker's whole label, as "One
    /// responsive order" lets a count's words go before the count: their
    /// words, then their digits beside the arrows, then the arrows alone,
    /// the counts in the state's tooltip and accessible name. Where even the
    /// arrows would not fit, the picker gives up the room they need, its
    /// label ending in an ellipsis and whole in its tooltip and name, since
    /// the kit's Button does not shrink. At 18 pt in a 461 × 490 half tile
    /// the words were cut to "↑ 0…", and to "↑ …" beside a longer branch
    /// name.
    pub(super) fn sync_fit(&self, compact: bool, window: &Window, cx: &App) -> SyncFit {
        let words = SyncFit {
            form: SyncForm::Words,
            picker: None,
        };
        if !compact
            || self.repository_edge().is_some()
            || self
                .header_status()
                .is_none_or(|status| status.upstream.is_none())
        {
            return words;
        }
        let widths = self.chrome_widths(window);
        let rem = window.rem_size();
        let snap = |length: Pixels| window.pixel_snap(length);
        // Fetch, Pull, Push and Targets: an edge stands in the counts' place.
        let room = self.compact_line_room(4., &widths, window, cx);
        // The state's `px_2` box and each count's 12 px arrow, the two
        // `gap_2` apart, and each count's text a `gap_1` after its arrow.
        let arrows = snap(rem * 0.5) * 2. + px(12.) * 2. + snap(rem * 0.5);
        let text = |ahead: Pixels, behind: Pixels| ahead + behind + snap(rem * 0.25) * 2.;
        if arrows + text(widths.ahead, widths.behind) <= room {
            return words;
        }
        if arrows + text(widths.ahead_count, widths.behind_count) <= room {
            return SyncFit {
                form: SyncForm::Digits,
                picker: None,
            };
        }
        SyncFit {
            form: SyncForm::Icons,
            picker: (arrows > room).then(|| {
                (self.compact_picker(&widths, window) - (arrows - room))
                    .max(snap(appearance::ui_size(28.)))
            }),
        }
    }

    /// The repository page's width beside the project pane, which the action
    /// bar spans ([`Self::repository_width`]).
    fn full_actions_page(&self, window: &Window, cx: &App) -> Pixels {
        self.repository_width(window, cx)
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
        let line_height = LineKey::line_height(window);
        let line = |size: Pixels| snap(line_height.to_pixels(size.into(), rem));
        let action = scaled(34.);
        let text = line(appearance::ui_text(11.));
        // Medium kit Buttons: `px_3`, a 16 px icon and the label `gap_2`
        // apart, the branch picker's caret after it, within its 280 px limit.
        let mut items = vec![(
            (snap(rem * 0.75) * 2. + px(16.) + snap(rem * 0.5) * 2. + widths.branch + snap(rem))
                .min(snap(px(280.))),
            action,
        )];
        if let Some(status) = self.header_status() {
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

/// The text the comparison's pane shows (`GitTurtle::shown_text`).
#[derive(Clone, Copy)]
pub(super) enum ShownText<'a> {
    /// The patch, the Before or the After text, which the pane rings.
    Editor(&'a Entity<EditorState>),
    /// Split, whose sides ring their own text.
    Split(&'a Entity<split_diff::SplitView>),
}

/// How the comparison path's row lays out (`path_actions`): its actions and
/// the room the path gets beside them on their one line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PathRow {
    actions: PathActions,
    room: Pixels,
}

impl PathRow {
    /// No file: the placeholder, whole, beside labelled actions.
    const WHOLE: Self = Self {
        actions: PathActions::Labelled,
        room: Pixels::MAX,
    };
}

/// `path`, a path's displayed form, in `room`, as `width` measures a line,
/// elided in its middle as macOS elides a path, so its top directory and its
/// file name stay: whole where it fits; otherwise the top directory and the
/// name around "…" with as many of the nearest directories between them as
/// fit whole ("crates/…/src/views.rs", "crates/…/views.rs"); where not even
/// the top directory fits whole beside the name, as much of its start as fits
/// before "…" ("ass…/turtle.png"), down to "…/" and the name. Where not even
/// that fits, the name alone, elided in its middle where it does not fit
/// either ([`name_middle`]). It cuts only at a separator or between
/// graphemes, so an accent stays on its letter, a flag stays whole and a
/// name that is not UTF-8 keeps the replacement characters its display
/// shows. An absolute path's root belongs to its top directory, so the
/// header's repository path keeps "/tmp" ("/tmp/…/compact").
pub(crate) fn path_tail(path: &str, room: Pixels, width: impl Fn(&str) -> Pixels) -> String {
    use unicode_segmentation::UnicodeSegmentation as _;
    if width(path) <= room {
        return path.to_owned();
    }
    let Some(slash) = path.rfind('/') else {
        return name_middle(path, room, &width);
    };
    let (directory, name) = path.split_at(slash);
    let root = usize::from(directory.starts_with('/'));
    let (top, between) = directory.split_at(
        directory[root..]
            .find('/')
            .map_or(directory.len(), |at| at + root),
    );
    // The nearest whole directories after the top one, the most first, then
    // none of them.
    let nearest = between
        .match_indices('/')
        .skip(1)
        .map(|(separator, _)| separator);
    for from in nearest.chain((!between.is_empty()).then_some(between.len())) {
        let shown = format!("{top}/…{}{name}", &between[from..]);
        if width(&shown) <= room {
            return shown;
        }
    }
    // The top directory's first `keep` characters before "…", whole
    // graphemes, so an accent or a flag is never split.
    let starts: Vec<usize> = top.grapheme_indices(true).map(|(at, _)| at).collect();
    let shown = |keep: usize| format!("{}…{name}", &top[..starts[keep]]);
    match most_that_fit(starts.len(), |keep| width(&shown(keep)) <= room) {
        Some(keep) => shown(keep),
        None => name_middle(&name[1..], room, &width),
    }
}

/// The header's repository path in `room`, as `width` measures a line:
/// elided as [`path_tail`] elides a path, but never down to its last
/// component alone, since the name line above already shows the name.
/// Where "…/" and the last component no longer fit, the top directory, an
/// absolute path's root with it, stays whole and the component is elided in
/// its middle in the room it leaves ([`name_middle`]), "/tmp/gitt…qa87", or
/// "/home/…/GitT…urtle" past directories between; where the top directory
/// leaves the component less than its first character and "…", "…/" stands
/// for the directories ("…/gitt…qa87"); and only where not even that fits,
/// what `path_tail` gives. Kept to the component alone, the line read as a
/// second elided name beneath the name and lost "/tmp" (decided 2026-11-02,
/// QA-87 #14).
pub(crate) fn location_tail(path: &str, room: Pixels, width: impl Fn(&str) -> Pixels) -> String {
    let shown = path_tail(path, room, &width);
    // `path_tail` keeps a separator wherever it keeps a directory.
    let Some(slash) = path.rfind('/').filter(|_| !shown.contains('/')) else {
        return shown;
    };
    let (directory, name) = (&path[..slash], &path[slash + 1..]);
    let root = usize::from(directory.starts_with('/'));
    let top = directory[root..]
        .find('/')
        .map_or(directory.len(), |at| at + root);
    let between = if top < directory.len() { "…/" } else { "" };
    let kept = format!("{}/{between}", &directory[..top]);
    for prefix in [kept.as_str(), "…/"] {
        let elided = name_middle(name, room - width(prefix), &width);
        let line = format!("{prefix}{elided}");
        if elided != name && width(&line) <= room {
            return line;
        }
    }
    shown
}

/// A file name in `room`, as `width` measures a line: whole where it fits,
/// else elided in its middle as macOS elides a name, its start and its
/// extension kept around "…" with as much of the rest of the name as fits,
/// the start taking the odd character ("Reposito…Sidebar.tsx"). The
/// extension runs from the last dot, unless that dot starts the name. Where
/// not even the name's first character, "…" and the extension fit, the name
/// whole, which the line's own middle elision cuts ([`ellipsis_middle`]).
pub(crate) fn name_middle(name: &str, room: Pixels, width: &impl Fn(&str) -> Pixels) -> String {
    use unicode_segmentation::UnicodeSegmentation as _;
    if width(name) <= room {
        return name.to_owned();
    }
    let dot = name.rfind('.').filter(|dot| *dot > 0).unwrap_or(name.len());
    let (stem, extension) = name.split_at(dot);
    let starts: Vec<usize> = stem.grapheme_indices(true).map(|(at, _)| at).collect();
    let count = starts.len();
    // `keep` of the stem's characters, whole graphemes, the larger half from
    // its start.
    let shown = |keep: usize| {
        let front = starts.get(keep.div_ceil(2)).copied().unwrap_or(stem.len());
        let back = starts.get(count - keep / 2).copied().unwrap_or(stem.len());
        format!("{}…{}{extension}", &stem[..front], &stem[back..])
    };
    // At least the name's first character.
    most_that_fit(count.saturating_sub(1), |more| {
        width(&shown(more + 1)) <= room
    })
    .map_or_else(|| name.to_owned(), |more| shown(more + 1))
}

/// The most of `0..count` for which `fits`, which holds for every number
/// below one it holds for; `None` where it holds for none.
fn most_that_fit(count: usize, fits: impl Fn(usize) -> bool) -> Option<usize> {
    if count == 0 || !fits(0) {
        return None;
    }
    let (mut low, mut high) = (0, count - 1);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Some(low)
}

/// Compare's path as it shows in `room` (`path_tail`), in its font, laid
/// out again only when the path, the room, the font or the sizes change.
fn shown_compare_path(path: &SharedString, room: Pixels, window: &Window) -> SharedString {
    let key = LineKey::current(window);
    COMPARE_PATH_TAIL.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached, cached_path, cached_room, shown)) = &*cache
            && *cached == key
            && cached_path == path
            && *cached_room == room
        {
            return shown.clone();
        }
        let shown: SharedString = path_tail(path, room, |text| {
            shaped_width(text, appearance::ui_text(12.), FontWeight::MEDIUM, window).ceil()
        })
        .into();
        *cache = Some((key, path.clone(), room, shown.clone()));
        shown
    })
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

/// [`ellipsis_end`] eliding the line's middle, as macOS elides a file name,
/// so a name keeps its start and its extension.
pub(crate) fn ellipsis_middle<E: Styled>(element: E) -> E {
    element
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
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

/// What History's scope toolbar counts (`GitTurtle::history_scope_count`).
#[derive(Clone, Copy, PartialEq)]
enum ScopeCount {
    /// The commits a search found.
    Found(usize),
    /// The first and last rows of the page shown, from one.
    Rows(usize, usize),
}

impl ScopeCount {
    /// The count as the toolbar draws it: "12 commits", "1–500".
    fn text(self) -> SharedString {
        #[cfg(test)]
        SCOPE_TEXT_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
        match self {
            Self::Found(found) => crate::counted(found, "commit", "commits"),
            Self::Rows(first, last) => format!("{first}–{last}"),
        }
        .into()
    }
}

/// The scope toolbar's name for the scope `name`, or All history's.
fn scope_name_text(name: Option<&str>) -> SharedString {
    #[cfg(test)]
    SCOPE_TEXT_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
    name.map_or("All history".into(), |name| name.to_owned().into())
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

/// A navigator folder's accessible name: what activating it does, its
/// name, and its branches counted with their noun ("Expand feature · 1
/// branch", where it read "1 branches").
fn folder_name(expanded: bool, label: &str, count: usize) -> String {
    format!(
        "{} {label} · {}",
        if expanded { "Collapse" } else { "Expand" },
        crate::counted(count, "branch", "branches")
    )
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
    /// The SHA cell at its narrowest (`sha_cell_width`): Git's default short
    /// form, seven hex characters, whole in the interface font at the
    /// current size, with the cell's insets.
    sha_cell: f32,
    /// The header cell narrow History's Graph title needs whole
    /// (`graph_title_fits`).
    graph_title: f32,
}

impl HistoryMetrics {
    /// The metrics for `window`'s font and sizes, with `references` the
    /// most any loaded commit shows. The line key is read once for all of
    /// them: History's frame callback runs outside the root render, where
    /// each read builds the window's text style, an allocation, and every
    /// width below is measured again only when that key changes.
    fn current(window: &Window, references: usize) -> Self {
        let key = LineKey::current(window);
        let rem = f32::from(window.rem_size());
        let inset = rem * 0.75;
        Self {
            subject_floor: f32::from(subject_floor(&key, window)),
            subject_insets: 10. + inset,
            pill_gap: rem * 0.25,
            compact_pill: compact_pill_width(references, &key, window),
            leading: 3. + inset,
            trailing: inset + 1.,
            sha_cell: f32::from(sha_cell_width(&key, window)),
            graph_title: f32::from(header_cell_width(
                &GRAPH_TITLE,
                ColumnId::Graph.label(),
                &key,
                window,
            )),
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
/// per font, size (`key`) and number of digits. Zero without references.
fn compact_pill_width(references: usize, key: &LineKey, window: &Window) -> f32 {
    if references == 0 {
        return 0.;
    }
    let digits = ["8", "88", "888"][(references.ilog10() as usize).min(2)];
    let count = cached_text_width_for(&COMPACT_COUNT, digits, key, || {
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

/// Role of a visible History commit row within an active Shift-click/keyboard
/// commit comparison range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryRangeRole {
    None,
    Before,
    After,
    Middle,
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
    /// Inclusive visible-row span `(lo, hi)` of the active `history_range`, if
    /// both endpoints are currently visible.
    range_span: Option<(usize, usize)>,
}

/// What a History column header cell needs for its whole `title` at the
/// header's 11 pt medium at the interface size: the title and the cell's
/// 10 px and `pr_3` insets. The title is measured from the interface font,
/// and again only when the font or the sizes, `key`, change, so a steady
/// render asks the text system nothing.
fn header_cell_width(
    cache: &'static std::thread::LocalKey<RefCell<Option<(LineKey, Pixels)>>>,
    title: &str,
    key: &LineKey,
    window: &Window,
) -> Pixels {
    let title = cached_width(cache, key, || {
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
fn subject_floor(key: &LineKey, window: &Window) -> Pixels {
    header_cell_width(&SUBJECT_TITLE, ColumnId::Subject.label(), key, window)
}

/// History's SHA cell at its narrowest: seven hex characters, Git's
/// default short form, at the cell's 11 pt in the interface font, the face
/// every object identifier takes (the inspector's, Reflog's and Stashes'
/// commit IDs and Compare's blob IDs), and its 10 px and `pr_3` insets. The column is never narrower,
/// at any text size or density, whatever width was saved: at 18 pt the
/// default 84 px cell cut a commit's seven characters to five and an
/// ellipsis. Each character is measured as the widest hex digit, since the
/// interface font is not monospaced, and again only when the font or the
/// sizes, `key`, change.
fn sha_cell_width(key: &LineKey, window: &Window) -> Pixels {
    let digits = cached_width(&SHA_DIGITS, key, || {
        let style = window.text_style();
        let size = appearance::ui_text(11.);
        let widest = (b'0'..=b'9')
            .chain(b'a'..=b'f')
            .map(|digit| {
                let digit = [digit];
                let digit = std::str::from_utf8(&digit).unwrap_or_default();
                window
                    .text_system()
                    .layout_line(digit, size, &[style.to_run(digit.len())], None)
                    .width
            })
            .fold(px(0.), Pixels::max);
        (widest * 7.).ceil()
    });
    px(10.) + window.rem_size() * 0.75 + digits
}

#[cfg(test)]
thread_local! {
    /// Test-only: the font each [`font_probe`] last stood in, by name.
    static PROBED_FONTS: RefCell<Vec<(&'static str, Font)>> = const { RefCell::new(Vec::new()) };
}

/// Test-only: an empty element that records, as it prepaints, the font of
/// the text beside it under `name`, for a test that asks which face a
/// piece of text took ([`probed_font`]); outside tests it is nothing.
#[cfg(test)]
fn font_probe(name: &'static str) -> Option<AnyElement> {
    Some(
        canvas(
            move |_, window, _| {
                let font = window.text_style().font();
                PROBED_FONTS.with_borrow_mut(|fonts| {
                    fonts.retain(|(probed, _)| *probed != name);
                    fonts.push((name, font));
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
        .into_any_element(),
    )
}

#[cfg(not(test))]
fn font_probe(_: &'static str) -> Option<AnyElement> {
    None
}

/// Test-only: the font the [`font_probe`] named `name` last stood in.
#[cfg(test)]
pub(crate) fn probed_font(name: &str) -> Option<Font> {
    PROBED_FONTS.with_borrow(|fonts| {
        fonts
            .iter()
            .find(|(probed, _)| *probed == name)
            .map(|(_, font)| font.clone())
    })
}

/// Whether narrow History's Graph title fits whole in its `width`-wide
/// header cell, as this frame's `metrics` measured it.
fn graph_title_fits(width: f32, metrics: HistoryMetrics) -> bool {
    width >= metrics.graph_title
}

/// What a line of interface text is laid out with besides its text: the
/// inherited font, and the interface and rem sizes its text size comes from.
///
/// Within the window's root render every key is the one the render read as
/// it began ([`LineKey::frame`]), which holds only while nothing laid out
/// inside the render's body sets another font: GPUI keeps its text-style
/// stack private, so the shared key cannot be checked against the stack's
/// depth. An element the render lays out as a root of its own to measure
/// it (`in_frame::measured_height`, as the inspector's message extent
/// does) pushes its own text style as it is laid out, and a component it
/// builds then, whose render runs inside that layout, would read the
/// render's key: none of those components reads one, and one that lays out
/// text by its key there must take the font it is given instead.
#[derive(Clone, PartialEq)]
pub(crate) struct LineKey {
    font: Font,
    scale: u32,
    rem: u32,
}

thread_local! {
    /// The key and line height the window's root render read as it began,
    /// which every [`LineKey::current`] and [`LineKey::line_height`] for that
    /// window shares until the render ends ([`LineKey::frame`]).
    static FRAME_LINE: RefCell<Option<(gpui::WindowId, LineKey, DefiniteLength)>> =
        const { RefCell::new(None) };
}

impl LineKey {
    /// The key a line laid out now takes. Within the window's root render
    /// it is the one that render read as it began ([`Self::frame`]):
    /// nothing a render does nests the text style it reads, which only its
    /// elements' layout does (the limit is the type's), and reading that
    /// style builds GPUI's default font features, an allocation, at every
    /// read, which the tab strip once made on every frame however little
    /// changed. Anywhere else, as in a view drawn inside the window or an
    /// element's prepaint, whose ancestors may set another font, it reads
    /// the window's text style.
    #[cfg_attr(test, track_caller)]
    pub(crate) fn current(window: &Window) -> Self {
        let window_id = window.window_handle().window_id();
        let shared = FRAME_LINE.with_borrow(|frame| {
            frame
                .as_ref()
                .filter(|(drawn, ..)| *drawn == window_id)
                .map(|(_, key, _)| key.clone())
        });
        #[cfg(test)]
        line_key_reads::current(shared.is_some(), std::panic::Location::caller());
        shared.unwrap_or_else(|| Self::read(window))
    }

    /// The line height interface text takes, as a length its text size
    /// resolves, shared as [`Self::current`] shares the key: within the
    /// window's root render the one that render read with its key as it
    /// began ([`Self::frame`]), anywhere else the window's text style's.
    /// The header's and the action bar's layout predictions read it many
    /// times a frame (`GitTurtle::chrome`, `GitTurtle::header_plan`), each
    /// read of the window's text style an allocation: 13 a frame in History
    /// at 1,480 × 800.
    #[cfg_attr(test, track_caller)]
    pub(crate) fn line_height(window: &Window) -> DefiniteLength {
        let window_id = window.window_handle().window_id();
        let shared = FRAME_LINE.with_borrow(|frame| {
            frame
                .as_ref()
                .filter(|(drawn, ..)| *drawn == window_id)
                .map(|(.., line_height)| *line_height)
        });
        #[cfg(test)]
        line_key_reads::line_height(shared.is_some(), std::panic::Location::caller());
        shared.unwrap_or_else(|| window.text_style().line_height)
    }

    /// The key the window's text style gives now.
    fn read(window: &Window) -> Self {
        Self::of(&window.text_style(), window)
    }

    /// The key `style`, the window's text style, gives.
    fn of(style: &gpui::TextStyle, window: &Window) -> Self {
        Self {
            font: style.font(),
            scale: appearance::ui_scale().to_bits(),
            rem: f32::from(window.rem_size()).to_bits(),
        }
    }

    /// Reads the key and the line height once for the window's root render,
    /// in one read of the window's text style; the render holds the returned
    /// guard for its whole body: every [`Self::current`] and
    /// [`Self::line_height`] for the window shares them until the guard
    /// drops, and what was shared before comes back as it drops.
    pub(crate) fn frame(window: &Window) -> FrameLine {
        let style = window.text_style();
        let key = Self::of(&style, window);
        #[cfg(test)]
        line_key_reads::frame();
        FrameLine(FRAME_LINE.replace(Some((
            window.window_handle().window_id(),
            key,
            style.line_height,
        ))))
    }
}

/// Test-only: how the line keys a stretch of frames took were read, for the
/// [`LineKey::current`] calls in one source file a test watches: the root
/// renders' reads ([`LineKey::frame`]), and the watched calls that shared
/// a root render's key or read the window's text style for their own, an
/// allocation each. It counts without allocating, so a test that counts
/// allocations can count reads too.
#[cfg(test)]
pub(crate) mod line_key_reads {
    use std::cell::Cell;

    /// What [`take`] returns.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Reads {
        /// The root renders' reads.
        pub(crate) frames: usize,
        /// The watched calls that shared a root render's key.
        pub(crate) shared: usize,
        /// The watched calls that read their own.
        pub(crate) read: usize,
    }

    thread_local! {
        static WATCHED: Cell<Option<&'static str>> = const { Cell::new(None) };
        static READS: Cell<Reads> = const {
            Cell::new(Reads { frames: 0, shared: 0, read: 0 })
        };
    }

    /// Counts from now the reads of the calls in the source file whose path
    /// ends with `file`, and the root renders'.
    pub(crate) fn watch(file: &'static str) {
        WATCHED.set(Some(file));
        READS.set(Reads::default());
    }

    /// The reads counted since [`watch`] or the last take, which stops
    /// nothing.
    pub(crate) fn take() -> Reads {
        READS.replace(Reads::default())
    }

    /// Stops counting.
    pub(crate) fn stop() {
        WATCHED.set(None);
        READS.set(Reads::default());
    }

    pub(super) fn frame() {
        if WATCHED.get().is_some() {
            READS.set(Reads {
                frames: READS.get().frames + 1,
                ..READS.get()
            });
        }
    }

    pub(super) fn current(shared: bool, caller: &std::panic::Location<'_>) {
        if WATCHED
            .get()
            .is_some_and(|file| caller.file().ends_with(file))
        {
            let mut reads = READS.get();
            if shared {
                reads.shared += 1;
            } else {
                reads.read += 1;
            }
            READS.set(reads);
        }
    }

    thread_local! {
        /// The line heights read ([`super::LineKey::line_height`]): those
        /// that shared a root render's, and those that read the window's
        /// text style for their own.
        static LINE_HEIGHTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    /// The line heights read since the last take, wherever they were read:
    /// how many shared a root render's and how many read their own.
    pub(crate) fn take_line_heights() -> (usize, usize) {
        LINE_HEIGHTS.replace((0, 0))
    }

    thread_local! {
        /// The lines in `views.rs` of the calls that shared a root render's
        /// line height, each with how often, kept without allocating.
        static SHARED_CALLERS: Cell<[(u32, u32); 8]> = const { Cell::new([(0, 0); 8]) };
    }

    /// The lines in `views.rs` of the calls that shared a root render's line
    /// height since the last take.
    pub(crate) fn take_shared_callers() -> Vec<u32> {
        (SHARED_CALLERS.replace([(0, 0); 8]).into_iter())
            .filter(|(_, count)| *count > 0)
            .map(|(line, _)| line)
            .collect()
    }

    pub(super) fn line_height(shared: bool, caller: &std::panic::Location<'_>) {
        let (kept, read) = LINE_HEIGHTS.get();
        LINE_HEIGHTS.set(if shared {
            (kept + 1, read)
        } else {
            (kept, read + 1)
        });
        if shared && caller.file().ends_with("views.rs") {
            let mut callers = SHARED_CALLERS.get();
            if let Some(slot) = callers
                .iter_mut()
                .find(|(line, count)| *line == caller.line() || *count == 0)
            {
                *slot = (caller.line(), slot.1 + 1);
            }
            SHARED_CALLERS.set(callers);
        }
    }
}

/// The line key and line height a window's root render shares while it runs
/// ([`LineKey::frame`]).
pub(crate) struct FrameLine(Option<(gpui::WindowId, LineKey, DefiniteLength)>);

impl Drop for FrameLine {
    fn drop(&mut self) {
        FRAME_LINE.set(self.0.take());
    }
}

/// One text's laid-out width, with the text and what it was laid out with.
pub(crate) type TextWidth = RefCell<Option<(LineKey, String, Pixels)>>;

thread_local! {
    static SCOPE_LABELS: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    /// History's scope toolbar count and scope name as drawn lately, by what
    /// they show (`GitTurtle::history_scope_count`, `history_scope_name`).
    static SCOPE_COUNT_TEXT: RefCell<Measured<ScopeCount, SharedString>> =
        const { RefCell::new(Measured::new()) };
    static SCOPE_NAME_TEXT: RefCell<Measured<Option<String>, SharedString>> =
        const { RefCell::new(Measured::new()) };
    static SCOPE_COUNT: TextWidth = const { RefCell::new(None) };
    static SCOPE_NOTICE: TextWidth = const { RefCell::new(None) };
    static SCOPE_NAME: TextWidth = const { RefCell::new(None) };
    static FILES_COUNT: TextWidth = const { RefCell::new(None) };
    static FILES_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static TEXT_MODES: TextWidth = const { RefCell::new(None) };
    static FOOTER_LINE: TextWidth = const { RefCell::new(None) };
    static COMPARE_PATH: TextWidth = const { RefCell::new(None) };
    /// The path Compare last showed, for its room (`shown_compare_path`).
    static COMPARE_PATH_TAIL: RefCell<Option<(LineKey, SharedString, Pixels, SharedString)>> =
        const { RefCell::new(None) };
    static PARENT_LABELS: TextWidth = const { RefCell::new(None) };
    static AREA_BADGE: TextWidth = const { RefCell::new(None) };
    static STATUS_WORDS: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static BLAME_LABEL: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static TEXT_MODE_WIDEST: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static SUBJECT_TITLE: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static GRAPH_TITLE: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static SHA_DIGITS: RefCell<Option<(LineKey, Pixels)>> = const { RefCell::new(None) };
    static COMPACT_COUNT: TextWidth = const { RefCell::new(None) };
    /// A fresh outcome in the status line (`GitTurtle::fit_status_bar`),
    /// kept as [`STATUS_TEXT`] keeps the line's whole text, for each
    /// outcome, font, size and scale factor drawn lately: one slot made two
    /// windows, each with its own outcome, measure theirs again on each
    /// other's frames.
    static STATUS_OUTCOME: StatusLineWidths = const { RefCell::new(Measured::new()) };
    /// The status line's whole text where it has no parts, measured where
    /// spare keyboard hints ask whether it keeps it
    /// (`GitTurtle::fit_status_bar`) or a hover whether it is drawn whole
    /// (`whole_status_width`), for each text, font, size and scale factor
    /// drawn lately, so windows drawn in turn keep their own, as
    /// [`STATUS_WIDTHS`] does. One slot, shared by every window, measured
    /// each window's line again on the other's frames while Keyboard
    /// Shortcuts was open in both.
    static STATUS_TEXT: StatusLineWidths = const { RefCell::new(Measured::new()) };
    /// The laid-out widths of the status line's leading whole parts, each
    /// with the byte where it ends, and its whole width (`status_parts_end`),
    /// kept for each text, font, size and scale factor drawn lately, as
    /// [`STATUS_TEXT`] keeps a text's without parts; the spare hints and a
    /// hover read a text's whole width with parts here.
    static STATUS_PARTS: StatusPartWidths = const { RefCell::new(Measured::new()) };
    /// [`StatusWidths`] for the keyboard hints (by what they follow from,
    /// [`HintsFor`]), font, sizes and scale factor drawn lately, so the
    /// windows, pages and text sizes drawn in turn keep their own; its gaps
    /// and the warning's controls snap to the window's device pixels.
    static STATUS_WIDTHS: RefCell<Measured<StatusWidthsKey, StatusWidths>> =
        const { RefCell::new(Measured::new()) };
    /// The changed-file filter's placeholder as it fits the width its field
    /// last painted at (`workspace::placeholder_fit`).
    static CHANGED_FILTER_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    /// History's search placeholder as it fits the width its field last
    /// painted at.
    static SEARCH_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    /// The navigation filter's placeholder as it fits the width its field
    /// last painted at.
    static NAV_FILTER_FIT: crate::workspace::PlaceholderFit = const { RefCell::new(None) };
    /// The Changes toggle's label without a count and its widest with one
    /// of six digits (`GitTurtle::full_bar_room`).
    static CHANGES_LABELS: RefCell<Option<(LineKey, (Pixels, Pixels))>> =
        const { RefCell::new(None) };
    /// Whether the status bar keeps Cancel's room, for each width and text
    /// size asked lately (`GitTurtle::full_bar_room`, [`BarRoomKey`]).
    static BAR_ROOM: RefCell<Measured<BarRoomKey, bool>> =
        const { RefCell::new(Measured::new()) };
    /// [`SidebarWidths`] for the current font and sizes.
    static SIDEBAR_WIDTHS: RefCell<Option<(LineKey, SidebarWidths)>> = const { RefCell::new(None) };
}

/// Empties [`CHROME_WIDTHS`], the chrome's kept widths and labels, for a test
/// that counts what the chrome builds: a GPUI test's seeds share the thread
/// and its cache, so from the second seed on a text the first built is kept.
#[cfg(test)]
pub(crate) fn forget_chrome_text() {
    CHROME_WIDTHS.with_borrow_mut(|cache| *cache = Measured::new());
}

/// Empties [`STATUS_TEXT`], for a test that counts what it measures: a GPUI
/// test's seeds share the thread and its cache, so from the second seed on
/// every line it draws was kept by the first.
#[cfg(test)]
pub(crate) fn forget_status_text() {
    STATUS_TEXT.with_borrow_mut(|cache| *cache = Measured::new());
}

thread_local! {
    /// Back's tooltip on the Settings page with the label it reads
    /// ([`settings_back_tooltip`]).
    static SETTINGS_BACK_TOOLTIP: RefCell<Option<(&'static str, SharedString)>> =
        const { RefCell::new(None) };
}

/// Back's tooltip on the Settings page, which reads `label` (back to
/// Projects or to the repository, as Settings was opened) and Back's
/// shortcut, formatted only when the label changes: every frame of Settings
/// formatted it again.
fn settings_back_tooltip(label: &'static str) -> SharedString {
    SETTINGS_BACK_TOOLTIP.with_borrow_mut(|kept| {
        if let Some((kept_label, tooltip)) = kept.as_ref()
            && *kept_label == label
        {
            return tooltip.clone();
        }
        #[cfg(test)]
        SETTINGS_BACK_TOOLTIP_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
        let tooltip = SharedString::from(format!("{label} · {}[", primary_label()));
        *kept = Some((label, tooltip.clone()));
        tooltip
    })
}

/// Empties [`STATUS_WIDTHS`] and [`SETTINGS_BACK_TOOLTIP`], for a test that
/// counts what the status bar's hints and Settings' Back build: a GPUI
/// test's seeds share the thread and its caches.
#[cfg(test)]
pub(crate) fn forget_status_hints() {
    STATUS_WIDTHS.with_borrow_mut(|cache| *cache = Measured::new());
    SETTINGS_BACK_TOOLTIP.set(None);
}

/// What the status bar's room for Cancel is kept by
/// (`GitTurtle::full_bar_room`): the font and sizes, the window's scale
/// factor, the header's width, whether it is narrow, and the widest
/// Changes. Without the scale factor a window moved to a monitor of another
/// scale, or two windows on two, read each other's answer at one logical
/// width, though the header snaps to each one's device pixels.
type BarRoomKey = (LineKey, u32, u32, bool, u32);

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

/// The status bar's warning while a local watch needs Refresh, and its two
/// actions. They stay whole: the hints give way beside them.
const WATCH_WARNING: &str = "Some changes need Refresh";
const WATCH_REFRESH: &str = "Refresh";
const WATCH_DETAILS: &str = "Details…";
/// The height of the warning's controls in the 26 px bar, and the side of
/// Details… where it is drawn as its icon, at the default interface size
/// ([`watch_control_height`]).
const WATCH_CONTROL: f32 = 20.;

/// The height of the warning's Refresh and Details… in the 26 px status
/// bar: [`WATCH_CONTROL`], or less where the Button focus ring outside them
/// would not fit inside the bar below its 1 px top rule. At 13 pt the bar
/// holds 25 px there, and the ring of a 20 px control ran 0.5 px past the
/// window's foot, cut along its bottom.
fn watch_control_height(cx: &App) -> Pixels {
    let room = appearance::ui_size(26.) - px(1.) - appearance::button_ring_room(cx) * 2.;
    appearance::ui_size(WATCH_CONTROL).min(room)
}

/// What the status bar's keyboard hints follow from
/// (`GitTurtle::key_hints`): under a modal dialog or sheet, the keys of the
/// dialog control holding focus; else the page, and on a Repository page its
/// mode, or Working's diff text holding focus with its per-line checkboxes
/// shown, where Space selects lines, for unstaging where `PartialLines` is
/// true (`diff_view::DiffView::line_keys`). The bar's measured hints are
/// kept by it with the font, sizes and scale factor ([`STATUS_WIDTHS`]), so
/// a frame builds the hints, their forms and their widths only where one of
/// those changes: every frame built the page's hints again, a few
/// allocations each, to look the widths up by them.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum HintsFor {
    Dialog(shortcuts::DialogKeys),
    Settings,
    Projects,
    Repository(WorkspaceMode),
    PartialLines(bool),
}

impl HintsFor {
    /// The hints, each key with what it does.
    pub(super) fn hints(self) -> shortcuts::KeyHints {
        #[cfg(test)]
        KEY_HINTS_BUILT.with(|built| built.set(built.get() + 1));
        let key = |key: &str, action| (key.to_owned(), action);
        let escape = |action| key(shortcuts::ESCAPE, action);
        shortcuts::KeyHints::new(match self {
            Self::Dialog(keys) => return keys.key_hints(),
            Self::Settings => vec![key("Tab", "Move between controls"), escape("Back")],
            Self::Projects => vec![
                (format!("{}O", primary_label()), "Open repository"),
                escape("Back"),
            ],
            Self::Repository(WorkspaceMode::Compare) => {
                vec![escape("Back to history"), key("↑↓", "Files")]
            }
            Self::Repository(WorkspaceMode::Working) => {
                vec![key("↑↓", "Review files"), escape("Back to history")]
            }
            Self::PartialLines(unstage) => vec![
                (
                    shortcuts::key_label("space"),
                    if unstage {
                        "Select lines to unstage"
                    } else {
                        "Select lines to stage"
                    },
                ),
                escape("Back to history"),
            ],
            Self::Repository(WorkspaceMode::History) => vec![
                (format!("{}F", primary_label()), "Search"),
                key("↑↓", "Navigate"),
                key(shortcuts::ENTER, "Open file"),
            ],
        })
    }
}

/// What [`STATUS_WIDTHS`] keeps each [`StatusWidths`] by: the line key, the
/// window's scale factor and what the hints follow from.
type StatusWidthsKey = (LineKey, u32, Option<HintsFor>);

/// What the status bar's fitting measures, in the font and sizes of a
/// [`LineKey`]: each form its keyboard hints can take, in the order they
/// give way (`shortcuts::KeyHints::giving_way`: whole, their keys alone,
/// then fewer keys, Escape the last to go); the
/// warning's words, Refresh and Details…, labelled or as its icon, with the
/// gaps between; and the ellipsis the status line keeps at least.
struct StatusWidths {
    forms: Vec<(SharedString, Pixels)>,
    /// The whole form with the spare hints after the others, where there
    /// are any (`shortcuts::KeyHints::spare`).
    spare: Option<(SharedString, Pixels)>,
    warning: Pixels,
    warning_compact: Pixels,
    ellipsis: Pixels,
}

/// How the status bar lays out in a window (`GitTurtle::fit_status_bar`):
/// the form of the keyboard hints it has room for, if any, whether the
/// watch warning's Details… is drawn as its icon, and where the status
/// line's text ends where it keeps only some of its parts
/// ([`status_parts_end`]).
#[derive(Debug, PartialEq, Eq)]
struct StatusFit {
    hints: Option<SharedString>,
    details_icon: bool,
    parts: Option<usize>,
}

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
/// The navigation tree's accessible name on a marked tab in `mode`, which
/// lists nothing read before the mark (`GitTurtle::render_sidebar`): its
/// line ([`nav_unavailable_line`]) and the mark's way back
/// (`branch_actions::UNAVAILABLE_WAY_BACK`).
fn nav_unavailable_name(mode: NavMode) -> &'static str {
    match mode {
        NavMode::Local => {
            "Branches unavailable. Refresh, or choose a repository with Open Repository"
        }
        NavMode::Remote => {
            "Remote branches unavailable. Refresh, or choose a repository with Open Repository"
        }
        NavMode::Worktrees => {
            "Worktrees unavailable. Refresh, or choose a repository with Open Repository"
        }
    }
}

/// What the navigation says on a marked tab in `mode`, as Changes says
/// "Changes unavailable" there, the mark's way back in its tooltip.
fn nav_unavailable_line(mode: NavMode) -> &'static str {
    match mode {
        NavMode::Local => "Branches unavailable",
        NavMode::Remote => "Remote branches unavailable",
        NavMode::Worktrees => "Worktrees unavailable",
    }
}

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
    /// Widths History has taken from the bounds it is laid out in
    /// (`GitTurtle::take_history_width`).
    static HISTORY_WIDTHS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// While a test records them, the lanes each frame's History rows were
    /// built to show: the first and how many.
    pub(crate) static HISTORY_LANES: RefCell<Option<Vec<(usize, usize)>>> = const { RefCell::new(None) };
    /// Room taken off the left of the body beside the project pane, in
    /// pixels, so the body is narrower than the window's content, as no test
    /// window draws it otherwise
    /// (`the_project_pane_is_fitted_to_the_body_it_is_drawn_in`).
    static BODY_INSET: std::cell::Cell<f32> = const { std::cell::Cell::new(0.) };
    /// The client-decorated frame, its decorations and paddings, a test lays
    /// its window out in, for what is decided from the body's size
    /// ([`window_frame`]), as no test window has client decorations
    /// (`the_header_and_status_bar_fit_the_body_they_are_drawn_in`).
    static TEST_FRAME: std::cell::Cell<Option<(Decorations, Edges<Pixels>)>> =
        const { std::cell::Cell::new(None) };
    /// The accessible name each control of a compact layout was given since
    /// the map was last cleared, by debug selector
    /// (`compact_controls_carry_their_accessible_names`).
    pub(crate) static COMPACT_NAMES: RefCell<std::collections::BTreeMap<&'static str, &'static str>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// [`COMPACT_NAMES`] for the names a compact control is given from what
    /// it shows, such as the Changes count (`compact_control_label`).
    pub(crate) static COMPACT_LABELS: RefCell<std::collections::BTreeMap<&'static str, SharedString>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    /// What the header's repository name and path showed in the last frame
    /// drawn (`GitTurtle::shown_identity`); the compact row shows no path.
    pub(crate) static HEADER_SHOWN: RefCell<Option<(SharedString, SharedString)>> =
        const { RefCell::new(None) };
    /// How many times the header's identity was looked up, how many of
    /// those measured it, and what the lookups allocated
    /// (`GitTurtle::shown_identity`).
    static IDENTITY_LOOKUPS: std::cell::Cell<(usize, usize, u64)> =
        const { std::cell::Cell::new((0, 0, 0)) };
    /// What Compare's path showed in the last frame drawn and its whole
    /// accessible name (`path_tail`).
    pub(crate) static COMPARE_PATH_SHOWN: RefCell<Option<(SharedString, SharedString)>> =
        const { RefCell::new(None) };
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
/// the cut-out. Above 99 it reads "99+" ([`changes_badge_text`], which the
/// chrome keeps).
fn changes_badge(count: SharedString, selected: bool, p: appearance::Palette) -> impl IntoElement {
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
                .debug_selector({
                    let count = count.clone();
                    move || format!("changes-count: {count}")
                })
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
                .child(count),
        )
}

/// The compact Changes toggle's badge text for `count` working files: the
/// count, or "99+" past 99, and nothing with none. Built with the chrome's
/// text ([`ChromeLabels`]), so a frame at rest draws the kept copy rather
/// than formatting the count again.
fn changes_badge_text(count: usize) -> SharedString {
    #[cfg(test)]
    CHROME_LABELS_FORMATTED.with(|formatted| formatted.set(formatted.get() + 1));
    match count {
        0 => SharedString::default(),
        100.. => SharedString::new_static("99+"),
        count => count.to_string().into(),
    }
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

/// `control` drawn with `tooltip`, noted under its element ID `id` as the
/// tooltip it is drawn with ([`note_control_tooltip`]): the value noted is
/// the one the control takes.
pub(crate) fn noted_tooltip(control: Button, id: &'static str, tooltip: SharedString) -> Button {
    note_control_tooltip(&id.into(), &tooltip);
    control.tooltip(tooltip)
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

/// A History commit row's element ID: its commit's object ID, its first 160
/// bits as bytes, so a frame formats nothing for it. The state GPUI keeps
/// for the row, its open right-click menu and its references' tooltip among
/// it, stays with that commit as commits arrive above it, as Latest or a
/// commit's reload brings them, and goes with the row when the commit leaves
/// the list. Keyed by the commit's index, a menu open on a commit passed to
/// whichever commit took that index, and acted on it. An ID that is not
/// hexadecimal keeps its text.
pub(crate) fn commit_row_id(oid: &str) -> ElementId {
    fn nibble(digit: u8) -> u8 {
        match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            _ => digit - b'A' + 10,
        }
    }
    let hex = oid.as_bytes();
    if hex.len() < 40 || !hex.iter().all(u8::is_ascii_hexdigit) {
        return ElementId::Name(oid.to_owned().into());
    }
    let mut id = [0u8; 20];
    let (pairs, _) = hex.as_chunks::<2>();
    for (byte, [high, low]) in id.iter_mut().zip(pairs) {
        *byte = (nibble(*high) << 4) | nibble(*low);
    }
    ElementId::OpaqueId(id)
}

/// `control`, a mode toggle, marked where `selected` with a 2 px accent bar
/// along its foot, `inset` in from either side, so under its label (the
/// label inset) or its glyph (the icon inset), over the `selected` fill it
/// keeps. That fill alone stood 1.40:1 against the panel in Midnight; the
/// bar stands 3:1 or more against it in every palette, as a row's leading
/// marker marks the selected row. Labelled and as glyphs, the navigator's
/// modes and the header's History and Changes carry the one mark, so a
/// toggle that becomes its glyph as the room narrows keeps the mark it had.
/// Laid out absolutely inside the toggle, the bar moves nothing.
fn mark_selected_mode(
    control: Button,
    selected: bool,
    inset: f32,
    selector: &'static str,
    accent: u32,
) -> Button {
    control.when(selected, |control| {
        control.relative().child(
            div()
                .debug_selector(move || selector.into())
                .absolute()
                .left(appearance::ui_size(inset))
                .right(appearance::ui_size(inset))
                .bottom(appearance::ui_size(2.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(rgb(accent)),
        )
    })
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

/// Values measured for a few keys, the most recently used first, so the
/// windows, repository tabs, pages or text sizes drawn in turn each keep
/// theirs. A cache of one slot, keyed by what it measured, made two windows
/// measure their chrome again on each other's frames, every frame. It keeps
/// [`MEASURED_KEYS`], the least recently used giving way to a new key.
pub(crate) struct Measured<K, V>(Vec<(K, V)>);

/// How many keys a [`Measured`] keeps: more windows and text sizes than a
/// session draws in turn, few enough that a lookup stays a short scan.
const MEASURED_KEYS: usize = 8;

impl<K: PartialEq, V> Measured<K, V> {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    /// What was measured for `key`, measured now with `measure` where it is
    /// not kept.
    pub(crate) fn get(&mut self, key: K, measure: impl FnOnce(&K) -> V) -> &V {
        let at = self.0.iter().position(|(kept, _)| *kept == key);
        self.kept(at, || {
            let value = measure(&key);
            (key, value)
        })
    }

    /// [`Self::get`] for the key `kept` finds, built by `key` only where it
    /// is not kept, so a lookup that finds an owned key allocates nothing.
    pub(crate) fn get_where(
        &mut self,
        kept: impl Fn(&K) -> bool,
        key: impl FnOnce() -> K,
        measure: impl FnOnce(&K) -> V,
    ) -> &V {
        let at = self.0.iter().position(|(held, _)| kept(held));
        self.kept(at, || {
            let key = key();
            let value = measure(&key);
            (key, value)
        })
    }

    /// The value kept `at`, moved to the front, or else the one `measured`
    /// makes, in front of the rest.
    fn kept(&mut self, at: Option<usize>, measured: impl FnOnce() -> (K, V)) -> &V {
        match at {
            Some(at) => self.0[..=at].rotate_right(1),
            None => {
                let entry = measured();
                self.0.truncate(MEASURED_KEYS - 1);
                self.0.insert(0, entry);
            }
        }
        &self.0[0].1
    }
}

#[cfg(test)]
thread_local! {
    /// How many times [`ChromeWidths`] and [`StatusWidths`] were measured,
    /// for a test.
    static CHROME_MEASURED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times History's scope toolbar count or name was formatted
    /// (`ScopeCount::text`, `scope_name_text`), for a test.
    static SCOPE_TEXT_FORMATTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The chrome's lookups (`GitTurtle::kept_chrome`), and the allocations
    /// they made, those that built its text among them, for a test.
    static CHROME_LOOKUPS: std::cell::Cell<(usize, u64)> = const { std::cell::Cell::new((0, 0)) };
    /// How many times the labels the header draws were formatted
    /// (`GitTurtle::changes_label`, `GitTurtle::heading_text`), for a test.
    static CHROME_LABELS_FORMATTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times the chrome's text was built (`GitTurtle::chrome_text`),
    /// for a test.
    static CHROME_TEXT_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times the status bar's keyboard hints were built
    /// ([`HintsFor::hints`]), for a test.
    static KEY_HINTS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times Settings' Back tooltip was formatted
    /// ([`settings_back_tooltip`]), for a test.
    static SETTINGS_BACK_TOOLTIP_FORMATTED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    static STATUS_MEASURED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times the status line's whole text was measured for spare
    /// hints ([`STATUS_TEXT`]), for a test.
    pub(crate) static STATUS_TEXT_MEASURED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// How many times a fresh outcome was measured ([`STATUS_OUTCOME`]),
    /// for a test.
    static STATUS_OUTCOME_MEASURED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// What the status line's tooltip last showed
    /// (`GitTurtle::render_status_line`), for a test.
    static STATUS_TOOLTIP: RefCell<Option<SharedString>> = const { RefCell::new(None) };
    /// What the status line was last given to draw
    /// (`GitTurtle::render_status_line`), for a test.
    static STATUS_DRAWN: RefCell<Option<SharedString>> = const { RefCell::new(None) };
    /// The text the status line was last laid out with and whether it was
    /// cut there ([`StatusLaidOut`]), for a test.
    static STATUS_CUT: RefCell<Option<(SharedString, bool)>> = const { RefCell::new(None) };
    /// How many times the status line built its texts ([`KeptStatusText`]),
    /// for a test.
    static STATUS_TEXT_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Widths of status line texts, by text, line key and scale factor
/// ([`status_line_width`]).
type StatusLineWidths = RefCell<Measured<(LineKey, u32, String), Pixels>>;

/// The laid-out widths of status line texts' leading whole parts, each with
/// the byte where it ends, and last their whole widths, by text, line key and
/// scale factor ([`status_part_widths`]).
type StatusPartWidths = RefCell<Measured<(LineKey, u32, String), Vec<(usize, Pixels)>>>;

/// The width of `text` in the status line, kept in `cache` for each text,
/// font, size and scale factor drawn lately, so windows drawn in turn keep
/// their own; `measure` lays it out where it is not kept. Finding a kept
/// text allocates nothing.
fn status_line_width(
    cache: &'static std::thread::LocalKey<StatusLineWidths>,
    text: &str,
    window: &Window,
    measure: impl FnOnce() -> Pixels,
) -> Pixels {
    let (line, scale) = (LineKey::current(window), window.scale_factor().to_bits());
    cache.with_borrow_mut(|cache| {
        *cache.get_where(
            |(kept, kept_scale, kept_text)| {
                *kept == line && *kept_scale == scale && kept_text == text
            },
            || (line.clone(), scale, text.to_owned()),
            |_| measure(),
        )
    })
}

/// The status line's whole `text` as it is laid out: for a text with parts
/// ([`STATUS_SEPARATOR`]), the last of the widths its parts are read from
/// ([`STATUS_PARTS`]), which the line's fitting lays it out for anyway;
/// for another, its own layout ([`STATUS_TEXT`]). Each laid out a text with
/// parts once more where spare hints asked for its whole width. Both are
/// rounded once, the one way layout rounds a text element
/// ([`laid_out_text_width`]): a text without parts was rounded to a whole
/// pixel only, at scale 1.5 up to a device pixel short of the same words
/// with a part after them, so the line could fit a text in one place and
/// not in another.
fn whole_status_width(text: &str, window: &Window) -> Pixels {
    if text.contains(STATUS_SEPARATOR) {
        return status_parts(text, window, |widths| {
            widths.last().map_or(px(0.), |(_, width)| *width)
        });
    }
    status_line_width(&STATUS_TEXT, text, window, || {
        #[cfg(test)]
        STATUS_TEXT_MEASURED.with(|measured| measured.set(measured.get() + 1));
        let weight = window.text_style().font_weight;
        laid_out_text_width(
            shaped_width(text, appearance::ui_text(10.), weight, window),
            window,
        )
    })
}

/// What separates the parts of a status line, such as "1 changed file",
/// "content read 12.3 ms" and "captured snapshot".
const STATUS_SEPARATOR: &str = " · ";

/// A status line's text joined from its parts where one comes from user
/// text, such as a path, which can hold [`STATUS_SEPARATOR`] itself: the
/// line gives way only where its parts end ([`status_parts_end`]), never
/// inside one. Split at every separator it holds, a cloned repository's
/// "Repository cloned · demo · /srv/clones/a · b · open its repository tab
/// or folder" gave way inside the path and read "Repository cloned · demo ·
/// /srv/clones/a", a whole path, and another one.
#[derive(Default)]
pub(crate) struct StatusParts {
    text: String,
    /// The byte where each part but the last ends.
    ends: Vec<usize>,
}

impl StatusParts {
    pub(crate) fn join(parts: &[&str]) -> Self {
        let mut joined = Self::default();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                joined.ends.push(joined.text.len());
                joined.text.push_str(STATUS_SEPARATOR);
            }
            joined.text.push_str(part);
        }
        joined
    }

    /// Where `text`'s parts end, where it is this text; another, as the
    /// status line says once anything else has replaced it, gives way at
    /// each separator it holds.
    fn ends_in(&self, text: &str) -> Option<&[usize]> {
        (self.text == text).then_some(self.ends.as_slice())
    }
}

/// The one line of a status text the bar draws before its parts give way
/// ([`first_status_line`]).
#[derive(Clone, Copy)]
struct StatusLine<'a> {
    /// The line, a slice of the status text.
    text: &'a str,
    /// Where it starts in the status text, after the line breaks it begins
    /// with.
    start: usize,
    /// Where the status text's parts end, where a part comes from user text
    /// ([`StatusParts`]).
    ends: Option<&'a [usize]>,
    /// Whether a line break inside its part ends it, so it is drawn ending
    /// in an ellipsis.
    ellipsis: bool,
    /// Whether it is the whole status text, but for the line breaks it ends
    /// in.
    whole: bool,
}

impl StatusLine<'_> {
    /// Whether a part of the line ends at its byte `end`, where a separator
    /// starts: at one of its known `ends`, else at every separator.
    fn part_ends_at(&self, end: usize) -> bool {
        self.ends
            .is_none_or(|ends| ends.contains(&(self.start + end)))
    }

    /// The room the ellipsis it ends in takes beside its text, `ellipsis`
    /// wide, or none.
    fn ellipsis_room(&self, ellipsis: Pixels) -> Pixels {
        if self.ellipsis { ellipsis } else { px(0.) }
    }
}

/// The line of the status line's `text` that the bar draws before its parts
/// give way ([`status_parts_end`]): the bar is one line high and never draws
/// a second. Drawn whole, a read's error that names its folder on a line
/// after its reason, as a marked tab's does (`repository_access::explain`),
/// laid out as two lines centred in the 26 px bar, the first above the
/// bar's top edge, where the page painted over it. Line breaks the text
/// ends in are trimmed, so "Up to date" and a line break is drawn whole,
/// and those it begins with are skipped, so its first line with text is
/// drawn. A line break inside the first part ends the line there, in an
/// ellipsis, so a path cut at its break never reads as another, whole path;
/// where `ends` knows the text's parts ([`StatusParts`]), one inside a
/// later part ends the line with the part before it, as that part would
/// give way. A text whose parts are not known takes the break as falling
/// in its first part, since a separator in it can lie inside user data. A
/// line that is not the whole text is shortened, as one that drops a part
/// is: its tooltip and accessible value keep the whole text
/// (`GitTurtle::render_status_line`).
fn first_status_line<'a>(text: &'a str, ends: Option<&'a [usize]>) -> StatusLine<'a> {
    let trimmed = text.trim_end_matches(['\n', '\r']);
    let rest = trimmed.trim_start_matches(['\n', '\r']);
    let start = trimmed.len() - rest.len();
    let line = |text, ellipsis, whole| StatusLine {
        text,
        start,
        ends,
        ellipsis,
        whole,
    };
    let Some(newline) = rest.find('\n') else {
        return line(rest, false, start == 0);
    };
    let head = line(&rest[..newline], false, false);
    // Only a part whose end is known gives way at a break: a separator in a
    // text without known ends can lie inside user data, as in a read's
    // error naming a repository at "/srv/a · b", whose break would end the
    // line at "/srv/a", a whole-looking path, at any width.
    let part_before = ends.and_then(|ends| {
        head.text
            .match_indices(STATUS_SEPARATOR)
            .map(|(end, _)| end)
            .filter(|end| ends.contains(&(start + end)))
            .last()
    });
    match part_before {
        Some(end) => line(&head.text[..end], false, false),
        None => line(
            head.text.strip_suffix('\r').unwrap_or(head.text),
            true,
            false,
        ),
    }
}

/// How the latest frame that drew the status line laid it out
/// (`GitTurtle::render_status_line`): the text it says, the width and text
/// size it laid that text out at, whether it drops a part or a line of it,
/// and so whether the line is shortened, decided as that frame prepainted the
/// line, in the line's own text style, as a dialog's error line decides
/// (`tags::error_line`). Kept together, so a hover never pairs one frame's
/// text with another's width, and decided again only when one of them
/// changes, so a steady frame measures nothing; the text's width itself is
/// kept for each text drawn lately (`whole_status_width`).
struct StatusLaidOut {
    text: SharedString,
    width: Pixels,
    size: Pixels,
    dropped: bool,
    cut: bool,
}

impl StatusLaidOut {
    /// Whether this is `text` laid out `width` wide at `size`, a part of it
    /// dropped where `dropped`.
    fn is(&self, text: &str, width: Pixels, size: Pixels, dropped: bool) -> bool {
        self.width == width
            && self.size == size
            && self.dropped == dropped
            && self.text.as_ref() == text
    }
}

/// The texts the status line built for a status and its fit
/// (`GitTurtle::render_status_line`): its whole text as its accessible value
/// carries it, bounded (`native_accessibility::bounded_accessible_text`),
/// and the text it draws ([`GitTurtle::status_line_drawn_text`]), the same
/// shared text where it draws the whole. Kept for the window's status line
/// and built again only when the status or its fit changes, decided without
/// building them (`native_accessibility::carries_accessible_text`), as a
/// dialog's error line keeps its text (`tags::error_line`): a status that
/// stays, as a marked tab's reason does, copies nothing in a steady frame.
struct KeptStatusText {
    whole: SharedString,
    parts: Option<usize>,
    drawn: SharedString,
}

impl KeptStatusText {
    /// Whether these are the texts for the status `text` drawn to `parts`.
    fn is(&self, text: &str, parts: Option<usize>) -> bool {
        self.parts == parts && native_accessibility::carries_accessible_text(&self.whole, text)
    }
}

/// Where the status `line` ends in a box `room` wide: all of it where it
/// fits (`None`), else after the last of its whole parts
/// ([`STATUS_SEPARATOR`]) that fit with those before it, else after its
/// first, which ends in an ellipsis. A part so reads whole or gives way
/// whole with the parts after it: at 18 pt in a 461 px half tile, Compare's
/// whole keyboard hints left "1 changed file · content read 12.3 ms" too
/// little room, and the app drew its timing cut mid-number. Its parts end
/// at each separator, or only at its known ends where a part comes from
/// user text ([`StatusLine::part_ends_at`]). Where a line break ends the
/// line, its ellipsis, `ellipsis` wide, keeps its room after whatever is
/// drawn, as the bar's fitting keeps it, so the line gives way before it
/// would run into the toolkit's own truncation. The widths are kept for
/// each text, from one layout of it ([`status_part_widths`]).
fn status_parts_end(
    line: StatusLine,
    room: Pixels,
    ellipsis: Pixels,
    window: &Window,
) -> Option<usize> {
    let text = line.text;
    if !text.contains(STATUS_SEPARATOR) {
        return None;
    }
    let room = room - line.ellipsis_room(ellipsis);
    status_parts(text, window, |widths| {
        let (whole, leading) = widths.split_last()?;
        if whole.1 <= room {
            return None;
        }
        let leading = leading.iter().filter(|(end, _)| line.part_ends_at(*end));
        let first = leading.clone().next();
        Some(
            leading
                .rev()
                .find(|(_, width)| *width <= room)
                .or(first)
                .map_or(whole.0, |(end, _)| *end),
        )
    })
}

/// `read` of the laid-out widths of `text`'s parts ([`status_part_widths`]),
/// kept for each text, font, size and scale factor drawn lately
/// ([`STATUS_PARTS`]).
fn status_parts<T>(text: &str, window: &Window, read: impl FnOnce(&[(usize, Pixels)]) -> T) -> T {
    let (line, scale) = (LineKey::current(window), window.scale_factor().to_bits());
    STATUS_PARTS.with_borrow_mut(|cache| {
        read(cache.get_where(
            |(kept, kept_scale, kept_text)| {
                *kept == line && *kept_scale == scale && kept_text == text
            },
            || (line.clone(), scale, text.to_owned()),
            |_| status_part_widths(text, window),
        ))
    })
}

/// The laid-out widths of `text`'s leading whole parts, each with the byte
/// where it ends, and last its whole width, all read from one layout of the
/// line: a new status line costs one layout, however many parts it has.
fn status_part_widths(text: &str, window: &Window) -> Vec<(usize, Pixels)> {
    #[cfg(test)]
    LINE_LAYOUTS.with(|count| count.set(count.get() + 1));
    let style = window.text_style();
    let layout = window.text_system().layout_line(
        text,
        appearance::ui_text(10.),
        &[style.to_run(text.len())],
        None,
    );
    text.match_indices(STATUS_SEPARATOR)
        .map(|(end, _)| end)
        .chain([text.len()])
        .map(|end| (end, laid_out_text_width(layout.x_for_index(end), window)))
        .collect()
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
    cached_text_width_for(cache, text, &LineKey::current(window), width)
}

/// `width` of `text` for the font and sizes `key` was read with, computed
/// again only when either changes.
fn cached_text_width_for(
    cache: &'static std::thread::LocalKey<TextWidth>,
    text: &str,
    key: &LineKey,
    width: impl FnOnce() -> Pixels,
) -> Pixels {
    cache.with(|cache| {
        let mut cache = cache.borrow_mut();
        match &*cache {
            Some((cached, cached_text, width)) if cached == key && cached_text == text => *width,
            _ => {
                let width = width();
                *cache = Some((key.clone(), text.to_owned(), width));
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

/// The width layout gives a line of text whose shaped width is `width`: its
/// text element rounds it up to a whole pixel, and layout rounds that up to
/// a whole device pixel (GPUI's `ceil_to_device_pixel`). At a whole scale
/// factor the two agree; at a fractional one, as at 1.5, the whole pixel
/// alone falls up to a device pixel short of what is drawn, so a form chosen
/// for a room it measured would not fit it.
pub(crate) fn laid_out_text_width(width: Pixels, window: &Window) -> Pixels {
    let scale = window.scale_factor();
    px((f32::from(width.ceil()) * scale).ceil() / scale)
}

/// A failure the operation error banner shows: the whole report, which
/// Details… and the alert keep, with the one line of it the banner draws,
/// built once where the failure arrives so a frame scans nothing, however
/// long Git's output makes the report. It reads as its report wherever only
/// the text matters (`Deref`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationFailure {
    report: String,
    banner: SharedString,
    /// The line is drawn whole, wrapped to as many lines as it needs as the
    /// notice's is: a write core refused before it started Git
    /// ([`Self::of`]). Every other line is cut to one with an ellipsis.
    whole: bool,
}

impl OperationFailure {
    /// `error` as the banner shows it: the summary core typed onto it where
    /// it still had Git's streams apart, otherwise the report's first line
    /// with text ([`From<String>`]). Never read back from the report's text:
    /// core's words open a report's first line, so no line of Git's first
    /// output ever starts one, and a banner that searched the report for a
    /// marker lost what `git fetch nope` says went wrong.
    ///
    /// A write core refused before it started Git is core's own sentence,
    /// naming the repository's path and the one Git's discovery finds there,
    /// and its line is drawn whole (`gitturtle_core::write_refused`),
    /// wrapped on screen: cut at 220 characters, the refusal of a
    /// repository in macOS's temporary folder lost "Git was not started"
    /// and the path Git now finds, and cut to one line on screen it lost
    /// them in any window narrower than about 1,600 pixels.
    pub(crate) fn of(error: &anyhow::Error) -> Self {
        let report = gitturtle_core::strip_bidi_controls(&format!("{error:#}")).into_owned();
        if gitturtle_core::write_refused(error) {
            let banner = first_line_with_text(&report).to_owned().into();
            return Self {
                report,
                banner,
                whole: true,
            };
        }
        Self::summarized(report, gitturtle_core::failure_summary(error))
    }

    /// This failure with `sentence`, what the window does about it, after
    /// its report and its line alike.
    pub(crate) fn followed_by(self, sentence: &str) -> Self {
        let sentence = gitturtle_core::strip_bidi_controls(sentence);
        Self {
            report: format!("{} {sentence}", self.report),
            banner: format!("{} {sentence}", self.banner).into(),
            whole: self.whole,
        }
    }

    /// `error`, a read's failure named by `what`: "`what`: …", in its report
    /// and its line alike, redacted and stripped of Unicode bidi overrides so
    /// echoed Git config values cannot leak through `Details…` or banners.
    pub(crate) fn of_read(what: &str, error: &anyhow::Error) -> Self {
        let report = gitturtle_core::redact_diagnostic(&format!("{what}: {error:#}"));
        let summary = gitturtle_core::failure_summary(error)
            .map(|line| gitturtle_core::redact_diagnostic(&format!("{what}: {line}")));
        Self::summarized(report, summary)
    }

    /// `report`, whose banner line is `summary` where it has one, otherwise
    /// its first line with text.
    pub(crate) fn summarized(report: String, summary: Option<String>) -> Self {
        let report = gitturtle_core::strip_bidi_controls(&report).into_owned();
        let summary = summary.map(|line| gitturtle_core::strip_bidi_controls(&line).into_owned());
        let banner = banner_line(summary.as_deref().unwrap_or(&report));
        Self {
            report,
            banner,
            whole: false,
        }
    }

    /// The line the banner draws.
    pub(crate) fn banner(&self) -> SharedString {
        self.banner.clone()
    }
}

/// At most 220 characters of `text`'s first line with text; `text` as it
/// stands when no line of it has any.
fn banner_line(text: &str) -> SharedString {
    first_line_with_text(text)
        .chars()
        .take(220)
        .collect::<String>()
        .into()
}

/// `text`'s first line with text; `text` as it stands when no line of it
/// has any.
fn first_line_with_text(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
}

impl From<String> for OperationFailure {
    /// A failure carrying no typed summary: its banner line is the report's
    /// first line with text.
    fn from(report: String) -> Self {
        Self::summarized(report, None)
    }
}

impl From<&str> for OperationFailure {
    fn from(report: &str) -> Self {
        report.to_owned().into()
    }
}

impl std::ops::Deref for OperationFailure {
    type Target = str;

    fn deref(&self) -> &str {
        &self.report
    }
}

impl std::fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.report)
    }
}

#[cfg(test)]
thread_local! {
    /// The line the operation error banner drew last.
    static BANNER_DRAWN: RefCell<Option<SharedString>> = const { RefCell::new(None) };
    /// The focus of the refusal's box the banner drew last
    /// ([`RefusalBox`]), held weakly.
    static REFUSAL_FOCUS: RefCell<Option<WeakFocusHandle>> = const { RefCell::new(None) };
}

#[cfg(test)]
impl GitTurtle {
    /// The operation error's whole report.
    pub(crate) fn operation_error_text(&self) -> Option<String> {
        self.operation_error.as_deref().map(str::to_owned)
    }
}

#[cfg(test)]
impl OperationFailure {
    /// A refusal before Git started whose report and line are `sentence`,
    /// drawn whole as core's refusal is ([`Self::of`]), for a refusal
    /// shorter than core's words.
    pub(crate) fn refused(sentence: &str) -> Self {
        Self {
            report: sentence.to_owned(),
            banner: sentence.to_owned().into(),
            whole: true,
        }
    }
}

/// Whether the refusal's box the banner drew last holds focus.
#[cfg(test)]
pub(crate) fn refusal_box_focused(window: &Window) -> bool {
    REFUSAL_FOCUS.with_borrow(|focus| {
        focus
            .as_ref()
            .and_then(WeakFocusHandle::upgrade)
            .is_some_and(|focus| focus.is_focused(window))
    })
}

/// The box a refusal before Git started is drawn in, in the operation error
/// banner, kept from frame to frame while the window draws, whether a
/// refusal shows it or not ([`RefusalBox::kept`]): its focus, a Tab stop
/// only while its words overflow it, the scroll of the refusal it was last
/// drawn with, so another refusal, or the same one again, opens at its top,
/// and the control the shown refusal's write was refused on, which focus the
/// box holds as it goes is given back to
/// ([`GitTurtle::return_refusal_focus`]).
struct RefusalBox {
    focus: FocusHandle,
    scroll: RefCell<Option<(Refusal, ScrollHandle)>>,
    /// The refusal shown and where focus rested as it was first drawn, the
    /// control the person pressed, held weakly so a control its frame no
    /// longer draws is let go; `None` while no refusal shows the box.
    refused_on: RefCell<Option<(Refusal, Option<WeakFocusHandle>)>>,
}

/// Which refusal a [`RefusalBox`] shows: the window's count of refusals
/// (`GitTurtle::error_refusals`), which the same refusal asked again moves,
/// and its line, which a refusal from another flow or another tab changes
/// without moving the count. Keyed on the line alone, the same refusal
/// again opened where the last was left scrolled.
type Refusal = (usize, SharedString);

impl RefusalBox {
    /// The window's box. Kept only while a frame drew it, the box went with
    /// a refusal a line Git answered replaced while it held focus, and focus
    /// stayed on its handle, which the window's ring kept alive with no
    /// frame drawing it: no key context reached a binding, so Tab moved
    /// nothing, and the next refusal's box, under another handle, was never
    /// reached.
    fn kept(window: &mut Window, cx: &mut App) -> std::rc::Rc<Self> {
        window
            .use_keyed_state(ElementId::from("operation-error-box"), cx, |_, cx| {
                std::rc::Rc::new(RefusalBox {
                    focus: cx.focus_handle(),
                    scroll: RefCell::default(),
                    refused_on: RefCell::default(),
                })
            })
            .read(cx)
            .clone()
    }

    /// Scrolls `scroll` one line of `line`, `down` or up, to the next whole
    /// line's edge, as far as its ends, so a box that holds whole lines
    /// holds them still, whatever a wheel left it at. Returns whether it
    /// moved.
    fn scroll_a_line(scroll: &ScrollHandle, line: Pixels, down: bool) -> bool {
        let (at, end) = (-scroll.offset().y, scroll.max_offset().y.max(Pixels::ZERO));
        // Within a hundredth of a line of an edge is at it: GPUI rounds the
        // offsets it keeps.
        let lines = at / line;
        let to = if down {
            (lines + 0.01).floor() + 1.
        } else {
            (lines - 0.01).ceil() - 1.
        };
        let to = (line * to).clamp(Pixels::ZERO, end);
        if to == at {
            return false;
        }
        scroll.set_offset(point(scroll.offset().x, -to));
        true
    }
}

impl GitTurtle {
    /// The operation error banner's line for `error`. A refusal before Git
    /// started ([`OperationFailure::of`]) wraps to as many lines as it needs,
    /// as the notice does, so the path Git now finds and "Git was not
    /// started" are read at every width, in a box at most a scaled 100 px
    /// tall that holds whole lines, so none is cut by its clip. Where its
    /// words are taller, as at 18 pt in a 461 × 490 half tile, the box
    /// scrolls, cued by the toolkit's scrollbar as every scrolling region is
    /// (`tags::ScrollCue`), and is a Tab stop that Up and Down scroll a line
    /// at a time, ringed on its own edge, which assistive technology reads
    /// by the alert's whole report, its ring 3 px out from the words on
    /// every side; where they fit it draws no scrollbar and is
    /// no stop, as the rewritten series' failed preview draws none and is
    /// none either (`rewrite_review::SeriesBrowser::failure_box`). Made to
    /// fit while it holds focus, it keeps focus and its ring until the next
    /// Tab; gone while it holds focus, it gives focus to the control the
    /// write was refused on ([`Self::return_refusal_focus`]). It once
    /// scrolled with nothing to say so and no key to reach its
    /// last sentence. A line Git answered stays one line, cut with an
    /// ellipsis.
    fn operation_error_line(
        &self,
        error: &OperationFailure,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        #[cfg(test)]
        BANNER_DRAWN.with_borrow_mut(|drawn| *drawn = Some(error.banner()));
        let size = crate::appearance::ui_text(11.);
        let summary = |summary: Div| {
            summary
                // A fresh node for each refusal, so the same one again is
                // announced again.
                .id(("operation-error-summary", self.error_refusals))
                .debug_selector(|| "operation-error-summary".into())
                // The whole report, which Git's output can make megabytes
                // long, bounded as every long accessible text is, and said
                // once: a multi-megabyte name and value went to assistive
                // technology in every frame.
                .alert(native_accessibility::bounded_accessible_text(&**error))
                .text_size(size)
        };
        if !error.whole {
            return native_accessibility::recorded(
                summary(div())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(error.banner()),
            )
            .into_any_element();
        }
        let refusal: Refusal = (self.error_refusals, error.banner());
        let kept = RefusalBox::kept(window, cx);
        let scroll = {
            let mut shown = kept.scroll.borrow_mut();
            match &*shown {
                Some((drawn, scroll)) if *drawn == refusal => scroll.clone(),
                _ => {
                    let scroll = ScrollHandle::new();
                    *shown = Some((refusal, scroll.clone()));
                    scroll
                }
            }
        };
        let focus = &kept.focus;
        #[cfg(test)]
        REFUSAL_FOCUS.with_borrow_mut(|kept| *kept = Some(focus.downgrade()));
        // The most whole lines a scaled 100 px holds, as the line's text
        // lays them out: the bound itself cut a line at either edge.
        let line = crate::image_compare::text_line(size, window);
        let most = line * (crate::appearance::ui_size(100.) / line).floor().max(1.);
        let cue =
            crate::tags::ScrollCue::new(&scroll, Pixels::ZERO).on(crate::tags::Surface::Removed);
        let overflows = cue.kept();
        let tracked = overflows || focus.is_focused(window);
        let keys = scroll.clone();
        let ring = self.focus_ring.edge(focus, rgb(palette(cx).accent));
        let summary = cue.list(
            summary(div().on_children_prepainted(cue.observe(|_, _, _| {})))
                .max_h(most)
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .when(tracked, |summary| {
                    summary
                        // The handle carries the Tab stop: GPUI ignores
                        // the `tab_stop` of the div that tracks it.
                        .track_focus(&focus.clone().tab_stop(overflows))
                        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, _, cx| {
                            let modifiers = &event.keystroke.modifiers;
                            if modifiers.modified() {
                                return;
                            }
                            let down = match event.keystroke.key.as_str() {
                                "up" => false,
                                "down" => true,
                                _ => return,
                            };
                            if RefusalBox::scroll_a_line(&keys, line, down) {
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }))
                })
                .child(
                    div()
                        .debug_selector(|| "operation-error-text".into())
                        .child(error.banner()),
                ),
            "operation-error-scrollbar",
        );
        // Its keyboard ring on the box's own edge, which keeps its room
        // around the scrolling words and pulls it back out of the banner's
        // padding, so neither the words nor anything beside them moves and
        // the words scroll inside the room, clipped at the lines they hold:
        // on the box's edge, the ring was drawn over the first column of
        // letters and the first line's tops. The room is a Button ring's,
        // snapped to device pixels as File History's failed message snaps
        // it, so it is the same whole pixels on every side at any scale.
        let room = window.pixel_snap(crate::appearance::button_ring_room(cx));
        div()
            .debug_selector(|| "operation-error-box".into())
            .relative()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .mx(-room)
            .my(-room)
            .p(room)
            .child(native_accessibility::recorded(summary))
            .children(ring)
            .into_any_element()
    }

    /// Follows the refusal's box as the frame is built, before anything
    /// reads focus: a refusal the box shows notes, as it is first drawn, the
    /// control focus rests on, the one the person pressed, which a refusal
    /// replacing it while the box holds focus keeps; and focus the box holds
    /// as it goes, a line Git answered or nothing taking the refusal's place,
    /// moves in this frame to that control while a frame draws it, else to
    /// the shown mode's own list, which the window names as its pane, else
    /// to the window's root, as a closing modal's focus does
    /// (`GitTurtle::modal_return_target`), so the move is heard once, on the
    /// control, and focus is never left on the box no frame draws. Gone
    /// under a dialog or sheet opened from it, the box keeps its control
    /// until the modal closes and gives focus back to it, which then moves
    /// on to the control: let go under the modal, the control was lost and
    /// focus went to the list. The box is kept while
    /// the window draws ([`RefusalBox::kept`]), so the next refusal's box is
    /// the same Tab stop, whatever came before it, and opens at its top.
    fn return_refusal_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kept = RefusalBox::kept(window, cx);
        let in_box = kept.focus.is_focused(window);
        let shown = match &self.operation_error {
            Some(error) if error.whole && self.page == AppPage::Repository => {
                Some((self.error_refusals, error.banner()))
            }
            _ => None,
        };
        if let Some(refusal) = shown {
            let mut noted = kept.refused_on.borrow_mut();
            if noted.as_ref().is_none_or(|(drawn, _)| *drawn != refusal) {
                let control = if in_box {
                    noted.take().and_then(|(_, control)| control)
                } else {
                    self.saved_focus(window.focused(cx))
                        .map(|control| control.downgrade())
                };
                *noted = Some((refusal, control));
            }
            return;
        }
        kept.scroll.replace(None);
        // Kept while a dialog or sheet is open: one opened from the box gives
        // focus back to it as it closes, and the control is where it goes on.
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let control = kept
            .refused_on
            .take()
            .and_then(|(_, control)| control?.upgrade());
        if in_box {
            self.modal_return_target(control.as_ref(), window)
                .focus(window, cx);
        }
    }
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

/// The project pane's least and greatest widths, which a drag of its edge
/// keeps it within.
const PROJECT_PANE_MIN: f32 = 180.;
const PROJECT_PANE_MAX: f32 = 360.;
/// The least width the page beside the project pane is laid out at: a
/// window whose body cannot hold it beside the pane's least width holds the
/// pane back ([`GitTurtle::project_pane_drawn`]).
const PROJECT_PAGE_MIN: f32 = 520.;

/// The element ID of the panel group that holds the project pane and the
/// page beside it, and the page's panel's place in that group, which names
/// the panel's element (`("resizable-panel", PAGE_PANEL)`, the kit's).
const WORKSPACE_COLUMNS: &str = "workspace-columns";
const PAGE_PANEL: usize = 1;

/// `page` as the body holds it alone, the project pane held back, closed or
/// not shown: inside elements with the IDs the pane's group gives it beside
/// the pane, so each control of the page keeps one element ID in both
/// forms. GPUI builds a stateless component's subtree under its type's
/// name (`ViewElement`), so the chain is the group's type and its own ID
/// ([`WORKSPACE_COLUMNS`]), then the panel's type and its ID
/// ([`PAGE_PANEL`]); `views::tests::the_page_keeps_focus_as_the_project_pane_comes_and_goes`
/// reads the path in both forms. What GPUI keeps for an element by its ID,
/// a kit Button's focus handle among it, then outlives the pane coming or
/// going: drawn alone, the page's controls took other IDs, so a Button
/// focused at 1,480 px lost its focus as the window narrowed past 700 px,
/// the window moving focus to the page's first stop, and one focused at
/// 461 px lost it as the window widened or the pane was turned on, focus
/// left on a handle no frame drew.
fn page_alone(page: AnyElement) -> AnyElement {
    use gpui_kit::component::resizable::{ResizablePanel, ResizablePanelGroup};
    let panel = record_page_path(
        div()
            .id(("resizable-panel", PAGE_PANEL))
            .size_full()
            .child(page),
    );
    div()
        .id(std::any::type_name::<ResizablePanelGroup>())
        .size_full()
        .child(
            div().id(WORKSPACE_COLUMNS).size_full().child(
                div()
                    .id(std::any::type_name::<ResizablePanel>())
                    .size_full()
                    .child(panel),
            ),
        )
        .into_any_element()
}

#[cfg(test)]
thread_local! {
    /// The element ID path the page was last drawn under
    /// ([`record_page_path`]), for a test.
    static PAGE_PATH: std::cell::RefCell<Option<GlobalElementId>> =
        const { std::cell::RefCell::new(None) };
}

/// `element`, the page's parent, recording in tests the element ID path the
/// page is drawn under as it is prepainted ([`PAGE_PATH`]); outside tests,
/// `element` as it is.
fn record_page_path<E: ParentElement>(element: E) -> E {
    #[cfg(test)]
    {
        use gpui_kit::base::ElementExt as _;
        element.on_prepaint(|_, window, _| {
            window.with_global_id(ElementId::from("page"), |path, _| {
                PAGE_PATH.with_borrow_mut(|recorded| *recorded = Some(path.clone()))
            })
        })
    }
    #[cfg(not(test))]
    {
        element
    }
}

/// The widest the project pane is drawn in a body `body` wide: its greatest
/// width, or less where the page beside it needs the room for its floor, on
/// the device pixel at or before that, so the page is never laid out past
/// the body's edge. A pane dragged wide in a wide window and narrowed with
/// it kept its share of the body, 270 px of a 750 px body for one dragged to
/// 360 px of 1,000, and pushed the 520 px page 40 px past the window's edge.
fn project_pane_most(body: Pixels, window: &Window) -> Pixels {
    let scale = window.scale_factor();
    let room = f32::from(body - px(PROJECT_PAGE_MIN)) * scale;
    // The small allowance keeps a whole device pixel that the division
    // leaves a hair under, as 811 at 1.5 from a 1,060.67 px body.
    px(((room + 1e-3).floor() / scale).min(PROJECT_PANE_MAX))
}

impl GitTurtle {
    /// Whether the project pane is drawn in this window: where it is shown,
    /// on Repository and Settings, and where the body holds the pane's least
    /// width beside the page's floor. Below that the pane is held back, its
    /// setting still on, and the page takes the body's whole width, as with
    /// the pane closed; it returns as soon as the window gives both room.
    /// Decided from the window's size before layout, as the page's width is
    /// ([`Self::repository_width`]), so the first frame at a new width lays
    /// out the form it settles in.
    pub(super) fn project_pane_drawn(&self, window: &Window) -> bool {
        self.settings.project_pane
            && self.page != AppPage::Projects
            && self.project_pane_fits(window)
    }

    /// Whether this window's body holds the project pane's least width
    /// beside the page's floor.
    pub(super) fn project_pane_fits(&self, window: &Window) -> bool {
        body_width(window.viewport_size().width, window) >= px(PROJECT_PANE_MIN + PROJECT_PAGE_MIN)
    }

    /// A narrow window shrinks the project pane toward its minimum before the
    /// repository's own panes lose room; the saved width is untouched.
    fn project_pane_width(&self, window: &Window) -> Pixels {
        px(self.settings.project_pane_width)
            .min((window.viewport_size().width - px(800.)).max(px(180.)))
    }

    /// Whether History has room for its navigation beside the table and the
    /// narrowest inspector.
    pub(super) fn navigation_fits(&self, window: &Window, cx: &App) -> bool {
        self.repository_width(window, cx) >= appearance::ui_size(SIDEBAR_FIT_WIDTH)
    }

    /// The shortcut, the menu bar and the command palette share this. Outside
    /// History it returns there. In History too narrow for navigation it keeps
    /// the saved choice, since flipping it would show nothing until the window
    /// widens, and the rail announces why; the rail button and the palette
    /// entry are disabled there.
    pub(super) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != WorkspaceMode::History {
            self.back_to_history(window, cx);
        } else if self.navigation_fits(window, cx) {
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
    /// keeps, which it rescales with the window, fitted before the frame that
    /// draws it (`render_repository`), and which a drag sets. That size can
    /// differ from the saved width, which only a drag changes: where
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

    /// The width the repository page is drawn at: the body's width as the
    /// layout draws it ([`body_width`]), less, where the project pane is
    /// drawn beside it ([`Self::project_pane_drawn`]), the pane as its group
    /// draws it there ([`Self::drawn_project_pane_width`]), which leaves the
    /// page at least its 520 px floor. Every choice the page's width
    /// makes reads this one width: the narrow rail, Toggle Sidebar and the
    /// palette's entry, the chrome's action bar and Compare's minimum, the
    /// columns' minimums and the inspector's range, and the width a drag of
    /// the inspector saves. Where the window holds the pane back, the page
    /// is the body: in a 461 px half tile the page was laid out at its floor
    /// beside the pane, 520 px from the pane's 180 px edge, and a paused
    /// merge's banner drew its actions at 559–690 px, past the window.
    fn repository_width(&self, window: &Window, cx: &App) -> Pixels {
        let viewport = window.viewport_size().width;
        let body = body_width(viewport, window);
        if self.project_pane_drawn(window) {
            // The page is laid out between the pane's edge and the body's,
            // both on device pixels: at scale factor 1.5, 840 px between a
            // 220.67 px pane and a 1,060.67 px body. The snap only drops the
            // rounding of the two divided lengths.
            let pane = self.drawn_project_pane_width(viewport, window, cx);
            // The floor is the drawn width: the group lays the page out at its 520 px minimum.
            window.pixel_snap(body - pane).max(px(PROJECT_PAGE_MIN))
        } else {
            body
        }
    }

    /// The project pane's width as its group draws it in a window `viewport`
    /// wide: the group's size for it, rescaled to the body's width
    /// ([`body_width`]) as the group is fitted to it before layout
    /// (`ResizableState::fit_container`), within the pane's 180–360 px range,
    /// no wider than the page's floor leaves it ([`project_pane_most`]),
    /// and at the device pixel its edge is laid out on; before the group
    /// first measures the pane, the width it starts at
    /// ([`Self::project_pane_width`]).
    fn drawn_project_pane_width(&self, viewport: Pixels, window: &Window, cx: &App) -> Pixels {
        let panels = self.project_panels.read(cx);
        let sizes = panels.sizes();
        // The group is fitted to the body's width as the layout draws it,
        // whole device pixels, which is not the window's content: at scale
        // factor 1.5, 1,060.67 px in a 1,061 px window.
        let body = body_width(viewport, window);
        let size = match sizes
            .first()
            .filter(|pane| **pane != gpui_kit::base::PANEL_MIN_SIZE)
        {
            None => self.project_pane_width(window),
            Some(&pane) => {
                // It rescales its panels for any other width, as exactly as
                // `fit_container` compares it: at scale factor 1.25 a group
                // fitted to a 1,002 px window's body (1,001.6 px) rescales
                // for a 1,001 px window's (1,000.8 px), though within a
                // device pixel of the window's 1,001 px content.
                let fitted = panels.container_size() == body;
                // As the group rescales it, the same arithmetic in the same
                // order.
                let total = px(sizes.iter().map(|size| f32::from(*size)).sum::<f32>());
                if fitted || total <= px(0.) {
                    pane
                } else {
                    body * (pane / total)
                }
            }
        };
        // The panel's basis, held to its range, is laid out at a device
        // pixel: a pane fitted to 212.2 px is drawn 212 px wide.
        window.pixel_snap(
            size.min(project_pane_most(body, window))
                .max(px(PROJECT_PANE_MIN)),
        )
    }

    /// `width` within the bounds a drag gives the inspector in this window
    /// at the interface size (`inspector_range`), for the repository page:
    /// the width a drag's end writes. Restores and narrower windows draw a
    /// saved width within the range without writing it.
    pub(super) fn bounded_inspector_width(&self, width: f32, window: &Window, cx: &App) -> f32 {
        let page = self.repository_width(window, cx);
        let narrow = page < appearance::ui_size(SIDEBAR_FIT_WIDTH);
        let columns = ColumnMinimums::new(page, appearance::ui_size(44.), narrow);
        let range = Self::inspector_range(page, &columns);
        f32::from(px(width).min(range.end).max(range.start))
    }

    /// The content column, History or the comparison beside the rail, and
    /// the inspector, in their panel group, as the frame that draws them
    /// builds them once the group fits their width
    /// ([`Self::render_repository`]).
    /// `working_layout` is the Working inspector's layout state, kept where
    /// the window renders ([`Self::working_inspector_layout`]). `height` is
    /// the group's, which the inspector takes whole.
    fn render_content_columns(
        &self,
        working_layout: Option<Entity<crate::workspace::WorkingInspectorLayout>>,
        height: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Tiling window managers ignore the window's minimum size. Below the
        // width that fits navigation, the history table and the narrowest
        // inspector, History shows the rail and the content column accepts
        // less, so the inspector stays whole; saved widths and the user's
        // navigation choice are untouched. The page's one width decides it,
        // as it decides Toggle Sidebar and the palette's entry.
        let width = self.repository_width(window, cx);
        let narrow = width < appearance::ui_size(SIDEBAR_FIT_WIDTH);
        let rail = appearance::ui_size(44.);
        let columns = ColumnMinimums::new(width, rail, narrow);
        let content_min = columns.content;
        // The inspector's message toolbar fits the commit's Actions beside
        // its copy controls at the width the inspector is drawn at, which can
        // be narrower than its saved width, and where the inspector is pinned
        // it reserves the rows they take there.
        let inspector_width = self.drawn_inspector_width(width, &columns, cx);
        // File History's panel lays them out in the room its overflow cue's
        // gutter leaves while it scrolls, and as icons where their labels'
        // rows alone would make it scroll (`GitTurtle::message_toolbar`).
        self.inspector_message
            .lay_out_toolbar(Some(self.message_toolbar(
                inspector_width,
                height,
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
                        .child(self.render_preview(content_min - rail, cx)),
                )
                .into_any_element()
        } else if self.sidebar && !narrow {
            // Navigation and History are laid out for the width the content
            // column has in this frame, their group fitted to it first, as
            // the content columns' is (`render_repository`).
            let owner = cx.entity().downgrade();
            let panels = self.history_panels.clone();
            crate::in_frame::in_frame(move |bounds, window, cx| {
                panels.update(cx, |panels, _| panels.fit_container(bounds.size.width));
                owner
                    .update(cx, |this, cx| this.render_history_columns(window, cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .named("history-columns")
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
                            // Only a drag writes the width, within the
                            // bounds it has at this moment.
                            this.settings.inspector_width =
                                this.bounded_inspector_width(f32::from(width), window, cx);
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
                    .child(match working_layout {
                        Some(layout) if self.mode == WorkspaceMode::Working => self
                            .render_working_inspector(
                                layout,
                                inspector_width,
                                columns.pinned,
                                window,
                                cx,
                            ),
                        _ => self.render_inspector(files_header, window, cx),
                    }),
            );
        workspace.into_any_element()
    }

    /// History's navigation and its table in their panel group, as the frame
    /// that draws them builds them once the group fits their width
    /// ([`Self::render_content_columns`]).
    fn render_history_columns(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        h_resizable("history-columns")
            .with_state(&self.history_panels)
            // The kit paints its divider over History's first pixel,
            // where the commit list draws its left edge and focus ring;
            // the sidebar's own right border is the divider.
            .with_handle_appearance(divider_on_own_border())
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
            .child(crate::in_frame::probed(
                resizable_panel()
                    .size(px(self.settings.navigation_width))
                    .size_range(px(180.)..px(360.))
                    .flex_none()
                    .child(self.render_sidebar(window, cx)),
                "history-navigation",
            ))
            .child(
                resizable_panel()
                    .size_range(px(320.)..px(10000.))
                    .child(self.render_history(window, cx)),
            )
            .into_any_element()
    }

    fn render_repository(
        &self,
        chrome: Chrome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The content column and the inspector are laid out for the width
        // the page gives them in the frame they are drawn
        // ([`crate::in_frame`]). Their panel group rescales its panels for a
        // new width only once it has been laid out at it, so the first frame
        // at a new width drew the inspector, and the column beside it, at
        // their widths for the old one, until the next input; fitted to the
        // width first, that frame lays them out where they settle.
        // The Working inspector's layout is taken here, as the window renders,
        // from the tab that keeps it, so the frame's build, which the project
        // pane changes, cannot start it again. Until the inspector is laid
        // out it expects the width the window's width gives it.
        let working_layout = (self.mode == WorkspaceMode::Working).then(|| {
            let page = self.repository_width(window, cx);
            let narrow = page < appearance::ui_size(SIDEBAR_FIT_WIDTH);
            let columns = ColumnMinimums::new(page, appearance::ui_size(44.), narrow);
            let expected = self.drawn_inspector_width(page, &columns, cx);
            self.working_inspector_layout(expected, window, cx)
        });
        let owner = cx.entity().downgrade();
        let panels = self.content_panels.clone();
        let columns = crate::in_frame::in_frame(move |bounds, window, cx| {
            panels.update(cx, |panels, _| panels.fit_container(bounds.size.width));
            owner
                .update(cx, |this, cx| {
                    this.render_content_columns(working_layout, bounds.size.height, window, cx)
                })
                .unwrap_or_else(|_| div().into_any_element())
        })
        .named("repository-columns");
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_git_actions(
                chrome.is_compact(),
                self.edge_form(chrome.is_compact(), window, cx),
                self.sync_fit(chrome.is_compact(), window, cx),
                cx,
            ))
            .child(self.render_operation_state(self.repository_width(window, cx), window, cx))
            .child(columns)
            .into_any_element()
    }

    /// What the status line says on this page before it gives way: the
    /// status bar draws it and its fitting measures it
    /// ([`Self::fit_status_bar`]). The project pane's held-back note, while
    /// it is fresh, takes the page's status's place, after a read's error
    /// and a write's fresh outcome (`GitTurtle::project_pane_note_line`).
    fn status_text(&self) -> &str {
        if let Some(note) = self.project_pane_note_line() {
            return note;
        }
        if let Some(reason) = self.github_refusal_line() {
            return reason;
        }
        match self.page {
            AppPage::Projects => "Open a project or start a new repository",
            AppPage::Settings => "Appearance and layout preferences",
            AppPage::Repository => self.status_line(),
        }
    }

    /// The status text's line that the status line draws before its parts
    /// give way, the one place that decides it ([`first_status_line`]).
    fn status_first_line(&self) -> StatusLine<'_> {
        let text = self.status_text();
        first_status_line(text, self.status_parts.ends_in(text))
    }

    /// The keyboard hints as the status bar draws them
    /// ([`Self::fit_status_bar`]), or none where they gave way.
    #[cfg(test)]
    pub(super) fn status_hints(&self, window: &mut Window, cx: &mut App) -> Option<String> {
        let plan = self.frame_plan(window, cx);
        (self.fit_status_bar(plan, window, cx).hints).map(|hints| hints.to_string())
    }

    /// The header plan this window's frame works out
    /// ([`Self::header_plan`]), none off the Repository page.
    #[cfg(test)]
    fn frame_plan(&self, window: &Window, cx: &App) -> Option<HeaderPlan> {
        (self.page == AppPage::Repository)
            .then(|| self.header_plan(self.chrome(window, cx), window))
    }

    /// The keyboard hints at the status bar's right, each key with what it
    /// does: the page's, or, under a modal dialog or sheet, those of the
    /// dialog control holding focus, which has keys of its own, or none. The
    /// page's keys do not reach the page beneath a modal, so its hints never
    /// show there. The status bar finds its measured hints by what they
    /// follow from ([`Self::hints_for`]) and builds them only for widths it
    /// has not kept, so only tests build them each time.
    #[cfg(test)]
    pub(super) fn key_hints(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<shortcuts::KeyHints> {
        self.hints_for(window, cx).map(HintsFor::hints)
    }

    /// What the keyboard hints follow from now ([`HintsFor`]), or none
    /// under a modal dialog or sheet whose focused control has no keys of
    /// its own.
    fn hints_for(&self, window: &mut Window, cx: &mut App) -> Option<HintsFor> {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return self.dialog_hints.focused(window).map(HintsFor::Dialog);
        }
        Some(match self.page {
            AppPage::Settings => HintsFor::Settings,
            AppPage::Projects => HintsFor::Projects,
            AppPage::Repository => self
                .partial_line_keys(window, cx)
                .map_or(HintsFor::Repository(self.mode), HintsFor::PartialLines),
        })
    }

    /// Whether the shown diff's text holds focus with its per-line
    /// checkboxes, and then whether they unstage
    /// (`diff_view::DiffView::line_keys`): Space selects lines there, but
    /// for while a partial write runs, when it selects nothing.
    fn partial_line_keys(&self, window: &Window, cx: &App) -> Option<bool> {
        let Some(ShownText::Editor(editor)) = self.shown_text() else {
            return None;
        };
        let view = self
            .patch_view
            .as_ref()
            .filter(|_| self.patch_editor.as_ref() == Some(editor))?
            .read(cx);
        view.line_keys(window, cx)
            .filter(|_| !view.partial_suspended())
    }

    /// How the status bar lays out in this window, in the one responsive
    /// order (DESIGN.md), decided before layout from the interface font so
    /// no frame shows a part of it cut. What keeps its room, first to last:
    /// the local refresh warning and its Refresh, whole; a write's fresh
    /// outcome ([`Self::fresh_outcome`]), whole in the status line, or else
    /// the status line's ellipsis, so the line is never cut, and the room of
    /// the Cancel a running write's status brings where it takes the line's
    /// place ([`OperationPlace::StatusBar`]), kept only in a Repository frame
    /// whose `plan` lets a write's status come there
    /// ([`HeaderPlan::bar_room`]); the keyboard hints, whole, else their keys
    /// alone ("↑↓ · Enter"), else fewer keys, Escape the last to go
    /// (`shortcuts::KeyHints::giving_way`), else none, with
    /// any spare hints after them, whole, only where the status line keeps
    /// its whole text beside them (Keyboard Shortcuts' page and end keys,
    /// `shortcuts::KeyHints::spare`); the status line's own text, in the
    /// room left, keeps its whole parts that fit, the last leaving first,
    /// so a measured timing reads whole or leaves whole, its first part
    /// ending in an ellipsis where even that does not fit
    /// ([`status_parts_end`]); and
    /// Details… keeps its label unless the labelled warning would leave the
    /// status line, beside any room kept for Cancel, less than it keeps with
    /// every hint gone, where it becomes its icon. At 18 pt in a 461 × 490
    /// half tile the whole hints cut "Up to date with origin · nothing new"
    /// to "Up to date with ori…", and with the watcher unable to start the
    /// warning pushed them past the window's edge, cut mid-glyph at 18 pt in
    /// windows up to 821 px wide. The bar spans the window's body as it is
    /// laid out ([`body_size`]).
    fn fit_status_bar(
        &self,
        plan: Option<HeaderPlan>,
        window: &mut Window,
        cx: &mut App,
    ) -> StatusFit {
        let bar_room = plan.is_some_and(|plan| plan.bar_room);
        let carried = self.bar_operation(plan).is_some();
        self.fit_status_bar_in(body_size(window).width, bar_room, carried, window, cx)
    }

    /// A running write's status where the header's rows have no room for it,
    /// which the status bar draws in its line's place
    /// ([`OperationPlace::StatusBar`]): the bar and its fitting read it alike.
    fn bar_operation(&self, plan: Option<HeaderPlan>) -> Option<&'static str> {
        self.operation_shown()
            .filter(|_| plan.is_some_and(|plan| plan.operation.place == OperationPlace::StatusBar))
    }

    /// [`Self::fit_status_bar`] for a bar `width` wide that keeps Cancel's
    /// room beside its line where `bar_room`, its line taken by a running
    /// write's status where `carried` ([`Self::bar_operation`]); the width
    /// walks fit it at widths of their choosing.
    fn fit_status_bar_in(
        &self,
        width: Pixels,
        bar_room: bool,
        carried: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> StatusFit {
        let hints = self.hints_for(window, cx);
        let warning = self.page == AppPage::Repository && self.automatic.watch_warning().is_some();
        let rem = window.rem_size();
        // The bar's `px_3` and `gap_2`, and its 5 px dot.
        let (inset, gap) = (window.pixel_snap(rem * 0.75), window.pixel_snap(rem * 0.5));
        let mut room = width - inset * 2. - px(5.) - gap;
        // Where a write's status can come to the bar, as one the header has
        // no room for does (`OperationPlace::StatusBar`), the line keeps
        // Cancel's room beside it, whether or not a write runs: its coming
        // moves nothing. Elsewhere the hints and Details… keep that room.
        let cancel = if bar_room {
            gap + window.pixel_snap(appearance::ui_size(WATCH_CONTROL))
        } else {
            px(0.)
        };
        room -= cancel;
        STATUS_WIDTHS.with_borrow_mut(|cache| {
            let key = (
                LineKey::current(window),
                window.scale_factor().to_bits(),
                hints,
            );
            // The hints themselves are built only for widths not kept.
            let widths = cache.get(key, |(_, _, hints)| {
                let hints =
                    hints.map_or_else(|| shortcuts::KeyHints::new(Vec::new()), HintsFor::hints);
                self.status_widths(&hints, window)
            });
            // What the line draws of the status text, a fresh outcome's
            // included, before its parts give way. The project pane's
            // held-back note is fresh feedback too, kept whole as it is. A
            // running write's status in the line's place draws neither, so
            // keeps no room for them.
            let first = self.status_first_line();
            let fresh = !carried
                && (self.project_pane_note_line().is_some()
                    || self.github_refusal_line().is_some()
                    || self.fresh_outcome().is_some());
            let line = if fresh {
                status_line_width(&STATUS_OUTCOME, first.text, window, || {
                    #[cfg(test)]
                    STATUS_OUTCOME_MEASURED.with(|measured| measured.set(measured.get() + 1));
                    let weight = window.text_style().font_weight;
                    laid_out_text_width(
                        shaped_width(first.text, appearance::ui_text(10.), weight, window),
                        window,
                    )
                }) + first.ellipsis_room(widths.ellipsis)
            } else {
                widths.ellipsis
            };
            let mut details_icon = false;
            if warning {
                details_icon = room - gap - widths.warning < line;
                room -= gap
                    + if details_icon {
                        widths.warning_compact
                    } else {
                        widths.warning
                    };
            }
            // Spare hints come whole, before the others' whole form, only
            // where the status line keeps the whole line it draws beside
            // them and no write's status takes its place: they give way
            // first. A write whose status the header draws leaves the line
            // its text, and the hints with it; they once hid through every
            // write.
            let spare = widths.spare.as_ref().filter(|(_, width)| {
                !carried
                    && whole_status_width(first.text, window)
                        + first.ellipsis_room(widths.ellipsis)
                        + gap
                        + *width
                        <= room
            });
            let hints = spare.or_else(|| {
                widths
                    .forms
                    .iter()
                    .find(|(_, width)| line + gap + *width <= room)
            });
            // The status line's own text, where neither a fresh outcome nor
            // a write's status takes its place, keeps the whole parts of the
            // line it draws that fit the box the warning and the hints leave
            // it, Cancel's room with it while no write comes there.
            let parts = (!carried && !fresh)
                .then(|| {
                    let hints = hints.map_or(px(0.), |(_, width)| {
                        // Laid out already (`status_widths`).
                        gap + *width
                    });
                    status_parts_end(first, room + cancel - hints, widths.ellipsis, window)
                })
                .flatten();
            StatusFit {
                hints: hints.map(|(form, _)| form.clone()),
                details_icon,
                parts,
            }
        })
    }

    /// The status line's text as the bar draws it for `fit`: the whole parts
    /// of the line it draws that fit ([`status_parts_end`]), or all of that
    /// line ([`first_status_line`]), ending in an ellipsis where a line
    /// break inside its part ends it, the spaces and a clause's punctuation
    /// before the ellipsis trimmed (DESIGN.md), and within the bound its
    /// accessible value keeps (`native_accessibility::bounded_accessible_text`),
    /// the ellipsis after it. Built only when the status or its fit changes
    /// (`GitTurtle::render_status_line`).
    fn status_line_drawn_text(&self, fit: &StatusFit) -> SharedString {
        let line = self.status_first_line();
        let shown = fit.parts.map_or(line.text, |end| &line.text[..end]);
        let shown = gitturtle_core::strip_bidi_controls(shown);
        let bounded = native_accessibility::bounded_accessible_text(shown.as_ref());
        // A line longer than the bound ends in the bound's ellipsis.
        if !line.ellipsis || bounded.len() < shown.len() {
            return bounded;
        }
        let kept = crate::text_cut::trim_end_clusters(shown.as_ref(), |c| {
            c.is_whitespace() || ".,;:".contains(c)
        });
        format!("{kept}…").into()
    }

    /// Sets the status to `parts` joined, a part of which comes from user
    /// text that can hold [`STATUS_SEPARATOR`] itself, as a path can: the
    /// line gives way only where they end ([`StatusParts`]).
    pub(super) fn set_status_parts(&mut self, parts: &[&str]) {
        let parts = StatusParts::join(parts);
        self.status.clone_from(&parts.text);
        self.status_parts = parts;
    }

    /// The status line, `line`, drawing the status text as `fit` keeps it
    /// ([`Self::status_line_drawn_text`]), never more than one line of it,
    /// its accessible value the whole text, both kept across frames for the
    /// status and its fit ([`KeptStatusText`]). Where the line drops a part
    /// or a line or ends in an ellipsis, a hover shows the whole text in the
    /// app's tooltip, after the shared tooltip delay; a line drawn whole has
    /// none, as a whole label has none. The tooltip
    /// shows the text's first lines, within the bound a dialog's error line
    /// keeps (`tags::error_tooltip`), since a read's error can carry Git's
    /// whole report. Whether the line is shortened is decided as each frame
    /// lays it out, its text, width and text size together
    /// ([`StatusLaidOut`]), and the tooltip, asked for as a hover's delay
    /// ends, shows what the latest frame decided, with that frame's text: a
    /// status that changes, or a resize, during the delay is answered for
    /// the line drawn last. Decided from the frame the hover began in, it
    /// showed the old text, or a tooltip for a line drawn whole since. A
    /// tooltip already shown keeps its text until the pointer leaves the
    /// line, as macOS keeps one, and the next hover shows the new text. The
    /// rule is the error lines': a person who reads the shortened line can
    /// still read all of it, and nothing drawn moves.
    fn render_status_line(
        &self,
        line: Div,
        fit: &StatusFit,
        window: &mut Window,
        cx: &mut App,
    ) -> native_accessibility::StaticText<Stateful<Div>> {
        let raw_text = self.status_text();
        let text = gitturtle_core::strip_bidi_controls(raw_text);
        let first = self.status_first_line();
        // The texts built for the status and its fit, kept across frames,
        // so a frame that draws the same status builds and copies neither.
        let kept = window
            .use_keyed_state(ElementId::from("status-line/text"), cx, |_, _| {
                std::rc::Rc::new(std::cell::RefCell::new(None::<KeptStatusText>))
            })
            .read(cx)
            .clone();
        let (whole, drawn) = {
            let mut kept = kept.borrow_mut();
            if !kept
                .as_ref()
                .is_some_and(|kept| kept.is(text.as_ref(), fit.parts))
            {
                #[cfg(test)]
                STATUS_TEXT_BUILT.with(|built| built.set(built.get() + 1));
                let whole = native_accessibility::bounded_accessible_text(text.as_ref());
                // The whole text is drawn where it is one line and no part
                // gives way, without a copy of its own.
                let drawn = if fit.parts.is_none() && first.text.len() == raw_text.len() {
                    whole.clone()
                } else {
                    self.status_line_drawn_text(fit)
                };
                *kept = Some(KeptStatusText {
                    whole,
                    parts: fit.parts,
                    drawn,
                });
            }
            let kept = kept.as_ref().expect("the status line's texts, kept above");
            (kept.whole.clone(), kept.drawn.clone())
        };
        #[cfg(test)]
        STATUS_DRAWN.with_borrow_mut(|last| *last = Some(drawn.clone()));
        // How the line was laid out last, which its prepaint records, kept
        // across frames so the hover a frame began reads the latest.
        let laid_out = window
            .use_keyed_state(ElementId::from("status-line/laid-out"), cx, |_, _| {
                std::rc::Rc::new(std::cell::RefCell::new(None::<StatusLaidOut>))
            })
            .read(cx)
            .clone();
        let recorded = laid_out.clone();
        // A part or a line of the text the line does not draw.
        let dropped = fit.parts.is_some() || !first.whole;
        let text = whole.clone();
        let measured = drawn.clone();
        native_accessibility::static_text(
            line.on_children_prepainted(move |bounds, window, _| {
                // The size the bar draws the line at and `whole_status_width`
                // measures it at, read without the window's text style, which
                // builds a default style, an allocation, at every read: a
                // steady frame reads nothing more. A miss measures inside the
                // line's own text style, so its font measures it.
                let size = crate::appearance::ui_text(10.);
                let mut laid = recorded.borrow_mut();
                let Some(width) = bounds.first().map(|line| line.size.width) else {
                    *laid = None;
                    return;
                };
                if !laid
                    .as_ref()
                    .is_some_and(|laid| laid.is(&text, width, size, dropped))
                {
                    *laid = Some(StatusLaidOut {
                        // A dropped part or line is never drawn, and a text
                        // wider than its line ends in an ellipsis.
                        cut: dropped || whole_status_width(&measured, window) > width,
                        text: text.clone(),
                        width,
                        size,
                        dropped,
                    });
                }
                #[cfg(test)]
                if let Some(laid) = laid.as_ref() {
                    STATUS_CUT.with_borrow_mut(|last| *last = Some((laid.text.clone(), laid.cut)));
                }
            })
            .id("status-line")
            .truncate()
            .tooltip(move |window, cx| {
                let shown = laid_out
                    .borrow()
                    .as_ref()
                    .filter(|laid| laid.cut)
                    .map(|laid| crate::tags::error_tooltip(&laid.text));
                match shown {
                    Some(shown) => {
                        #[cfg(test)]
                        STATUS_TOOLTIP.with_borrow_mut(|last| *last = Some(shown.clone()));
                        gpui_kit::component::tooltip::Tooltip::new(shown).build(window, cx)
                    }
                    // Nothing to add to a whole line: an empty view, drawn
                    // as nothing.
                    None => cx.new(|_| gpui::EmptyView).into(),
                }
            })
            .child(drawn),
            whole,
        )
    }

    /// The status line's text as the status bar draws it
    /// ([`Self::status_line_drawn_text`]).
    #[cfg(test)]
    pub(super) fn status_line_drawn(&self, window: &mut Window, cx: &mut App) -> String {
        let plan = self.frame_plan(window, cx);
        let fit = self.fit_status_bar(plan, window, cx);
        self.status_line_drawn_text(&fit).to_string()
    }

    /// What [`Self::fit_status_bar`] measures for `hints`.
    fn status_widths(&self, hints: &shortcuts::KeyHints, window: &Window) -> StatusWidths {
        #[cfg(test)]
        STATUS_MEASURED.with(|measured| measured.set(measured.get() + 1));
        // Each as layout lays it out, rounded once, as the status line's own
        // text is (`whole_status_width`): rounded to a whole pixel only, a
        // hint form at scale 1.5 measured up to a device pixel less than the
        // parts fit took it to be.
        let weight = window.text_style().font_weight;
        let text = |text: &str| {
            laid_out_text_width(
                shaped_width(text, appearance::ui_text(10.), weight, window),
                window,
            )
        };
        let gap = window.pixel_snap(window.rem_size() * 0.5);
        let lead = text(WATCH_WARNING)
            + gap
            + self.labelled_width(text(WATCH_REFRESH), false, window)
            + gap;
        let measured = |form: String| {
            let width = text(&form);
            (SharedString::from(form), width)
        };
        StatusWidths {
            spare: (!hints.spare.is_empty()).then(|| measured(hints.whole())),
            forms: hints.giving_way().into_iter().map(measured).collect(),
            warning: lead + self.labelled_width(text(WATCH_DETAILS), false, window),
            warning_compact: lead + window.pixel_snap(appearance::ui_size(WATCH_CONTROL)),
            ellipsis: text("…"),
        }
    }
}

/// A panel group's divider drawn on the leading panel's own right border,
/// in the border's colour at rest and the ring's while dragged, where the
/// kit paints it over the next panel's first pixel, which holds that
/// panel's left edge and focus ring; the hit area and drag stay the kit's.
fn divider_on_own_border() -> gpui_kit::base::ResizeHandleRenderer {
    std::rc::Rc::new(|handle: &gpui_kit::base::ResizeHandleContext, _, cx| {
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
    })
}

impl Render for GitTurtle {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.draws.push(appearance::palette(cx));
        // Everything the window holds, its dialogs included, is built below.
        #[cfg(test)]
        native_accessibility::written::draw_begins();
        // One read of the window's line key for everything measured below.
        let _line = LineKey::frame(window);
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
        // Before anything reads it: whether a busy spell looks busy yet,
        // and whether a working status not read looks it.
        self.track_busy_look(cx);
        self.track_unread_look(cx);
        // A refused Toggle Sidebar is announced only from the narrow History
        // rail. Forget it once that is not drawn, so the node cannot reappear,
        // and be announced again, when the window narrows or History returns.
        if self.navigation_refusals > 0
            && (self.page != AppPage::Repository
                || self.mode != WorkspaceMode::History
                || self.navigation_fits(window, cx))
        {
            self.navigation_refusals = 0;
        }
        if self.dialog_layer_subscription.is_none()
            && let Some(Some(root)) = window.root::<Root>()
        {
            // Root owns WindowState's dialog and sheet layers as a sibling
            // plugin. Observe it so GitTurtle updates modal focus repair and
            // status hints when a modal opens or closes without calling
            // `window.refresh()` on every plugin animation frame (which would
            // invalidate cached subtrees like Settings theme cards).
            self.dialog_layer_subscription =
                Some(cx.observe_in(&root, window, |this, _, window, cx| {
                    let modal_open = window.has_active_dialog(cx) || window.has_active_sheet(cx);
                    if modal_open {
                        this.modal_focus_generation = this.modal_focus_generation.wrapping_add(1);
                    }
                    if this.modal_was_open != modal_open {
                        cx.notify();
                    }
                }));
        }
        self.update_modal_focus(window, cx);
        self.return_focus_after_delete(window, cx);
        self.return_tab_focus(window, cx);
        self.return_refusal_focus(window, cx);
        // After the focus repairs above, before any list is built.
        self.focus_ring.begin_frame(window, cx);
        let colors = palette(cx);
        self.update_native_menus(window, cx);
        self.sync_window_chrome_metadata(window);
        let chrome = self.chrome(window, cx);
        self.hide_tooltip_on_page_change(window, cx);
        let page = match self.page {
            AppPage::Repository => self.render_repository(chrome, window, cx),
            AppPage::Projects => self.hub.clone().into_any_element(),
            AppPage::Settings => self.settings_page_element(window, cx),
        };
        // The pane spans the whole body so a project stays one click away on
        // Repository and Settings. The hub already lists projects full width.
        // A window that cannot hold the pane beside the page's floor holds
        // it back, decided from its size before layout, as the page's width
        // is, so the first frame at a new width lays out the form it settles
        // in; focus in the pane as it goes moves to the page. The page keeps
        // its controls' element IDs in both forms (`page_alone`).
        let pane_drawn = self.project_pane_drawn(window);
        let page_start = self.follow_project_pane(pane_drawn, window, cx);
        let body = if pane_drawn {
            let pane_width = self.project_pane_width(window);
            let pane = self.render_project_pane(cx).into_any_element();
            let owner = cx.entity().downgrade();
            let panels = self.project_panels.clone();
            // The group rescales the pane and the page for a new width only
            // once it has been laid out at it, so the first frame at a new
            // width drew them at their old widths, until the next input.
            // Laid out for the body's width in the frame it is drawn, the
            // group fitted to it first, as the content columns' is
            // (`render_repository`), that frame lays them out where they
            // settle; what was chosen from the page's width before layout
            // read the pane's width as this fit leaves it
            // (`drawn_project_pane_width`).
            let frame = div().size_full().flex().flex_col();
            #[cfg(test)]
            let frame = frame.pl(px(BODY_INSET.with(std::cell::Cell::get)));
            frame
                .child(
                    crate::in_frame::in_frame(move |bounds, window, cx| {
                        panels.update(cx, |panels, _| panels.fit_container(bounds.size.width));
                        // No wider than the page's floor leaves it, as
                        // `drawn_project_pane_width` reads it.
                        let most =
                            project_pane_most(bounds.size.width, window).max(px(PROJECT_PANE_MIN));
                        // The page's panel comes second, at `PAGE_PANEL`,
                        // the place `page_alone` names it by.
                        h_resizable(WORKSPACE_COLUMNS)
                            .with_state(&panels)
                            // The kit paints its divider over the page's
                            // first pixel, where the navigator draws its left
                            // edge and focus ring, beside the pane's own right
                            // border, which so read doubled.
                            .with_handle_appearance(divider_on_own_border())
                            .on_resize(move |panels, window, cx| {
                                if let Some(width) = panels.read(cx).sizes().first().copied() {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.settings.project_pane_width = f32::from(width);
                                        this.save_preferences(window, cx);
                                    });
                                }
                            })
                            .child(crate::in_frame::probed(
                                resizable_panel()
                                    .size(pane_width)
                                    .size_range(px(PROJECT_PANE_MIN)..most)
                                    .flex_none()
                                    .child(pane),
                                "project-pane-panel",
                            ))
                            .child(record_page_path(crate::in_frame::probed(
                                resizable_panel()
                                    .size_range(px(PROJECT_PAGE_MIN)..px(10000.))
                                    .child(page),
                                "page-panel",
                            )))
                            .into_any_element()
                    })
                    .named(WORKSPACE_COLUMNS),
                )
                .into_any_element()
        } else {
            page_alone(page)
        };
        let watch_warning =
            self.page == AppPage::Repository && self.automatic.watch_warning().is_some();
        // The header's arrangement, worked out once for the frame: the
        // header, the status bar's room and a running write's status read it.
        let plan = (self.page == AppPage::Repository).then(|| self.header_plan(chrome, window));
        let status = self.fit_status_bar(plan, window, cx);
        // A running write's status, where the header's rows have no room for
        // it ([`OperationPlace::StatusBar`]).
        let bar_operation = self.bar_operation(plan);
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
                root.child(self.render_header(chrome, plan, window, cx))
            })
            // The busy look, which waits for the write to run a moment
            // (`workspace::BusyLook`), as the header's progress label does.
            .when(
                self.page == AppPage::Projects && self.operation_shown().is_some(),
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
                            .child(self.render_operation_cancel(false, window, cx)),
                    )
                },
            )
            .when(self.page == AppPage::Repository, |el| {
                el.children(self.operation_error.as_ref().map(|error| {
                    // Unbounded, as the notice's row is: the line bounds
                    // itself. Clipped at 100 pixels, the row hid the top and
                    // bottom of a refusal's wrapped line, and the lines its
                    // scroll reaches with them.
                    div()
                        .debug_selector(|| "operation-error-row".into())
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_3()
                        .bg(rgb(palette(cx).removed_background))
                        .text_color(rgb(palette(cx).removed))
                        .child(self.operation_error_line(error, window, cx))
                        // Whole beside the line, which gives way to them.
                        .child(
                            button("operation-error-details", "Details…", "", false)
                                .debug_selector(|| "operation-error-details".into())
                                .flex_shrink_0()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_operation_error(window, cx)
                                })),
                        )
                        .child(
                            button("dismiss-operation-error", "Dismiss", "", false)
                                .debug_selector(|| "dismiss-operation-error".into())
                                .flex_shrink_0()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.operation_error = None;
                                    cx.notify();
                                })),
                        )
                }))
            })
            .when(
                self.page == AppPage::Repository && self.operation_error.is_none(),
                |el| {
                    el.children(self.operation_notice.as_ref().map(|notice| {
                        let notice: SharedString = gitturtle_core::strip_bidi_controls(notice)
                            .into_owned()
                            .into();
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
                            .child(native_accessibility::recorded(
                                div()
                                    .id("operation-notice-summary")
                                    .debug_selector(|| "operation-notice-summary".into())
                                    .role(Role::Status)
                                    .polite()
                                    .aria_label(notice.clone())
                                    .flex_1()
                                    .min_w_0()
                                    .max_h(crate::appearance::ui_size(100.))
                                    .overflow_y_scroll()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .child(
                                        div()
                                            .debug_selector(|| "operation-notice-text".into())
                                            .child(
                                                // Test-only: the selector names the
                                                // text the banner draws, so a test
                                                // reads what it shows rather than
                                                // the notice it is built from.
                                                div()
                                                    .debug_selector({
                                                        let drawn = notice.clone();
                                                        move || {
                                                            format!("operation-notice-drawn: {drawn}")
                                                        }
                                                    })
                                                    .child(notice.clone()),
                                            ),
                                    ),
                            ))
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    // Where the page begins in the Tab order, in the frame
                    // that gives it focus left in the pane
                    // (`follow_project_pane`): out of flow, it draws nothing.
                    .children(page_start.map(|start| div().absolute().size_0().track_focus(&start)))
                    .child(body),
            )
            .child(
                div()
                    .debug_selector(|| "status-bar".into())
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
                    // The status line keeps the whole parts that fit the room
                    // the warning and the hints leave it, its first ending in
                    // an ellipsis where even it does not; those two stay whole. A
                    // running write's status the header has no room for takes
                    // its place, Cancel in the room the bar keeps beside it.
                    .child({
                        let line = div().debug_selector(|| "status-line".into()).flex_1().min_w_0();
                        match bar_operation {
                            Some(label) => line
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(self.render_operation_progress(label, true, true, cx))
                                .children(self.operation_cancel(true, cx).map(|cancel| {
                                    let side = crate::appearance::ui_size(WATCH_CONTROL);
                                    cancel
                                        .flex_shrink_0()
                                        .w(side)
                                        .min_w(side)
                                        .px(crate::appearance::ui_size(2.))
                                        .h(watch_control_height(cx))
                                }))
                                .into_any_element(),
                            None => self.render_status_line(line, &status, window, cx).into_any_element(),
                        }
                    })
                    // The held-back note changes nothing else on screen, so
                    // it is announced politely, a fresh node for each note,
                    // as the narrow rail's refusal is, and also where a
                    // read's error, a write's fresh outcome or a running
                    // write's status holds the line: it answers the person's
                    // own action (`project_pane_note`).
                    .children(self.project_pane_note().map(|(turn, note)| {
                        native_accessibility::recorded(
                            div()
                                .id(("project-pane-held-back", turn))
                                .debug_selector(|| "project-pane-held-back".into())
                                .role(Role::Status)
                                .polite()
                                .aria_label(note)
                                .aria_value(note)
                                .absolute()
                                .size(px(1.))
                                .overflow_hidden(),
                        )
                    }))
                    .children(self.github_refusal_announcement())
                    .when(watch_warning, |footer| {
                        footer.child(div().flex().flex_shrink_0().items_center().gap_2()
                            .debug_selector(|| "local-refresh-warning".into())
                            .child(div().id("local-refresh-status").role(Role::Status)
                                .whitespace_nowrap()
                                .aria_label("Some local changes need Refresh")
                                .child(WATCH_WARNING))
                            .child(crate::workspace::refusing(button("retry-local-refresh", WATCH_REFRESH, "", false).h(watch_control_height(cx)).text_size(crate::appearance::ui_text(10.)), || "retry-local-refresh".into(), self.operation_busy.is_some(), self.operation_shown().is_some())
                                .debug_selector(|| "retry-local-refresh".into())
                                // Refused at once while a write runs, before
                                // it looks so (`workspace::BusyLook`).
                                .on_click(cx.listener(|this, _, window, cx| if this.operation_busy.is_none() { this.retry_automatic_refresh(window, cx) })))
                            .child(if status.details_icon {
                                // Its whole label names it and is its tooltip.
                                let side = crate::appearance::ui_size(WATCH_CONTROL);
                                compact_control_name(button("local-refresh-details", "", "more", false), "local-refresh-details", WATCH_DETAILS)
                                    .tooltip(WATCH_DETAILS)
                                    .w(side).min_w(side).px(crate::appearance::ui_size(2.))
                            } else {
                                button("local-refresh-details", WATCH_DETAILS, "", false)
                            }
                                .h(watch_control_height(cx)).text_size(crate::appearance::ui_text(10.))
                                .debug_selector(|| "local-refresh-details".into())
                                .on_click(cx.listener(|this, _, window, cx| this.show_automatic_refresh_details(window, cx)))))
                    })
                    .children(status.hints.map(|hints| {
                        div()
                            .debug_selector(|| "status-hints".into())
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(hints)
                    })),
            );
        div()
            .id("gitturtle")
            .relative()
            .track_focus(&self.app_focus)
            // Return's and Space's presses, noted for the mark's way back,
            // which takes none that began elsewhere (`way_back_key_down`).
            .capture_key_down(cx.listener(Self::note_key_press_down))
            .capture_key_up(cx.listener(Self::note_key_press_up))
            // Compare names itself, so the Find shortcut finds in the
            // comparison from anywhere in the window (`shortcuts::COMPARE`).
            .key_context(if self.comparing() {
                "GitTurtle GitTurtleCompare"
            } else if self.page == AppPage::Repository && self.mode == WorkspaceMode::Working {
                crate::workspace::WORKING_ROOT
            } else {
                "GitTurtle"
            })
            .on_action(
                cx.listener(|this, _: &crate::workspace::NextWorkingStop, window, cx| {
                    this.tab_working_row_stops(true, window, cx)
                }),
            )
            .on_action(cx.listener(
                |this, _: &crate::workspace::PreviousWorkingStop, window, cx| {
                    this.tab_working_row_stops(false, window, cx)
                },
            ))
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
                } else if this.can_extend_history_range(window, cx) {
                    this.extend_history_range(true, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ExtendPreviousWorking, window, cx| {
                if this.mode == WorkspaceMode::Working {
                    this.extend_working_selection(false, window, cx);
                } else if this.can_extend_history_range(window, cx) {
                    this.extend_history_range(false, window, cx);
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
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
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
            .on_action(cx.listener(|this, _: &JumpToHead, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.jump_to_head(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &JumpToRevision, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_jump_to_revision(window, cx);
                }
            }))
            // The menu bar's View ▸ Browse Stashes…, checked again as it
            // arrives, as the Linux Main Menu's row is: GPUI validates a
            // macOS menu item by whether the window handles its action
            // (`App::is_action_available`), which can offer an item built
            // refused.
            .on_action(cx.listener(|this, _: &BrowseStashes, window, cx| {
                native_accessibility::dismiss_menus_for_overlay(window, cx);
                let path = this.path.clone();
                this.run_menu_command(command_palette::CommandId::Stashes, &path, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
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
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
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
            .on_action(cx.listener(|_, _: &ToggleFullScreen, window, _| window.toggle_fullscreen()))
            .on_action(cx.listener(|this, _: &CloseWindow, window, _| {
                if this.operation_busy.is_none() {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
            .on_action(cx.listener(|this, _: &OpenRecentRepository0, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(0, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository1, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(1, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository2, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(2, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository3, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(3, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository4, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(4, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository5, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(5, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository6, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(6, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository7, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(7, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository8, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(8, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenRecentRepository9, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.open_recent_repository_index(9, window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &ClearRecentRepositories, window, cx| {
                    if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                        this.clear_recent_repositories(window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| {
                    if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                        this.navigate_back(window, cx);
                    }
                }),
            )
            .drag_over::<ExternalPaths>(|style, _, _, _| style)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.open_external_paths(paths.paths(), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowProjects, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.show_projects(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowAbout, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.about(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.show_settings(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ShowChanges, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.show_working(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CommitStaged, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    this.commit_staged_shortcut(window, cx);
                }
            }))
            .on_action(cx.listener(|this, action: &OpenRepository, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.choose_repository(action, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                if !window.has_active_dialog(cx) && !window.has_active_sheet(cx) {
                    native_accessibility::dismiss_menus_for_overlay(window, cx);
                    this.choose_repository(&OpenRepository, window, cx);
                }
            }))
            .on_action(cx.listener(|this, action: &Refresh, window, cx| {
                if this.refresh_open_stashes(window, cx) {
                    return;
                }
                if !window.has_active_dialog(cx)
                    && !window.has_active_sheet(cx)
                    && this.page == AppPage::Repository
                {
                    this.refresh(action, window, cx);
                }
            }))
            .on_action(cx.listener(Self::search))
            .on_action(cx.listener(Self::find_in_comparison))
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
                    // A control inside Working Changes' list takes Enter
                    // itself, as the key goes on to it.
                    if this.working_control_focused(window, cx) {
                        cx.propagate();
                    }
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
            // Copies from any page or dialog are announced from here, and
            // what a scrolling body cannot announce from where it lies.
            .children(copy_feedback::announcement(window, cx))
            .children(copy_feedback::visual_toast(window, cx))
            .children(copy_feedback::alert_announcement(window, cx))
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
#[path = "views_tests.rs"]
pub(crate) mod tests;
