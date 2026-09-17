//! The project list pane. It presents the saved project library on the far
//! left of the window as a keyboard tree with the History navigator's rows and
//! bindings, opens a project on activation, and edits groups through the
//! serialized preference writer. It never touches a repository.

use crate::{
    AppPage, GitTurtle, button, icon, native_accessibility,
    preferences::Preferences,
    project_library::{LibraryRow, MAX_DEPTH, ProjectLibrary},
};
use gpui_kit::base::POPUP_PRIORITY;
use gpui_kit::component::{
    Disableable, Icon, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// What the group dialog will do when the user confirms it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GroupIntent {
    /// Create a group at the top level, or inside the given group.
    Create(Option<u32>),
    Rename(u32),
}

/// One visible line of the pane, ready to draw. Rows are rebuilt when the
/// library or a project name changes, not on every frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneRow {
    Group {
        id: u32,
        name: String,
        depth: usize,
        collapsed: bool,
        projects: usize,
    },
    Project {
        path: PathBuf,
        name: String,
        /// The parent folder, shown only when another project has the same
        /// display name, so two identically named checkouts stay apart.
        detail: Option<String>,
        location: String,
        depth: usize,
        parent: Option<u32>,
    },
}

impl PaneRow {
    fn depth(&self) -> usize {
        match self {
            Self::Group { depth, .. } | Self::Project { depth, .. } => *depth,
        }
    }

    fn key(&self) -> RowKey {
        match self {
            Self::Group { id, .. } => RowKey::Group(*id),
            Self::Project { path, .. } => RowKey::Project(path.clone()),
        }
    }
}

/// A stable identity for a row: the keyboard cursor follows it across list
/// changes, and the row menu acts on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKey {
    Group(u32),
    Project(PathBuf),
}

/// One choice in a "Move to" menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    pub into: Option<u32>,
    pub label: String,
    /// The row already lives here.
    pub current: bool,
    /// A group cannot move inside itself, its own groups, or past the depth
    /// limit; those destinations stay visible but disabled.
    pub allowed: bool,
}

/// Groups first, then projects, each sorted by display name within its level.
/// The saved order still decides which ungrouped project a full list drops.
pub fn present_rows(library: &ProjectLibrary, name_of: impl Fn(&Path) -> String) -> Vec<PaneRow> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for path in library.projects() {
        *counts.entry(name_of(path)).or_default() += 1;
    }
    library
        .sorted_rows(&name_of)
        .into_iter()
        .map(|row| match row {
            LibraryRow::Group {
                id,
                name,
                depth,
                collapsed,
                projects,
            } => PaneRow::Group {
                id,
                name,
                depth,
                collapsed,
                projects,
            },
            LibraryRow::Project {
                path,
                depth,
                parent,
            } => {
                let name = name_of(&path);
                let detail =
                    (counts.get(&name).copied().unwrap_or(0) > 1).then(|| parent_label(&path));
                let location = path.display().to_string();
                PaneRow::Project {
                    path,
                    name,
                    detail,
                    location,
                    depth,
                    parent,
                }
            }
        })
        .collect()
}

fn parent_label(path: &Path) -> String {
    let parent = path.parent();
    parent
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| parent.map(|p| p.display().to_string()).unwrap_or_default())
}

/// Every place a row can move to, in the pane's order. The current location
/// is marked, and impossible destinations are disabled instead of failing
/// after the click.
pub fn move_destinations(library: &ProjectLibrary, target: &RowKey) -> Vec<Destination> {
    let (current, moving_group, height) = match target {
        RowKey::Group(id) => (
            library.parent_of_group(*id).flatten(),
            Some(*id),
            library.group_height(*id).unwrap_or(1),
        ),
        RowKey::Project(path) => (library.parent_of_project(path), None, 0),
    };
    let mut destinations = vec![Destination {
        into: None,
        label: "Top level".into(),
        current: current.is_none(),
        allowed: true,
    }];
    for entry in library.sorted_groups() {
        if moving_group.is_some_and(|id| id == entry.id || library.group_contains(id, entry.id)) {
            continue;
        }
        // A group placed inside `entry` starts one level below it and brings
        // its own levels along; a project needs no room.
        let allowed = moving_group.is_none() || entry.depth + 1 + height <= MAX_DEPTH;
        destinations.push(Destination {
            into: Some(entry.id),
            label: library.group_path(entry.id).join(" › "),
            current: current == Some(entry.id),
            allowed,
        });
    }
    destinations
}

