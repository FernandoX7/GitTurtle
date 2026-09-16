//! The project list pane. It presents the saved project library on the far
//! left of the window, opens a project on click, and edits groups through the
//! serialized preference writer. It never touches a repository.

use crate::{
    AppPage, GitTurtle, button,
    preferences::Preferences,
    project_library::{GroupEntry, LibraryRow, MAX_DEPTH, ProjectLibrary},
};
use gpui_kit::component::{
    Disableable, Icon, Selectable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::path::PathBuf;

/// What the group dialog will do when the user confirms it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GroupIntent {
    /// Create a group at the top level, or inside the given group.
    Create(Option<u32>),
    Rename(u32),
}

#[derive(Default)]
pub struct State {
    pub scroll: UniformListScrollHandle,
    pub group_form: Option<Entity<GroupForm>>,
}

impl GitTurtle {
    /// A Git operation owns the repository, so the pane offers no destination
    /// while one runs. Naming a group is modal and needs no extra guard.
    fn pane_busy(&self) -> bool {
        self.operation_busy.is_some()
    }

    /// Edit a copy of the saved list, then let the preference writer merge and
    /// validate it. A failed write leaves the presented list unchanged.
    fn edit_project_library(
        &mut self,
        edit: impl FnOnce(&mut ProjectLibrary) -> Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut library = self.project_library.clone();
        if let Err(message) = edit(&mut library) {
            self.operation_error = Some(message);
            cx.notify();
            return;
        }
        if library == self.project_library {
            return;
        }
        self.project_library = library.clone();
        cx.notify();
        let response = self
            .preferences_writer
            .submit(move || Preferences::save_project_library(&library));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "The settings writer stopped before reporting a result"
                ))
            });
            let _ = this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(preferences) => this.project_library = preferences.project_library,
                    Err(error) => {
                        this.operation_error =
                            Some(format!("Could not save the project list: {error:#}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn toggle_project_group(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let collapsed = self
            .project_library
            .group(id)
            .is_some_and(|group| group.collapsed);
        self.edit_project_library(
            move |library| {
                library.set_collapsed(id, !collapsed);
                Ok(())
            },
            window,
            cx,
        );
    }

    fn open_project_from_pane(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        // Selecting the open project from Settings returns to it; selecting it
        // again on Repository must not restart the read.
        if self.path.as_ref() == Some(&path) {
            if self.page != AppPage::Repository {
                self.return_from_page(window, cx);
            }
            return;
        }
        self.limit = 500;
        self.open(path, None, window, cx);
    }

    /// Ask for a group name. The dialog owns validation and reports its own
    /// failure, so the pane stays usable while the writer runs.
    pub(super) fn open_group_form(
        &mut self,
        intent: GroupIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let current = match intent {
            GroupIntent::Rename(id) => self
                .project_library
                .group(id)
                .map(|group| group.name.clone()),
            GroupIntent::Create(_) => None,
        };
        let owner = cx.entity().downgrade();
        let form = cx.new(|cx| GroupForm::new(owner, intent, current, window, cx));
        let focus_form = form.downgrade();
        self.project_pane.group_form = Some(form.clone());
        let title = match intent {
            GroupIntent::Rename(_) => "Rename group",
            GroupIntent::Create(_) => "New group",
        };
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let pending = form.read(cx).pending;
            dialog
                .title(title)
                .width(px(460.))
                .child(form.clone())
                .keyboard(!pending)
                .footer(
                    DialogFooter::new()
                        .child(
                            button("cancel-project-group", "Cancel", "", false)
                                .disabled(pending)
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            button(
                                "save-project-group",
                                if pending { "Saving…" } else { "Save group" },
                                "",
                                false,
                            )
                            .primary()
                            .disabled(pending)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                            }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |form, _| {
                        if form.pending {
                            return false;
                        }
                        form.visible = false;
                        true
                    })
                })
        });
        window.refresh();
        window.on_next_frame(move |window, cx| {
            let _ = focus_form.update(cx, |form, cx| {
                if form.visible {
                    form.input.update(cx, |input, cx| {
                        input.focus(window, cx);
                        input.select_all(window, cx);
                    });
                }
            });
        });
    }

    fn submit_group_name(
        &mut self,
        intent: GroupIntent,
        name: String,
        form: WeakEntity<GroupForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut library = self.project_library.clone();
        let outcome = match intent {
            GroupIntent::Create(parent) => library.create_group(parent, &name).map(|_| ()),
            GroupIntent::Rename(id) => library.rename_group(id, &name),
        };
        if let Err(message) = outcome {
            let _ = form.update(cx, |form, cx| {
                form.pending = false;
                form.error = Some(message);
                cx.notify();
            });
            return;
        }
        self.project_library = library.clone();
        let response = self
            .preferences_writer
            .submit(move || Preferences::save_project_library(&library));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "The settings writer stopped before reporting a result"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_group_form(form, result, window, cx);
            });
        })
        .detach();
    }

    fn finish_group_form(
        &mut self,
        form: WeakEntity<GroupForm>,
        result: anyhow::Result<Preferences>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visible = self
            .project_pane
            .group_form
            .as_ref()
            .is_some_and(|current| {
                current.entity_id() == form.entity_id() && current.read(cx).visible
            });
        match result {
            Ok(preferences) => {
                self.project_library = preferences.project_library;
                let _ = form.update(cx, |form, cx| {
                    form.pending = false;
                    form.visible = false;
                    cx.notify();
                });
                if visible {
                    window.close_dialog(cx);
                }
                self.project_pane.group_form = None;
            }
            Err(error) => {
                let _ = form.update(cx, |form, cx| {
                    form.pending = false;
                    form.error = Some(format!("Could not save the project list: {error:#}"));
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    pub(super) fn render_project_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = crate::appearance::palette(cx);
        let rows = self.project_library.rows();
        let busy = self.pane_busy();
        div()
            .id("project-pane")
            .debug_selector(|| "project-pane".into())
            .role(Role::Tree)
            .aria_label("Projects. Select a project to open it; groups collapse and expand.")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors.panel))
            .border_r_1()
            .border_color(rgb(colors.border))
            .child(
                div()
                    .h(crate::appearance::ui_size(34.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(colors.border))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child("Projects"),
                    )
                    .child(
                        Button::new("project-pane-new-group")
                            .ghost()
                            .flex_shrink_0()
                            .size(crate::appearance::ui_size(24.))
                            .rounded(px(6.))
                            .icon(Icon::default().path("icons/plus.svg").size(px(13.)))
                            .accessibility_label("New project group")
                            .tooltip("Create a group in the project list")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_group_form(GroupIntent::Create(None), window, cx)
                            })),
                    ),
            )
            .child(if rows.is_empty() {
                div()
                    .flex_1()
                    .min_h_0()
                    .p_4()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .child("Projects you open appear here. Create a group to organize them.")
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(
                        uniform_list(
                            "project-pane-rows",
                            rows.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                let rows = this.project_library.rows();
                                range
                                    .filter_map(|index| {
                                        rows.get(index)
                                            .map(|row| this.render_project_row(index, row, cx))
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.project_pane.scroll),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }

    /// Each row is a button so the keyboard reaches it, with the row menu as a
    /// separate control beside it.
    fn render_project_row(
        &self,
        index: usize,
        row: &LibraryRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = crate::appearance::palette(cx);
        let busy = self.pane_busy();
        let depth = row_depth(row);
        let indent = px(depth as f32 * 12.);
        let (control, menu) = match row {
            LibraryRow::Group {
                id,
                name,
                collapsed,
                projects,
                ..
            } => {
                let id = *id;
                let collapsed = *collapsed;
                let control = Button::new(("project-group", index))
                    .ghost()
                    .h(crate::appearance::ui_size(26.))
                    .w_full()
                    .pl(px(8.) + indent)
                    .pr(px(4.))
                    .rounded(px(6.))
                    .disabled(busy)
                    .accessibility_label(format!(
                        "{} group {name}, {projects} projects",
                        if collapsed { "Expand" } else { "Collapse" }
                    ))
                    .tooltip(if collapsed {
                        "Expand this group"
                    } else {
                        "Collapse this group"
                    })
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(div().w(px(12.)).flex_shrink_0().child(if collapsed {
                                "›"
                            } else {
                                "⌄"
                            }))
                            .child(crate::icon("folder", 13., colors.muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_left()
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(crate::appearance::ui_text(10.))
                                    .child(projects.to_string()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_project_group(id, window, cx)
                    }));
                (
                    control.into_any_element(),
                    self.render_group_menu(index, id, busy, cx),
                )
            }
            LibraryRow::Project { path, parent, .. } => {
                let path = path.clone();
                let parent = *parent;
                let name = self.project_name(&path);
                let location = path.display().to_string();
                let active = self.path.as_ref() == Some(&path);
                let open = path.clone();
                let control = Button::new(("project-row", index))
                    .ghost()
                    .h(crate::appearance::ui_size(26.))
                    .w_full()
                    .pl(px(8.) + indent)
                    .pr(px(4.))
                    .rounded(px(6.))
                    .selected(active)
                    // A ghost button keeps no surface when it is selected, so
                    // the open project needs its own resting background.
                    .when(active, |control| control.bg(rgb(colors.selected)))
                    .disabled(busy)
                    .accessibility_label(format!("Open {name}, {location}"))
                    .tooltip(location)
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(crate::appearance::ui_text(12.))
                            .when(active, |row| row.text_color(rgb(colors.accent)))
                            // A project carries the turtle mark; only a group
                            // keeps the folder, so the two never look alike.
                            .child(crate::icon(
                                "turtle",
                                14.,
                                if active { colors.accent } else { colors.muted },
                            ))
                            .child(div().flex_1().min_w_0().truncate().text_left().child(name)),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_project_from_pane(open.clone(), window, cx)
                    }));
                (
                    control.into_any_element(),
                    self.render_project_menu(index, path, parent, busy, cx),
                )
            }
        };
        div()
            .id(("project-pane-row", index))
            .debug_selector(move || format!("project-pane-row-{index}"))
            .role(Role::TreeItem)
            .aria_level(depth + 1)
            .w_full()
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .child(div().flex_1().min_w_0().child(control))
            .child(menu)
            .into_any_element()
    }

    fn render_group_menu(
        &self,
        index: usize,
        id: u32,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let groups = self.project_library.groups();
        Button::new(("project-group-menu", index))
            .ghost()
            .flex_shrink_0()
            .size(crate::appearance::ui_size(22.))
            .rounded(px(6.))
            .icon(Icon::default().path("icons/sliders.svg").size(px(12.)))
            .accessibility_label("Group actions")
            .tooltip("Rename, nest or remove this group")
            .disabled(busy)
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu.label("Group");
                let rename = owner.clone();
                menu = menu.item(PopupMenuItem::new("Rename group…").on_click(
                    move |_, window, cx| {
                        let _ = rename.update(cx, |this, cx| {
                            this.open_group_form(GroupIntent::Rename(id), window, cx)
                        });
                    },
                ));
                let nest = owner.clone();
                menu = menu.item(PopupMenuItem::new("New group inside…").on_click(
                    move |_, window, cx| {
                        let _ = nest.update(cx, |this, cx| {
                            this.open_group_form(GroupIntent::Create(Some(id)), window, cx)
                        });
                    },
                ));
                menu = menu.separator().label("Move this group to");
                let top = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new("Top level").on_click(move |_, window, cx| {
                        let _ = top.update(cx, |this, cx| {
                            this.edit_project_library(
                                |library| library.move_group(id, None),
                                window,
                                cx,
                            )
                        });
                    }),
                );
                for entry in groups.iter().filter(|entry| entry.id != id) {
                    let target = entry.id;
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::new(group_label(entry)).on_click(
                        move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.edit_project_library(
                                    |library| library.move_group(id, Some(target)),
                                    window,
                                    cx,
                                )
                            });
                        },
                    ));
                }
                let remove = owner.clone();
                menu.separator()
                    .item(
                        PopupMenuItem::new("Remove group").on_click(move |_, window, cx| {
                            let _ = remove.update(cx, |this, cx| {
                                this.edit_project_library(
                                    |library| {
                                        library.remove_group(id);
                                        Ok(())
                                    },
                                    window,
                                    cx,
                                )
                            });
                        }),
                    )
            })
            .into_any_element()
    }

    fn render_project_menu(
        &self,
        index: usize,
        path: PathBuf,
        parent: Option<u32>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let groups = self.project_library.groups();
        Button::new(("project-row-menu", index))
            .ghost()
            .flex_shrink_0()
            .size(crate::appearance::ui_size(22.))
            .rounded(px(6.))
            .icon(Icon::default().path("icons/sliders.svg").size(px(12.)))
            .accessibility_label("Project actions")
            .tooltip("Rename this project, or move it to a group")
            .disabled(busy)
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu.label("Project");
                let rename = owner.clone();
                let rename_path = path.clone();
                menu = menu.item(PopupMenuItem::new("Rename project…").on_click(
                    move |_, window, cx| {
                        let target = rename_path.clone();
                        let _ = rename
                            .update(cx, |this, cx| this.open_rename_project(target, window, cx));
                    },
                ));
                menu = menu.separator().label("Move to group");
                let top = owner.clone();
                let top_path = path.clone();
                menu = menu.item(
                    PopupMenuItem::new("Top level")
                        .checked(parent.is_none())
                        .on_click(move |_, window, cx| {
                            let target = top_path.clone();
                            let _ = top.update(cx, |this, cx| {
                                this.edit_project_library(
                                    |library| library.move_project(&target, None),
                                    window,
                                    cx,
                                )
                            });
                        }),
                );
                for entry in &groups {
                    let group = entry.id;
                    let owner = owner.clone();
                    let move_path = path.clone();
                    menu = menu.item(
                        PopupMenuItem::new(group_label(entry))
                            .checked(parent == Some(group))
                            .on_click(move |_, window, cx| {
                                let target = move_path.clone();
                                let _ = owner.update(cx, |this, cx| {
                                    this.edit_project_library(
                                        |library| library.move_project(&target, Some(group)),
                                        window,
                                        cx,
                                    )
                                });
                            }),
                    );
                }
                let forget = owner.clone();
                let forget_path = path.clone();
                menu.separator()
                    .item(PopupMenuItem::new("Remove from the project list").on_click(
                        move |_, window, cx| {
                            let target = forget_path.clone();
                            let _ = forget.update(cx, |this, cx| {
                                this.edit_project_library(
                                    |library| {
                                        library.forget_project(&target);
                                        Ok(())
                                    },
                                    window,
                                    cx,
                                )
                            });
                        },
                    ))
            })
            .into_any_element()
    }
}

