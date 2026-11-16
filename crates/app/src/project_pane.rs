//! The project list pane. It presents the saved project library on the far
//! left of the window as a keyboard tree with the History navigator's rows and
//! bindings, opens a project on activation, and edits groups through the
//! serialized preference writer. It never touches a repository.

use crate::native_accessibility::LiveRegion as _;
use crate::{
    AppContextMenu, AppMenu, AppPage, GitTurtle, button,
    commit_drafts::CoalescingSaver,
    icon, native_accessibility,
    operations::Busy,
    preferences::Preferences,
    project_library::{LibraryRow, MAX_DEPTH, ProjectLibrary},
};
use gpui_kit::component::{
    Disableable, Icon, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
        /// Shared with the row's key and element ID, which each frame
        /// clones rather than copying the path.
        path: Arc<Path>,
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
    /// The project's path, shared with its row ([`PaneRow::Project`]).
    Project(Arc<Path>),
}

impl RowKey {
    /// The row's element ID, from its group or project: the state GPUI keeps
    /// for the row, its open right-click and actions menus and its tooltip
    /// among it, stays with that group or project as rows come or go around
    /// it, and goes with the row when it leaves the list. Keyed by its place,
    /// a menu open on a project passed to the row that moved into that
    /// place, as a project added above it or a renamed one sorted elsewhere.
    /// It allocates nothing: a group's is its number beside a static name, a
    /// project's its row's shared path.
    fn element_id(&self) -> ElementId {
        match self {
            Self::Group(id) => ElementId::NamedInteger("project-group".into(), u64::from(*id)),
            Self::Project(path) => ElementId::Path(path.clone()),
        }
    }
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
                    path: path.into(),
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

/// Why a project-list save failed, as the pane and the group dialog show it.
fn save_failure(error: &anyhow::Error) -> String {
    format!("Could not save the project list: {error:#}")
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

/// A recent-project save awaiting its reply. The preference writer adds the
/// opened project to the stored list; the reply merges that addition into the
/// pane instead of replacing the pane's list.
#[derive(Clone, Copy)]
pub(super) struct RecentProjectSave {
    save: u64,
    /// The pane already listed the project when the save was asked for, so
    /// its current list, including a removal made since, stands.
    listed: bool,
}

/// A project's save and what the job answered for it.
type AnsweredSave = (RecentProjectSave, anyhow::Result<Preferences>);

/// The recent-project saves of a window, which share one job on the
/// preference writer ([`GitTurtle::queue_recent_project_save`]).
#[derive(Default)]
struct RecentProjectSaves {
    saver: CoalescingSaver<PathBuf, RecentProjectSave>,
    /// What the job has answered for each project, which its reply takes.
    answered: Arc<Mutex<Vec<AnsweredSave>>>,
    /// The newest project asked for, which a refused job asks for again and
    /// so schedules everything still waiting.
    newest: Option<(PathBuf, RecentProjectSave)>,
    /// The wait before asking again, one at a time.
    retry: Option<Task<()>>,
}

pub struct State {
    pub scroll: UniformListScrollHandle,
    pub focus: FocusHandle,
    pub cursor: Option<usize>,
    pub rows: Vec<PaneRow>,
    pub group_form: Option<Entity<GroupForm>>,
    /// A refused edit or a failed save, shown inside the pane with Retry.
    pub error: Option<String>,
    /// Pane saves submitted and not yet answered.
    pending_saves: usize,
    /// The number of the newest save that returns the project list, pane and
    /// recent-project saves alike. Only its reply may replace the presented
    /// list: each reply is awaited by its own task, and a newer save's task
    /// can run before an older one's wake, so an older reply handled last
    /// must not undo the newer edit.
    latest_save: u64,
    /// The number of the newest pane save. Only its reply sets or clears the
    /// error, and it wrote the whole list over any recent-project save
    /// numbered before it.
    latest_pane_save: u64,
    /// The newest pane save answered with a failure: the list on screen is
    /// unsaved until Retry, which the user starts.
    pane_save_failed: bool,
    recent_saves: RecentProjectSaves,
    /// The row whose actions button's menu is open, by its group or project.
    menu_open: Option<RowKey>,
    keyboard_menu: Option<native_accessibility::KeyboardMenu>,
    /// Whether the window's last frame drew the pane: the frame that no
    /// longer draws it gives focus left in it to the page
    /// ([`GitTurtle::follow_project_pane`]).
    drawn: bool,
    /// Where the page begins in the Tab order, focused only to step from to
    /// the page's first stop: no Tab stop, drawn only in the frame that
    /// gives focus to the page, before the page.
    page_start: FocusHandle,
    /// Toggle Project List turned the pane on or off where the window holds
    /// it back, which the status bar says while it is fresh
    /// ([`HELD_BACK_NOTE`], [`HELD_BACK_OFF_NOTE`]).
    held_note: Option<HeldNote>,
    /// How many notes the toggle has started, so each is announced again
    /// with a fresh node.
    held_turns: usize,
}

/// What the status bar says where Toggle Project List turns the pane on in a
/// window that cannot draw it beside the page's floor: nothing else on
/// screen changes, so the bar says why, feedback in place, in words short
/// enough to stay whole in a 461 px half tile at 18 pt, as the narrow
/// rail's "… unavailable: widen the window" does. "Project list hidden: the
/// window is too narrow to show it beside the page" ended in an ellipsis
/// there.
pub(super) const HELD_BACK_NOTE: &str = "Project list hidden: widen the window";

/// What the status bar says where Toggle Project List turns the pane on from
/// the Projects hub, which never draws it, at any window width.
pub(super) const HUB_ON_NOTE: &str = "Project list on: hidden on this page";

/// What the status bar says where Toggle Project List turns the pane off
/// while the window holds it back: nothing on screen changes there either.
pub(super) const HELD_BACK_OFF_NOTE: &str = "Project list off";

/// What Settings' Show the project list switch says in its description's
/// place while it is on and the window holds the pane back, announced
/// politely as it is turned on there: the switch's row is where the person
/// looks, so the status line keeps the page's status.
pub(super) const HELD_BACK_DESCRIPTION: &str =
    "Hidden: widen the window to show the list beside the page";

/// A held-back note while it is fresh: for [`crate::copy_feedback::RESTORE_AFTER`],
/// as a write's outcome is, or until the window draws the pane, the setting
/// changes from the one it answers or the project hub is shown.
struct HeldNote {
    turn: usize,
    text: &'static str,
    /// The setting it answers: off only for [`HELD_BACK_OFF_NOTE`].
    on: bool,
    /// The page it answered on: it goes as the page changes
    /// ([`GitTurtle::follow_project_pane`]).
    page: AppPage,
    /// Whether the status line reads it, as it reads the toggle's notes,
    /// or only the polite node announces it, as the switch's.
    shown: bool,
    _expiry: Task<()>,
}

impl State {
    pub fn new(cx: &mut App) -> Self {
        Self {
            scroll: UniformListScrollHandle::new(),
            // The tree is one Tab stop: GPUI ignores the `tab_stop` of the div
            // that tracks this handle, so the handle carries it.
            focus: cx.focus_handle().tab_stop(true),
            cursor: None,
            rows: Vec::new(),
            group_form: None,
            error: None,
            pending_saves: 0,
            latest_save: 0,
            latest_pane_save: 0,
            pane_save_failed: false,
            recent_saves: RecentProjectSaves::default(),
            menu_open: None,
            keyboard_menu: None,
            drawn: false,
            page_start: cx.focus_handle(),
            held_note: None,
            held_turns: 0,
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
        let found = followed.and_then(|key| rows.iter().position(|row| row.key() == key));
        // A keyboard's menu, opened on the cursor row, is drawn beside that
        // row where it now stands.
        if let (Some(row), Some(menu)) = (found, self.project_pane.keyboard_menu.as_mut()) {
            menu.follow_row(row);
        }
        self.project_pane.cursor = found.or_else(|| {
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

    /// Number a recent-project save among the project-list saves, noting
    /// whether the pane already lists the project it adds.
    pub(super) fn begin_recent_project_save(&mut self, path: &Path) -> RecentProjectSave {
        self.project_pane.latest_save += 1;
        RecentProjectSave {
            save: self.project_pane.latest_save,
            listed: self.project_library.contains_project(path),
        }
    }

    /// Asks the preference writer to remember `path`. Every snapshot that
    /// lands remembers its repository, so nine Refresh presses while the
    /// writer was behind took nine slots of its bounded queue and the next
    /// was refused as busy, which showed the person an error about a save
    /// they never asked for and lost the project. The projects asked for
    /// now share one job, oldest first so the newest ends the list, which
    /// keeps one slot however many open; a full queue of other saves refuses
    /// it, and it asks again until there is room.
    ///
    /// The job answers with the number of the request that scheduled it,
    /// its place in the writer's order: a pane save numbered after that
    /// wrote the list after it, whichever request joined the job later.
    pub(super) fn queue_recent_project_save(
        &mut self,
        path: PathBuf,
        recent: RecentProjectSave,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answered = Arc::clone(&self.project_pane.recent_saves.answered);
        let job = recent.save;
        // Saves a full queue refused may still wait: the list as it stands
        // now says which of their projects the person has removed since.
        let shown = self
            .project_pane
            .recent_saves
            .saver
            .is_pending()
            .then(|| self.project_library.clone());
        let response = self.project_pane.recent_saves.saver.queue_with(
            &self.preferences_writer,
            path.clone(),
            recent,
            move |projects| {
                let mut projects: Vec<_> = projects.iter().collect();
                projects.sort_by_key(|(_, recent)| recent.save);
                for (path, recent) in projects {
                    // Remembering would add a project the person removed
                    // since its save was refused back to the store.
                    if recent.listed
                        && shown
                            .as_ref()
                            .is_some_and(|shown| !shown.contains_project(path))
                    {
                        continue;
                    }
                    // Remembering rebuilds the whole store from disk; nothing is read here first.
                    let mut preferences = Preferences::default();
                    let result = preferences.remember_repository(path).map(|()| preferences);
                    let answer = RecentProjectSave {
                        save: job,
                        ..*recent
                    };
                    answered
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push((answer, result));
                }
                // Each project answers for itself: failing the job would keep
                // a project that can no longer be saved ahead of every later one.
                Ok(())
            },
        );
        self.project_pane.recent_saves.newest = Some((path, recent));
        let Some(response) = response else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let outcome = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.land_recent_project_saves(outcome, window, cx);
            });
        })
        .detach();
    }

    fn land_recent_project_saves(
        &mut self,
        outcome: Result<anyhow::Result<()>, futures::channel::oneshot::Canceled>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answered = std::mem::take(
            &mut *self
                .project_pane
                .recent_saves
                .answered
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for (recent, result) in answered {
            match result {
                Ok(preferences) => {
                    self.absorb_remembered_project(recent, &preferences, window, cx);
                    self.hub.update(cx, |hub, cx| {
                        hub.set_recent(preferences.recent_repositories, cx)
                    });
                }
                Err(error) => {
                    self.operation_error =
                        Some(format!("Could not save recent projects: {error:#}").into())
                }
            }
        }
        match outcome {
            Ok(Err(error)) if error.is::<Busy>() => self.ask_again_for_recent_projects(window, cx),
            Ok(Err(error)) => {
                self.operation_error =
                    Some(format!("Could not save recent projects: {error:#}").into())
            }
            Ok(Ok(())) | Err(_) => {}
        }
    }

    /// Asks again, after the wait, for the projects a full queue refused.
    fn ask_again_for_recent_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_pane.recent_saves.retry.is_some() {
            return;
        }
        let retry = self.retry_refused_save(window, cx, |this, window, cx| {
            this.project_pane.recent_saves.retry = None;
            if !this.project_pane.recent_saves.saver.is_pending() {
                return;
            }
            if let Some((path, recent)) = this.project_pane.recent_saves.newest.clone() {
                this.queue_recent_project_save(path, recent, window, cx);
            }
        });
        self.project_pane.recent_saves.retry = Some(retry);
    }

    /// Merge the project a recent-project save added into the pane's current
    /// list. It never replaces the list or touches the error, so an unsaved
    /// or newer edit survives the reply.
    pub(super) fn absorb_remembered_project(
        &mut self,
        recent: RecentProjectSave,
        stored: &Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if recent.listed {
            return;
        }
        // The writer puts the canonical path first; a list full of grouped
        // projects refuses it.
        let Some(path) = stored
            .recent_repositories
            .first()
            .filter(|path| stored.project_library.contains_project(path))
        else {
            return;
        };
        let mut library = self.project_library.clone();
        if !library.remember(path) {
            return;
        }
        // A full list drops an ungrouped project to fit this one, and replies
        // handled out of order can drop a different one than the store did.
        let evicted = library.project_count() == self.project_library.project_count();
        self.set_project_library(library);
        // A pane save numbered after this one wrote the whole list without
        // the project, and an eviction may differ from the store's, so the
        // merged list has to be written again. When the newest pane save
        // failed, Retry writes it with the unsaved edit.
        if (evicted || self.project_pane.latest_pane_save > recent.save)
            && !self.project_pane.pane_save_failed
        {
            self.submit_project_library_save(window, cx);
        }
        cx.notify();
    }

    /// Count a pane save as outstanding and number it for its reply.
    fn begin_project_library_save(&mut self) -> u64 {
        self.project_pane.pending_saves += 1;
        self.project_pane.latest_save += 1;
        self.project_pane.latest_pane_save = self.project_pane.latest_save;
        self.project_pane.pane_save_failed = false;
        self.project_pane.latest_save
    }

    fn submit_project_library_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.project_library.clone();
        let save = self.begin_project_library_save();
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
                this.finish_project_library_save(save, &result, cx);
            });
        })
        .detach();
    }

    pub(super) fn finish_project_library_save(
        &mut self,
        save: u64,
        result: &anyhow::Result<Preferences>,
        cx: &mut Context<Self>,
    ) {
        self.project_pane.pending_saves = self.project_pane.pending_saves.saturating_sub(1);
        // Every pane save writes the whole list, so only the newest one's
        // outcome is current: an older reply neither sets nor clears the
        // error.
        if save == self.project_pane.latest_pane_save {
            self.project_pane.pane_save_failed = result.is_err();
            self.project_pane.error = result.as_ref().err().map(save_failure);
        }
        // A newer recent-project save's addition arrives through its own
        // reply, so this list stands only when no save of either kind
        // followed it.
        if save == self.project_pane.latest_save
            && let Ok(preferences) = result
        {
            self.set_project_library(preferences.project_library.clone());
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
    /// switch in Settings changes the same saved preference. Turned on where
    /// the window cannot draw the pane beside the page
    /// (`GitTurtle::project_pane_fits`), the setting is saved on and the
    /// status bar says why nothing appears ([`HELD_BACK_NOTE`]); the pane
    /// comes as soon as the window gives it room. On the Projects hub, which
    /// never draws it, it says the list is on but hidden there
    /// ([`HUB_ON_NOTE`]). Turned off in either, the status bar says so
    /// ([`HELD_BACK_OFF_NOTE`]), since nothing on screen changes either.
    pub(super) fn toggle_project_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let on = !self.settings.project_pane;
        self.settings.project_pane = on;
        self.project_pane.held_note = None;
        let text = match (on, self.page == AppPage::Projects) {
            (false, _) => HELD_BACK_OFF_NOTE,
            (true, true) => HUB_ON_NOTE,
            (true, false) => HELD_BACK_NOTE,
        };
        self.note_held_back_pane(text, true, window, cx);
        self.save_preferences(window, cx);
        cx.notify();
    }

    /// Settings' Show the project list switch turned the pane on or off.
    /// Turned on where the window holds the pane back, the switch's own
    /// description says why no list is drawn
    /// ([`HELD_BACK_DESCRIPTION`], `GitTurtle::render_settings`), which the
    /// status bar's polite node announces, the status line left to the page:
    /// the description is the feedback in place.
    pub(super) fn switch_project_pane(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings.project_pane == on {
            return;
        }
        self.settings.project_pane = on;
        self.project_pane.held_note = None;
        if on {
            self.note_held_back_pane(HELD_BACK_DESCRIPTION, false, window, cx);
        }
        self.save_preferences(window, cx);
    }

    /// Starts the held-back note `text`, answering the setting as it now is,
    /// where the window holds the pane back on this page: in the status
    /// line too where `shown`, and announced politely, a fresh node for each
    /// note, for [`crate::copy_feedback::RESTORE_AFTER`].
    fn note_held_back_pane(
        &mut self,
        text: &'static str,
        shown: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // The hub never draws the pane, so a change there shows nothing
        // whatever the window's width and is answered like a held-back one.
        if self.page != AppPage::Projects && self.project_pane_fits(window) {
            return;
        }
        self.project_pane.held_turns = self.project_pane.held_turns.wrapping_add(1);
        let expiry = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::copy_feedback::RESTORE_AFTER)
                .await;
            let _ = this.update(cx, |this, cx| {
                this.project_pane.held_note = None;
                cx.notify();
            });
        });
        self.project_pane.held_note = Some(HeldNote {
            turn: self.project_pane.held_turns,
            text,
            on: self.settings.project_pane,
            page: self.page,
            shown,
            _expiry: expiry,
        });
    }

    /// The held-back note while it is fresh, with the number of the toggle
    /// it answers, which the status bar's polite node announces whatever
    /// holds the status line: a read's error, a write's fresh outcome and a
    /// running write's status keep the line, and the note, feedback for the
    /// person's own action, is still heard.
    pub(super) fn project_pane_note(&self) -> Option<(usize, &'static str)> {
        let note = self.project_pane.held_note.as_ref()?;
        Some((note.turn, note.text))
    }

    /// The held-back note where the status line reads it in place of the
    /// page's status: on Repository a read's error comes first, as it does
    /// before a write's outcome, and a write's fresh outcome keeps the line,
    /// heard as it came; the note is announced beside both
    /// ([`Self::project_pane_note`]).
    pub(super) fn project_pane_note_line(&self) -> Option<&'static str> {
        let note = self
            .project_pane
            .held_note
            .as_ref()
            .filter(|note| note.shown)?;
        let error = self.page == AppPage::Repository && self.error.is_some();
        (!error && self.fresh_outcome().is_none()).then_some(note.text)
    }

    /// Follows the pane as the window builds a frame, before the body,
    /// `drawn` saying whether this frame draws it
    /// (`GitTurtle::project_pane_drawn`). The held-back note ends once the
    /// pane is drawn, its setting changes from the one the note answers or
    /// the hub is shown. In the frame that no longer draws the pane on
    /// Repository or Settings, as the window narrows past it or the pane is
    /// closed, focus left on a control the frame does not draw, one in the
    /// pane, goes to the page's first stop as the frame ends, its ring whole
    /// in the next, rather than staying on a control no longer drawn, where
    /// no key reaches the page.
    /// So does focus given back to the pane's list while it is not drawn, as
    /// a group dialog opened from it gives focus back as it closes. Focus
    /// that the frame draws stays where it is. A row's menu open as the pane
    /// leaves closes with it: a Shift+F10 menu came back beside its row as
    /// the window widened again, with focus elsewhere. Returns the page's
    /// start to draw before the page in this frame where it does.
    pub(super) fn follow_project_pane(
        &mut self,
        drawn: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        let setting = self.settings.project_pane;
        let held = self.project_pane.held_note.as_ref();
        if drawn
            // A note answers the page it was made on: the hub's says the
            // list is hidden "on this page", which no other page can say.
            || held.is_some_and(|note| note.page != self.page)
            || held.is_some_and(|note| note.on != setting)
        {
            self.project_pane.held_note = None;
        }
        let was = std::mem::replace(&mut self.project_pane.drawn, drawn);
        // A row's menu goes with the pane: its keyboard's menu, which would
        // otherwise be drawn again beside its row as the pane comes back,
        // open with focus elsewhere, and the record of the actions button's
        // menu, whose popover goes with its element.
        if was && !drawn {
            self.project_pane.keyboard_menu = None;
            self.project_pane.menu_open = None;
        }
        if !(was || self.project_pane.focus.is_focused(window))
            || drawn
            || self.page == AppPage::Projects
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || window.focused(cx).is_none()
        {
            return None;
        }
        let start = self.project_pane.page_start.clone();
        let step_from = start.clone();
        cx.defer_in(window, move |this, window, cx| {
            if this.project_pane_drawn(window)
                || window.has_active_dialog(cx)
                || window.has_active_sheet(cx)
                || window
                    .focused(cx)
                    .is_some_and(|focused| this.app_focus.contains(&focused, window))
            {
                return;
            }
            step_from.focus(window, cx);
            window.focus_next(cx);
        });
        Some(start)
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
        // A marked tab's row reads its folder afresh, as the Projects page's
        // does (`project_event`).
        if self.repository_tabs.is_marked(&path) {
            self.open_repository_tab(path, window, cx);
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
        self.project_pane_drawn(window)
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
        // A write refuses a project before the cursor or focus moves, as
        // `open_project_from_pane` would once they had.
        if matches!(row, PaneRow::Project { .. }) && self.operation_busy.is_some() {
            return;
        }
        self.project_pane.cursor = Some(index);
        window.focus(&self.project_pane.focus, cx);
        match row {
            PaneRow::Group { id, .. } => self.toggle_project_group(id, window, cx),
            PaneRow::Project { path, .. } => {
                self.open_project_from_pane(path.to_path_buf(), window, cx)
            }
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
        // list's origin, its scroll offset and the row height, in each frame,
        // where `rebuild_project_rows` moves it as rows come or go above it.
        let anchor = native_accessibility::ListRow::new(
            &self.project_pane.scroll,
            index,
            crate::appearance::ui_size(30.),
            indent,
        );
        let owner = cx.entity().downgrade();
        // The cursor row still holds the group or project, wherever rows
        // that came or went above it moved that row (`rebuild_project_rows`).
        let held = key.clone();
        let holds = move |app: &GitTurtle| {
            app.project_pane
                .cursor
                .and_then(|index| app.project_pane.rows.get(index))
                .is_some_and(|row| row.key() == held)
        };
        self.project_pane.keyboard_menu = Some(native_accessibility::KeyboardMenu::following(
            anchor,
            self.project_pane.focus.clone(),
            // Read as it is built: a write's start or end builds the open
            // menu again.
            move |menu, app: &GitTurtle| {
                row_menu(menu, &owner, &key, Some(app.menu_snapshot(&key)))
            },
            holds,
            // A project's or a group's items read nothing HEAD changes.
            None,
            Self::dismiss_project_menu,
            self,
            window,
            cx,
        ));
        cx.notify();
    }

    /// Closes the keyboard's menu. A press outside it leaves focus in the
    /// menu, so focus returns to the tree; an item or Escape has moved it
    /// already, to the tree or to the form the item opened.
    fn dismiss_project_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.project_pane.keyboard_menu.take() {
            if menu.has_focus(window, cx) || window.focused(cx).is_none() {
                window.focus(&self.project_pane.focus, cx);
            }
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
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let pending = form.read(cx).pending;
            dialog
                .title(title)
                .width(crate::appearance::dialog_width(px(520.), window))
                .max_h(crate::appearance::dialog_max_height(window))
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

    /// Begins saving `name` for the group `form` names, or returns why the
    /// project list refuses it. `form` asks while it is being updated, so the
    /// refusal goes back to it to show: updating it here again panicked
    /// ("already being updated") for a name over the byte limit, a group
    /// past the nesting depth or the group limit, or one removed while its
    /// dialog was open. Only the save's later answer updates `form`.
    fn submit_group_name(
        &mut self,
        intent: GroupIntent,
        name: String,
        form: WeakEntity<GroupForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut library = self.project_library.clone();
        match intent {
            GroupIntent::Create(parent) => library.create_group(parent, &name).map(|_| ()),
            GroupIntent::Rename(id) => library.rename_group(id, &name),
        }?;
        self.set_project_library(library.clone());
        let save = self.begin_project_library_save();
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
                this.finish_project_library_save(save, &result, cx);
                let failure = result.as_ref().err().map(save_failure);
                this.finish_group_form(form, failure, window, cx);
            });
        })
        .detach();
        Ok(())
    }

    fn finish_group_form(
        &mut self,
        form: WeakEntity<GroupForm>,
        failure: Option<String>,
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
        if failure.is_none() {
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
            // The dialog keeps the typed name and gives this save's own reason:
            // the pane shows only the newest save's outcome, and a newer save
            // may have succeeded or still be pending.
            let _ = form.update(cx, |form, cx| {
                form.pending = false;
                if let Some(failure) = failure {
                    form.refuse(failure);
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(super) fn render_project_pane(&self, cx: &mut Context<Self>) -> AnyElement {
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
                crate::native_accessibility::clips_children(div()
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
                    // Its keyboard ring on its own edge, over the rows, so
                    // none of them moves.
                    .children(self.focus_ring.edge(&self.project_pane.focus, rgb(colors.accent)))
                    .children(self.render_keyboard_menu(cx))),
            )
            .children(self.project_pane.error.as_ref().map(|error| {
                native_accessibility::recorded(div()
                    .id("project-pane-error")
                    .debug_selector(|| "project-pane-error".into())
                    .alert(error.clone())
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
                    ))
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
                        "{} {name} · {}",
                        if *collapsed { "Expand" } else { "Collapse" },
                        crate::counted(*projects, "project", "projects")
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
                let active = self.path.as_deref() == Some(&**path);
                let body = div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    // A project carries the turtle mark; only a group keeps the
                    // folder, so the two never look alike.
                    .child(icon("turtle", 15., colors.sidebar_row(active).1))
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
                // `open_project_from_pane` refuses while a write runs; the
                // row looks so once the write looks busy
                // (`workspace::BusyLook`).
                (
                    body,
                    active,
                    self.operation_shown().is_some(),
                    format!("Open {name}, {location}"),
                    location.clone(),
                )
            }
        };
        let is_group = matches!(row, PaneRow::Group { .. });
        // A write refuses a project's row at once (`activate_project_row`),
        // and assistive technology hears it unavailable at once, while it
        // dims only once the write looks busy, as `workspace::refusing` has a
        // control. A group's row only folds, which no write refuses.
        let refused = !is_group && self.operation_busy.is_some();
        #[cfg(test)]
        crate::workspace::refusals::record(
            format!("project-pane-row-{index}"),
            refused || dimmed,
            dimmed,
        );
        let expanded = match row {
            PaneRow::Group { collapsed, .. } => Some(!collapsed),
            PaneRow::Project { .. } => None,
        };
        let surface = if active {
            colors.selected
        } else {
            colors.panel
        };
        let open = self.project_pane.menu_open.as_ref() == Some(&key);
        div()
            .id(key.element_id())
            .debug_selector(move || format!("project-pane-row-{index}"))
            .role(Role::TreeItem)
            .aria_level(depth + 1)
            .aria_label(label)
            // GPUI's Div has no disabled setter; the hook sets the row's own
            // node, as a live region's is set.
            .when(refused || dimmed, |row| {
                row.a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
            })
            .when(cursor, |row| row.aria_active_descendant())
            .when_some(expanded, |row, expanded| row.aria_expanded(expanded))
            .when(!is_group, |row| row.aria_selected(active))
            .when(!open, |row| {
                row.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .group("project-pane-row")
            .w_full()
            .h(crate::appearance::ui_size(30.))
            // The marker keeps its width so the text never shifts when the
            // cursor arrives.
            .border_l_2()
            .border_color(rgb(if cursor { colors.accent } else { surface }))
            // Its icon at the heading's edge, the marker's 2 px out of the
            // pane's inset, and 12 px further for each tree level, as History's
            // navigation rows are; a fixed 10 px stood it 2.25 px past the
            // heading at 13 pt and 1.5 px before it at 18.
            .pl(
                crate::appearance::ui_size(crate::appearance::PANE_INSET) - px(2.)
                    + px(depth as f32 * 12.),
            )
            .pr_1()
            .flex()
            .items_center()
            .gap_1()
            .text_size(crate::appearance::ui_text(12.))
            .overflow_hidden()
            .bg(rgb(surface))
            .text_color(rgb(colors.sidebar_row(active).0))
            .when(active, |row| row.font_weight(FontWeight::MEDIUM))
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
            .child(self.render_row_actions(index, menu_key, cursor || open, cx))
            .app_context_menu({
                let owner = owner.clone();
                move |menu, _, cx| {
                    let snapshot = owner.upgrade().map(|app| app.read(cx).menu_snapshot(&key));
                    row_menu(menu, &owner, &key, snapshot)
                }
            })
            // A write's start or end builds the open menu again.
            .follows_writes(owner.clone())
            .into_any_element()
    }

    /// The row menu button appears on hover, on the keyboard cursor row and
    /// while its menu is open (`reveal`). It stays out of the Tab order: the
    /// tree is one stop, and Shift-F10 or a right-click opens the same menu.
    /// Its ID lies inside its row's, which is keyed by the row's group or
    /// project, so its open menu stays with them.
    fn render_row_actions(
        &self,
        index: usize,
        key: RowKey,
        reveal: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let label = match &key {
            RowKey::Group(_) => "Group actions",
            RowKey::Project(_) => "Project actions",
        };
        let toggled = cx.entity().downgrade();
        let opened = key.clone();
        let tree = self.project_pane.focus.clone();
        Button::new("project-row-menu")
            .ghost()
            .tab_stop(false)
            .flex_shrink_0()
            .size(crate::appearance::ui_size(22.))
            .rounded(px(6.))
            .icon(Icon::default().path("icons/sliders.svg").size(px(12.)))
            .accessibility_label(label)
            .debug_selector(move || format!("project-row-menu-{index}"))
            .opacity(if reveal { 1. } else { 0. })
            .group_hover("project-pane-row", |style| style.opacity(1.))
            // The popover trigger consumes the press, so the row beneath it
            // never sees a click; a click handler here would consume it first
            // and keep the menu shut.
            .app_menu(move |menu, _, cx| {
                let snapshot = owner.upgrade().map(|app| app.read(cx).menu_snapshot(&key));
                let mut menu = row_menu(menu, &owner, &key, snapshot);
                // This Button is no Tab stop and takes no focus: a dialog
                // from its menu returns to the tree, as a keyboard's row
                // menu's does.
                menu.set_dialog_return_focus(Some(tree.clone()));
                menu
            })
            // A write's start or end builds the open menu again, as it does
            // the row's right-click and keyboard menus: read only as it
            // opened, Open project stayed as the write found it.
            .follows_writes(cx.entity().downgrade())
            .on_open_change(move |open, _, cx| {
                let open = *open;
                let _ = toggled.update(cx, |this, cx| {
                    let next = open.then(|| opened.clone());
                    if this.project_pane.menu_open != next {
                        this.project_pane.menu_open = next;
                        cx.notify();
                    }
                });
            })
            .into_any_element()
    }

    fn render_keyboard_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let keyboard = self.project_pane.keyboard_menu.as_ref()?;
        Some(keyboard.render("project-pane-menu-overlay", Self::dismiss_project_menu, cx))
    }
}

/// What a row menu shows, captured when the menu opens or is built again.
/// Pointer menus read it from the entity at that moment; the keyboard menu
/// captures it before building, because the entity is being updated then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSnapshot {
    pub name: String,
    /// Why a running write refuses Open project, which it then states.
    pub busy: Option<native_accessibility::MenuWait>,
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
            // Opening another project reads nothing the window's repository
            // reloads after a write, so only a running write refuses it.
            busy: self.running_wait(),
            destinations: move_destinations(&self.project_library, key),
        }
    }
}

/// One menu for the hover button, the right-click and Shift-F10. Nothing is
/// rebuilt while rows render; the snapshot arrives when the menu opens, and
/// again as a write starts or ends, which builds each of them again
/// (`native_accessibility::MenuInputs`).
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
                .item(
                    native_accessibility::waiting_item("Open project", false, busy).on_click(
                        move |_, window, cx| {
                            let target = open_path.to_path_buf();
                            let _ = open.update(cx, |this, cx| {
                                this.open_project_from_pane(target, window, cx)
                            });
                        },
                    ),
                )
                .item(
                    PopupMenuItem::new("Rename project…").on_click(move |_, window, cx| {
                        let target = rename_path.to_path_buf();
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
    /// How many refusals the form has given, each on a fresh error node that
    /// assistive technology announces, as Rename project's are: the line
    /// kept one node, so a name refused again was heard as nothing.
    errors_shown: usize,
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
        // Return submits through the dialog's default action alone, which
        // runs once per press, as the key goes down, never on a held key's
        // repeats, under X11 too, and never for a modified Enter. The field's
        // own `PressEnter`, which the field sends for every Enter key-down,
        // submitted the form as well, so after a refused save each repeat
        // began another write, and an empty name's refusal was given twice
        // per press, as Rename project's was.
        let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {
            if let InputEvent::Change = event {
                this.error = None;
                cx.notify();
            }
        });
        Self {
            owner,
            intent,
            parent,
            input,
            error: None,
            errors_shown: 0,
            pending: false,
            visible: true,
            _subscription: subscription,
        }
    }

    /// Shows `error` on a fresh node, announced at once, however often the
    /// same reason is given.
    fn refuse(&mut self, error: String) {
        self.errors_shown = self.errors_shown.wrapping_add(1);
        self.error = Some(error);
    }

    /// Keep the reviewed target and text until persistence reports success.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.visible {
            return;
        }
        let name = self.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.refuse("Enter a name for this group.".into());
            cx.notify();
            return;
        }
        self.pending = true;
        self.error = None;
        let intent = self.intent;
        let form = cx.entity().downgrade();
        // The owner answers a refusal here rather than updating this form,
        // which is being updated while it asks.
        let refusal = match self.owner.update(cx, |owner, cx| {
            owner.submit_group_name(intent, name, form, window, cx)
        }) {
            Ok(Ok(())) => None,
            Ok(Err(refusal)) => Some(refusal),
            Err(_) => Some("The project window is no longer available.".into()),
        };
        if let Some(refusal) = refusal {
            self.pending = false;
            self.refuse(refusal);
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
                    // A fresh node for each refusal, heard again however
                    // often the same one is given.
                    .id(("project-group-error", self.errors_shown))
                    .alert(error.clone())
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

#[cfg(test)]
mod recent_save_tests {
    use super::RecentProjectSave;
    use crate::{
        GitTurtle, operations::Held, preferences::Preferences, workspace::REFUSED_SAVE_RETRY,
    };
    use gpui_kit::{Entity, TestAppContext, VisualTestContext, gpui};
    use std::path::Path;

    fn writer_answers(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        cx.executor().run_until_parked();
    }

    /// Stalls the preference writer behind a queue of other saves, which
    /// refuses the next.
    fn a_full_queue(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Held {
        let held = app.read_with(cx, |app, _| app.preferences_writer.hold_reached());
        let fillers: Vec<_> = (0..8)
            .map(|_| app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(()))))
            .collect();
        for mut filler in fillers {
            assert!(matches!(filler.try_recv(), Ok(None)), "the queue fills");
        }
        held
    }

    fn ask_to_remember(
        app: &Entity<GitTurtle>,
        project: &Path,
        cx: &mut VisualTestContext,
    ) -> RecentProjectSave {
        let recent = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let recent = app.begin_recent_project_save(project);
                app.queue_recent_project_save(project.to_owned(), recent, window, cx);
                recent
            })
        });
        cx.executor().run_until_parked();
        recent
    }

    fn stored_project(project: &Path) -> bool {
        Preferences::load()
            .project_library
            .contains_project(project)
    }

    /// A project whose save a full queue refused, removed before the save
    /// asks again, whichever way that save is next scheduled: the removal
    /// stands, in the store as on screen. It once came back in the store on
    /// the retry and returned to the list at the next launch.
    async fn a_removal_since_the_refusal_stands(by_another_project: bool, cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        writer_answers(&app, cx);
        let project = app
            .read_with(cx, |app, _| app.path.clone())
            .expect("a repository")
            .canonicalize()
            .unwrap();

        let held = a_full_queue(&app, cx);
        let recent = ask_to_remember(&app, &project, cx);
        assert!(recent.listed, "the pane listed it when the save was asked");
        assert_eq!(app.read_with(cx, |app, _| app.operation_error_text()), None);
        drop(held);
        writer_answers(&app, cx);

        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.edit_project_library(
                    |library| {
                        library.forget_project(&project);
                        Ok(())
                    },
                    window,
                    cx,
                )
            })
        });
        cx.executor().run_until_parked();
        writer_answers(&app, cx);
        assert!(!stored_project(&project), "the pane's save removed it");

        if by_another_project {
            let another = fixture.path().canonicalize().unwrap();
            ask_to_remember(&app, &another, cx);
            writer_answers(&app, cx);
            assert!(stored_project(&another), "the other project is saved");
        } else {
            cx.executor().advance_clock(REFUSED_SAVE_RETRY);
            cx.executor().run_until_parked();
            writer_answers(&app, cx);
        }
        assert!(!stored_project(&project), "the removal stands in the store");
        app.read_with(cx, |app, _| {
            assert!(!app.project_library.contains_project(&project));
            assert_eq!(app.operation_error, None);
        });
    }

    /// A project the store cannot take is a real failure and no busy writer.
    #[gpui::test]
    async fn a_project_that_cannot_be_saved_still_reports(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        writer_answers(&app, cx);
        ask_to_remember(&app, &fixture.path().join("never-created"), cx);
        writer_answers(&app, cx);
        let error = app
            .read_with(cx, |app, _| app.operation_error_text())
            .expect("the failure is reported");
        assert!(
            error.starts_with("Could not save recent projects:"),
            "{error}"
        );
    }

    /// A writer whose thread has stopped can never take the save, so asking
    /// again could not help: the person hears about it.
    #[gpui::test]
    async fn a_stopped_writer_still_reports_a_recent_project_save(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        writer_answers(&app, cx);
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.preferences_writer = crate::operations::SerialExecutor::stopped("stopped");
            })
        });
        ask_to_remember(&app, fixture.path(), cx);
        let error = app
            .read_with(cx, |app, _| app.operation_error_text())
            .expect("the failure is reported");
        assert!(
            error.starts_with("Could not save recent projects:"),
            "{error}"
        );
        assert!(
            app.read_with(cx, |app, _| app.project_pane.recent_saves.retry.is_none()),
            "nothing asks again"
        );
    }

    #[gpui::test]
    async fn a_retry_leaves_a_project_removed_since_its_refusal(cx: &mut TestAppContext) {
        a_removal_since_the_refusal_stands(false, cx).await;
    }

    #[gpui::test]
    async fn another_projects_save_leaves_a_project_removed_since_its_refusal(
        cx: &mut TestAppContext,
    ) {
        a_removal_since_the_refusal_stands(true, cx).await;
    }
}