/// A popup opened from the keyboard. Right-click menus belong to their rows;
/// this one is positioned from the cursor row's geometry.
struct KeyboardMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _dismiss: Subscription,
}

pub struct State {
    pub scroll: UniformListScrollHandle,
    pub focus: FocusHandle,
    pub cursor: Option<usize>,
    pub rows: Vec<PaneRow>,
    pub group_form: Option<Entity<GroupForm>>,
    /// A refused edit or a failed save, shown inside the pane with Retry.
    pub error: Option<String>,
    /// Saves submitted and not yet answered. Only the last outstanding reply
    /// may replace the presented list, so an older reply cannot undo a newer
    /// edit that is still being written.
    pending_saves: usize,
    menu_open: Option<usize>,
    keyboard_menu: Option<KeyboardMenu>,
}

impl State {
    pub fn new(cx: &mut App) -> Self {
        Self {
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            cursor: None,
            rows: Vec::new(),
            group_form: None,
            error: None,
            pending_saves: 0,
            menu_open: None,
            keyboard_menu: None,
        }
    }

    #[cfg(test)]
    pub fn pending_saves(&self) -> usize {
        self.pending_saves
    }
}

impl GitTurtle {
    /// Present the current library with the current project names, keeping
    /// the keyboard cursor on the same group or project when it still exists.
    pub(super) fn rebuild_project_rows(&mut self) {
        let followed = self
            .project_pane
            .cursor
            .and_then(|index| self.project_pane.rows.get(index))
            .map(PaneRow::key);
        let rows = present_rows(&self.project_library, |path| self.project_name(path));
        self.project_pane.cursor = match followed {
            Some(key) => rows.iter().position(|row| row.key() == key),
            None => None,
        }
        .or_else(|| {
            self.project_pane
                .cursor
                .filter(|_| !rows.is_empty())
                .map(|index| index.min(rows.len() - 1))
        });
        self.project_pane.rows = rows;
    }

    pub(super) fn set_project_library(&mut self, library: ProjectLibrary) {
        if library != self.project_library {
            self.project_library = library;
        }
        self.rebuild_project_rows();
    }

    /// A recent-project save also returns the stored list. It is older than
    /// any pane edit still being written, so it only applies when none is.
    pub(super) fn absorb_saved_project_library(&mut self, library: ProjectLibrary) {
        if self.project_pane.pending_saves == 0 {
            self.set_project_library(library);
        }
    }