fn row_depth(row: &LibraryRow) -> usize {
    match row {
        LibraryRow::Group { depth, .. } | LibraryRow::Project { depth, .. } => *depth,
    }
}

/// Indent a nested destination so a flat menu still shows the hierarchy.
fn group_label(entry: &GroupEntry) -> String {
    format!("{}{}", "   ".repeat(entry.depth.min(MAX_DEPTH)), entry.name)
}

/// Naming a group changes application data only, so this form validates text
/// and keeps what the user typed when a save fails.
pub struct GroupForm {
    owner: WeakEntity<GitTurtle>,
    intent: GroupIntent,
    pub input: Entity<InputState>,
    error: Option<String>,
    pub pending: bool,
    pub visible: bool,
    _subscription: Subscription,
}

impl GroupForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        intent: GroupIntent,
        current: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let state = InputState::new(window, cx).placeholder("Work");
            match &current {
                Some(name) => state.default_value(name.clone()),
                None => state,
            }
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => {}
            });
        Self {
            owner,
            intent,
            input,
            error: None,
            pending: false,
            visible: true,
            _subscription: subscription,
        }
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let name = self.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.error = Some("Enter a name for this group.".into());
            cx.notify();
            return;
        }
        self.pending = true;
        self.error = None;
        cx.notify();
        let intent = self.intent;
        let form = cx.entity().downgrade();
        let _ = self.owner.update(cx, |owner, cx| {
            owner.submit_group_name(intent, name, form, window, cx)
        });
    }
}

impl Render for GroupForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = crate::appearance::palette(cx);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(colors.muted))
                    .child(match self.intent {
                        GroupIntent::Rename(_) => {
                            "Give this group a new name. Nothing on disk changes."
                        }
                        GroupIntent::Create(None) => {
                            "Name a group for the top level of your project list."
                        }
                        GroupIntent::Create(Some(_)) => {
                            "Name a group inside the group you selected."
                        }
                    }),
            )
            .child(Input::new(&self.input).aria_label("Group name"))
            .children(self.error.as_ref().map(|error| {
                div()
                    .id("project-group-error")
                    .role(Role::Alert)
                    .aria_label(error.clone())
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(colors.removed))
                    .child(error.clone())
            }))
    }
}

#[cfg(test)]
mod tests;