    fn submit_project_library_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.project_library.clone();
        self.project_pane.pending_saves += 1;
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
                this.finish_project_library_save(&result, cx);
            });
        })
        .detach();
    }

    pub(super) fn finish_project_library_save(
        &mut self,
        result: &anyhow::Result<Preferences>,
        cx: &mut Context<Self>,
    ) {
        self.project_pane.pending_saves = self.project_pane.pending_saves.saturating_sub(1);
        match result {
            Ok(preferences) => {
                if self.project_pane.pending_saves == 0 {
                    self.set_project_library(preferences.project_library.clone());
                }
                self.project_pane.error = None;
            }
            Err(error) => {
                self.project_pane.error =
                    Some(format!("Could not save the project list: {error:#}"));
            }
        }
        cx.notify();
    }

    /// Edit a copy of the saved list, then let the preference writer merge and
    /// validate it. The pane shows the edit at once; a failed write keeps it
    /// visible beside an explanation and a Retry action.
    pub(super) fn edit_project_library(
        &mut self,
        edit: impl FnOnce(&mut ProjectLibrary) -> Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut library = self.project_library.clone();
        if let Err(message) = edit(&mut library) {
            self.project_pane.error = Some(message);
            cx.notify();
            return;
        }
        if library == self.project_library {
            return;
        }
        self.set_project_library(library);
        self.submit_project_library_save(window, cx);
        cx.notify();
    }

    fn retry_project_library_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.project_pane.error = None;
        self.submit_project_library_save(window, cx);
        cx.notify();
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

    /// Show or hide the pane from the View menu or the command palette. The
    /// switch in Settings changes the same saved preference.
    pub(super) fn toggle_project_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        self.settings.project_pane = !self.settings.project_pane;
        self.save_preferences(window, cx);
        cx.notify();
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

    fn pane_available(&self, window: &mut Window, cx: &mut App) -> bool {
        self.settings.project_pane
            && self.page != AppPage::Projects
            && !window.has_active_dialog(cx)
            && !window.has_active_sheet(cx)
    }

    pub(super) fn move_project_cursor(
        &mut self,
        forward: bool,
        boundary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pane_available(window, cx) {
            return;
        }
        let count = self.project_pane.rows.len();
        if count == 0 {
            return;
        }
        let current = self.project_pane.cursor.filter(|index| *index < count);
        self.project_pane.cursor = Some(match (current, forward, boundary) {
            (_, true, true) => count - 1,
            (_, false, true) => 0,
            (None, _, false) => 0,
            (Some(index), true, false) => (index + 1).min(count - 1),
            (Some(index), false, false) => index.saturating_sub(1),
        });
        if let Some(index) = self.project_pane.cursor {
            self.project_pane
                .scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
        window.focus(&self.project_pane.focus, cx);
        cx.notify();
    }

    /// Right expands a group or steps into it; Left collapses a group or
    /// returns to the parent group, as in the History navigator.
    pub(super) fn expand_project_row(
        &mut self,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pane_available(window, cx) {
            return;
        }
        let Some(index) = self.project_pane.cursor else {
            return;
        };
        let Some(row) = self.project_pane.rows.get(index).cloned() else {
            return;
        };
        if let PaneRow::Group { id, collapsed, .. } = row
            && collapsed == expand
        {
            self.edit_project_library(
                move |library| {
                    library.set_collapsed(id, !expand);
                    Ok(())
                },
                window,
                cx,
            );
            return;
        }
        let depth = row.depth();
        if expand {
            if self
                .project_pane
                .rows
                .get(index + 1)
                .is_some_and(|next| next.depth() > depth)
            {
                self.move_project_cursor(true, false, window, cx);
            }
        } else if depth > 0
            && let Some(parent) = (0..index).rev().find(|candidate| {
                matches!(self.project_pane.rows[*candidate], PaneRow::Group { depth: parent, .. } if parent < depth)
            })
        {
            self.project_pane.cursor = Some(parent);
            self.project_pane
                .scroll
                .scroll_to_item(parent, ScrollStrategy::Center);
            cx.notify();
        }
    }

    /// Enter, Space or a click: a group toggles, a project opens.
    pub(super) fn activate_project_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pane_available(window, cx) {
            return;
        }
        let Some(row) = self.project_pane.rows.get(index).cloned() else {
            return;
        };
        self.project_pane.cursor = Some(index);
        window.focus(&self.project_pane.focus, cx);
        match row {
            PaneRow::Group { id, .. } => self.toggle_project_group(id, window, cx),
            PaneRow::Project { path, .. } => self.open_project_from_pane(path, window, cx),
        }
        cx.notify();
    }

    /// Shift-F10: open the cursor row's actions beside it, so the menu that a
    /// pointer reaches by hovering or right-clicking is available by keyboard.
    pub(super) fn manage_project_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pane_available(window, cx) {
            return;
        }
        let Some(index) = self.project_pane.cursor else {
            return;
        };
        let Some(row) = self.project_pane.rows.get(index) else {
            return;
        };
        let key = row.key();
        let indent = px(24. + row.depth() as f32 * 12.);
        // Rows are uniform, so the cursor row's bottom edge follows from the
        // list's origin, its scroll offset and the row height.
        let position = {
            let state = self.project_pane.scroll.0.borrow();
            let bounds = state.base_handle.bounds();
            let height = crate::appearance::ui_size(30.);
            point(
                bounds.left() + indent,
                bounds.top() + state.base_handle.offset().y + height * (index as f32 + 1.),
            )
        };
        let owner = cx.entity().downgrade();
        let focus = self.project_pane.focus.clone();
        let snapshot = self.menu_snapshot(&key);
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            row_menu(menu, &owner, &key, Some(snapshot)).action_context(focus)
        });
        let dismiss = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
            this.dismiss_project_menu(window, cx);
        });
        // The menu takes the keys until it closes; dismissal hands focus
        // back to the tree through the action context and the subscription.
        let handle = menu.focus_handle(cx);
        window.focus(&handle, cx);
        self.project_pane.keyboard_menu = Some(KeyboardMenu {
            menu,
            position,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    fn dismiss_project_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_pane.keyboard_menu.take().is_some() {
            window.focus(&self.project_pane.focus, cx);
            cx.notify();
        }
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
        let (current, parent) = match intent {
            GroupIntent::Rename(id) => (
                self.project_library
                    .group(id)
                    .map(|group| group.name.clone()),
                None,
            ),
            GroupIntent::Create(parent) => (
                None,
                parent.and_then(|id| self.project_library.group(id).map(|g| g.name.clone())),
            ),
        };
        let owner = cx.entity().downgrade();
        let form = cx.new(|cx| GroupForm::new(owner, intent, current, parent, window, cx));
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
                .width(px(520.))
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
        self.set_project_library(library.clone());
        self.project_pane.pending_saves += 1;
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
                this.finish_project_library_save(&result, cx);
                this.finish_group_form(form, result.is_ok(), window, cx);
            });
        })
        .detach();
    }

    fn finish_group_form(
        &mut self,
        form: WeakEntity<GroupForm>,
        saved: bool,
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
        if saved {
            let _ = form.update(cx, |form, cx| {
                form.pending = false;
                form.visible = false;
                cx.notify();
            });
            if visible {
                window.close_dialog(cx);
            }
            self.project_pane.group_form = None;
        } else {
            // The pane already shows the failure with Retry; the dialog keeps
            // the typed name and repeats the reason beside it.
            let error = self.project_pane.error.clone();
            let _ = form.update(cx, |form, cx| {
                form.pending = false;
                form.error = error;
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(super) fn render_project_pane(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = crate::appearance::palette(cx);
        let rows = &self.project_pane.rows;
        let projects = self.project_library.project_count();
        div()
            .id("project-pane")
            .debug_selector(|| "project-pane".into())
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
                    .pl_3()
                    .pr_2()
                    .border_b_1()
                    .border_color(rgb(colors.border))
                    .child(
                        div()
                            .id("project-pane-heading")
                            .role(Role::Heading)
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(crate::appearance::ui_text(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(colors.muted))
                            .child("Projects"),
                    )
                    .when(projects > 0, |header| {
                        header.child(
                            div()
                                .flex_shrink_0()
                                .text_size(crate::appearance::ui_text(10.))
                                .text_color(rgb(colors.muted))
                                .child(projects.to_string()),
                        )
                    })
                    .child(
                        Button::new("project-pane-new-group")
                            .ghost()
                            .flex_shrink_0()
                            .size(crate::appearance::ui_size(24.))
                            .rounded(px(6.))
                            .icon(Icon::default().path("icons/plus.svg").size(px(13.)))
                            .accessibility_label("New project group")
                            .tooltip("Create a group in the project list")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_group_form(GroupIntent::Create(None), window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("project-pane-tree")
                    .role(Role::Tree)
                    .aria_label("Projects. Arrow keys browse; Left and Right collapse or expand a group; Enter opens a project; Shift F10 opens project or group actions")
                    .key_context("GitTurtleNavigation")
                    .tab_stop(true)
                    .track_focus(&self.project_pane.focus)
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .on_action(cx.listener(|this, _: &native_accessibility::NextNavigation, window, cx| this.move_project_cursor(true, false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::PreviousNavigation, window, cx| this.move_project_cursor(false, false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::FirstNavigation, window, cx| this.move_project_cursor(false, true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::LastNavigation, window, cx| this.move_project_cursor(true, true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::ExpandNavigation, window, cx| this.expand_project_row(true, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::CollapseNavigation, window, cx| this.expand_project_row(false, window, cx)))
                    .on_action(cx.listener(|this, _: &native_accessibility::ActivateNavigation, window, cx| {
                        if let Some(index) = this.project_pane.cursor {
                            this.activate_project_row(index, window, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &native_accessibility::ManageNavigation, window, cx| this.manage_project_row(window, cx)))
                    .child(if rows.is_empty() {
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .p_4()
                            .text_center()
                            .child(icon("turtle", 22., colors.muted))
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("No projects yet"),
                            )
                            .child(
                                div()
                                    .max_w(px(220.))
                                    .text_size(crate::appearance::ui_text(11.))
                                    .line_height(relative(1.5))
                                    .text_color(rgb(colors.muted))
                                    .child("Projects you open appear here. Use + to make a group."),
                            )
                            .into_any_element()
                    } else {
                        uniform_list(
                            "project-pane-rows",
                            rows.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .filter(|index| *index < this.project_pane.rows.len())
                                    .map(|index| this.render_project_row(index, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.project_pane.scroll)
                        .into_any_element()
                    })
                    .children(self.render_keyboard_menu(window, cx)),
            )
            .children(self.project_pane.error.as_ref().map(|error| {
                div()
                    .id("project-pane-error")
                    .debug_selector(|| "project-pane-error".into())
                    .role(Role::Alert)
                    .aria_label(error.clone())
                    .a11y_synthetic_children(native_accessibility::assertive)
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(rgb(colors.border))
                    .bg(rgb(colors.removed_background))
                    .text_color(rgb(colors.removed))
                    .text_size(crate::appearance::ui_text(11.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().line_height(relative(1.4)).child(error.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                button("retry-project-list-save", "Retry", "", false)
                                    .h(crate::appearance::ui_size(22.))
                                    .text_size(crate::appearance::ui_text(11.))
                                    .tooltip("Save the project list again")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.retry_project_library_save(window, cx)
                                    })),
                            )
                            .child(
                                button("dismiss-project-pane-error", "Dismiss", "", false)
                                    .h(crate::appearance::ui_size(22.))
                                    .text_size(crate::appearance::ui_text(11.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.project_pane.error = None;
                                        cx.notify();
                                    })),
                            ),
                    )
            }))
            .into_any_element()
    }

    /// Rows share the History navigator's geometry: 30 px, a 12 px indent per
    /// level, flat hover and selected surfaces, and an accent cursor marker.
    fn render_project_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = crate::appearance::palette(cx);
        let Some(row) = self.project_pane.rows.get(index) else {
            return div().into_any_element();
        };
        let key = row.key();
        let depth = row.depth();
        let cursor = self.project_pane.cursor == Some(index);
        let owner = cx.entity().downgrade();
        let menu_key = key.clone();
        let (body, active, dimmed, label, tooltip) = match row {
            PaneRow::Group {
                name,
                collapsed,
                projects,
                ..
            } => {
                let body = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(rgb(colors.muted))
                    .child(div().w(px(12.)).flex_shrink_0().child(if *collapsed {
                        "›"
                    } else {
                        "⌄"
                    }))
                    .child(icon("folder", 14., colors.muted))
                    .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(crate::appearance::ui_text(10.))
                            .child(projects.to_string()),
                    )
                    .into_any_element();
                (
                    body,
                    false,
                    false,
                    format!(
                        "{} {name} · {projects} projects",
                        if *collapsed { "Expand" } else { "Collapse" }
                    ),
                    if *collapsed {
                        "Expand this group".to_owned()
                    } else {
                        "Collapse this group".to_owned()
                    },
                )
            }
            PaneRow::Project {
                path,
                name,
                detail,
                location,
                ..
            } => {
                let active = self.path.as_ref() == Some(path);
                let body = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    // A project carries the turtle mark; only a group keeps the
                    // folder, so the two never look alike.
                    .child(icon(
                        "turtle",
                        15.,
                        if active { colors.accent } else { colors.muted },
                    ))
                    .child(div().min_w_0().truncate().child(name.clone()))
                    .children(detail.as_ref().map(|detail| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(colors.muted))
                            .child(format!("· {detail}"))
                    }))
                    .into_any_element();
                (
                    body,
                    active,
                    self.operation_busy.is_some(),
                    format!("Open {name}, {location}"),
                    location.clone(),
                )
            }
        };
        let is_group = matches!(row, PaneRow::Group { .. });
        let expanded = match row {
            PaneRow::Group { collapsed, .. } => Some(!collapsed),
            PaneRow::Project { .. } => None,
        };
        let surface = if active {
            colors.selected
        } else {
            colors.panel
        };
        div()
            .id(("project-pane-row", index))
            .debug_selector(move || format!("project-pane-row-{index}"))
            .role(Role::TreeItem)
            .aria_level(depth + 1)
            .aria_label(label)
            .when(cursor, |row| row.aria_active_descendant())
            .when_some(expanded, |row, expanded| row.aria_expanded(expanded))
            .when(!is_group, |row| row.aria_selected(active))
            .when(self.project_pane.menu_open != Some(index), |row| {
                row.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .group("project-pane-row")
            .w_full()
            .h(crate::appearance::ui_size(30.))
            // The marker keeps its width so the text never shifts when the
            // cursor arrives.
            .border_l_2()
            .border_color(rgb(if cursor { colors.accent } else { surface }))
            .pl(px(10. + depth as f32 * 12.))
            .pr_1()
            .flex()
            .items_center()
            .gap_1()
            .text_size(crate::appearance::ui_text(12.))
            .overflow_hidden()
            .bg(rgb(surface))
            .text_color(rgb(if active { colors.accent } else { colors.text }))
            .when(dimmed, |row| row.opacity(0.6))
            .when(!dimmed, |row| {
                row.cursor_pointer()
                    .hover(|style| style.bg(rgb(colors.row_hover(active))))
                    .active(|style| style.bg(rgb(colors.selected)))
            })
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.activate_project_row(index, window, cx)
                }),
            )
            .child(body)
            .child(self.render_row_actions(index, menu_key, cursor, cx))
            .context_menu({
                let owner = owner.clone();
                move |menu, _, cx| {
                    let snapshot = owner.upgrade().map(|app| app.read(cx).menu_snapshot(&key));
                    row_menu(menu, &owner, &key, snapshot)
                }
            })
            .into_any_element()
    }

    /// The row menu button appears on hover and on the keyboard cursor row.
    /// It stays out of the Tab order: the tree is one stop, and Shift-F10 or
    /// a right-click opens the same menu.
    fn render_row_actions(
        &self,
        index: usize,
        key: RowKey,
        cursor: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let reveal = cursor || self.project_pane.menu_open == Some(index);
        let label = match &key {
            RowKey::Group(_) => "Group actions",
            RowKey::Project(_) => "Project actions",
        };
        let toggled = cx.entity().downgrade();
        Button::new(("project-row-menu", index))
            .ghost()
            .tab_stop(false)
            .flex_shrink_0()
            .size(crate::appearance::ui_size(22.))
            .rounded(px(6.))
            .icon(Icon::default().path("icons/sliders.svg").size(px(12.)))
            .accessibility_label(label)
            .opacity(if reveal { 1. } else { 0. })
            .group_hover("project-pane-row", |style| style.opacity(1.))
            // The popover trigger consumes the press, so the row beneath it
            // never sees a click; a click handler here would consume it first
            // and keep the menu shut.
            .dropdown_menu(move |menu, _, cx| {
                let snapshot = owner.upgrade().map(|app| app.read(cx).menu_snapshot(&key));
                row_menu(menu, &owner, &key, snapshot)
            })
            .on_open_change(move |open, _, cx| {
                let open = *open;
                let _ = toggled.update(cx, |this, cx| {
                    let next = open.then_some(index);
                    if this.project_pane.menu_open != next {
                        this.project_pane.menu_open = next;
                        cx.notify();
                    }
                });
            })
            .into_any_element()
    }

    fn render_keyboard_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let keyboard = self.project_pane.keyboard_menu.as_ref()?;
        let menu = keyboard.menu.clone();
        let position = keyboard.position;
        let viewport = window.bounds().size;
        Some(
            deferred(
                anchored().child(
                    div()
                        .id("project-pane-menu-overlay")
                        .debug_selector(|| "project-pane-menu-overlay".into())
                        .occlude()
                        .w(viewport.width)
                        .h(viewport.height)
                        .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                            this.dismiss_project_menu(window, cx);
                        }))
                        .child(
                            anchored()
                                .position(position)
                                .snap_to_window_with_margin(px(8.))
                                .child(
                                    // A press inside the menu must reach its
                                    // item, not the overlay behind it.
                                    div()
                                        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                        .child(menu),
                                ),
                        ),
                ),
            )
            .with_priority(POPUP_PRIORITY)
            .into_any_element(),
        )
    }
}

/// What a row menu shows, captured when the menu opens. Pointer menus read
/// it from the entity at that moment; the keyboard menu captures it before
/// building, because the entity is being updated then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSnapshot {
    pub name: String,
    pub busy: bool,
    pub destinations: Vec<Destination>,
}

impl GitTurtle {
    fn menu_snapshot(&self, key: &RowKey) -> MenuSnapshot {
        let name = match key {
            RowKey::Group(id) => self
                .project_library
                .group(*id)
                .map(|group| group.name.clone())
                .unwrap_or_default(),
            RowKey::Project(path) => self.project_name(path),
        };
        MenuSnapshot {
            name,
            busy: self.operation_busy.is_some(),
            destinations: move_destinations(&self.project_library, key),
        }
    }
}

/// One menu for the hover button, the right-click and Shift-F10. Nothing is
/// rebuilt while rows render; the snapshot arrives when the menu opens.
fn row_menu(
    menu: PopupMenu,
    owner: &WeakEntity<GitTurtle>,
    key: &RowKey,
    snapshot: Option<MenuSnapshot>,
) -> PopupMenu {
    let Some(snapshot) = snapshot else {
        return menu.item(PopupMenuItem::new("Unavailable").disabled(true));
    };
    let MenuSnapshot {
        name,
        busy,
        destinations,
    } = snapshot;
    let mut menu = match key {
        RowKey::Group(id) => {
            let id = *id;
            let rename = owner.clone();
            let nest = owner.clone();
            menu.label(name)
                .item(
                    PopupMenuItem::new("Rename group…").on_click(move |_, window, cx| {
                        let _ = rename.update(cx, |this, cx| {
                            this.open_group_form(GroupIntent::Rename(id), window, cx)
                        });
                    }),
                )
                .item(
                    PopupMenuItem::new("New group inside…").on_click(move |_, window, cx| {
                        let _ = nest.update(cx, |this, cx| {
                            this.open_group_form(GroupIntent::Create(Some(id)), window, cx)
                        });
                    }),
                )
        }
        RowKey::Project(path) => {
            let open = owner.clone();
            let open_path = path.clone();
            let rename = owner.clone();
            let rename_path = path.clone();
            menu.label(name)
                .item(PopupMenuItem::new("Open project").disabled(busy).on_click(
                    move |_, window, cx| {
                        let target = open_path.clone();
                        let _ = open.update(cx, |this, cx| {
                            this.open_project_from_pane(target, window, cx)
                        });
                    },
                ))
                .item(
                    PopupMenuItem::new("Rename project…").on_click(move |_, window, cx| {
                        let target = rename_path.clone();
                        let _ = rename
                            .update(cx, |this, cx| this.open_rename_project(target, window, cx));
                    }),
                )
        }
    };
    menu = menu.separator().label("Move to");
    for destination in destinations {
        let owner = owner.clone();
        let key = key.clone();
        let into = destination.into;
        menu = menu.item(
            PopupMenuItem::new(destination.label)
                .checked(destination.current)
                .disabled(!destination.allowed || destination.current)
                .on_click(move |_, window, cx| {
                    let key = key.clone();
                    let _ = owner.update(cx, |this, cx| {
                        this.edit_project_library(
                            |library| match &key {
                                RowKey::Group(id) => library.move_group(*id, into),
                                RowKey::Project(path) => library.move_project(path, into),
                            },
                            window,
                            cx,
                        )
                    });
                }),
        );
    }
    let remove = owner.clone();
    let key = key.clone();
    menu.separator().item(
        PopupMenuItem::new(match &key {
            RowKey::Group(_) => "Remove group, keep its projects",
            RowKey::Project(_) => "Remove from the project list",
        })
        .on_click(move |_, window, cx| {
            let key = key.clone();
            let _ = remove.update(cx, |this, cx| {
                this.edit_project_library(
                    |library| {
                        match &key {
                            RowKey::Group(id) => {
                                library.remove_group(*id);
                            }
                            RowKey::Project(path) => {
                                library.forget_project(path);
                            }
                        }
                        Ok(())
                    },
                    window,
                    cx,
                )
            });
        }),
    )
}

/// Naming a group changes application data only, so this form validates text
/// and keeps what the user typed when a save fails.
pub struct GroupForm {
    owner: WeakEntity<GitTurtle>,
    intent: GroupIntent,
    parent: Option<String>,
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
        parent: Option<String>,
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
            parent,
            input,
            error: None,
            pending: false,
            visible: true,
            _subscription: subscription,
        }
    }

    /// Keep the reviewed target and text until persistence reports success.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.visible {
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
        let intent = self.intent;
        let form = cx.entity().downgrade();
        if self
            .owner
            .update(cx, |owner, cx| {
                owner.submit_group_name(intent, name, form, window, cx)
            })
            .is_err()
        {
            self.pending = false;
            self.error = Some("The project window is no longer available.".into());
        }
        cx.notify();
    }
}

impl Render for GroupForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = crate::appearance::palette(cx);
        let description = match (self.intent, &self.parent) {
            (GroupIntent::Rename(_), _) => {
                "Groups only organize this list. Nothing on disk changes.".to_owned()
            }
            (GroupIntent::Create(None), _) => {
                "A group at the top level of your project list. Nothing on disk changes.".to_owned()
            }
            (GroupIntent::Create(Some(_)), Some(parent)) => {
                format!("A group inside {parent}. Nothing on disk changes.")
            }
            (GroupIntent::Create(Some(_)), None) => {
                "A group inside the group you chose. Nothing on disk changes.".to_owned()
            }
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .id("project-group-description")
                    .text_size(crate::appearance::ui_text(12.))
                    .line_height(relative(1.5))
                    .text_color(rgb(colors.muted))
                    .child(description),
            )
            .child(
                div()
                    .id("project-group-label")
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Group name"),
            )
            .child(
                Input::new(&self.input)
                    .aria_label("Group name")
                    .disabled(self.pending),
            )
            .children(self.error.as_ref().map(|error| {
                div()
                    .id("project-group-error")
                    .role(Role::Alert)
                    .aria_label(error.clone())
                    .a11y_synthetic_children(native_accessibility::assertive)
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(colors.removed))
                    .p_2()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(colors.removed))
                    .child(error.clone())
            }))
    }
}

#[cfg(test)]
mod tests;
