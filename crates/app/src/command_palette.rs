//! A bounded registry of native workflows. Highlighting a command is inert;
//! activation rechecks the current context before invoking its existing handler.
use crate::native_accessibility::LiveRegion as _;
use crate::shortcuts::{Listed, ShortcutId};
use crate::*;
use gpui_kit::{
    component::{Theme, WindowExt, dialog::DialogFooter},
    prelude::FluentBuilder,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandId {
    NextRepositoryTab,
    PreviousRepositoryTab,
    CloseRepositoryTab,
    MoveRepositoryTabLeft,
    MoveRepositoryTabRight,
    LocalWorkspaces,
    PinRepository,
    Projects,
    Open,
    History,
    Changes,
    QuickOpen,
    Compare,
    Branch,
    Worktrees,
    Tags,
    Stashes,
    GitHub,
    Reflog,
    Rebase,
    RewriteReview,
    FileHistory,
    Blame,
    Profiles,
    Appearance,
    Settings,
    RecoveryDrafts,
    Activity,
    Refresh,
    Search,
    Sidebar,
    ProjectPane,
    EarlierGraphLanes,
    LaterGraphLanes,
    Editor,
    Terminal,
    Reveal,
    Help,
    Palette,
    About,
}
#[derive(Clone, Copy)]
pub(super) struct CommandSpec {
    pub id: CommandId,
    pub label: &'static str,
    keywords: &'static str,
    pub shortcut: Option<ShortcutId>,
}
macro_rules! command {
    ($id:ident, $label:literal, $keywords:literal, $shortcut:expr) => {
        CommandSpec {
            id: CommandId::$id,
            label: $label,
            keywords: $keywords,
            shortcut: $shortcut,
        }
    };
}
pub(super) const COMMANDS: &[CommandSpec] = &[
    command!(
        NextRepositoryTab,
        "Next repository tab",
        "tabs switch worktree",
        Some(ShortcutId::NextTab)
    ),
    command!(
        PreviousRepositoryTab,
        "Previous repository tab",
        "tabs switch worktree",
        Some(ShortcutId::PreviousTab)
    ),
    command!(
        CloseRepositoryTab,
        "Close repository tab",
        "tabs close",
        Some(ShortcutId::CloseTab)
    ),
    command!(
        MoveRepositoryTabLeft,
        "Move repository tab left",
        "tabs reorder earlier",
        None
    ),
    command!(
        MoveRepositoryTabRight,
        "Move repository tab right",
        "tabs reorder later",
        None
    ),
    command!(
        LocalWorkspaces,
        "Local workspaces and pinned repositories…",
        "tabs groups organize pin library",
        None
    ),
    command!(
        PinRepository,
        "Pin or unpin this repository",
        "tabs library favorite",
        None
    ),
    command!(
        GitHub,
        "GitHub pull requests…",
        "github collaboration PR review comment create draft account",
        None
    ),
    command!(
        EarlierGraphLanes,
        "Show earlier graph lanes",
        "graph horizontal left overflow",
        None
    ),
    command!(
        LaterGraphLanes,
        "Show later graph lanes",
        "graph horizontal right overflow",
        None
    ),
    command!(
        Projects,
        "Go to Projects",
        "hub home repositories recent",
        Some(ShortcutId::Projects)
    ),
    command!(
        Open,
        "Open a repository…",
        "folder project choose browse",
        Some(ShortcutId::Open)
    ),
    command!(
        History,
        "Go to History",
        "commits graph log",
        Some(ShortcutId::History)
    ),
    command!(
        Changes,
        "Go to Working Changes",
        "status stage unstage commit composer",
        Some(ShortcutId::Changes)
    ),
    command!(
        QuickOpen,
        "Quick Open File…",
        "find path source tracked",
        Some(ShortcutId::QuickOpen)
    ),
    command!(
        Compare,
        "Compare revisions…",
        "diff branch tag commit before after",
        Some(ShortcutId::Compare)
    ),
    command!(
        Branch,
        "Manage current branch…",
        "rename delete upstream tracking checkout switch",
        None
    ),
    command!(
        Worktrees,
        "Manage worktrees…",
        "linked create add remove open",
        None
    ),
    command!(
        Tags,
        "Browse and manage tags…",
        "annotate release create delete push",
        None
    ),
    // Named as the Working Changes Actions menu's item, which opens the
    // same Stashes.
    command!(
        Stashes,
        "Browse stashes…",
        "stash saved shelve apply pop drop restore",
        None
    ),
    command!(
        Reflog,
        "Browse reflog and recover a commit…",
        "lost undo history recovery",
        None
    ),
    command!(
        Rebase,
        "Review an interactive rebase…",
        "rewrite reorder squash fixup drop reword",
        None
    ),
    command!(
        RewriteReview,
        "Review rewritten branch series…",
        "rebase original changed commits publish force lease push",
        None
    ),
    command!(
        FileHistory,
        "Open selected file history",
        "path log rename previous revisions",
        None
    ),
    command!(
        Blame,
        "Show selected file attribution",
        "blame author annotate line history",
        None
    ),
    command!(
        Profiles,
        "Manage Git profiles…",
        "personal work author name email identity signing",
        None
    ),
    command!(
        Appearance,
        "Change appearance and themes…",
        "braden daylight light dark density interface code text size",
        None
    ),
    command!(
        Settings,
        "Open Settings",
        "preferences editor default branch configuration",
        Some(ShortcutId::Settings)
    ),
    command!(
        RecoveryDrafts,
        "Recover saved conflict and rebase drafts…",
        "unfinished text restart restore copy",
        None
    ),
    command!(
        Activity,
        "Show GitTurtle activity…",
        "operations errors cancelled outcomes",
        Some(ShortcutId::Activity)
    ),
    command!(
        Refresh,
        "Refresh local repository state",
        "reload status refs files",
        Some(ShortcutId::Refresh)
    ),
    command!(
        Search,
        "Search commits",
        "find message author email hash",
        Some(ShortcutId::Search)
    ),
    command!(
        Sidebar,
        "Toggle repository sidebar",
        "navigation branches references hide show",
        Some(ShortcutId::Sidebar)
    ),
    command!(
        ProjectPane,
        "Show or hide the project list",
        "projects pane groups library sidebar toggle",
        Some(ShortcutId::ProjectPane)
    ),
    command!(
        Editor,
        "Open repository in editor",
        "external ide code omarchy",
        None
    ),
    command!(
        Terminal,
        "Open repository in terminal",
        "shell console command line",
        None
    ),
    command!(
        Reveal,
        "Reveal repository in file manager",
        "finder folder external",
        None
    ),
    command!(
        Help,
        "Show keyboard shortcuts",
        "help keys commands",
        Some(ShortcutId::Help)
    ),
    command!(
        Palette,
        "Command Palette…",
        "search all commands actions",
        Some(ShortcutId::Palette)
    ),
    command!(
        About,
        "About GitTurtle",
        "version build native git client",
        None
    ),
];

#[derive(Clone, Default)]
struct Availability {
    repository: bool,
    workspace: bool,
    /// While the busy spell runs, why the commands it refuses wait
    /// (`GitTurtle::busy_reason`).
    busy: Option<&'static str>,
    /// Whether that spell is only a review being prepared or a wait for a
    /// dropped review's read, which Projects ends as it opens
    /// (`GitTurtle::projects_refusal`).
    projects_ends_busy: bool,
    /// Whether a write runs or the reload after it has yet to land, for
    /// which the Working Changes Actions menu's items wait
    /// (`GitTurtle::write_unsettled`, `GitTurtle::menu_wait`).
    unsettled: bool,
    /// Whether the shown tab is marked, whose branch would be the one from
    /// before (`GitTurtle::header_status`).
    unavailable: bool,
    /// Whether the shown tab's working status is not read yet, which says
    /// whether a branch is checked out (`GitTurtle::working_status_unread`).
    reading: bool,
    /// Whether it is not read and no read of it runs, as after a read that
    /// failed (`GitTurtle::working_status_not_read`).
    not_read: bool,
    branch: bool,
    file: bool,
    working_file: bool,
    working: bool,
    file_history: bool,
    blame: bool,
    integration: bool,
    profile_saving: bool,
    /// History in a window too narrow for its navigation.
    narrow_history: bool,
    /// The project pane on where the window holds it back
    /// (`GitTurtle::project_pane_drawn`).
    project_pane_held_back: bool,
    /// The GitHub panel can open now, whatever the busy spell says: a read
    /// of its own that closing cancelled is still draining
    /// (`GitTurtle::github_open_refusal`).
    github_opens: bool,
}
impl CommandId {
    fn workspace(self) -> bool {
        matches!(
            self,
            Self::QuickOpen
                | Self::Compare
                | Self::Branch
                | Self::Worktrees
                | Self::GitHub
                | Self::Tags
                | Self::Stashes
                | Self::Reflog
                | Self::Rebase
                | Self::RewriteReview
                | Self::FileHistory
                | Self::Blame
                | Self::Refresh
                | Self::Search
                | Self::Sidebar
                | Self::EarlierGraphLanes
                | Self::LaterGraphLanes
        )
    }
    fn selected_file(self) -> bool {
        matches!(self, Self::FileHistory | Self::Blame)
    }
    fn reason(self, context: &Availability) -> Option<&'static str> {
        // Going to History, Changes or Settings only reads, so it stays
        // available while a write runs, as their shortcuts and the header's
        // toggle do; the palette and the Linux Main Menu once refused them.
        // Projects waits: every action it offers starts another operation,
        // but not for a review being prepared or a dropped review's wait,
        // which it ends as it opens.
        if let Some(reason) = context.busy
            && !(self == Self::Projects && context.projects_ends_busy)
            && !(self == Self::GitHub && context.github_opens)
            && !matches!(
                self,
                Self::History
                    | Self::Changes
                    | Self::Settings
                    | Self::Appearance
                    | Self::Activity
                    | Self::Help
                    | Self::About
                    | Self::Palette
                    | Self::NextRepositoryTab
                    | Self::PreviousRepositoryTab
                    | Self::CloseRepositoryTab
                    | Self::LocalWorkspaces
                    | Self::MoveRepositoryTabLeft
                    | Self::MoveRepositoryTabRight
            )
        {
            return Some(reason);
        }
        if (self.workspace()
            || matches!(
                self,
                Self::History | Self::Changes | Self::Editor | Self::Terminal | Self::Reveal
            ))
            && !context.repository
        {
            return Some("Open a repository first");
        }
        if self.workspace() && !context.workspace {
            return Some("Return to the repository workspace first");
        }
        // Said as Branch says it below: a marked tab's repository is the one
        // read before the mark, which the tab can no longer open, so what
        // these open reads it or writes to it. They opened dialogs whose
        // reads named the replaced repository and whose every write Git core
        // then refused (security re-check of FU39-A, probe Q3). Recovery
        // drafts stay offered: they are the app's own data, read and kept in
        // its data directory, which the mark says nothing of (decided
        // 2026-11-11: an entry is offered only where it can act, and there
        // it can).
        if context.unavailable
            && matches!(
                self,
                Self::Tags
                    | Self::Stashes
                    | Self::Worktrees
                    | Self::Reflog
                    | Self::RewriteReview
                    | Self::Profiles
            )
        {
            return Some(crate::branch_actions::UNAVAILABLE_REASON);
        }
        // Browse stashes… waits where the Actions menu's item does, through
        // the reload after a write as well, and says so as the menu's
        // "Refreshing…" does.
        if self == Self::Stashes && context.unsettled {
            return Some(crate::workspace::REFRESHING_ROWS_REASON);
        }
        // Said as the branch picker says it, which opens no menu there.
        if self == Self::Branch && context.unavailable {
            return Some(crate::branch_actions::UNAVAILABLE_REASON);
        }
        // With no status read yet, Branch opens the branch the accepted
        // snapshot found checked out, as the picker's "Current branch
        // settings…" does, and where it found none says the read as Fetch,
        // Pull and Push do: it said "Check out a named local branch first"
        // while the picker named the branch, and then refused the branch it
        // named.
        if self == Self::Branch && context.reading && !context.branch {
            return Some(crate::workspace::READING_WORKING_TREE);
        }
        // With none read and none being read, as after a read that failed,
        // it says so as Fetch, Pull and Push do, the picker reading "Not
        // read": it said "Check out a named local branch first", as for a
        // detached HEAD.
        if self == Self::Branch && context.not_read {
            return Some(crate::workspace::NOT_READ_REASON);
        }
        if self == Self::Branch && !context.branch {
            return Some("Check out a named local branch first");
        }
        if self == Self::Rebase && context.integration {
            return Some("Finish or abort the current Git operation first");
        }
        // Said as Branch says it, the picker's menu that offers it opening
        // none there: a marked tab's history is the last one read, of a
        // repository the tab can no longer open.
        if self == Self::Rebase && context.unavailable {
            return Some(crate::branch_actions::UNAVAILABLE_REASON);
        }
        // Until the working status is read, which says whether an operation
        // is in progress, it waits for the read, as the picker's menu offers
        // no Edit local commits until then, and where none is read and none
        // is being read it says so, as Branch does: it was offered while the
        // status was read or after a read that failed (decided 2026-11-11).
        if self == Self::Rebase && context.reading {
            return Some(crate::workspace::READING_WORKING_TREE);
        }
        if self == Self::Rebase && context.not_read {
            return Some(crate::workspace::NOT_READ_REASON);
        }
        if self == Self::FileHistory && (context.working || context.file_history || !context.file) {
            return Some("Select a history file or open a tracked file first");
        }
        if self == Self::Blame && (context.blame || !(context.file || context.working_file)) {
            return Some("Select a file whose attribution is not already open");
        }
        if self == Self::Profiles && context.profile_saving {
            return Some("Wait for the profile save to finish");
        }
        if self == Self::Sidebar && context.narrow_history {
            return Some("Widen the window to show branches and worktrees");
        }
        None
    }
    /// What an available command's row says in its description's place, as
    /// an unavailable one's says why ([`Self::reason`]): the project list's
    /// command where the window holds the pane back, which it still turns
    /// on and off there.
    fn note(self, context: &Availability) -> Option<&'static str> {
        (self == Self::ProjectPane && context.project_pane_held_back)
            .then_some("Hidden: widen the window")
    }
    /// What a row's name and the status's reading say after the command: why
    /// the context cannot run it, after `separator`, or, where it runs, its
    /// note ([`Self::note`]), which never says "Unavailable", since the row
    /// still runs.
    fn spoken_state(self, separator: &str, context: &Availability) -> String {
        match (self.reason(context), self.note(context)) {
            (Some(reason), _) => format!("{separator}Unavailable: {reason}"),
            (None, Some(note)) => format!(". {note}"),
            (None, None) => String::new(),
        }
    }
}

/// The description line of `spec`'s row: why it is unavailable, a note
/// ([`CommandId::note`]), or what kind of command it is.
fn row_description(spec: &CommandSpec, context: &Availability) -> &'static str {
    spec.id
        .reason(context)
        .or_else(|| spec.id.note(context))
        .unwrap_or(if spec.id.selected_file() {
            "Selected file · opens its existing inspection workflow"
        } else if spec.id.workspace() {
            "Current repository · opens the existing workflow"
        } else {
            "Application action"
        })
}
fn matches(query: &str) -> Vec<usize> {
    if query.len() > 4096 {
        return Vec::new();
    }
    let query = query.trim().to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    let mut matches: Vec<_> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let name = spec.label.to_lowercase();
            let haystack = format!("{name} {}", spec.keywords);
            terms.iter().all(|term| haystack.contains(term)).then_some((
                if name.starts_with(&query) {
                    0
                } else if name.contains(&query) {
                    1
                } else {
                    2
                },
                index,
            ))
        })
        .collect();
    matches.sort_unstable();
    matches
        .into_iter()
        .map(|(_, index)| index)
        .take(40)
        .collect()
}
/// The position in `results` of the first command `availability` allows, or
/// 0 when it allows none.
fn first_available(results: &[usize], availability: &Availability) -> usize {
    results
        .iter()
        .position(|&index| COMMANDS[index].id.reason(availability).is_none())
        .unwrap_or(0)
}
#[derive(Clone, Default, PartialEq, Eq)]
struct FileScope {
    commit: Option<String>,
    file: Option<FileChange>,
    working: Option<(gitturtle_core::StatusEntry, gitturtle_core::ChangeArea)>,
}
impl FileScope {
    fn label(&self) -> String {
        if let Some((entry, _)) = &self.working {
            format!("Working file: {}", entry.path.display())
        } else if let Some(file) = &self.file {
            format!("Selected file: {}", file.path().display())
        } else {
            "No file selected".into()
        }
    }
}

#[derive(Clone)]
struct PaletteContext {
    availability: Availability,
    path: Option<PathBuf>,
    label: String,
}
impl PaletteContext {
    fn allows(&self, original_path: &Option<PathBuf>, command: CommandId) -> bool {
        original_path == &self.path && command.reason(&self.availability).is_none()
    }
}

#[derive(Default)]
struct Selection {
    index: usize,
    closed: bool,
}
impl Selection {
    fn move_by(&mut self, delta: i32, count: usize) {
        self.index = self
            .index
            .saturating_add_signed(delta as isize)
            .min(count.saturating_sub(1));
    }
    fn claim(&mut self, allowed: bool) -> bool {
        if self.closed || !allowed {
            return false;
        }
        self.closed = true;
        true
    }
}
impl GitTurtle {
    pub(super) fn menu_command_reason(
        &self,
        id: CommandId,
        window: &Window,
        cx: &App,
    ) -> Option<&'static str> {
        id.reason(&self.palette_availability(window, cx))
    }

    pub(super) fn run_menu_command(
        &mut self,
        id: CommandId,
        path: &Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if &self.path != path
            || id.reason(&self.palette_availability(window, cx)).is_some()
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        self.run_palette_command(id, window, cx);
    }

    fn palette_context(&self, window: &Window, cx: &App) -> PaletteContext {
        PaletteContext {
            availability: self.palette_availability(window, cx),
            path: self.path.clone(),
            label: self
                .repository
                .as_ref()
                .map(|repo| {
                    format!(
                        "{} · {}",
                        self.project_name(repo.path()),
                        if self.page == AppPage::Repository {
                            "current workspace"
                        } else {
                            "retained repository"
                        }
                    )
                })
                .unwrap_or_else(|| "Application · no repository open".into()),
        }
    }
    /// The branch Branch opens: the one the status read found checked
    /// out, none on a marked tab, whose branch is the one from before,
    /// and until the status is read, the one the accepted snapshot found
    /// (`unread_branch`), as the picker names it and its "Current branch
    /// settings…" opens it.
    fn palette_branch(&self) -> Option<&str> {
        (self.header_status())
            .and_then(|status| status.branch.as_deref())
            .or_else(|| self.unread_branch())
    }
    fn palette_file_scope(&self) -> FileScope {
        if self.mode == WorkspaceMode::Working {
            // A marked tab's selection indexes the status from before, which
            // Changes lists none of (`header_status`).
            return FileScope {
                working: self.working_selected.and_then(|(index, area)| {
                    self.header_status()?
                        .entries
                        .get(index)
                        .cloned()
                        .map(|entry| (entry, area))
                }),
                ..FileScope::default()
            };
        }
        FileScope {
            commit: self
                .selected_commit
                .and_then(|index| self.commits.get(index))
                .map(|commit| commit.oid.clone()),
            file: self
                .selected_file
                .and_then(|index| self.files.get(index))
                .cloned(),
            working: None,
        }
    }
    fn palette_availability(&self, window: &Window, cx: &App) -> Availability {
        let file_scope = self.palette_file_scope();
        Availability {
            repository: self.repository.is_some(),
            workspace: self.page == AppPage::Repository,
            busy: self.operation_busy.map(|_| self.busy_reason()),
            projects_ends_busy: self.projects_ends_busy(),
            unsettled: self.write_unsettled(),
            unavailable: self.shown_tab_unavailable(),
            reading: self.working_status_unread(),
            not_read: self.working_status_not_read(),
            branch: self.palette_branch().is_some(),
            file: file_scope.file.is_some(),
            working_file: file_scope.working.is_some(),
            working: self.mode == WorkspaceMode::Working,
            file_history: self.file_history.is_active(),
            blame: self.blame.is_visible(),
            // A marked tab's operation is the one read before the mark, as
            // the banner hides it (`integration`).
            integration: self.integration_state.is_some() && !self.shown_tab_unavailable(),
            profile_saving: self.profile_save_pending(),
            narrow_history: self.mode == WorkspaceMode::History
                && !self.navigation_fits(window, cx),
            project_pane_held_back: self.settings.project_pane
                && self.page != AppPage::Projects
                && !self.project_pane_fits(window),
            github_opens: self.github_open_refusal().is_none(),
        }
    }
    pub(super) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let return_focus = window.focused(cx);
        let file_scope = self.palette_file_scope();
        let context = self.palette_context(window, cx);
        let form =
            cx.new(|cx| Palette::new(owner, path, file_scope, context, return_focus, window, cx));
        #[cfg(test)]
        OPENED_PALETTE.with_borrow_mut(|opened| *opened = Some(form.downgrade()));
        let query_focus = form.read(cx).query.read(cx).focus_handle(cx);
        self.dialog_hints
            .register(&query_focus, shortcuts::DialogKeys::Palette);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let cancel = form.clone();
            let cancel_action = form.clone();
            let activate = form.clone();
            let submit = form.clone();
            // The palette is a menu of commands, so it floats as the menus
            // do: on the raised surface with their fitted edge rather than
            // on the page's own background. Read on every frame, so a theme
            // switch while it is open retints it with the menus.
            let surface = palette(cx);
            dialog
                .title("Command palette")
                .width(appearance::dialog_width(px(660.), window))
                .max_h(appearance::dialog_max_height(window))
                .bg(rgb(surface.raised()))
                .border_color(rgb(surface.raised_edge()))
                .child(form.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            shortcuts::DialogKeys::Palette
                                .dismiss_button("cancel-command-palette", false)
                                .on_click(move |_, window, cx| {
                                    cancel.update(cx, |form, cx| form.cancel(window, cx))
                                }),
                        )
                        .child({
                            // Disabled, it rests on `subtle`, which is the raised
                            // surface itself in most Omarchy dark palettes, and
                            // keeps its shape with the toolkit's disabled edge.
                            let disabled = !form.read(cx).can_activate();
                            button("run-palette-command", "Open command", "", true)
                                .debug_selector(|| "run-palette-command".into())
                                .disabled(disabled)
                                .on_click(move |_, window, cx| {
                                    activate.update(cx, |form, cx| form.activate(window, cx))
                                })
                        }),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.activate(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel_action.update(cx, |form, cx| form.cancel(window, cx));
                    false
                })
        });
        // Opening the dialog first focuses its shell. Set the query now: GPUI
        // redraws before the next key event, without running frame callbacks.
        // Deferring this focus loses characters typed before the next frame.
        query_focus.focus(window, cx);
        window.refresh();
    }
    pub(super) fn run_palette_command(
        &mut self,
        id: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match id {
            CommandId::NextRepositoryTab => self.cycle_repository_tab(1, window, cx),
            CommandId::PreviousRepositoryTab => self.cycle_repository_tab(-1, window, cx),
            CommandId::CloseRepositoryTab => {
                if let Some(index) = self.repository_tabs.active {
                    self.close_repository_tab(index, window, cx);
                }
            }
            CommandId::MoveRepositoryTabLeft => self.move_repository_tab(-1, window, cx),
            CommandId::MoveRepositoryTabRight => self.move_repository_tab(1, window, cx),
            CommandId::LocalWorkspaces => self.open_repository_library(window, cx),
            CommandId::PinRepository => self.toggle_repository_pin(window, cx),
            CommandId::Projects => self.show_projects(window, cx),
            CommandId::Open => self.choose_repository(&OpenRepository, window, cx),
            CommandId::History => self.show_history(window, cx),
            CommandId::Changes => self.show_working(window, cx),
            CommandId::QuickOpen => self.open_quick_file(window, cx),
            CommandId::Compare => self.open_revision_comparison(window, cx),
            CommandId::Branch => {
                if let Some(branch) = self.palette_branch().map(str::to_owned) {
                    self.open_contextual_branch(branch, false, window, cx);
                }
            }
            CommandId::Worktrees => self.open_worktree_manager(window, cx),
            CommandId::Tags => self.open_tags(window, cx),
            CommandId::Stashes => self.open_stashes(window, cx),
            CommandId::GitHub => self.open_github(window, cx),
            CommandId::Reflog => {
                self.open_reflog_browser(window, cx);
            }
            CommandId::Rebase => self.open_interactive_rebase(None, window, cx),
            CommandId::RewriteReview => self.open_rewrite_review(window, cx),
            CommandId::FileHistory => {
                if let Some(index) = self.selected_file {
                    self.open_file_history(index, window, cx);
                }
            }
            CommandId::Blame => {
                if let Some(index) = if self.mode == WorkspaceMode::Working {
                    self.working_selected.map(|(index, _)| index)
                } else {
                    self.selected_file
                } {
                    self.open_blame(index, window, cx);
                }
            }
            CommandId::Profiles => self.open_profiles(window, cx),
            CommandId::Appearance | CommandId::Settings => self.show_settings(window, cx),
            CommandId::RecoveryDrafts => self.open_recovery_drafts(window, cx),
            CommandId::Activity => self.open_activity(window, cx),
            CommandId::Refresh => self.refresh(&Refresh, window, cx),
            CommandId::Search => {
                self.show_history(window, cx);
                self.search(&Search, window, cx);
            }
            CommandId::Sidebar => self.toggle_sidebar(window, cx),
            CommandId::ProjectPane => self.toggle_project_pane(window, cx),
            CommandId::EarlierGraphLanes | CommandId::LaterGraphLanes => {
                self.shift_graph_lanes(id == CommandId::LaterGraphLanes);
                cx.notify();
            }
            CommandId::Editor => self.open_external_editor(window, cx),
            CommandId::Terminal => self.open_terminal(window, cx),
            CommandId::Reveal => {
                if let Some(repo) = &self.repository {
                    cx.reveal_path(repo.path());
                }
            }
            CommandId::Help => self.shortcut_help(window, cx),
            CommandId::Palette => self.open_command_palette(window, cx),
            CommandId::About => self.about(window, cx),
        }
        window.refresh();
    }
}
#[cfg(test)]
thread_local! {
    /// The command palette last opened, for a test to read its state.
    static OPENED_PALETTE: std::cell::RefCell<Option<WeakEntity<Palette>>> =
        const { std::cell::RefCell::new(None) };
}

/// Whether the selected row's name, rather than the status, carries a
/// change of context that changes the selected command's reading (a write
/// starting or ending, or the reload after it landing): on Linux Orca speaks
/// each rename of the list's selected item, the item the AT-SPI adapter's
/// change of selection made the one it presents, even with focus in the
/// query. A Fetch's end and its reload's end 57 ms later renamed Browse
/// stashes… twice, and Orca 51.1 spoke both renames, the second taking back
/// the first's "Wait for the file list to refresh", and then the status's
/// announcement of the same reading a third time (QA-96). So there the row's
/// name keeps the reading it had until the context rests
/// ([`shortcuts::COUNT_REST`]) and then takes the one the changes end on, one
/// rename, and the status announces nothing at a rest that renames it, unless
/// the query changed in the same rest ([`Palette::queried`]), or the selection
/// rests there on a command Orca may not have spoken ([`Palette::unheard`]),
/// whose rename it does not speak while it presents another row: Orca drops a
/// burst of the list's changes of selection as the person types, and ignores
/// a rename of a row that is not its locus of focus, so a query typed as a
/// Fetch ended, whose rest renamed the row it selected, was heard as nothing
/// in 2 of 3 runs (QA-97). On macOS nothing
/// shows VoiceOver speaking a rename of a row that focus is not on, so there
/// the status announces the rested reading (not yet checked natively).
///
/// A change of selection, by a key, a click or the query, is heard once.
/// On Linux the list's change of selection speaks the row it selects, the
/// command and any reason it waits ([`shortcuts::LIST_SPEAKS_SELECTION`]),
/// so a move announces nothing of the status's own, and the status's
/// announcement at the rest of a query that changed the selection says only
/// what the row does not, the command's place ("1 of 1", "2 of 3"), never
/// the command or the reason again, unless it is not sure Orca spoke that
/// change ([`Listed::spoken()`]), where it says the whole reading. Announcing the
/// whole reading there, it had Orca 51.1 speak "Go to History" and then
/// "Go to History · 1 of 1" (QA-105). A reading that no change of selection
/// led to stays whole: "No matching commands", a query that leaves the
/// command selected at another place, and a rest of the context. When the
/// selection rests, a move's rest included ([`Palette::list_moved`]), the
/// person has heard the command it rests on once: where the status is not
/// sure Orca spoke the list's last change of selection, the rest says the
/// whole reading, once, even where the status took it already, as after
/// Down and Up 50 ms apart, which left Orca's last word Down's command, or
/// Down held at 33 ms a repeat, which left the command it ended on unheard
/// ([`Palette::settle`]); where it is sure, nothing more. On macOS the status
/// announces the whole reading of every change, a move's at once (not yet
/// checked natively).
const ROW_CARRIES_CONTEXT: bool = shortcuts::LIST_SPEAKS_SELECTION;

/// A row's accessible name in `context`: its command and, where the context
/// cannot run it, why ("Browse stashes…. Unavailable: Wait for the file list
/// to refresh"), or, where it runs, its note ("Show or hide the project
/// list. Hidden: widen the window").
fn row_name(spec: &CommandSpec, context: &Availability) -> String {
    format!("{}{}", spec.label, spec.id.spoken_state(". ", context))
}

struct Palette {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    file_scope: FileScope,
    context: PaletteContext,
    _owner_subscription: Option<Subscription>,
    _bounds_subscription: Subscription,
    return_focus: Option<FocusHandle>,
    query: Entity<InputState>,
    results: Vec<usize>,
    selection: Selection,
    /// The status's whole reading ([`Self::reading`]): what it last
    /// announced, or took quietly. A move sets it at once; a change of the
    /// query, or of the context that changes the selected command's
    /// reading, sets it once they have rested ([`Self::settle`]).
    status: SharedString,
    /// What the status's node says of that reading, its accessible name and
    /// value: the whole reading, or, on Linux at the rest after a change of
    /// selection that Orca spoke ([`Listed::spoken()`]), the command's place
    /// alone ([`Self::place`]), which the row Orca spoke, naming the command
    /// and any reason it waits, does not say. Where that equals what the
    /// node said before, as
    /// "1 of 1" after another query's "1 of 1", the node says nothing new
    /// and the row alone is heard.
    status_name: SharedString,
    /// The list's last change of its selected command.
    listed: Listed,
    /// Whether the status last changed quietly, for a move the list speaks
    /// itself ([`shortcuts::LIST_SPEAKS_SELECTION`], Linux), or for a change
    /// of context the selected row's name carries ([`ROW_CARRIES_CONTEXT`]):
    /// it is no live region until it next announces, so the screen reader
    /// does not say the command a second time.
    quiet: bool,
    /// Where the selected row's name carries a change of context
    /// ([`ROW_CARRIES_CONTEXT`], Linux), the command whose row's name still
    /// reads the context it had before a change of it that changed that
    /// command's reading while it was selected, and that context, until the
    /// change rests: the rest ([`Self::settle`]) drops it, renaming the row
    /// once to the reading the changes end on. A move to another row keeps
    /// it on the row it leaves ([`Self::selection_moved`]), and a move to
    /// that row drops it. Only that row's name reads it; the row draws, and
    /// Enter acts on, the live context.
    held: Option<(usize, Availability)>,
    /// Whether the query changed since the last rest ([`Self::settle`]), which
    /// that rest announces on the status even where it renames the selected
    /// row ([`ROW_CARRIES_CONTEXT`]).
    queried: bool,
    /// The one rest the next reading of the query or the context waits for
    /// ([`Self::settle`]): replaced by each change to either, so only the
    /// last one runs, and dropped as the palette closes.
    settling: Option<Task<()>>,
    /// Whether the rest pending ([`Self::settling`]) waits for a reading of
    /// the query or the context, rather than only for the selection to rest
    /// after a move ([`Self::list_moved`]).
    rest_reads: bool,
    /// Whether the selection changed since the person last surely heard the
    /// command it rests on, from the list or the status: where the list
    /// speaks its selection ([`shortcuts::LIST_SPEAKS_SELECTION`], Linux),
    /// the rest that follows says the whole reading again unless Orca surely
    /// spoke the last change ([`Self::settle`]).
    unheard: bool,
    /// How many times the status's node was replaced to announce again the
    /// reading its name already says ([`Self::settle`]): AccessKit's
    /// adapters announce a live node only as it is added or its name
    /// changes.
    status_node: usize,
    scroll: UniformListScrollHandle,
    /// The rows the list last asked its rows for in this frame
    /// ([`Self::note_drawn_rows`]).
    drawn_rows: Option<std::ops::Range<usize>>,
    error: Option<String>,
    _subscription: Subscription,
}
impl Palette {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        file_scope: FileScope,
        context: PaletteContext,
        return_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search commands, such as profiles, blame, or themes…")
        });
        // Return runs the selected command through the dialog's default
        // action alone (`Self::show`'s `on_ok`), once per press, as the key
        // goes down, never on a held key's repeats, under X11 too. The
        // query's own `PressEnter`, which it sends for every Enter key-down,
        // ran it as well, so a command a write refused ran on the first
        // repeat after the write ended.
        let subscription = cx.subscribe(&query, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let was = this.selected_command();
                this.results = matches(&this.query.read(cx).value());
                this.highlight_first_available();
                if this.selected_command() != was {
                    this.selection_moved();
                }
                this.error = None;
                this.queried = true;
                this.settle(true, cx);
                cx.notify();
            }
        });
        // Notifications run after the owner update has ended. Rendering only
        // reads this owned snapshot because the dialog layer itself is rendered
        // while GitTurtle is mutably borrowed.
        let owner_subscription = owner.upgrade().map(|owner| {
            cx.observe_in(&owner, window, |this, owner, window, cx| {
                this.follow_context(&owner, window, cx);
            })
        });
        // A resize notifies no entity, and what a row says can follow the
        // window's size: Toggle Sidebar's reason, the project list's note.
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            if let Some(owner) = this.owner.upgrade() {
                this.follow_context(&owner, window, cx);
            }
        });
        let mut palette = Self {
            owner,
            path,
            file_scope,
            context,
            _owner_subscription: owner_subscription,
            _bounds_subscription: bounds_subscription,
            return_focus,
            query,
            results: matches(""),
            selection: Selection::default(),
            status: SharedString::default(),
            status_name: SharedString::default(),
            listed: Listed::opened(),
            quiet: false,
            held: None,
            queried: false,
            settling: None,
            rest_reads: false,
            unheard: false,
            status_node: 0,
            scroll: UniformListScrollHandle::new(),
            drawn_rows: None,
            error: None,
            _subscription: subscription,
        };
        palette.highlight_first_available();
        palette.status = palette.reading();
        palette.status_name = palette.status.clone();
        palette
    }
    /// Takes the context as the application has it now, while the palette
    /// is open. A reading the context changes, as a write starts or ends, is
    /// heard once it rests, as the query's is. Announced at once, a Fetch's
    /// end and its reload's end 33 ms later were two announcements, and Orca
    /// 51.1's spam filter dropped the second, the command becoming available
    /// (QA-95). Where the selected row's name carries it (Linux), that name
    /// keeps the reading it had until the rest, and the status takes it
    /// quietly then, unless the query changed too (`queried`). The hold
    /// moves to the selected row; a row a move left that still held its name
    /// takes its live reading now, a frame after the move reported the
    /// list's change of selection.
    fn follow_context(
        &mut self,
        owner: &Entity<GitTurtle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selection.closed {
            return;
        }
        let was = self.reading();
        let previous = std::mem::replace(
            &mut self.context,
            owner.read(cx).palette_context(window, cx),
        );
        if self.reading() != was {
            if let Some(command) = self.selected_command()
                && ROW_CARRIES_CONTEXT
                && self.held.as_ref().is_none_or(|(held, _)| *held != command)
            {
                self.held = Some((command, previous.availability));
            }
            self.settle(true, cx);
        }
        cx.notify();
    }
    /// Highlights the first result the current context can run, or the first
    /// result when it can run none, and shows the list from its top with
    /// that row in view. A disabled row draws in the faintest text on screen,
    /// so it is a poor place for the highlight to start; Up/Down still reach
    /// every row, disabled ones included.
    fn highlight_first_available(&mut self) {
        self.selection.index = first_available(&self.results, &self.context.availability);
        // Strictly, so the list starts from its top whatever it showed
        // before: the bottom placement scrolls only when the row lies below
        // the first screen.
        self.scroll
            .scroll_to_item_strict(self.selection.index, ScrollStrategy::Bottom);
    }
    /// What the status reads for the selected result: its command, its
    /// place among the results and, where the context cannot run it, why
    /// ("Go to History · 2 of 3"), or, where it runs, its note, or that
    /// nothing matches.
    fn reading(&self) -> SharedString {
        self.selected_command()
            .zip(self.place())
            .map(|(index, place)| {
                format!(
                    "{} · {place}{}",
                    COMMANDS[index].label,
                    COMMANDS[index]
                        .id
                        .spoken_state(" · ", &self.context.availability)
                )
                .into()
            })
            .unwrap_or_else(|| "No matching commands".into())
    }
    /// The selected result's place among the results ("2 of 3"): all its
    /// reading says that the selected row's name does not, which names the
    /// command and why the context cannot run it.
    fn place(&self) -> Option<String> {
        self.selected_command()
            .map(|_| format!("{} of {}", self.selection.index + 1, self.results.len()))
    }
    /// The selected result's command, by its place in the registry.
    fn selected_command(&self) -> Option<usize> {
        self.results.get(self.selection.index).copied()
    }
    /// The query moved the selection to another command: the rest its change
    /// begins follows it ([`Listed::changed`]).
    fn selection_moved(&mut self) {
        self.listed.changed();
        self.unheard = true;
        self.reached();
    }
    /// The selection changed other than by the query, by a key, a click or
    /// the selected row coming back into the list ([`Listed::moved`]): the
    /// rest pending then follows it, where one is, and a later rest does
    /// not. A move with none pending is heard from the list, and a query
    /// that keeps its command selected long after is a reading no change of
    /// selection led to, read whole: counted toward that later rest, a click
    /// on a command the context cannot run and, 5 s on, a query that kept it
    /// selected at another place rested on "1 of 1" alone.
    ///
    /// Where the list speaks its selection ([`shortcuts::LIST_SPEAKS_SELECTION`],
    /// Linux), the move begins the selection's rest, which each change
    /// replaces ([`Self::settle`]), so the person hears the command the
    /// selection rests on once: Orca drops a change of selection less than
    /// 100 ms after the one before, so Down and Up within that window left
    /// Orca's last word Down's command, and Down held at 33 ms a repeat the
    /// first command alone, the one the selection rested on unheard. A rest
    /// begun by moves alone is none pending for the move after
    /// ([`Self::rest_reads`]).
    fn list_moved(&mut self, cx: &mut Context<Self>) {
        self.listed
            .moved(self.settling.is_some() && self.rest_reads);
        self.unheard = true;
        self.reached();
        if shortcuts::LIST_SPEAKS_SELECTION {
            self.settle(false, cx);
        }
    }
    /// A key or a click moved the selection ([`Self::list_moved`]): the
    /// status takes the move's whole reading at once, quietly where the list
    /// speaks the move ([`shortcuts::LIST_SPEAKS_SELECTION`]). A click on a
    /// command the context cannot run, which leaves the palette open on it
    /// with why, once left the status on the reading before.
    fn take_move(&mut self, cx: &mut Context<Self>) {
        self.list_moved(cx);
        self.status = self.reading();
        self.status_name = self.status.clone();
        self.quiet = shortcuts::LIST_SPEAKS_SELECTION;
    }
    /// The selection reached a command, the list's change ([`Self::listed`]),
    /// timed by a frame after the one that draws it. The row it reaches
    /// reads its live reading at once, a row holding its name
    /// ([`Self::held`]) included, while the row it leaves keeps the name it
    /// holds until the rest: the AT-SPI adapter reports a row's rename as it
    /// walks a frame's changes and the list's change of selection after
    /// them, so a row renamed in the move's frame was still the item Orca
    /// presents as its rename arrived, and would be spoken before the row
    /// reached. Renamed at the rest, it is no longer selected, and Orca does
    /// not speak it.
    fn reached(&mut self) {
        if self
            .held
            .as_ref()
            .is_some_and(|(command, _)| Some(*command) == self.selected_command())
        {
            self.held = None;
        }
    }
    /// Notes whether this frame draws the selected command's row, `drawn`,
    /// as the list lays out its rows: a selected row coming back into the
    /// list, as the wheel, a resize or a text size can bring it, is a change
    /// of its selection ([`Listed::laid_out`]), timed by this frame and the
    /// next.
    fn note_selected_row(&mut self, drawn: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.listed.laid_out(self.selected_command(), drawn) {
            self.list_moved(cx);
            self.listed.time(window, cx, |this| &mut this.listed);
        }
    }
    /// Notes, once the list has laid out this frame's rows, whether it drew
    /// the selected command's row ([`Self::note_selected_row`]). Each frame
    /// the list asks for its first row alone, in its layout and again as it
    /// prepaints, to measure it (`UniformList::measure_item`), and then for
    /// the rows it draws, so the last rows it asked for are the ones drawn,
    /// whatever their range ([`Self::drawn_rows`]). Read from a measuring
    /// call, a selected row scrolled out of the list read as leaving and
    /// coming back on every frame; told apart by their range, the drawn
    /// rows of a list one row tall, its first row alone at its top, read as
    /// a measuring call, so the second row's return there went untimed, and
    /// a key 50 ms after it, whose change Orca drops, was taken as spoken.
    fn note_drawn_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(rows) = self.drawn_rows.take() {
            self.note_selected_row(rows.contains(&self.selection.index), window, cx);
        }
    }
    /// Announces the selected command's reading once the query and the
    /// context have rested for [`shortcuts::COUNT_REST`] since this change,
    /// unless it is what the status last said: each change to either drops
    /// the rest the one before it began, so a burst of keys, or of context
    /// changes such as a write's end and its reload's, announces only the
    /// reading it ends on, once, after the echo of its last key, as
    /// Keyboard Shortcuts' search does. Announced at each key, typing "go"
    /// 60 ms a key, Orca 51.1 spoke "GitHub pull requests… · 1 of 24" and
    /// dropped "Go to Projects · 1 of 3", sent 51 ms after it: its spam
    /// filter drops an event of a type the application sent less than
    /// 100 ms before ("Ignoring object:announcement due to multiple
    /// instances in short time"), so the last reading heard was stale
    /// (QA-94); announced at each change of context, a Fetch's end and its
    /// reload's 33 ms later lost Browse stashes… becoming available
    /// (QA-95).
    ///
    /// Where the selected row's name carries a change of context
    /// ([`ROW_CARRIES_CONTEXT`], Linux), the rest renames that row once,
    /// from the reading it held ([`Self::held`]) to the one the changes end
    /// on, and at a rest of the context alone that renames it the status
    /// takes its reading quietly: the rename carries the reading, and the
    /// status announcing it as well was heard a second time (QA-96), so such
    /// a rest gives up the command's place. A rest the query's change joined
    /// ([`Self::queried`]) announces as without it, the row's rename beside
    /// it: Orca drops a burst of the list's changes of selection as the
    /// person types and ignores a rename of a row that is not its locus of
    /// focus, so taken quietly, a query typed as a Fetch ended was heard as
    /// nothing in 2 of 3 runs (QA-97). Where Orca did take the row the query
    /// selected as its locus, it speaks the rename and the status both.
    ///
    /// Where the list speaks a change of its selection itself
    /// ([`shortcuts::LIST_SPEAKS_SELECTION`], Linux) and Orca spoke the one
    /// this rest follows ([`Listed::spoken()`]), the status says only the
    /// command's place ([`Self::status_name`]): the row Orca spoke says the
    /// command and any reason it waits, and the whole reading said them again
    /// (QA-105).
    ///
    /// The selection's rest, which a move begins where the list speaks its
    /// selection ([`Self::list_moved`]), is this same rest: there the person
    /// has heard the command the selection rests on once. Where Orca surely
    /// spoke the last change of selection, nothing more is said; otherwise
    /// the status says the whole reading, once, even where the reading is
    /// what it last took, as at Down and Up 50 ms apart, after which Orca
    /// had last said Down's command, or where the rest renames the selected
    /// row, whose rename Orca does not speak while it presents another row
    /// (QA-97). A wrong guess says the command twice, never leaves it
    /// unheard. A reading the status's name already says is announced by a
    /// fresh node ([`Self::status_node`]). `reads` says whether a change of
    /// the query or the context asks for this rest, rather than a move
    /// ([`Self::rest_reads`]).
    fn settle(&mut self, reads: bool, cx: &mut Context<Self>) {
        self.rest_reads |= reads;
        let rest = cx.background_executor().timer(shortcuts::COUNT_REST);
        self.settling = Some(cx.spawn(async move |this, cx| {
            rest.await;
            let _ = this.update(cx, |this, cx| {
                this.settling = None;
                this.rest_reads = false;
                let queried = std::mem::take(&mut this.queried);
                let held = this.held.take();
                let renames_row = held.as_ref().is_some_and(|(command, held)| {
                    let spec = COMMANDS[*command];
                    this.selected_command() == Some(*command)
                        && row_name(&spec, held) != row_name(&spec, &this.context.availability)
                });
                let reading = this.reading();
                let listed = this.listed.take_unread();
                // Orca surely spoke the list's last change of selection.
                let spoken = shortcuts::LIST_SPEAKS_SELECTION && this.listed.spoken();
                // The selection rests on a command the person may not have
                // heard.
                let unheard = std::mem::take(&mut this.unheard)
                    && shortcuts::LIST_SPEAKS_SELECTION
                    && !spoken;
                if reading != this.status {
                    this.status_name = match this.place() {
                        Some(place) if listed && spoken => place.into(),
                        _ => reading.clone(),
                    };
                    this.status = reading;
                    this.quiet = renames_row && !queried && !unheard;
                    cx.notify();
                } else if unheard {
                    if this.status_name == reading {
                        this.status_node += 1;
                    }
                    this.status_name = reading;
                    this.quiet = false;
                    cx.notify();
                } else if held.is_some() {
                    cx.notify();
                }
            });
        }));
    }
    fn can_activate(&self) -> bool {
        !self.selection.closed
            && self
                .results
                .get(self.selection.index)
                .is_some_and(|index| self.context.allows(&self.path, COMMANDS[*index].id))
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.claim(true) {
            return;
        }
        // A reading still resting is never announced.
        self.settling = None;
        window.close_dialog(cx);
        window.refresh();
        if let Some(focus) = &self.return_focus {
            focus.focus(window, cx);
        }
    }
    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.closed {
            return;
        }
        let Some(index) = self.results.get(self.selection.index).copied() else {
            return;
        };
        let command = COMMANDS[index].id;
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path != self.path {
                self.error =
                    Some("The repository changed. Close and reopen the command palette.".into());
                return;
            }
            if command.selected_file() && owner.palette_file_scope() != self.file_scope {
                self.error = Some("The selected file changed. Close and reopen the palette to review its current target.".into());
                return;
            }
            if let Some(reason) = command.reason(&owner.palette_availability(window, cx)) {
                self.error = Some(reason.into());
                return;
            }
            if !self.selection.claim(true) {
                return;
            }
            self.settling = None;
            window.close_dialog(cx);
            window.refresh();
            if let Some(focus) = &self.return_focus {
                focus.focus(window, cx);
            }
            owner.run_palette_command(command, window, cx);
        });
        cx.notify();
    }
}
impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // This frame draws the list's last change of selection, which the
        // AT-SPI adapter reports once it is drawn (`Listed::time`).
        self.listed.time(window, cx, |this| &mut this.listed);
        self.drawn_rows = None;
        let p = palette(cx);
        let count = self.results.len();
        let desired_height = appearance::ui_size(if count == 0 {
            78.
        } else {
            54. * count.min(7) as f32 + 2.
        });
        let available_height = (window.viewport_size().height - appearance::ui_size(300.))
            .max(appearance::ui_size(54.));
        let context = self.context.availability.clone();
        let selected_file_command = self
            .results
            .get(self.selection.index)
            .is_some_and(|index| COMMANDS[*index].id.selected_file());
        let scope = self.context.label.clone();
        let scope = if selected_file_command {
            format!("{scope} · {}", self.file_scope.label())
        } else {
            scope
        };
        let list = uniform_list(
            "command-palette-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                // The last rows asked for this frame are the ones the list
                // draws (`note_drawn_rows`).
                this.drawn_rows = Some(range.clone());
                let context = this.context.availability.clone();
                range
                    .map(|index| {
                        let colors = palette(cx);
                        let spec = COMMANDS[this.results[index]];
                        let disabled = spec.id.reason(&context);
                        let selected = this.selection.index == index;
                        let shortcut = spec.shortcut.map(shortcuts::label).unwrap_or_default();
                        // The row's ID is the command it shows, its place in
                        // the whole registry, rather than its place among the
                        // results, so the window's tree keeps one node for each
                        // command across searches and a search that changes the
                        // results takes nodes away and adds others rather than
                        // renaming them. Keyed by place, a query changed after
                        // a move renamed the node at each place, and Orca would
                        // speak the command now in it, "not selected", as it did
                        // in Keyboard Shortcuts' list (QA-92). Its debug
                        // selector is still its place, which tests read.
                        // The held row's name reads the context a change
                        // still resting left it in (`held`, Linux); what the
                        // row draws, and whether it acts, follow the live one.
                        let named = match &this.held {
                            Some((command, held)) if *command == this.results[index] => held,
                            _ => &context,
                        };
                        let row = div()
                            .id(("palette-command", this.results[index]))
                            .debug_selector(move || format!("palette-command-{index}"))
                            .role(Role::ListBoxOption)
                            .aria_label(row_name(&spec, named))
                            .aria_selected(selected)
                            .aria_position_in_set(index + 1)
                            .aria_size_of_set(this.results.len())
                            .w_full()
                            .h(crate::appearance::ui_size(54.))
                            // The list's 1 px border and this padding start
                            // the row's text where the query field above
                            // starts its own, at every interface size; the
                            // kit's 0.75 rem put it 3.5 px right of the
                            // field's text at 18 pt.
                            .px(crate::appearance::FIELD_TEXT_PADDING)
                            .py_2()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap_1()
                            .relative()
                            // A resting row shows the raised surface beneath
                            // it, as a menu's rows do; the highlight is the
                            // keyboard's cursor and marked as such.
                            .when(selected, |row| {
                                row.bg(rgb(colors.selected))
                                    .child(appearance::cursor_marker(colors))
                            })
                            .text_color(if disabled.is_some() {
                                Theme::global(cx).disabled_foreground()
                            } else {
                                rgb(palette(cx).text).into()
                            })
                            .when(disabled.is_none(), |element| {
                                element
                                    .cursor_pointer()
                                    .hover(move |style| style.bg(rgb(colors.hover)))
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            // An unavailable command's title is
                                            // regular, so its weight marks it as
                                            // well as the disabled color.
                                            .font_weight(if disabled.is_some() {
                                                FontWeight::NORMAL
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .child(spec.label)
                                            .children(appearance::text_probe(move || {
                                                format!("palette-command-title-{index}")
                                            })),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(crate::appearance::ui_text(11.))
                                            .text_color(if disabled.is_some() {
                                                Theme::global(cx).disabled_foreground()
                                            } else {
                                                rgb(palette(cx).muted).into()
                                            })
                                            .child(shortcut),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .text_color(rgb(palette(cx).muted))
                                    .truncate()
                                    .child(row_description(&spec, &context)),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let moved = this.selection.index != index;
                                this.selection.index = index;
                                if moved {
                                    this.take_move(cx);
                                }
                                this.activate(window, cx);
                            }));
                        // Recorded for tests, which read the rows the window's
                        // tree holds.
                        native_accessibility::recorded(row).into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let reason = self
            .results
            .get(self.selection.index)
            .and_then(|index| COMMANDS[*index].id.reason(&context));
        div()
            .id("command-palette-form")
            .debug_selector(|| "command-palette-form".into())
            .key_context("GitTurtlePalette")
            .flex()
            .flex_col()
            .gap_3()
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                    this.cancel(window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| {
                this.cancel(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(
                |this, _: &gpui_kit::component::dialog::Cancel, window, cx| {
                    this.cancel(window, cx);
                    cx.stop_propagation();
                },
            ))
            .on_action(cx.listener(|this, _: &ShowCommandPalette, window, cx| {
                this.query.read(cx).focus_handle(cx).focus(window, cx);
                cx.stop_propagation();
            }))
            .on_action(|_: &QuickOpenFile, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowProjects, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowHistory, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowChanges, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowSettings, _, cx| cx.stop_propagation())
            .on_action(|_: &Refresh, _, cx| cx.stop_propagation())
            .on_action(|_: &CompareRevisions, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowActivity, _, cx| cx.stop_propagation())
            .on_action(|_: &OpenRepository, _, cx| cx.stop_propagation())
            .on_action(|_: &BackHistory, _, cx| cx.stop_propagation())
            .on_action(|_: &ToggleSidebar, _, cx| cx.stop_propagation())
            .on_action(|_: &Search, _, cx| cx.stop_propagation())
            .on_action(|_: &ShortcutHelp, _, cx| cx.stop_propagation())
            .on_action(|_: &RevealRepository, _, cx| cx.stop_propagation())
            .on_action(|_: &OpenEditor, _, cx| cx.stop_propagation())
            .on_action(|_: &OpenTerminal, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let delta = match event.keystroke.key.as_str() {
                    "down" => 1,
                    "up" => -1,
                    _ => return,
                };
                let was = this.selection.index;
                this.selection.move_by(delta, this.results.len());
                if this.selection.index != was {
                    this.take_move(cx);
                }
                this.scroll
                    .scroll_to_item(this.selection.index, ScrollStrategy::Center);
                cx.stop_propagation();
                cx.notify();
            }))
            // The status draws nothing and reads the selected command, its
            // place and why it cannot run, as a move left it at once and as
            // the query or the context left it once they have rested
            // (`settle`). Where the list speaks a moved
            // selection itself (`shortcuts::LIST_SPEAKS_SELECTION`, Linux), a
            // move changes it quietly, as no live region (`quiet`): Orca 51.1
            // spoke each Down twice, 41 ms apart, the status's "Go to History
            // · 2 of 3" and the list's "Go to History" (QA-94); there a rest
            // of the context alone that renames the selected row, whose name
            // carries a change of context (`ROW_CARRIES_CONTEXT`), changes it
            // quietly too, while a rest the query's change joined announces
            // (`queried`, QA-97), and a rest after a change of selection
            // Orca spoke says only the place (`status_name`, QA-105).
            // AT-SPI announces a live node's name and
            // macOS's adapter its value alone, so both carry the reading, as
            // Keyboard Shortcuts' status's do: named alone, it was announced
            // on Linux only.
            .child({
                let reading = self.status_name.clone();
                native_accessibility::static_text(
                    div()
                        .id(("command-palette-current-result", self.status_node))
                        .role(Role::Status)
                        .live_region(
                            if self.quiet {
                                gpui::accesskit::Live::Off
                            } else {
                                gpui::accesskit::Live::Polite
                            },
                            false,
                        )
                        .aria_value(reading.clone())
                        .h(px(1.))
                        .overflow_hidden(),
                    reading,
                )
            })
            .child(
                Input::new(&self.query)
                    .aria_label("Search app commands")
                    .cleanable(true),
            )
            .child(
                native_accessibility::static_text(div().id("command-palette-scope"), scope.clone())
                    .tooltip({
                        let scope = scope.clone();
                        move |window, cx| Tooltip::new(scope.clone()).build(window, cx)
                    })
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .truncate()
                    .child(scope),
            )
            .child(crate::native_accessibility::clips_children(
                div()
                    .id("command-palette-results")
                    .debug_selector(|| "command-palette-results".into())
                    .role(Role::ListBox)
                    .aria_label("Matching commands")
                    .h(desired_height.min(available_height))
                    .overflow_hidden()
                    .border_1()
                    // Edged as a menu's separators are, against the raised
                    // surface it lies on.
                    .border_color(rgb(p.raised_edge()))
                    .rounded(px(6.))
                    .when(count > 0, |element| {
                        let view = cx.weak_entity();
                        element.child(list).child(
                            // Prepainted after the list, as it has asked for
                            // the rows it draws.
                            gpui_kit::canvas(
                                move |_, window, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        this.note_drawn_rows(window, cx);
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_0(),
                        )
                    })
                    .when(count == 0, |element| {
                        element.child(
                            native_accessibility::text(
                                "command-palette-empty",
                                if self.query.read(cx).value().len() > 4096 {
                                    "The search is too long. Use up to 4,096 bytes."
                                } else {
                                    "No matching commands. Try profiles, compare, branches, or themes."
                                },
                            )
                            .p_4()
                            .text_color(rgb(p.muted)),
                        )
                    }),
            ))
            .child(
                shortcuts::dialog_help(
                    "command-palette-help",
                    reason.map_or_else(help_keys, str::to_owned),
                )
                .text_size(crate::appearance::ui_text(11.))
                .text_color(rgb(p.muted)),
            )
            .children(self.error.as_ref().map(|error| {
                native_accessibility::text("command-palette-error", error.clone())
                    .text_color(rgb(p.warning))
            }))
    }
}

/// The palette's inline help where nothing keeps it from opening a command:
/// the keys its list takes, in the platform's key labels.
fn help_keys() -> String {
    format!(
        "↑ / ↓ select · {} opens · {}",
        shortcuts::ENTER,
        shortcuts::DialogKeys::Palette.escape_help()
    )
}

#[cfg(test)]
mod tests {
    use super::{Availability, COMMANDS, CommandId, PaletteContext, Selection, matches};
    use std::path::PathBuf;
    #[test]
    fn search_finds_synonyms_and_respects_all_terms_and_bounds() {
        for (query, expected) in [
            ("personal", CommandId::Profiles),
            ("squash", CommandId::Rebase),
            ("blame", CommandId::Blame),
            ("light dark", CommandId::Appearance),
            ("lost commit", CommandId::Reflog),
            ("stash", CommandId::Stashes),
            ("shelve", CommandId::Stashes),
        ] {
            assert!(
                matches(query)
                    .iter()
                    .any(|index| COMMANDS[*index].id == expected)
            );
        }
        assert!(matches("profiles totallyunrelatedterm").is_empty());
        assert!(matches(&"a".repeat(4097)).is_empty());
        assert!(matches("").len() <= 40);
    }
    #[test]
    fn availability_distinguishes_hub_busy_selection_and_recovery_states() {
        let mut context = Availability::default();
        assert!(CommandId::Profiles.reason(&context).is_none());
        assert!(CommandId::QuickOpen.reason(&context).is_some());
        assert_eq!(
            CommandId::Stashes.reason(&context),
            Some("Open a repository first")
        );
        context.repository = true;
        assert!(CommandId::History.reason(&context).is_none());
        assert!(CommandId::QuickOpen.reason(&context).is_some());
        // Browse stashes… is available where the Working Changes Actions
        // menu's item is: on the repository page while no write runs.
        assert_eq!(
            CommandId::Stashes.reason(&context),
            Some("Return to the repository workspace first")
        );
        context.workspace = true;
        assert!(CommandId::QuickOpen.reason(&context).is_none());
        assert_eq!(CommandId::Stashes.reason(&context), None);
        // Through the reload after a write too, as the menu's item says
        // "Refreshing…".
        context.unsettled = true;
        assert_eq!(
            CommandId::Stashes.reason(&context),
            Some(crate::workspace::REFRESHING_ROWS_REASON)
        );
        assert!(CommandId::QuickOpen.reason(&context).is_none());
        context.unsettled = false;
        assert!(CommandId::Blame.reason(&context).is_some());
        context.working_file = true;
        assert!(CommandId::Blame.reason(&context).is_none());
        context.integration = true;
        assert!(CommandId::Rebase.reason(&context).is_some());
        context.busy = Some(crate::workspace::BUSY_REASON);
        assert!(CommandId::Profiles.reason(&context).is_some());
        assert_eq!(
            CommandId::Stashes.reason(&context),
            Some(crate::workspace::BUSY_REASON)
        );
        assert!(CommandId::Activity.reason(&context).is_none());
        // Page navigation only reads, so a write leaves it available, while
        // Projects, whose every action starts another operation, waits.
        for page in [
            CommandId::History,
            CommandId::Changes,
            CommandId::Settings,
            CommandId::Appearance,
        ] {
            assert_eq!(page.reason(&context), None, "{page:?} during a write");
        }
        assert_eq!(
            CommandId::Projects.reason(&context),
            Some(crate::workspace::BUSY_REASON)
        );
    }
    /// "Review an interactive rebase…" is offered only where it can act:
    /// while the working status, which says whether an operation is in
    /// progress, is being read it says the read, and where none is read and
    /// none is being read it says so, as Branch does, the mark's words
    /// before either; it was offered in both. On a marked tab, "Manage Git
    /// profiles…" says the mark, as the other repository entries do, while
    /// the recovery drafts, the app's own data, stay offered (decided
    /// 2026-11-11).
    #[test]
    fn entries_are_offered_only_where_they_can_act() {
        let mut context = Availability {
            repository: true,
            workspace: true,
            ..Availability::default()
        };
        assert_eq!(CommandId::Rebase.reason(&context), None, "at rest");
        context.reading = true;
        assert_eq!(
            CommandId::Rebase.reason(&context),
            Some(crate::workspace::READING_WORKING_TREE),
            "while the status is read"
        );
        context.reading = false;
        context.not_read = true;
        assert_eq!(
            CommandId::Rebase.reason(&context),
            Some(crate::workspace::NOT_READ_REASON),
            "with none read and none being read"
        );
        context.unavailable = true;
        assert_eq!(
            CommandId::Rebase.reason(&context),
            Some(crate::branch_actions::UNAVAILABLE_REASON),
            "the mark's words first"
        );
        assert_eq!(
            CommandId::Profiles.reason(&context),
            Some(crate::branch_actions::UNAVAILABLE_REASON),
            "Manage Git profiles… on a marked tab"
        );
        assert_eq!(
            CommandId::RecoveryDrafts.reason(&context),
            None,
            "the recovery drafts on a marked tab"
        );
    }
    #[test]
    fn selection_is_bounded_and_enter_or_cancel_can_only_claim_once() {
        let mut selection = Selection::default();
        selection.move_by(-1, 0);
        assert_eq!(selection.index, 0);
        selection.move_by(10, 3);
        assert_eq!(selection.index, 2);
        assert!(!selection.claim(false));
        assert!(selection.claim(true));
        assert!(!selection.claim(true));
    }
    #[test]
    fn refreshed_snapshot_disables_changed_repository_and_hidden_or_busy_workspace() {
        let original = Some(PathBuf::from("/fixture/original"));
        let mut snapshot = PaletteContext {
            availability: Availability {
                repository: true,
                workspace: true,
                ..Availability::default()
            },
            path: original.clone(),
            label: "Original repository".into(),
        };
        assert!(snapshot.allows(&original, CommandId::QuickOpen));
        snapshot.path = Some(PathBuf::from("/fixture/replacement"));
        assert!(!snapshot.allows(&original, CommandId::QuickOpen));
        assert!(!snapshot.allows(&original, CommandId::Profiles));
        snapshot.path = original.clone();
        snapshot.availability.workspace = false;
        assert!(!snapshot.allows(&original, CommandId::QuickOpen));
        assert!(snapshot.allows(&original, CommandId::History));
        snapshot.availability.workspace = true;
        snapshot.availability.busy = Some(crate::workspace::BUSY_REASON);
        assert!(!snapshot.allows(&original, CommandId::QuickOpen));
        assert!(snapshot.allows(&original, CommandId::Activity));
    }
}

/// The palette as a window draws it.
#[cfg(test)]
mod render_tests {
    use super::*;

    /// The GitHub command runs over a read of the panel's that closing cancelled and that
    /// still drains, as the header's Pull requests does, and says why it waits for a write.
    #[gpui::test]
    async fn the_github_command_opens_over_a_draining_read_and_waits_for_a_write(
        cx: &mut TestAppContext,
    ) {
        use crate::github_view::RequestKind;
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let reason = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                CommandId::GitHub.reason(&app.read(cx).palette_availability(window, cx))
            })
        };
        app.update(cx, |app, _| {
            app.operation_busy = Some(RequestKind::Load.label())
        });
        assert_eq!(reason(cx), None, "a draining read does not stop it");
        app.update(cx, |app, _| app.operation_busy = Some("Committing…"));
        assert_eq!(
            reason(cx),
            Some(crate::workspace::BUSY_REASON),
            "a write does, and it says why"
        );
    }

    /// A marked tab's palette refuses what reads or writes its repository
    /// in the mark's words, as Branch does: Tags, Stashes, Worktrees,
    /// Reflog, Rewrite review, Profiles and Recovery drafts opened dialogs
    /// that read the repository from before the mark and whose every write
    /// Git core refused (security probe Q3, FU39-A). The Linux Main Menu,
    /// which shares the palette's reasons, opens none of them; History,
    /// Changes and Refresh stay offered.
    #[gpui::test]
    async fn a_marked_tab_s_palette_refuses_what_it_cannot_act_on(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let third = crate::views::tests::fixture_repository(fixture.path().join("third"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", third.git_dir().display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&crate::Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.shown_tab_unavailable()),
            "marked"
        );
        let reason = |cx: &mut VisualTestContext, id: CommandId| {
            cx.update(|window, cx| app.read(cx).menu_command_reason(id, window, cx))
        };
        let refused = [
            CommandId::Tags,
            CommandId::Stashes,
            CommandId::Worktrees,
            CommandId::Reflog,
            CommandId::RewriteReview,
            CommandId::Profiles,
        ];
        for id in refused {
            assert_eq!(
                reason(cx, id),
                Some(crate::branch_actions::UNAVAILABLE_REASON),
                "{id:?}"
            );
        }
        // The recovery drafts are the app's own data, which the mark says
        // nothing of.
        for id in [
            CommandId::History,
            CommandId::Changes,
            CommandId::Refresh,
            CommandId::RecoveryDrafts,
        ] {
            assert_eq!(reason(cx, id), None, "{id:?} stays offered");
        }
        let path = app.read_with(cx, |app, _| app.path.clone());
        for id in refused {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.run_menu_command(id, &path, window, cx))
            });
            crate::views::tests::settle(&app, cx).await;
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{id:?} opens nothing"
            );
        }
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.run_menu_command(CommandId::RecoveryDrafts, &path, window, cx)
            })
        });
        crate::views::tests::settle(&app, cx).await;
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "the recovery drafts open"
        );
    }

    /// Where the tab is marked, Changes shows "Changes unavailable", as
    /// before any status is read, and the composer names no branch: the
    /// list kept the last status's rows, the inspector's count and the
    /// composer's badge while the header said "Changes", and the composer
    /// said "Commit to main", or "detached HEAD" where no status had been
    /// read. The palette's Branch command opens nothing from before and
    /// says why, as the picker does, and its file commands take no working
    /// file from before: Blame was offered for the row selected before the
    /// mark. "Changes unavailable" names the mark's way back, Open
    /// Repository among it, where it said Refresh alone. Changes is shown
    /// on the tab once it is marked, with nothing read and with a status
    /// read landing behind the mark: a failed Refresh in Changes goes back
    /// to History.
    #[gpui::test]
    async fn a_marked_tab_s_changes_show_nothing_from_before(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
        std::fs::write(root.join("staged.txt"), "new\n").unwrap();
        crate::views::tests::fixture_git(&root, &["add", "staged.txt"]);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.refresh(&crate::Refresh, window, cx);
                app.show_working(window, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::draw(cx);
        let heading = || {
            crate::views::CONTROL_TOOLTIPS
                .with(|tooltips| tooltips.borrow().get("commit-composer-heading").cloned())
        };
        assert!(cx.debug_bounds("working-header-count").is_some(), "a count");
        assert!(cx.debug_bounds("commit-staged-count").is_some(), "a badge");
        assert!(cx.debug_bounds("working-unavailable").is_none());
        assert_eq!(heading().as_deref(), Some("Commit to main"));
        let branch_reason = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let availability = app.read(cx).palette_availability(window, cx);
                CommandId::Branch.reason(&availability)
            })
        };
        assert_eq!(branch_reason(cx), None, "Branch opens the one checked out");
        let blame_reason = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let availability = app.read(cx).palette_availability(window, cx);
                CommandId::Blame.reason(&availability)
            })
        };
        let select_a_row = |cx: &mut VisualTestContext| {
            app.update(cx, |app, _| {
                app.working_selected = Some((0, gitturtle_core::ChangeArea::Unstaged))
            })
        };
        select_a_row(cx);
        assert_eq!(blame_reason(cx), None, "Blame for the row selected");
        let kept = app.read_with(cx, |app, _| {
            (
                app.work_status.clone(),
                app.working_rows.clone(),
                app.working_paths.staged_count,
            )
        });

        std::fs::rename(root.join(".git"), fixture.path().join("git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        // Refresh fails in History: its failed Open in Changes goes back to
        // History with nothing read. Changes is then shown on the marked tab.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_history(window, cx);
                app.refresh(&crate::Refresh, window, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.shown_tab_unavailable()));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.mode == crate::WorkspaceMode::Working),
            "Changes is shown"
        );
        let says_the_mark = |cx: &mut VisualTestContext, at: &str| {
            assert!(
                cx.debug_bounds("working-unavailable").is_some(),
                "{at}: Changes unavailable"
            );
            assert!(
                app.read_with(cx, |app, _| !app.working_files_drawn()),
                "{at}"
            );
            assert!(
                app.read_with(cx, |app, _| app.actionable_working_rows().is_empty()),
                "{at}: no row to act on"
            );
            assert!(cx.debug_bounds("working-header-count").is_none(), "{at}");
            assert!(cx.debug_bounds("commit-staged-count").is_none(), "{at}");
            assert_eq!(
                heading().as_deref(),
                Some(crate::branch_actions::UNAVAILABLE_REASON),
                "{at}: no branch from before"
            );
            assert_eq!(
                app.read_with(cx, |app, cx| app.commit_action_label(cx)),
                "Changes unavailable",
                "{at}"
            );
            assert_eq!(
                branch_reason(cx),
                Some(crate::branch_actions::UNAVAILABLE_REASON),
                "{at}: the palette's Branch says why"
            );
            let explanation = crate::native_accessibility::written::get("working-unavailable-line")
                .and_then(|line| line.node.description().map(str::to_owned));
            assert_eq!(
                explanation.as_deref(),
                Some(crate::branch_actions::UNAVAILABLE_WAY_BACK),
                "{at}: the mark's way back"
            );
        };
        says_the_mark(cx, "no status read");
        // A status read landing behind the mark, its rows and staged count
        // those of the status from before.
        app.update(cx, |app, cx| {
            (
                app.work_status,
                app.working_rows,
                app.working_paths.staged_count,
            ) = kept;
            cx.notify();
        });
        crate::views::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| !app.working_rows.is_empty()),
            "rows kept"
        );
        says_the_mark(cx, "a status kept beside the mark");
        select_a_row(cx);
        assert!(
            blame_reason(cx).is_some(),
            "no Blame for a row selected before the mark"
        );
        // With no error in the window, the preview beside the list says the
        // mark and its way back, whatever the status from before listed: it
        // said "Choose a file", or "Working tree clean" for a clean one.
        let preview_says_the_mark = |cx: &mut VisualTestContext, at: &str| {
            assert!(cx.debug_bounds("empty-state").is_some(), "{at}");
            let said = crate::native_accessibility::written::get(&format!(
                "empty-state:{}",
                crate::branch_actions::UNAVAILABLE_REASON
            ))
            .and_then(|written| written.node.value().map(str::to_owned));
            assert!(
                said.as_deref()
                    .is_some_and(|said| said.contains(crate::branch_actions::UNAVAILABLE_WAY_BACK)),
                "{at}: the mark and its way back: {said:?}"
            );
            assert!(
                cx.debug_bounds("working-preview-clean-line").is_none(),
                "{at}: no clean tree from before"
            );
        };
        app.update(cx, |app, cx| {
            app.error = None;
            cx.notify();
        });
        crate::views::tests::draw(cx);
        preview_says_the_mark(cx, "rows from before");
        app.update(cx, |app, cx| {
            app.work_status = Some(Default::default());
            app.working_rows = Default::default();
            cx.notify();
        });
        crate::views::tests::draw(cx);
        preview_says_the_mark(cx, "a clean tree from before");
    }

    use crate::{activity, recovery_drafts, repository_tabs};
    use core::prelude::v1::test;
    use gpui_kit::{Background, Bounds, ScaledPixels, component::Root, point};
    use std::{cell::RefCell, rc::Rc};

    /// The project list's command says "Hidden: widen the window" in its
    /// description's place where the window holds the pane back, as Toggle
    /// Sidebar's says why it waits in narrow History, and its row's name
    /// says so too, without "Unavailable": the command stays available, and
    /// still turns the setting on and off there. Off, or where the window
    /// draws the pane, its row describes an application action and its name
    /// is the command's.
    #[gpui::test]
    async fn the_project_list_command_says_where_the_pane_is_held_back(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let spec = COMMANDS
            .iter()
            .find(|spec| spec.id == CommandId::ProjectPane)
            .expect("the command");
        let row = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let availability = app.read(cx).palette_availability(window, cx);
                (
                    spec.id.reason(&availability),
                    row_description(spec, &availability),
                    row_name(spec, &availability),
                )
            })
        };
        let plain = spec.label.to_owned();
        let noted = format!("{}. Hidden: widen the window", spec.label);
        for (width, on, described, named) in [
            (461., true, "Hidden: widen the window", &noted),
            (461., false, "Application action", &plain),
            (1480., true, "Application action", &plain),
        ] {
            app.update(cx, |app, cx| {
                app.settings.project_pane = on;
                cx.notify();
            });
            cx.simulate_resize(size(px(width), px(973.)));
            crate::views::tests::draw(cx);
            assert_eq!(
                row(cx),
                (None, described, named.clone()),
                "{width} px with the pane {}",
                if on { "on" } else { "off" }
            );
        }
    }

    /// A selected project list row takes the held-back note in its name once
    /// the context has rested, as a reason's coming is heard: where the
    /// row's name carries a change of context ([`ROW_CARRIES_CONTEXT`],
    /// Linux) it keeps the name it had until the rest, is renamed once then,
    /// and the status announces none of it; elsewhere the row takes the note
    /// as the context changes and the status announces it at the rest. Going
    /// again as the window gives the pane room is heard the same way.
    #[gpui::test]
    fn the_selected_project_list_row_is_renamed_as_the_pane_is_held_back(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        app.update(cx, |app, cx| {
            app.page = AppPage::Settings;
            app.settings.project_pane = true;
            cx.notify();
        });
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        let results = matches("");
        let at = results
            .iter()
            .position(|&command| COMMANDS[command].id == CommandId::ProjectPane)
            .expect("the command");
        let start = first_available(&results, &availability);
        assert!(start <= at, "Down reaches the command");
        for _ in start..at {
            heard.step(cx, "down");
        }
        let spec = COMMANDS[results[at]];
        let plain = spec.label.to_owned();
        let noted = format!("{}. Hidden: widen the window", spec.label);
        let place = place_of("", at);
        assert_eq!(heard.row_name(), Some(plain.as_str()), "beside the pane");
        heard.take();
        heard.take_renamed();
        // One height, so the selected row stays in the list's view.
        let (narrow, wide) = (size(px(461.), px(800.)), size(px(1200.), px(800.)));
        for (window, had, ends, reading) in [
            (
                narrow,
                &plain,
                &noted,
                format!("{} · {place}. Hidden: widen the window", spec.label),
            ),
            (wide, &noted, &plain, format!("{} · {place}", spec.label)),
        ] {
            cx.simulate_resize(window);
            heard.frame(cx);
            heard.wait(cx, 499);
            assert_eq!(
                heard.take(),
                Vec::<String>::new(),
                "{reading:?}: nothing is announced before the rest"
            );
            let before_rest = if ROW_CARRIES_CONTEXT { had } else { ends };
            assert_eq!(
                heard.row_name(),
                Some(before_rest.as_str()),
                "{reading:?}: before the rest"
            );
            heard.wait(cx, 1);
            assert_eq!(
                heard.row_name(),
                Some(ends.as_str()),
                "{reading:?}: its name"
            );
            assert_eq!(
                heard.named.as_deref(),
                Some(reading.as_str()),
                "the status reads it"
            );
            assert_eq!(
                heard.take_renamed(),
                std::slice::from_ref(ends),
                "{reading:?}: renamed once"
            );
            let announced = if ROW_CARRIES_CONTEXT {
                Vec::new()
            } else {
                vec![reading.clone()]
            };
            assert_eq!(heard.take(), announced, "{reading:?}: the status");
        }
    }

    /// The palette's inline help, laid out by phrase in a box swept from 461 px down to its
    /// widest run at 13 and 18 pt, never breaks Enter or Escape across two lines nor starts
    /// a line with its separator, and its accessible text is the whole help. Drawn as one
    /// wrapping text, it started a line with "/" or "·" where it wrapped (FU30-D6 design
    /// review).
    #[gpui::test]
    fn the_palettes_help_keeps_its_key_names_and_separators_whole(cx: &mut TestAppContext) {
        let help = help_keys();
        let keys = [shortcuts::ENTER.to_owned(), "Escape".to_owned()];
        let runs: Vec<SharedString> = crate::phrase_runs(&help)
            .into_iter()
            .flat_map(|phrase| phrase.runs)
            .collect();
        crate::phrase_tests::sweep(
            cx,
            ("command-palette-help", "command-palette-help"),
            &help,
            &keys,
            move |window| {
                crate::phrase_tests::widest(&runs, crate::appearance::ui_text(11.), window)
            },
            || {
                shortcuts::dialog_help("command-palette-help", help_keys())
                    .text_size(crate::appearance::ui_text(11.))
                    .into_any_element()
            },
        );
    }

    /// The application on its Projects hub, with no repository open, so the
    /// repository's commands are unavailable, and dialogs opening at rest.
    fn hub(cx: &mut TestAppContext) -> (Entity<GitTurtle>, &mut VisualTestContext) {
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            // A dialog otherwise fades in on a wall-clock animation.
            cx.set_reduce_motion(true);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                GitTurtle::new(
                    None,
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
        cx.simulate_resize(size(px(1200.), px(800.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.apply_appearance(window, cx);
                app.open_command_palette(window, cx)
            })
        });
        crate::tags::tests::draw(cx);
        (app, cx)
    }

    fn logical(bounds: Bounds<ScaledPixels>, scale: f32) -> Bounds<Pixels> {
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        Bounds::from_corners(
            point(logical(bounds.left()), logical(bounds.top())),
            point(logical(bounds.right()), logical(bounds.bottom())),
        )
    }

    /// What the last frame painted for the palette: the dialog panel around
    /// it (the smallest quad with a 1 px border on every side that holds the
    /// palette's form) and the result list, each as the fills and the 1 px
    /// edge colors painted at its bounds, and every fill painted inside the
    /// list, the rows'.
    struct Painted {
        panel_fills: Vec<Background>,
        panel_edges: Vec<Hsla>,
        list_edges: Vec<Hsla>,
        row_fills: Vec<Background>,
        /// What the list paints in its scrollbar's track at its trailing
        /// edge, no wider than the track: the track and the thumb.
        scroll_fills: Vec<Background>,
    }

    fn painted(cx: &mut VisualTestContext) -> Painted {
        painted_popup(cx, "command-palette-form", "command-palette-results")
    }

    /// [`painted`] for any dialog: its panel around `form`, and `list`.
    fn painted_popup(
        cx: &mut VisualTestContext,
        form: &'static str,
        list: &'static str,
    ) -> Painted {
        let form = crate::tags::tests::rendered(cx, form);
        let list = crate::tags::tests::rendered(cx, list);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            let bordered = |quad: &gpui_kit::Quad| {
                let widths = quad.border_widths;
                [widths.top, widths.right, widths.bottom, widths.left]
                    .iter()
                    .all(|width| px(width.as_f32() / scale) == px(1.))
            };
            let panel = quads
                .iter()
                .filter(|quad| bordered(quad))
                .map(|quad| logical(quad.bounds, scale))
                .filter(|panel| {
                    panel.left() < form.left()
                        && panel.top() < form.top()
                        && panel.right() > form.right()
                        && panel.bottom() > form.bottom()
                })
                .min_by(|a, b| {
                    f32::from(a.size.width * f32::from(a.size.height))
                        .total_cmp(&f32::from(b.size.width * f32::from(b.size.height)))
                })
                .expect("the dialog paints a bordered panel around the palette");
            let at = |bounds: Bounds<Pixels>| {
                quads
                    .iter()
                    .filter(move |quad| logical(quad.bounds, scale) == bounds)
            };
            // GPUI paints a border once per side; each color counts once.
            let edges = |bounds| {
                let mut edges = Vec::new();
                for quad in at(bounds).filter(|quad| bordered(quad)) {
                    if !edges.contains(&quad.border_color) {
                        edges.push(quad.border_color);
                    }
                }
                edges
            };
            let inside = |drawn: Bounds<Pixels>| {
                drawn != list
                    && drawn.left() >= list.left()
                    && drawn.top() >= list.top()
                    && drawn.right() <= list.right()
                    && drawn.bottom() <= list.bottom()
            };
            // Inside the list's 1 px edge.
            let track = list.right() - px(1.) - gpui_kit::base::Scrollbar::width();
            let in_track = |drawn: Bounds<Pixels>| {
                drawn.left() >= track && drawn.size.width <= gpui_kit::base::Scrollbar::width()
            };
            // Quads are listed in paint order; the page beneath the dialog
            // comes before the panel.
            let above = quads
                .iter()
                .position(|quad| logical(quad.bounds, scale) == panel)
                .expect("the panel was painted");
            Painted {
                panel_fills: at(panel)
                    .filter(|quad| !quad.background.is_transparent())
                    .map(|quad| quad.background)
                    .collect(),
                panel_edges: edges(panel),
                list_edges: edges(list),
                row_fills: quads[above..]
                    .iter()
                    .filter(|quad| {
                        let drawn = logical(quad.bounds, scale);
                        inside(drawn) && !in_track(drawn) && !quad.background.is_transparent()
                    })
                    .map(|quad| quad.background)
                    .collect(),
                scroll_fills: quads[above..]
                    .iter()
                    .filter(|quad| {
                        let drawn = logical(quad.bounds, scale);
                        inside(drawn) && in_track(drawn) && !quad.background.is_transparent()
                    })
                    .map(|quad| quad.background)
                    .collect(),
            }
        })
    }

    /// Keyboard Shortcuts' list shows its scrollbar while it holds more rows
    /// than it draws: given up to a few rows, as at 18 pt in a 461 × 490
    /// window, where it drew three of 61 with no sign of the rest, and in a
    /// full window, where it draws ten. A search that leaves rows enough to
    /// fit shows none.
    #[gpui::test]
    async fn the_shortcut_list_shows_its_scroll_while_it_holds_more(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for (text, width, height) in [(18, 461., 490.), (13, 1200., 800.)] {
            let at = format!("{text} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
            // The list's height lands in its first frame; the scrollbar
            // reads it in the next.
            draw(cx);
            draw(cx);
            let painted = painted_popup(cx, "keyboard-shortcuts-view", "shortcut-help-results");
            assert!(
                !painted.scroll_fills.is_empty(),
                "{at}: the list shows its scroll"
            );
            if text == 13 {
                cx.simulate_input("command palette");
                draw(cx);
                draw(cx);
                let rows = cx.debug_bounds("shortcut-help-row-1");
                assert!(rows.is_some(), "{at}: the search leaves its entry");
                let painted = painted_popup(cx, "keyboard-shortcuts-view", "shortcut-help-results");
                assert_eq!(
                    painted.scroll_fills,
                    Vec::<Background>::new(),
                    "{at}: every row fits, so no scrollbar"
                );
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// At 18 pt in a 461 × 490 window, where the dialog's height cap gives
    /// the list less than its full height, a search that leaves one shortcut
    /// ("command palette") shows that entry whole below its group's heading,
    /// its key chip too, and the count above the list reads "1 shortcut",
    /// as it draws, followed by the modifier legend exactly where the line,
    /// measured from the text drawn, fits the view drawn. The list kept room for
    /// one row, which the heading took: the selected entry's chip was cut at
    /// the list's foot, and the count read "1 shortcuts".
    #[gpui::test]
    async fn a_search_for_one_shortcut_shows_it_whole_and_counts_one(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
        draw(cx);
        draw(cx);
        cx.simulate_input("command palette");
        // The list's height lands in its first frame; the scroll that keeps
        // the entry in view, in the next, whose nodes are read, once the
        // search has rested and the status announces its count.
        draw(cx);
        cx.executor().advance_clock(crate::shortcuts::COUNT_REST);
        written::forget();
        draw(cx);
        let list = cx.debug_bounds("shortcut-help-results").expect("the list");
        let full = crate::shortcuts::list_height();
        assert!(
            list.size.height < full,
            "the cap gives the list less than its {full:?}: {list:?}"
        );
        // Inside the list's 1 px border.
        let (top, bottom) = (list.top() + px(1.), list.bottom() - px(1.));
        for part in ["shortcut-help-row-1", "shortcut-help-key-1"] {
            let bounds = cx.debug_bounds(part).unwrap_or_else(|| panic!("{part}"));
            assert!(
                bounds.top() >= top - px(0.5) && bounds.bottom() <= bottom + px(0.5),
                "{part} {bounds:?} whole in the list's {top:?}..{bottom:?}"
            );
        }
        let count = written::get("shortcut-help-count").expect("the count is written");
        assert_eq!(count.node.value(), Some("1 shortcut"), "the count");
        // The legend follows the count exactly where the line, as the
        // interface font lays out the text drawn, fits the view drawn.
        let view = cx
            .debug_bounds("keyboard-shortcuts-view")
            .expect("the view");
        let legend_text = format!(
            "{}{}",
            crate::shortcuts::LEGEND_SEPARATOR,
            crate::shortcuts::modifier_legend()
        );
        let line = cx.update(|window, _| {
            let laid_out = |text: &str| {
                crate::views::laid_out_text_width(
                    crate::views::shaped_width(
                        text,
                        appearance::ui_text(12.),
                        window.text_style().font_weight,
                        window,
                    ),
                    window,
                )
            };
            laid_out("1 shortcut") + laid_out(&legend_text)
        });
        match written::get("shortcut-help-legend") {
            Some(legend) => {
                assert_eq!(
                    legend.node.value(),
                    Some(crate::shortcuts::modifier_legend()),
                    "the legend reads the modifier keys"
                );
                let (count, legend) = (
                    count.layout.expect("the count's layout"),
                    legend.layout.expect("the legend's layout"),
                );
                let drawn = px(((legend.x1 - count.x0) / 2.) as f32);
                assert!(
                    line <= view.size.width && drawn <= view.size.width + px(0.5),
                    "the line, {line:?} measured and {drawn:?} drawn, fits the view {view:?}"
                );
            }
            None => assert!(
                line > view.size.width,
                "the count stands alone only where its {line:?} line does not fit the view {view:?}"
            ),
        }
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The count above the shortcut list keeps one line, at 13 and 18 pt in
    /// 400, 461 and 640 px wide windows, at the test window's scale factor,
    /// 2, and at 1.5: the count, read by what it draws, and after it on its line the platform's modifier legend where
    /// the interface font lays the whole line out on one line in the view,
    /// the count and the legend each as its text lays it out; otherwise the
    /// count alone, its legend giving way first as words beside a count do.
    /// The view is as wide as the line was measured for
    /// (`shortcuts::help_width`). The macOS legend once wrapped to a second
    /// line at 18 pt in a 461 px window, where the list was already at its
    /// floor, and the hint below the list ran 21 px under Close; Linux's
    /// shorter legend would wrap the same way at 18 pt in a 400 px window.
    #[gpui::test]
    async fn the_shortcut_count_keeps_one_line(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let count = crate::shortcuts::listed_shortcuts();
        let (counted, legend) = (
            format!("{count} shortcuts"),
            format!(" · {}", crate::shortcuts::modifier_legend()),
        );
        let (mut alone, mut whole) = (0, 0);
        for scale in [2., 1.5] {
            for text in [13, 18] {
                for width in [400., 461., 640.] {
                    let at = format!("{text} pt in {width} × 490 at scale {scale}");
                    cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                    cx.simulate_resize(size(px(width), px(490.)));
                    cx.update(|window, _| window.set_scale_factor(scale));
                    cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
                    draw(cx);
                    written::forget();
                    draw(cx);
                    let view = cx
                        .debug_bounds("keyboard-shortcuts-view")
                        .unwrap_or_else(|| panic!("{at}: the view"));
                    let (predicted, line, fits) = cx.update(|window, cx| {
                        let mut style = window.text_style();
                        style.font_size = appearance::ui_text(12.).into();
                        let laid_out = |text: &str| {
                            crate::views::laid_out_text_width(
                                crate::views::shaped_width(
                                    text,
                                    appearance::ui_text(12.),
                                    style.font_weight,
                                    window,
                                ),
                                window,
                            )
                        };
                        (
                            crate::shortcuts::help_width(window, cx),
                            style.line_height_in_pixels(window.rem_size()),
                            laid_out(&counted) + laid_out(&legend) <= view.size.width,
                        )
                    });
                    assert!(
                        (view.size.width - predicted).abs() < px(0.01),
                        "{at}: the view is {:?} wide, measured for {predicted:?}",
                        view.size.width
                    );
                    // One line, in logical pixels.
                    let one_line = |drawn: gpui::accesskit::Rect| {
                        let height = px(((drawn.y1 - drawn.y0) / f64::from(scale)) as f32);
                        assert!(
                            height < line * 1.5,
                            "{at}: {drawn:?} is {height:?} tall, one line {line:?}"
                        );
                    };
                    let status = written::get("shortcut-help-count")
                        .unwrap_or_else(|| panic!("{at}: the count is written"));
                    assert_eq!(
                        status.node.value(),
                        Some(counted.as_str()),
                        "{at}: the count reads what it draws"
                    );
                    let drawn = status.layout.expect("the count's layout");
                    one_line(drawn);
                    match (fits, written::get("shortcut-help-legend")) {
                        (true, Some(shown)) => {
                            whole += 1;
                            let after = shown.layout.expect("the legend's layout");
                            one_line(after);
                            let view_right = f64::from(f32::from(view.right()) * scale);
                            assert!(
                                (after.y0 - drawn.y0).abs() < 0.5
                                    && (after.x0 - drawn.x1).abs() < 0.5
                                    && after.x1 <= view_right + 0.5,
                                "{at}: the legend {after:?} follows the count {drawn:?} on its \
                                 line, inside the view {view:?}"
                            );
                        }
                        (false, None) => alone += 1,
                        (fits, shown) => panic!(
                            "{at}: the legend drawn where it fits ({fits}), not {:?}",
                            shown.map(|shown| shown.layout)
                        ),
                    }
                    cx.simulate_keystrokes("escape");
                    draw(cx);
                }
            }
        }
        assert!(
            alone > 0 && whole > 0,
            "the walk meets the count with its legend ({whole}) and alone ({alone})"
        );
        cx.update(|window, _| window.set_scale_factor(2.));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Keyboard Shortcuts' status, a polite live region that reads the count
    /// with the selected entry, is announced only as they change: at 18 pt,
    /// resizing the window between 640 and 400 px wide brings and takes the
    /// modifier legend after the count line and leaves the status's name as
    /// it was, "61 shortcuts · Command Palette: …", while a search changes it
    /// once the search has rested. Drawn and named as one line, the count's
    /// name changed as a resize crossed the width where the legend fits, and
    /// AT-SPI announces a live region whose name changes.
    #[gpui::test]
    async fn the_shortcut_count_is_announced_only_as_it_changes(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(640.), px(490.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
        draw(cx);
        let read = |cx: &mut VisualTestContext| {
            written::forget();
            draw(cx);
            let status = written::get("shortcut-help-status").expect("the status is written");
            assert_eq!(
                written::announced("shortcut-help-status").map(|live| live.live),
                Some(gpui::accesskit::Live::Polite),
                "the status is polite"
            );
            assert_eq!(
                written::announced("shortcut-help-count"),
                None,
                "the count line is no live region of its own"
            );
            (
                status.node.label().map(str::to_owned),
                written::get("shortcut-help-legend").is_some(),
            )
        };
        let opened = format!(
            "{} shortcuts · Command Palette: {}",
            crate::shortcuts::listed_shortcuts(),
            crate::shortcuts::label(crate::shortcuts::ShortcutId::Palette)
        );
        let mut legends = Vec::new();
        for width in [640., 400., 640.] {
            cx.simulate_resize(size(px(width), px(490.)));
            draw(cx);
            let (name, legend) = read(cx);
            assert_eq!(
                name.as_deref(),
                Some(opened.as_str()),
                "in a {width} px window the status keeps its name"
            );
            legends.push(legend);
        }
        assert_eq!(
            legends,
            [true, false, true],
            "the resize brings and takes the legend"
        );
        cx.simulate_input("command palette");
        draw(cx);
        cx.executor().advance_clock(crate::shortcuts::COUNT_REST);
        assert_eq!(
            read(cx).0,
            Some(format!(
                "1 shortcut · Command Palette: {}",
                crate::shortcuts::label(crate::shortcuts::ShortcutId::Palette)
            )),
            "a search changes the count and entry once it rests"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Keyboard Shortcuts' hint below the list lies whole inside the dialog,
    /// its last line drawn: at 13 and 18 pt in a 461 × 490 window, at the
    /// test window's scale factor, 2, and at 1.5, the part of the hint the
    /// dialog's clip leaves drawn is its layout, and it ends above Close;
    /// at 18 pt the window is then shortened in 1/8 px steps until the list
    /// reaches its floor, one entry, where the hint is whole too. At 18 pt,
    /// the list at its floor, the hint was laid out 270 to 368 px down
    /// against the clip at 358, its last line cut: the selected entry's
    /// status, which draws nothing, took a gap of its own between the list
    /// and the hint. Compared with Close alone, the hint passed.
    ///
    /// That status, now the dialog's one status, which names the count with
    /// the selected entry, is drawn whole wherever the
    /// dialog is, so an assistive technology finds it showing: in those
    /// windows, and in a 461 × 400 one, too short for the dialog's content,
    /// where the clip cuts the hint. Along the view's foot, it lay past the
    /// clip there, and the AT-SPI adapter reported it not showing.
    #[gpui::test]
    async fn the_shortcut_hint_lies_whole_inside_the_dialog(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::{draw, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let close = format!(
            "dialog-dismiss: {}",
            crate::shortcuts::DialogKeys::Shortcuts.dismiss()
        )
        .leak();
        // What the window draws of the written node `id`, against its layout.
        let drawn = |id: &str, at: &str| {
            let node = written::get(id).unwrap_or_else(|| panic!("{at}: {id} is written"));
            let laid = node.layout.expect("its layout");
            let shown = node
                .node
                .bounds()
                .unwrap_or_else(|| panic!("{at}: {id} is drawn"));
            (node, laid, shown)
        };
        let whole = |laid: gpui::accesskit::Rect, shown: gpui::accesskit::Rect| {
            laid.y1 > laid.y0
                && (shown.y0 - laid.y0).abs() < 0.5
                && (shown.y1 - laid.y1).abs() < 0.5
        };
        let status_is_drawn = |at: &str| {
            let (status, laid, shown) = drawn("shortcut-help-status", at);
            // A node the window draws none of reports where it lies, so the
            // status is held within the column's drawn part too: above the
            // drawn foot of the hint, its last part.
            let (_, _, hint) = drawn("shortcut-help-keys", at);
            assert!(
                whole(laid, shown) && laid.y1 <= hint.y1 + 0.5,
                "{at}: the dialog's status laid out {laid:?} is drawn, not {shown:?}, \
                 within the column drawn to {}",
                hint.y1
            );
            assert!(
                status.node.label().is_some_and(|name| name.contains(": ")),
                "{at}: the status names the selected entry, not {:?}",
                status.node.label()
            );
            assert_eq!(
                written::announced("shortcut-help-status").map(|live| live.live),
                Some(gpui::accesskit::Live::Polite),
                "{at}: the status is a polite live region"
            );
        };
        let mut floor = false;
        for scale in [2., 1.5] {
            for text in [13, 18] {
                let at = format!("{text} pt at scale {scale}");
                cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
                cx.simulate_resize(size(px(461.), px(490.)));
                cx.update(|window, _| window.set_scale_factor(scale));
                cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
                draw(cx);
                draw(cx);
                let mut height = 490.;
                loop {
                    let at = format!("{at} in 461 × {height}");
                    written::forget();
                    draw(cx);
                    let (_, laid, shown) = drawn("shortcut-help-keys", &at);
                    assert!(
                        whole(laid, shown),
                        "{at}: the hint laid out {laid:?} is drawn whole, not {shown:?}"
                    );
                    let close = cx.debug_bounds(close).expect("Close");
                    assert!(
                        laid.y1 <= f64::from(f32::from(close.top()) * scale) + 0.5,
                        "{at}: the hint {laid:?} ends above Close {close:?}"
                    );
                    status_is_drawn(&at);
                    let list = cx.debug_bounds("shortcut-help-results").expect("the list");
                    if text == 13 || list.size.height <= crate::shortcuts::list_floor() + px(0.01) {
                        floor |= text == 18;
                        break;
                    }
                    height -= 0.125;
                    assert!(height > 440., "{at}: the list reaches its floor");
                    // A resize sets the test window's scale again.
                    cx.simulate_resize(size(px(461.), px(height)));
                    cx.update(|window, _| window.set_scale_factor(scale));
                    draw(cx);
                }
                // Too short for the dialog's content, the clip cuts the hint,
                // and the status is still drawn.
                cx.simulate_resize(size(px(461.), px(400.)));
                cx.update(|window, _| window.set_scale_factor(scale));
                draw(cx);
                written::forget();
                draw(cx);
                status_is_drawn(&format!("{at} in 461 × 400"));
                cx.simulate_keystrokes("escape");
                draw(cx);
            }
        }
        assert!(floor, "the walk meets the list at its floor");
        cx.update(|window, _| window.set_scale_factor(2.));
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
    /// A group's heading row is its name's line and the group gap rather
    /// than an entry's height, so after a search that leaves one shortcut
    /// ("command palette") in a 461 × 490 window the General heading and its
    /// entry, key chip too, both lie whole in the list, at 13 and at 18 pt.
    /// At 18 pt the list holds 99 px there, and the heading's 58 px row, as
    /// tall as an entry, had its top cut by the list's edge.
    ///
    /// The test text system sets the hint below the list wider than the
    /// desktop's fonts do, wrapping it to a fourth line at 18 pt, so the list
    /// takes those 99 px in a slightly taller test window: the shortest from
    /// 490 px up whose list holds them.
    #[gpui::test]
    async fn a_one_match_search_shows_its_heading_and_entry_whole(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, tagged_repository, window};
        const NATIVE_LIST: Pixels = px(99.);
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
            draw(cx);
            draw(cx);
            let mut height = 490.;
            while cx
                .debug_bounds("shortcut-help-results")
                .expect("the list")
                .size
                .height
                < NATIVE_LIST
            {
                height += 1.;
                assert!(height <= 560., "{text} pt: a list of {NATIVE_LIST:?}");
                cx.simulate_resize(size(px(461.), px(height)));
                draw(cx);
                draw(cx);
            }
            let at = format!("{text} pt in 461 × {height}");
            cx.simulate_input("command palette");
            // The list's height lands in its first frame; the scroll that
            // keeps the entry in view, in the next.
            draw(cx);
            draw(cx);
            let list = cx.debug_bounds("shortcut-help-results").expect("the list");
            // Inside the list's 1 px border.
            let (top, bottom) = (list.top() + px(1.), list.bottom() - px(1.));
            let heading = cx
                .debug_bounds("shortcut-help-heading-0")
                .unwrap_or_else(|| panic!("{at}: the heading is drawn"));
            let entry = cx
                .debug_bounds("shortcut-help-row-1")
                .unwrap_or_else(|| panic!("{at}: the entry is drawn"));
            assert!(
                heading.size.height < entry.size.height,
                "{at}: the heading {heading:?} is shorter than its entry {entry:?}"
            );
            for (part, bounds) in [
                ("the heading", heading),
                ("the entry", entry),
                (
                    "the key chip",
                    cx.debug_bounds("shortcut-help-key-1")
                        .expect("the key chip"),
                ),
            ] {
                assert!(
                    bounds.top() >= top - px(0.5) && bounds.bottom() <= bottom + px(0.5),
                    "{at}: {part} {bounds:?} whole in the list's {top:?}..{bottom:?}"
                );
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The palette is a menu of commands, so in every palette it floats on
    /// the surface the menus do, `Palette::raised`, with their fitted edge,
    /// `Palette::raised_edge`, rather than on the page background (the
    /// canvas) it was drawn on. Its result list takes that edge too, as a
    /// menu's separators do, and a resting row shows the raised surface, so
    /// only the highlighted row paints a fill of its own: rows on the panel
    /// color drew a darker box inside the palette in every Omarchy palette
    /// and in Solarized Light, whose panel is darker than their raised surface.
    #[gpui::test]
    fn the_palette_floats_on_the_menus_raised_surface_and_edge(cx: &mut TestAppContext) {
        let (_, cx) = hub(cx);
        for (name, palette) in appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            crate::tags::tests::draw(cx);
            // What a popup menu paints: its surface and its edge.
            let (surface, edge) = cx.update(|_, cx| {
                (
                    Theme::global(cx).popover,
                    gpui_kit::component::popover_ring(cx),
                )
            });
            assert_eq!(surface, Hsla::from(rgb(palette.raised())), "{name} menus");
            assert_eq!(edge, Hsla::from(rgb(palette.raised_edge())), "{name} menus");
            let painted = painted(cx);
            assert_eq!(
                painted.panel_fills,
                vec![Background::from(surface)],
                "{name} surface"
            );
            assert_eq!(painted.panel_edges, vec![edge], "{name} edge");
            assert_eq!(painted.list_edges, vec![edge], "{name} list edge");
            assert_eq!(
                painted.row_fills,
                vec![Background::from(Hsla::from(rgb(palette.selected)))],
                "{name} rows"
            );
        }
    }

    /// Quick Open and Keyboard Shortcuts are pickers, as the command palette
    /// is, so they float as it does: on `Palette::raised` with
    /// `Palette::raised_edge` in every palette, their result lists edged the
    /// same way, and only the highlighted row painting a fill of its own.
    /// They were dialogs on the page background with rows on the panel color,
    /// and Keyboard Shortcuts' key chips on it too.
    #[gpui::test]
    async fn quick_open_and_keyboard_shortcuts_float_on_the_menus_raised_surface(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for name in ["notes.txt", "readme.txt"] {
            std::fs::write(repo.path().join(name), "text\n").unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        type Open = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        let dialogs: [(&str, Open, &'static str, &'static str); 2] = [
            (
                "Quick Open",
                GitTurtle::open_quick_file,
                "quick-file-form",
                "quick-open-results",
            ),
            (
                "Keyboard Shortcuts",
                GitTurtle::shortcut_help,
                "keyboard-shortcuts-view",
                "shortcut-help-results",
            ),
        ];
        for (dialog, open, form, list) in dialogs {
            cx.update(|window, cx| app.update(cx, |app, cx| open(app, window, cx)));
            for (name, palette) in appearance::test_palettes() {
                cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
                // Quick Open reads its paths on the worker after a 120 ms
                // pause for typing.
                for _ in 0..20 {
                    cx.executor()
                        .advance_clock(std::time::Duration::from_millis(150));
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    draw(cx);
                    if !painted_popup(cx, form, list).row_fills.is_empty() {
                        break;
                    }
                }
                let (surface, edge) = cx.update(|_, cx| {
                    (
                        Theme::global(cx).popover,
                        gpui_kit::component::popover_ring(cx),
                    )
                });
                let painted = painted_popup(cx, form, list);
                assert_eq!(
                    painted.panel_fills,
                    vec![Background::from(surface)],
                    "{dialog} {name} surface"
                );
                assert_eq!(painted.panel_edges, vec![edge], "{dialog} {name} edge");
                assert_eq!(painted.list_edges, vec![edge], "{dialog} {name} list edge");
                assert_eq!(
                    painted.row_fills,
                    vec![Background::from(Hsla::from(rgb(palette.selected)))],
                    "{dialog} {name} rows"
                );
                // Keyboard Shortcuts' list overflows at 1200 × 800, and its
                // scrollbar's track rests on the raised surface, as the
                // dialogs' list tracks rest on theirs, beneath the thumb.
                if list == "shortcut-help-results" {
                    let thumb = cx.update(|_, cx| Theme::global(cx).tokens.scrollbar_thumb);
                    assert!(
                        painted.scroll_fills.contains(&Background::from(surface)),
                        "{dialog} {name}: the track {:?}",
                        painted.scroll_fills
                    );
                    assert!(
                        painted
                            .scroll_fills
                            .iter()
                            .all(|fill| *fill == Background::from(surface)
                                || *fill == Background::from(thumb)),
                        "{dialog} {name}: the track and thumb {:?}",
                        painted.scroll_fills
                    );
                }
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{dialog}"
            );
        }
    }

    /// Quick Open counts its matches with the noun that agrees: two tracked
    /// paths read "2 matches", and a query that leaves one reads "1 match",
    /// where it read "1 matches".
    #[gpui::test]
    async fn quick_open_counts_one_match_and_many(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for name in ["notes.txt", "readme.txt"] {
            std::fs::write(repo.path().join(name), "text\n").unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_quick_file(window, cx)));
        // Whether the help line reads `count` once Quick Open has read its
        // paths, on the worker after a 120 ms pause for typing.
        let reads = |cx: &mut VisualTestContext, count: &str| {
            let help = format!(
                "quick-open-help: {count} · ↑/↓ select · {} opens · {}",
                crate::shortcuts::ENTER,
                crate::shortcuts::DialogKeys::QuickOpen.escape_help()
            );
            (0..20).any(|_| {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(150));
                std::thread::sleep(std::time::Duration::from_millis(5));
                draw(cx);
                cx.debug_bounds(help.clone().leak()).is_some()
            })
        };
        assert!(reads(cx, "2 matches"), "two tracked paths");
        cx.simulate_input("notes");
        assert!(reads(cx, "1 match"), "one tracked path matches \"notes\"");
        cx.simulate_keystrokes("escape");
        draw(cx);
    }

    /// At 18 pt in a 461 × 490 half tile Quick Open's hint wraps between its
    /// phrases, each whole on one line: it left "cancels" alone on its last
    /// line.
    #[gpui::test]
    async fn quick_open_wraps_its_hint_between_phrases(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for name in ["notes.txt", "readme.txt"] {
            std::fs::write(repo.path().join(name), "text\n").unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(461.), px(490.)));
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_quick_file(window, cx)));
        // The hint counts its matches once Quick Open has read its paths, on
        // the worker after a 120 ms pause for typing.
        let landed = (0..20).any(|_| {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(150));
            std::thread::sleep(std::time::Duration::from_millis(5));
            draw(cx);
            cx.debug_bounds("dialog-help-phrase-3").is_some()
        });
        assert!(landed, "the hint's four phrases are drawn");
        let phrases: Vec<_> = (0..4)
            .map(|index| {
                cx.debug_bounds(format!("dialog-help-phrase-{index}").leak())
                    .expect("a phrase")
            })
            .collect();
        let line = phrases[0].size.height;
        for (index, phrase) in phrases.iter().enumerate() {
            assert!(
                (phrase.size.height - line).abs() < px(0.5),
                "phrase {index} {phrase:?} is one line of {line:?}: {phrases:?}"
            );
        }
        assert!(
            phrases[3].top() > phrases[0].top(),
            "at 18 pt in a half tile the hint wraps: {phrases:?}"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A picker's highlighted row is the keyboard's cursor, so each picker
    /// marks it as the menus do: the `selected` fill and a 2 px leading
    /// marker of `accent`, at 3:1 or more against that fill and against the
    /// raised surface, in every palette. The fill alone stood 1.40:1 off the
    /// raised surface in Midnight and 1.28:1 in Braden.
    #[gpui::test]
    async fn every_picker_marks_its_keyboard_cursor_at_three_to_one(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for name in ["notes.txt", "readme.txt"] {
            std::fs::write(repo.path().join(name), "text\n").unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        type Open = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        let pickers: [(&str, Open, &str); 3] = [
            (
                "Command palette",
                GitTurtle::open_command_palette,
                "palette-command-",
            ),
            ("Quick Open", GitTurtle::open_quick_file, "quick-file-"),
            (
                "Keyboard Shortcuts",
                GitTurtle::shortcut_help,
                "shortcut-help-row-",
            ),
        ];
        let mut failures = Vec::new();
        for (picker, open, rows) in pickers {
            cx.update(|window, cx| app.update(cx, |app, cx| open(app, window, cx)));
            // The highlighted row: the one painting the `selected` fill.
            let mut cursor = None;
            // Quick Open reads its paths on the worker after a 120 ms pause
            // for typing.
            for _ in 0..20 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(150));
                std::thread::sleep(std::time::Duration::from_millis(5));
                draw(cx);
                let selected = cx.update(|_, cx| palette(cx).selected);
                let selected = Background::from(Hsla::from(rgb(selected)));
                let shown: Vec<_> = (0..8)
                    .filter_map(|row| cx.debug_bounds(format!("{rows}{row}").leak()))
                    .collect();
                cursor = shown
                    .into_iter()
                    .find(|&bounds| appearance::painted_row(cx, bounds).0.contains(&selected));
                if cursor.is_some() {
                    break;
                }
            }
            let cursor = cursor.unwrap_or_else(|| panic!("{picker} highlights a row"));
            for (name, palette) in appearance::test_palettes() {
                cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
                draw(cx);
                failures.extend(appearance::cursor_issues(
                    cx,
                    cursor,
                    palette,
                    &format!("{picker} {name}"),
                ));
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{picker}"
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Keyboard Shortcuts outlines its key chips in the raised surface's edge,
    /// which the selected row's fill nearly equals in some palettes: #2c384a
    /// on #223b3b (1.01:1) in Midnight and #c4ced9 on #d6e7e2 (1.24:1) in
    /// Braden. On the selected row a chip's edge holds 1.4:1 against that
    /// fill, as a resting row's chip holds it against the raised surface, in
    /// every palette.
    #[gpui::test]
    fn key_chips_keep_their_edge_on_the_selected_row(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, rendered};
        use appearance::custom::contrast;
        let (app, cx) = hub(cx);
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
        draw(cx);
        let mut failures = Vec::new();
        for (name, palette) in appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            draw(cx);
            // The first entry is highlighted as the dialog opens.
            for (row, fill, beneath) in [
                (1, Some(palette.selected), palette.selected),
                (2, None, palette.raised()),
            ] {
                let bounds = rendered(cx, format!("shortcut-help-row-{row}").leak());
                let fills = appearance::painted_row(cx, bounds).0;
                let expected: Vec<_> = fill
                    .map(|fill| Background::from(Hsla::from(rgb(fill))))
                    .into_iter()
                    .collect();
                if fills != expected {
                    failures.push(format!("{name}: row {row} fills {fills:?}"));
                }
                let edges = crate::views::tests::painted_edges(
                    cx,
                    format!("shortcut-help-key-{row}").leak(),
                );
                match edges[..] {
                    [edge] => {
                        let ratio = contrast(appearance::packed(edge), beneath);
                        if ratio < 1.4 {
                            failures.push(format!(
                                "{name}: row {row}'s chip edge #{:06x} on #{beneath:06x} {ratio:.3}:1",
                                appearance::packed(edge)
                            ));
                        }
                    }
                    _ => failures.push(format!("{name}: row {row}'s chip edges {edges:?}")),
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The rows among the first `rows` that the last frame painted with the
    /// highlight's `selected` fill.
    fn highlighted(cx: &mut VisualTestContext, rows: usize) -> Vec<usize> {
        let bounds: Vec<_> = (0..rows)
            .map(|row| cx.debug_bounds(format!("palette-command-{row}").leak()))
            .collect();
        cx.update(|window, cx| {
            let fill = Background::from(Hsla::from(rgb(palette(cx).selected)));
            let scale = window.scale_factor();
            let quads = window.painted_quads();
            (0..rows)
                .filter(|&row| {
                    bounds[row].is_some_and(|bounds| {
                        quads.iter().any(|quad| {
                            quad.background == fill && logical(quad.bounds, scale) == bounds
                        })
                    })
                })
                .collect()
        })
    }

    /// The highlight starts on the first command the context can run: a
    /// highlighted unavailable row drew its title in the faintest text on
    /// screen (2.16:1 to 2.65:1) under a reason at secondary strength. On the
    /// Projects hub "show" first matches the graph-lane commands, which need
    /// a repository, so typing it highlights the first available match
    /// instead of the first row, and Up/Down still reach the unavailable rows.
    #[gpui::test]
    fn the_highlight_starts_on_the_first_available_command(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
        crate::tags::tests::draw(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        let available = |results: &[usize]| {
            results
                .iter()
                .map(|&index| COMMANDS[index].id.reason(&availability).is_none())
                .collect::<Vec<_>>()
        };
        // Opening: the registry's first command runs anywhere.
        assert!(available(&matches(""))[0]);
        assert_eq!(highlighted(cx, 7), vec![0], "on opening");

        cx.simulate_input("show");
        crate::tags::tests::draw(cx);
        let rows = available(&matches("show"));
        let first = rows
            .iter()
            .position(|&available| available)
            .expect("a match runs on the hub");
        assert!(
            first > 0 && first < 7,
            "the first match is unavailable: {rows:?}"
        );
        assert_eq!(
            highlighted(cx, rows.len().min(7)),
            vec![first],
            "after typing"
        );

        // Up/Down move one row at a time, onto unavailable rows too.
        cx.simulate_keystrokes("up");
        crate::tags::tests::draw(cx);
        assert!(!rows[first - 1]);
        assert_eq!(
            highlighted(cx, rows.len().min(7)),
            vec![first - 1],
            "after Up"
        );
    }

    /// Disabled, Open command rests on the neutral `subtle`, which is the
    /// palette's raised surface itself in most Omarchy dark palettes
    /// (1.000:1), so it drew only its dim label. In every palette it keeps
    /// its shape with the toolkit's disabled edge, 1.5:1 or more off that
    /// surface, and keeps the bounds it has, the edge laid over them.
    #[gpui::test]
    fn a_disabled_open_command_keeps_its_shape_on_the_raised_surface(cx: &mut TestAppContext) {
        use appearance::custom::contrast;
        let (_, cx) = hub(cx);
        // "show" on the hub highlights an available match; Up reaches the
        // unavailable row above it, which Open command cannot run.
        cx.simulate_input("show");
        cx.simulate_keystrokes("up");
        crate::tags::tests::draw(cx);
        let mut failures = Vec::new();
        let mut place = None;
        for (name, palette) in appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            crate::tags::tests::draw(cx);
            let open = cx
                .debug_bounds("run-palette-command")
                .expect("Open command");
            assert_eq!(
                *place.get_or_insert(open),
                open,
                "{name} moves Open command"
            );
            let edges = crate::views::tests::painted_edges(cx, "run-palette-command");
            let edge = palette.disabled_edge();
            if edges != [gpui_kit::Hsla::from(gpui_kit::rgb(edge))]
                || contrast(edge, palette.raised()) < 1.5
            {
                failures.push(format!("{name}: edges {edges:?}, not #{edge:06x}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// An unavailable command's title is regular and an available one's
    /// medium, so the weight marks an unavailable row as well as the disabled
    /// color does; it was medium either way. "show" on the Projects hub
    /// matches both kinds within the first screen of rows.
    #[gpui::test]
    fn an_unavailable_commands_title_is_regular(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        cx.simulate_input("show");
        crate::tags::tests::draw(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        let results = matches("show");
        let shown = results.len().min(7);
        let mut kinds = (false, false);
        for (row, &index) in results[..shown].iter().enumerate() {
            let command = COMMANDS[index];
            let available = command.id.reason(&availability).is_none();
            if available {
                kinds.0 = true;
            } else {
                kinds.1 = true;
            }
            assert_eq!(
                appearance::drawn_weight(&format!("palette-command-title-{row}")),
                Some(if available {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                }),
                "{}, available: {available}",
                command.label
            );
        }
        assert_eq!(kinds, (true, true), "both kinds of row are shown");
    }

    /// In a repository with a stash, at 13 and 18 pt in a 461 × 490 window,
    /// the palette finds Browse stashes… by "stash", and Return opens
    /// Stashes, as the Working Changes Actions menu's Browse stashes… does,
    /// with the palette closed and focus in Stashes: the palette offered no
    /// stash command, so the search matched nothing and Return did nothing.
    /// While a write runs, and through the reload after it, as the menu item
    /// waits, the entry says why and Return says so again and opens nothing:
    /// through the reload it once opened Stashes while the item still waited.
    #[gpui::test]
    async fn browse_stashes_opens_stashes_with_focus_in_it(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "kept for later",
            ],
        );
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(461.), px(490.)));
        for text in [13, 18] {
            let at = format!("{text} pt in 461 × 490");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
            draw(cx);
            cx.simulate_input("stash");
            draw(cx);
            for query in ["stash", "Browse stashes"] {
                assert_eq!(
                    matches(query).first().map(|&index| COMMANDS[index].id),
                    Some(CommandId::Stashes),
                    "{at}: the first match for {query:?} is Browse stashes…"
                );
            }
            // Each press comes up, as a person's does: `simulate_keystrokes`
            // sends no key-up, and a dialog opened while Enter is down takes
            // no Enter until it comes up, the palette opened next included.
            cx.simulate_keystrokes("enter");
            enter_up(cx);
            draw(cx);
            let (browser, list) = app
                .read_with(cx, crate::recovery::open_stash_list)
                .unwrap_or_else(|| panic!("{at}: Return opens Stashes"));
            crate::recovery::StashBrowser::lands(&browser, cx);
            assert!(
                cx.debug_bounds("saved-stash-row-0").is_some(),
                "{at}: Stashes shows the saved stash"
            );
            assert!(
                cx.debug_bounds("palette-command-0").is_none(),
                "{at}: the palette closed"
            );
            assert!(
                cx.update(|window, cx| window
                    .focused(cx)
                    .is_some_and(|focused| focused == list || focused.contains(&list, window))),
                "{at}: focus is in Stashes"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}: Escape closes Stashes"
            );

            // While a write runs, and through the reload after it, the
            // entry says why, as the menu item waits ("Fetching…", then
            // "Refreshing…"), and Return says so again and opens nothing.
            let value =
                |id: &str| written::get(id).and_then(|line| line.node.value().map(str::to_owned));
            for (state, reason) in [
                ("during a write", crate::workspace::BUSY_REASON),
                (
                    "through the reload after a write",
                    crate::workspace::REFRESHING_ROWS_REASON,
                ),
            ] {
                app.update(cx, |app, _| {
                    if reason == crate::workspace::BUSY_REASON {
                        app.operation_busy = Some("Fetching…");
                    } else {
                        app.status_task = Some(Task::ready(()));
                        app.status_after_write = Some(app.work_generation);
                    }
                });
                assert!(
                    app.read_with(cx, |app, _| app.menu_wait().is_some()),
                    "{at}: {state}, the Actions menu's item waits"
                );
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.open_command_palette(window, cx))
                });
                draw(cx);
                cx.simulate_input("stash");
                draw(cx);
                assert_eq!(
                    value("command-palette-help").as_deref(),
                    Some(reason),
                    "{at}: {state}, the highlighted Browse stashes… says why it waits"
                );
                cx.simulate_keystrokes("enter");
                enter_up(cx);
                draw(cx);
                assert!(
                    cx.debug_bounds("palette-command-0").is_some()
                        && cx.debug_bounds("saved-stash-row-0").is_none(),
                    "{at}: {state}, Return leaves the palette open and opens no Stashes"
                );
                assert_eq!(
                    value("command-palette-error").as_deref(),
                    Some(reason),
                    "{at}: {state}, and says why"
                );
                cx.simulate_keystrokes("escape");
                draw(cx);
                app.update(cx, |app, _| {
                    app.operation_busy = None;
                    app.status_task = None;
                    app.status_after_write = None;
                });
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A dialog's list as the window's tree holds its rows frame by frame
    /// and, on Linux, the window's AT-SPI adapter fed those frames, as
    /// GPUI's Linux backend feeds `accesskit_unix`.
    struct ListRows {
        list: &'static str,
        #[cfg(target_os = "linux")]
        bus: crate::native_accessibility::atspi_state_tests::Bus,
    }

    impl ListRows {
        /// Starts on the window's next frame.
        fn new(cx: &mut VisualTestContext, list: &'static str) -> Self {
            #[cfg(target_os = "linux")]
            let bus = crate::native_accessibility::atspi_state_tests::Bus::new(
                crate::native_accessibility::atspi_state_tests::next_frame(cx),
            );
            #[cfg(not(target_os = "linux"))]
            crate::tags::tests::draw(cx);
            Self {
                list,
                #[cfg(target_os = "linux")]
                bus,
            }
        }

        /// Draws a frame and returns the list's rows the window's tree then
        /// holds, by node, with their names.
        fn frame(
            &mut self,
            cx: &mut VisualTestContext,
        ) -> std::collections::HashMap<String, Option<String>> {
            use crate::native_accessibility::written;
            #[cfg(target_os = "linux")]
            self.bus
                .update(crate::native_accessibility::atspi_state_tests::next_frame(
                    cx,
                ));
            #[cfg(not(target_os = "linux"))]
            {
                written::forget();
                crate::tags::tests::draw(cx);
            }
            written::inside(self.list)
                .into_iter()
                .filter_map(|id| {
                    let row = written::get(&id)?;
                    (row.node.role() == Role::ListBoxOption)
                        .then(|| (id, row.node.label().map(str::to_owned)))
                })
                .collect()
        }

        /// The list items the adapter reported renamed since the last call,
        /// by the name each reads now; none off Linux, which has no adapter
        /// here.
        fn renamed(&mut self) -> Vec<String> {
            #[cfg(target_os = "linux")]
            {
                self.bus
                    .renamed()
                    .into_iter()
                    .filter(|(role, _)| *role == accesskit_atspi_common::Role::ListItem)
                    .map(|(_, name)| name)
                    .collect()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Vec::new()
            }
        }
    }

    /// Asserts that each row node `after` holds that `before` held too reads
    /// the name it read, and that some do.
    fn renames_none(
        before: &std::collections::HashMap<String, Option<String>>,
        after: &std::collections::HashMap<String, Option<String>>,
    ) {
        let kept: Vec<_> = after
            .iter()
            .filter(|(id, _)| before.contains_key(*id))
            .collect();
        assert!(
            !kept.is_empty(),
            "rows stay in the tree across the search's change: {before:?} → {after:?}"
        );
        let renamed: Vec<_> = kept
            .iter()
            .filter(|(id, name)| before[*id] != **name)
            .map(|(id, name)| format!("{id}: {:?} → {name:?}", before[*id]))
            .collect();
        assert_eq!(
            renamed,
            Vec::<String>::new(),
            "a row node keeps its name across the search's change"
        );
    }

    /// A query changed after a move takes the palette's rows away and adds
    /// others, renaming none: after "go" and Down, Backspace ("g") puts
    /// GitHub pull requests… ahead of Go to Projects, and each row node the
    /// window's tree holds before and after it keeps its name. On Linux the
    /// AT-SPI adapter reports no list item renamed. Each row was its place
    /// among the results, so Backspace renamed the node at each place, and
    /// Orca would speak the command now in it, "not selected", as it did in
    /// Keyboard Shortcuts' list (QA-92).
    #[gpui::test]
    fn a_query_changed_after_a_move_renames_no_command(cx: &mut TestAppContext) {
        let (_, cx) = hub(cx);
        let mut rows = ListRows::new(cx, "command-palette-results");
        // Backspace moves a command the list shows to another place.
        let (go, g) = (matches("go"), matches("g"));
        assert!(
            (0..go.len().min(g.len()).min(7)).any(|place| go[place] != g[place]),
            "\"g\" lists another command where \"go\" listed one"
        );
        cx.simulate_input("go");
        rows.frame(cx);
        cx.simulate_keystrokes("down");
        let before = rows.frame(cx);
        rows.renamed();
        cx.simulate_keystrokes("backspace");
        let after = rows.frame(cx);
        let github = |rows: &std::collections::HashMap<String, Option<String>>| {
            rows.values()
                .flatten()
                .any(|name| name.starts_with("GitHub pull requests"))
        };
        assert!(
            !github(&before) && github(&after),
            "Backspace leaves \"g\": {before:?} → {after:?}"
        );
        renames_none(&before, &after);
        assert_eq!(
            rows.renamed(),
            Vec::<String>::new(),
            "the adapter reports no command renamed"
        );
    }

    /// Quick Open's query changed after a move takes rows away and adds
    /// others, renaming none: after Down in the whole list (a.txt,
    /// b-notes.txt, c-notes.txt), "notes" drops a.txt, so b-notes.txt and
    /// c-notes.txt each move up a place, and each row node the window's tree
    /// holds before and after it keeps its name. On Linux the AT-SPI adapter
    /// reports no list item renamed. Each row was its place in the results,
    /// so the search renamed the node at each place, and Orca would speak
    /// the path now in it, "not selected", as it did in Keyboard Shortcuts'
    /// list (QA-92).
    #[gpui::test]
    async fn quick_opens_query_changed_after_a_move_renames_no_path(cx: &mut TestAppContext) {
        use crate::tags::tests::{git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for name in ["a.txt", "b-notes.txt", "c-notes.txt"] {
            std::fs::write(repo.path().join(name), "text\n").unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add files"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_quick_file(window, cx)));
        crate::tags::tests::draw(cx);
        crate::revision_inspection::settle_quick_open(cx).await;
        let mut rows = ListRows::new(cx, "quick-open-results");
        let names = |rows: &std::collections::HashMap<String, Option<String>>| {
            let mut names: Vec<_> = rows.values().flatten().cloned().collect();
            names.sort();
            names
        };
        assert_eq!(
            names(&rows.frame(cx)),
            ["a.txt", "b-notes.txt", "c-notes.txt"],
            "the whole list"
        );
        cx.simulate_keystrokes("down");
        let before = rows.frame(cx);
        rows.renamed();
        cx.simulate_input("notes");
        crate::revision_inspection::settle_quick_open(cx).await;
        let after = rows.frame(cx);
        assert_eq!(
            names(&after),
            ["b-notes.txt", "c-notes.txt"],
            "\"notes\" drops a.txt"
        );
        renames_none(&before, &after);
        assert_eq!(
            rows.renamed(),
            Vec::<String>::new(),
            "the adapter reports no path renamed"
        );
        cx.simulate_keystrokes("escape");
        crate::tags::tests::draw(cx);
    }

    /// What a screen reader hears from the command palette, frame by frame:
    /// its status's name each time it changes while it is polite, as AT-SPI
    /// announces a live node whose name changes, and none it takes quietly;
    /// on Linux every announcement the window's AT-SPI adapter itself made,
    /// fed each frame's tree, so a second live node announcing beside the
    /// status is heard too, and the command the list holds selected each
    /// time the adapter reported a change of its selection, which Orca
    /// speaks; and of both, what Orca 51.1's spam filter lets it speak
    /// (`atspi_state_tests::Bus::heard_by_orca`).
    struct Heard {
        named: Option<String>,
        /// The status's node, by element ID: a fresh one announces its name
        /// as it is added, the name before included.
        status: Option<String>,
        heard: Vec<String>,
        /// The list's selected row as the window's tree last held it, by
        /// element ID, with its name.
        row: Option<(String, Option<String>)>,
        /// Each name the selected row's node took while it stayed selected.
        renamed: Vec<String>,
        #[cfg(target_os = "linux")]
        bus: crate::native_accessibility::atspi_state_tests::Bus,
        #[cfg(target_os = "linux")]
        adapter: Vec<String>,
        #[cfg(target_os = "linux")]
        listed: Vec<String>,
        #[cfg(target_os = "linux")]
        orca: Vec<String>,
        /// Orca's locus of focus, where a test follows it
        /// (`atspi_state_tests::Bus::heard_by_orca_from`).
        #[cfg(target_os = "linux")]
        locus: Option<Option<accesskit_atspi_common::NodeId>>,
        /// The name of each list item the AT-SPI adapter reported renamed,
        /// as it read then.
        #[cfg(target_os = "linux")]
        items_renamed: Vec<String>,
    }

    impl Heard {
        /// The palette's status, by element ID.
        const STATUS: &str = "command-palette-current-result";

        /// Starts listening to the open palette, as it opened already heard.
        fn new(cx: &mut VisualTestContext) -> Self {
            #[cfg(target_os = "linux")]
            let bus = crate::native_accessibility::atspi_state_tests::Bus::new(
                crate::native_accessibility::atspi_state_tests::next_frame(cx),
            );
            let mut heard = Self {
                named: None,
                status: None,
                heard: Vec::new(),
                row: None,
                renamed: Vec::new(),
                #[cfg(target_os = "linux")]
                bus,
                #[cfg(target_os = "linux")]
                adapter: Vec::new(),
                #[cfg(target_os = "linux")]
                listed: Vec::new(),
                #[cfg(target_os = "linux")]
                orca: Vec::new(),
                #[cfg(target_os = "linux")]
                locus: None,
                #[cfg(target_os = "linux")]
                items_renamed: Vec::new(),
            };
            heard.frame(cx);
            heard.heard.clear();
            heard.renamed.clear();
            #[cfg(target_os = "linux")]
            {
                heard.adapter.clear();
                heard.listed.clear();
                heard.orca.clear();
                heard.items_renamed.clear();
            }
            heard
        }

        /// Draws a frame and records what it announces.
        fn frame(&mut self, cx: &mut VisualTestContext) {
            use crate::native_accessibility::written;
            #[cfg(target_os = "linux")]
            let tree = crate::native_accessibility::atspi_state_tests::next_frame(cx);
            #[cfg(not(target_os = "linux"))]
            {
                written::forget();
                crate::tags::tests::draw(cx);
            }
            // The status is polite, or off for a change it takes quietly,
            // and carries its reading as its name, which AT-SPI announces,
            // and its value, which macOS's adapter announces.
            let mut polite = true;
            let status = written::announced_from(Self::STATUS);
            assert!(status.len() <= 1, "the palette has one status: {status:?}");
            let status = status.into_iter().next().map(|(id, live)| {
                polite = live.live == gpui::accesskit::Live::Polite;
                assert!(
                    polite || live.live == gpui::accesskit::Live::Off,
                    "the palette's status is polite, or quiet: {:?}",
                    live.live
                );
                let name = written::get(&id).and_then(|status| {
                    assert_eq!(
                        status.node.value(),
                        status.node.label(),
                        "the status's value is its name"
                    );
                    status.node.label().map(str::to_owned)
                });
                (id, name)
            });
            let (id, name) = status.unzip();
            let name = name.flatten();
            let fresh = id.is_some() && id != self.status;
            if name != self.named || fresh {
                if polite {
                    self.heard.extend(name.clone());
                }
                self.named = name;
            }
            self.status = id;
            let row = written::inside("command-palette-results")
                .into_iter()
                .find_map(|id| {
                    let row = written::get(&id)?;
                    (row.node.role() == Role::ListBoxOption && row.node.is_selected() == Some(true))
                        .then(|| (id, row.node.label().map(str::to_owned)))
                });
            if let (Some((id, name)), Some((was, named))) = (&row, &self.row)
                && id == was
                && name != named
            {
                self.renamed.extend(name.clone());
            }
            self.row = row;
            #[cfg(target_os = "linux")]
            {
                self.bus.update(tree);
                self.adapter.extend(self.bus.announced());
                self.listed.extend(self.bus.selection_changed());
                let now = cx.executor().now();
                let orca = match &mut self.locus {
                    Some(locus) => self.bus.heard_by_orca_from(now, locus),
                    None => self.bus.heard_by_orca(now),
                };
                self.orca.extend(orca);
                self.items_renamed.extend(
                    self.bus
                        .renamed()
                        .into_iter()
                        .filter(|(role, _)| *role == accesskit_atspi_common::Role::ListItem)
                        .map(|(_, name)| name),
                );
            }
        }

        /// The selected row's name as the window's tree holds it.
        fn row_name(&self) -> Option<&str> {
            self.row.as_ref().and_then(|(_, name)| name.as_deref())
        }

        /// The names the selected row's node took while it stayed selected,
        /// since the last call. Where that name does not carry a change of
        /// context (macOS), it follows the live context at once, and the
        /// tests assert those renames behind `cfg(not(target_os = "linux"))`
        /// rather than behind [`ROW_CARRIES_CONTEXT`], so that a hold there
        /// fails them.
        fn take_renamed(&mut self) -> Vec<String> {
            std::mem::take(&mut self.renamed)
        }

        /// The names the AT-SPI adapter reported the list item showing
        /// `command` renamed to since the last call, each of which Orca
        /// speaks while that item is the list's selected one (QA-96); the
        /// other items' renames since then are dropped.
        #[cfg(target_os = "linux")]
        fn take_items_renamed(&mut self, command: &str) -> Vec<String> {
            std::mem::take(&mut self.items_renamed)
                .into_iter()
                .filter(|name| shows(name, command))
                .collect()
        }
    }

    /// Whether a row named `name` shows the command labelled `command`.
    #[cfg(target_os = "linux")]
    fn shows(name: &str, command: &str) -> bool {
        name == command
            || name
                .strip_prefix(command)
                .is_some_and(|rest| rest.starts_with(". Unavailable: "))
    }

    impl Heard {
        /// Lets the test clock run `millis` and draws, recording what the
        /// frame announces.
        fn wait(&mut self, cx: &mut VisualTestContext, millis: u64) {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(millis));
            cx.run_until_parked();
            self.frame(cx);
        }

        /// Moves the selection by `key` 200 ms after the frame before, so
        /// that Orca surely spoke the list's change ([`Listed::spoken()`]),
        /// and draws.
        fn step(&mut self, cx: &mut VisualTestContext, key: &str) {
            self.wait(cx, 200);
            cx.simulate_keystrokes(key);
            self.frame(cx);
        }

        /// What was announced since the last call, which on Linux is all the
        /// AT-SPI adapter announced, nothing besides it.
        fn take(&mut self) -> Vec<String> {
            let heard = std::mem::take(&mut self.heard);
            #[cfg(target_os = "linux")]
            assert_eq!(
                std::mem::take(&mut self.adapter),
                heard,
                "the AT-SPI adapter announces what the status's name says, and nothing else"
            );
            heard
        }

        /// The commands the list held selected each time the AT-SPI adapter
        /// reported a change of its selection since the last call, each of
        /// which Orca speaks.
        #[cfg(target_os = "linux")]
        fn take_listed(&mut self) -> Vec<String> {
            std::mem::take(&mut self.listed)
        }

        /// What Orca spoke of those since the last call, in order.
        #[cfg(target_os = "linux")]
        fn take_orca(&mut self) -> Vec<String> {
            std::mem::take(&mut self.orca)
        }
    }

    /// The place of `query`'s result `at` among its results ("2 of 3"): all
    /// the status says of a change of selection the list spoke
    /// ([`shortcuts::LIST_SPEAKS_SELECTION`], Linux), whose row names the
    /// command and any reason it waits.
    fn place_of(query: &str, at: usize) -> String {
        format!("{} of {}", at + 1, matches(query).len())
    }

    /// The status's whole reading and the row's name for place `at` among
    /// `query`'s results in `availability`.
    fn reading_and_row(query: &str, at: usize, availability: &Availability) -> (String, String) {
        let results = matches(query);
        let spec = COMMANDS[results[at]];
        let reason = spec.id.reason(availability);
        let status = format!(
            "{} · {}{}",
            spec.label,
            place_of(query, at),
            reason
                .map(|reason| format!(" · Unavailable: {reason}"))
                .unwrap_or_default()
        );
        let row = format!(
            "{}{}",
            spec.label,
            reason
                .map(|reason| format!(". Unavailable: {reason}"))
                .unwrap_or_default()
        );
        (status, row)
    }

    /// Each Up and Down in the palette is spoken once, in the whole list
    /// and in a search's, and a Down to a row the list had not drawn too.
    /// Where the list speaks a moved selection itself
    /// ([`shortcuts::LIST_SPEAKS_SELECTION`], Linux), the status announces
    /// no move and the AT-SPI adapter reports the list's change of
    /// selection, whose command Orca speaks; elsewhere the status announces
    /// the command each move reaches with its place, at once. The status
    /// announced every move on Linux too, and Orca 51.1 spoke a Down twice,
    /// 41 ms apart, "Go to History · 2 of 3" and then "Go to History"
    /// (QA-94).
    #[gpui::test]
    fn a_move_is_left_to_the_list_where_it_speaks_it(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        let moved = |cx: &mut VisualTestContext, heard: &mut Heard, keys: &str, query: &str, at| {
            cx.simulate_keystrokes(keys);
            heard.frame(cx);
            let (status, row) = reading_and_row(query, at, &availability);
            assert_eq!(
                heard.named.as_deref(),
                Some(status.as_str()),
                "{keys} in {query:?}'s list reaches place {at}"
            );
            let own = if shortcuts::LIST_SPEAKS_SELECTION {
                Vec::new()
            } else {
                vec![status]
            };
            assert_eq!(
                heard.take(),
                own,
                "{keys} in {query:?}'s list: the app announces the command only where the list does not"
            );
            #[cfg(target_os = "linux")]
            assert_eq!(
                heard.take_listed(),
                [row],
                "{keys} in {query:?}'s list: the list reports its selection changed, and Orca speaks the command"
            );
            #[cfg(not(target_os = "linux"))]
            let _ = row;
        };
        let start = first_available(&matches(""), &availability);
        assert_eq!(start, 0, "the whole list starts on its first command");
        moved(cx, &mut heard, "down", "", 1);
        moved(cx, &mut heard, "down", "", 2);
        moved(cx, &mut heard, "up", "", 1);
        // Down to the row past the list's foot, which it had not drawn.
        let drawn = (0..matches("").len())
            .filter(|&row| {
                cx.debug_bounds(format!("palette-command-{row}").leak())
                    .is_some()
            })
            .max()
            .expect("the list draws rows");
        for at in 2..=drawn + 1 {
            if at == drawn + 1 {
                assert!(
                    cx.debug_bounds(format!("palette-command-{at}").leak())
                        .is_none(),
                    "row {at} is not drawn before Down reaches it"
                );
            }
            moved(cx, &mut heard, "down", "", at);
        }

        // A search's list: its first available command, then Down and Up.
        cx.simulate_input("go");
        heard.wait(cx, 2000);
        let first = first_available(&matches("go"), &availability);
        assert_eq!(
            heard.take(),
            [reading_and_row("go", first, &availability).0],
            "after the moves the search announces the command it selects, with its place, once"
        );
        #[cfg(target_os = "linux")]
        heard.take_listed();
        assert!(
            matches("go").len() > first + 1,
            "\"go\" finds more than one command"
        );
        assert_eq!(
            heard.named,
            Some(reading_and_row("go", first, &availability).0),
            "the search selects its first available command"
        );
        moved(cx, &mut heard, "down", "go", first + 1);
        moved(cx, &mut heard, "up", "go", first);
        heard.wait(cx, 2000);
        // Down and Up in one instant, where Orca drops Up's change as one of
        // a burst: where the list speaks the moves, the rest says the command
        // Up reached, whole.
        let again = if shortcuts::LIST_SPEAKS_SELECTION {
            vec![reading_and_row("go", first, &availability).0]
        } else {
            Vec::new()
        };
        assert_eq!(
            heard.take(),
            again,
            "and after them only the command Up reached, where Orca may have missed it"
        );
    }

    /// QA-105: a change of selection is heard once, made by Down or by a
    /// query typed 150 ms a key. The selected row's accessible name is its
    /// command and any reason it waits, which the list speaks where it
    /// speaks a change of its selection itself
    /// ([`shortcuts::LIST_SPEAKS_SELECTION`], Linux): there a move announces
    /// nothing of the status's own, and the rest of a query whose change of
    /// selection Orca spoke announces only what the row does not say, the
    /// command's place ("1 of 4"), never the command or the reason again,
    /// here "Open a repository first". Elsewhere (macOS) the status
    /// announces the whole reading each time. A reading no change of
    /// selection led to stays
    /// whole on both: a query that leaves the selected command at another
    /// place, and "No matching commands". The status announced "Go to
    /// History · 1 of 1" after Orca 51.1 had spoken the row's "Go to
    /// History" (QA-105). A query whose last change of selection Orca
    /// dropped keeps the command in the status's reading
    /// ([`a_burst_of_keys_announces_once_the_command_it_rests_on`]).
    #[gpui::test]
    fn a_change_of_selection_is_heard_once(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        // Past the list's change of selection as the palette opened.
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }

        // Down twice, a second apart.
        for at in [1, 2] {
            cx.simulate_keystrokes("down");
            heard.frame(cx);
            let (status, row) = reading_and_row("", at, &availability);
            assert_eq!(
                heard.row_name(),
                Some(row.as_str()),
                "Down selects row {at}, named by its command"
            );
            let own = if shortcuts::LIST_SPEAKS_SELECTION {
                Vec::new()
            } else {
                vec![status]
            };
            assert_eq!(
                heard.take(),
                own,
                "Down to row {at}: the status announces the command only where the list does not \
                 speak it"
            );
            #[cfg(target_os = "linux")]
            assert_eq!(
                heard.take_orca(),
                [row],
                "Orca speaks the row Down reaches, once"
            );
            heard.wait(cx, 1000);
            assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        }
        #[cfg(target_os = "linux")]
        heard.take_listed();

        // Keys typed 150 ms apart, each a frame of its own, and then the
        // query's rest.
        let typed = |cx: &mut VisualTestContext, heard: &mut Heard, keys: &str| {
            for key in keys.split_inclusive(|_| true) {
                cx.simulate_input(key);
                heard.frame(cx);
                heard.wait(cx, 150);
            }
            heard.wait(cx, 2000);
        };
        // What the rest of `query` is heard as: where its keys `changed` the
        // selection and the list speaks that (Linux), the row it selects and
        // then the status's place; otherwise the status's whole reading.
        let rests = |heard: &mut Heard, query: &str, changed: bool| {
            let at = first_available(&matches(query), &availability);
            let (whole, row) = reading_and_row(query, at, &availability);
            assert_eq!(
                heard.row_name(),
                Some(row.as_str()),
                "{query:?} selects its command's row"
            );
            let said = if changed && shortcuts::LIST_SPEAKS_SELECTION {
                place_of(query, at)
            } else {
                whole
            };
            assert_eq!(
                heard.take(),
                std::slice::from_ref(&said),
                "{query:?}'s rest announces {said:?}, once"
            );
            assert_eq!(heard.named.as_ref(), Some(&said), "and the status reads it");
            #[cfg(target_os = "linux")]
            {
                let listed = heard.take_listed();
                let orca = heard.take_orca();
                if changed {
                    assert_eq!(
                        listed.last(),
                        Some(&row),
                        "{query:?}: the list reports its change of selection to {row:?}"
                    );
                    assert!(
                        orca.ends_with(&[row, said]),
                        "{query:?}: Orca speaks the row and then its place, the command once: \
                         {orca:?}"
                    );
                } else {
                    assert_eq!(
                        listed,
                        Vec::<String>::new(),
                        "{query:?}: the list reports no change of selection"
                    );
                    assert_eq!(orca, [said], "{query:?}: Orca speaks the whole reading");
                }
            }
        };
        let selected = |query: &str| {
            let results = matches(query);
            results[first_available(&results, &availability)]
        };

        // A query that selects Go to History.
        typed(cx, &mut heard, "hist");
        assert_eq!(
            COMMANDS[selected("hist")].id,
            CommandId::History,
            "\"hist\" selects Go to History"
        );
        rests(&mut heard, "hist", true);
        // One that leaves it selected, the one command it matches.
        typed(cx, &mut heard, " go");
        assert_eq!(
            matches("hist go"),
            [selected("hist")],
            "\"hist go\" matches it alone"
        );
        rests(&mut heard, "hist go", false);
        // One that matches none.
        typed(cx, &mut heard, "x");
        assert!(
            matches("hist gox").is_empty(),
            "\"hist gox\" matches nothing"
        );
        assert_eq!(
            heard.take(),
            ["No matching commands"],
            "a query that matches nothing is announced whole"
        );
        assert_eq!(heard.named.as_deref(), Some("No matching commands"));
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            assert_eq!(
                heard.take_orca(),
                ["No matching commands"],
                "and Orca speaks it"
            );
        }
        // Backspace brings back the one command, a change of selection.
        cx.simulate_keystrokes("backspace");
        heard.frame(cx);
        heard.wait(cx, 2000);
        rests(&mut heard, "hist go", true);
    }

    /// A change of selection whose frame is drawn late, as on a loaded host,
    /// keeps the command in the status's reading. "i" selects Pin or unpin
    /// this repository and its frame is drawn 140 ms after it is handled;
    /// "t", handled 156 ms after "i", selects Go to History in a frame drawn
    /// at once, so the AT-SPI adapter reports the two changes 16 ms apart
    /// and Orca 51.1 drops the second. The rest announces Go to History's
    /// whole reading, the last thing Orca says, and Orca never names it
    /// otherwise. Timed as their keys were handled, the second change was
    /// taken as spoken and the status said "1 of 4" alone, Go to History's
    /// place after the row Orca spoke for "i".
    #[gpui::test]
    fn a_late_frame_keeps_the_command_in_the_reading(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        heard.wait(cx, 1000);
        cx.simulate_input("h");
        heard.frame(cx);
        heard.wait(cx, 2000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        // "i", whose frame is drawn 140 ms after it is handled.
        cx.simulate_input("i");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(140));
        heard.frame(cx);
        let first = |query: &str| first_available(&matches(query), &availability);
        let (_, pin) = reading_and_row("hi", first("hi"), &availability);
        assert_eq!(
            heard.row_name(),
            Some(pin.as_str()),
            "\"hi\" selects {pin:?}"
        );
        // "s" and "t" 16 ms after that frame, drawn at once.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(16));
        cx.simulate_input("st");
        heard.frame(cx);
        let (whole, row) = reading_and_row("hist", first("hist"), &availability);
        assert_eq!(
            heard.row_name(),
            Some(row.as_str()),
            "\"hist\" selects {row:?}"
        );
        heard.wait(cx, 2000);
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&whole),
            "the rest announces the command with its place"
        );
        #[cfg(target_os = "linux")]
        {
            let orca = heard.take_orca();
            assert_eq!(
                orca,
                [pin, whole],
                "Orca speaks the row \"i\" selected, drops the change \"t\" made 16 ms after it, \
                 and speaks the status's reading"
            );
        }
    }

    /// A change of selection reported soon after the one before it keeps the
    /// command in the status's reading even where the frame after the one
    /// that drew it comes late, as behind a long task. "i" selects Pin or
    /// unpin this repository, drawn and timed at once; "t", 60 ms later,
    /// selects Go to History in a frame drawn at once, so the AT-SPI adapter
    /// reports it 60 ms after "i"'s change and Orca 51.1 drops it, and the
    /// frame after that one comes 110 ms later. The rest announces Go to
    /// History's whole reading. Timed by that late frame, 170 ms after
    /// "i"'s, the change was taken as spoken and the status said "1 of 4"
    /// alone after Orca had said Pin's name. The test's AT-SPI model sees
    /// the window's tree only with frames that also run the next frame's
    /// callbacks, so it hears "t"'s change at 170 ms and cannot show Orca
    /// dropping it; the status is what this checks.
    #[gpui::test]
    fn a_late_frame_after_a_change_keeps_the_command_in_the_reading(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        heard.wait(cx, 1000);
        cx.simulate_input("h");
        heard.frame(cx);
        heard.wait(cx, 2000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        // "i", drawn and timed at once.
        cx.simulate_input("i");
        heard.frame(cx);
        // "s" and "t" 60 ms later, drawn at once, and the frame after that
        // one 110 ms later.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(60));
        cx.simulate_input("st");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(110));
        heard.frame(cx);
        let results = matches("hist");
        let (whole, row) = reading_and_row(
            "hist",
            first_available(&results, &availability),
            &availability,
        );
        assert_eq!(
            heard.row_name(),
            Some(row.as_str()),
            "\"hist\" selects {row:?}"
        );
        heard.wait(cx, 2000);
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&whole),
            "the rest announces the command with its place"
        );
        #[cfg(target_os = "linux")]
        {
            let orca = heard.take_orca();
            assert_eq!(
                orca.last(),
                Some(&whole),
                "Orca speaks the status's reading last: {orca:?}"
            );
        }
    }

    /// A query that keeps the selected command reads whole, even where a
    /// click outside any rest selected that command: a click on a command
    /// the context cannot run, which leaves the palette open, and, 5 s
    /// later, a query that keeps that command selected at another place.
    /// The rest announces the whole reading, on every platform: the click,
    /// heard from the list on Linux, led to no rest. Counted toward the
    /// query's rest, the click had the status say the place alone, "1 of
    /// 1", though no change of selection led to it.
    #[gpui::test]
    fn a_query_that_keeps_a_clicked_command_reads_whole(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        // A key whose list draws, past its first rows, a command the context
        // cannot run, and letters typed after it that keep that command
        // selected at each key, at another place: the shortest part of its
        // label that starts with the key.
        let selects = |query: &str, command: usize| {
            let found = matches(query);
            let first = first_available(&found, &availability);
            (found.get(first) == Some(&command)).then_some(first)
        };
        let (key, at, then) = ('a'..='z')
            .map(String::from)
            .find_map(|key| {
                let results = matches(&key);
                (1..results.len().min(6)).find_map(|at| {
                    let spec = COMMANDS[results[at]];
                    spec.id.reason(&availability)?;
                    let label = spec.label.to_lowercase();
                    (2..=label.len())
                        .flat_map(|len| (0..=label.len() - len).map(move |start| (start, len)))
                        .filter_map(|(start, len)| label.get(start..start + len))
                        .filter(|part| part.starts_with(key.as_str()))
                        .find(|part| {
                            (key.len() + 1..=part.len())
                                .filter_map(|end| part.get(..end))
                                .all(|typed| selects(typed, results[at]).is_some())
                                && selects(part, results[at]) != Some(at)
                        })
                        .map(|part| (key.clone(), at, part[key.len()..].to_owned()))
                })
            })
            .expect("a command the context cannot run, and letters that keep it selected");
        let query = format!("{key}{then}");
        cx.simulate_input(&key);
        heard.frame(cx);
        heard.wait(cx, 2000);
        // A second later, past the frame that timed the key's change.
        heard.wait(cx, 1000);
        heard.take();
        let row = cx
            .debug_bounds(format!("palette-command-{at}").leak())
            .expect("the list draws the command's row");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        heard.frame(cx);
        assert_eq!(
            heard.row_name(),
            Some(reading_and_row(&key, at, &availability).1.as_str()),
            "the click selects the command"
        );
        heard.wait(cx, 5000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        cx.simulate_input(&then);
        heard.frame(cx);
        heard.wait(cx, 2000);
        let found = matches(&query);
        let whole = reading_and_row(
            &query,
            first_available(&found, &availability),
            &availability,
        )
        .0;
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&whole),
            "{query:?}, which keeps the clicked command selected, reads whole"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_orca(),
            [whole],
            "Orca speaks the whole reading, the list's selection unchanged"
        );
    }

    /// A click on a command the context cannot run, which leaves the palette
    /// open on it and shows why, is a change of selection as a key's is: the
    /// status takes the command's whole reading at once, announcing it where
    /// the list does not speak its selection (macOS) and taking it quietly
    /// where it does (Linux), where Orca speaks the row, which names the
    /// command and why it cannot run. The click left the status on the
    /// reading before.
    #[gpui::test]
    fn a_click_on_a_command_that_cannot_run_reads_it(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        // A key whose list draws, past its first rows, a command the context
        // cannot run.
        let (key, at) = ('a'..='z')
            .map(String::from)
            .find_map(|key| {
                let results = matches(&key);
                let first = first_available(&results, &availability);
                (1..results.len().min(6))
                    .find(|&at| {
                        at != first && COMMANDS[results[at]].id.reason(&availability).is_some()
                    })
                    .map(|at| (key, at))
            })
            .expect("a key that lists a command the context cannot run");
        cx.simulate_input(&key);
        heard.frame(cx);
        heard.wait(cx, 2000);
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        let row = cx
            .debug_bounds(format!("palette-command-{at}").leak())
            .expect("the list draws the command's row");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        heard.frame(cx);
        let (whole, named) = reading_and_row(&key, at, &availability);
        assert_eq!(
            heard.row_name(),
            Some(named.as_str()),
            "the click selects the command"
        );
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "and leaves the palette open"
        );
        assert!(
            cx.debug_bounds("command-palette-error").is_some(),
            "showing why the command cannot run"
        );
        assert_eq!(
            heard.named.as_deref(),
            Some(whole.as_str()),
            "the status reads the command clicked"
        );
        let own = if shortcuts::LIST_SPEAKS_SELECTION {
            Vec::new()
        } else {
            vec![whole]
        };
        assert_eq!(
            heard.take(),
            own,
            "the click is the list's where it speaks it, and the status's elsewhere"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_orca(),
            [named],
            "Orca speaks the row clicked, once"
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
    }

    /// Down and Up 50 ms apart leave Orca's last word Down's command, since
    /// it drops Up's change as one of a burst, though the selection rests on
    /// the command Up came back to, the reading the status took before:
    /// where the list speaks its selection (Linux), the rest says that
    /// command's whole reading, once, from a fresh node of the status, whose
    /// name already says it. 200 ms apart, Orca surely spoke both, and the
    /// rest says nothing. A key that selects another command and Backspace
    /// 60 ms later end on the reading they started from the same way. The
    /// rest said nothing, and Orca's last word was a command no longer
    /// selected. On macOS the status announces each move at once, and the
    /// rests nothing.
    #[gpui::test]
    fn a_burst_that_comes_back_to_its_reading_is_heard_once(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        let first = reading_and_row("", 0, &availability).0;
        let second = reading_and_row("", 1, &availability).0;
        // The rows Orca speaks, where the list speaks its selection.
        #[cfg(target_os = "linux")]
        let (first_row, second_row) = (
            reading_and_row("", 0, &availability).1,
            reading_and_row("", 1, &availability).1,
        );
        let again = |missed: bool| {
            if shortcuts::LIST_SPEAKS_SELECTION && missed {
                vec![first.clone()]
            } else {
                Vec::new()
            }
        };
        for (apart, missed) in [(50, true), (200, false)] {
            cx.simulate_keystrokes("down");
            heard.frame(cx);
            heard.wait(cx, apart);
            cx.simulate_keystrokes("up");
            heard.frame(cx);
            let own = if shortcuts::LIST_SPEAKS_SELECTION {
                Vec::new()
            } else {
                vec![second.clone(), first.clone()]
            };
            assert_eq!(
                heard.take(),
                own,
                "Down and Up {apart} ms apart are the list's where it speaks them"
            );
            heard.wait(cx, 2000);
            assert_eq!(
                heard.take(),
                again(missed),
                "Down and Up {apart} ms apart: the rest says the command Up came back to only \
                 where Orca may have missed it"
            );
            assert_eq!(heard.named.as_deref(), Some(first.as_str()));
            #[cfg(target_os = "linux")]
            assert_eq!(
                heard.take_orca(),
                if missed {
                    vec![second_row.clone(), first.clone()]
                } else {
                    vec![second_row.clone(), first_row.clone()]
                },
                "Down and Up {apart} ms apart: what Orca says"
            );
            heard.wait(cx, 1000);
        }

        // A key that selects another command, and Backspace 60 ms later.
        let key = ('a'..='z')
            .map(String::from)
            .find(|key| {
                let found = matches(key);
                found.get(first_available(&found, &availability)) != Some(&matches("")[0])
            })
            .expect("a key that selects another command");
        cx.simulate_input(&key);
        heard.frame(cx);
        heard.wait(cx, 60);
        cx.simulate_keystrokes("backspace");
        heard.frame(cx);
        heard.wait(cx, 2000);
        assert_eq!(
            heard.take(),
            again(true),
            "{key:?} and Backspace 60 ms later: the rest says the command they came back to \
             where Orca may have missed it"
        );
        #[cfg(target_os = "linux")]
        {
            let found = matches(&key);
            let typed =
                reading_and_row(&key, first_available(&found, &availability), &availability).1;
            assert_eq!(
                heard.take_orca(),
                [typed, first],
                "Orca speaks {key:?}'s command, drops Backspace's change, and speaks the status"
            );
        }
    }

    /// Down held at 33 ms a repeat makes changes of selection Orca drops,
    /// each less than 100 ms after the one before: where the list speaks
    /// its selection (Linux), Orca says the first command Down reached and
    /// none after it, and the rest says the whole reading of the command the
    /// repeats end on, once, which went unheard (QA-94's quiet moves). A
    /// write that starts as the repeats end, which the rest renames that
    /// command's row for, is said there too: Orca does not speak the rename
    /// of a row it does not present, so the status, which says nothing at a
    /// rest that renames a row Orca surely spoke, says the whole reading
    /// beside it. On macOS the status announces each move at once, and the
    /// write's reading at the rest.
    #[gpui::test]
    fn held_down_is_heard_as_the_command_it_ends_on(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        let idle = availability(cx);
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        // Each key 33 ms after the one before.
        let held = |cx: &mut VisualTestContext, heard: &mut Heard, keys: &[&str]| {
            for (at, key) in keys.iter().enumerate() {
                if at > 0 {
                    heard.wait(cx, 33);
                }
                cx.simulate_keystrokes(key);
                heard.frame(cx);
            }
        };
        held(cx, &mut heard, &["down"; 5]);
        let moved: Vec<String> = (1..=5).map(|at| reading_and_row("", at, &idle).0).collect();
        let own = if shortcuts::LIST_SPEAKS_SELECTION {
            Vec::new()
        } else {
            moved.clone()
        };
        assert_eq!(
            heard.take(),
            own,
            "the moves are the list's where it speaks them"
        );
        heard.wait(cx, 2000);
        let rested = if shortcuts::LIST_SPEAKS_SELECTION {
            vec![moved[4].clone()]
        } else {
            Vec::new()
        };
        assert_eq!(
            heard.take(),
            rested,
            "the rest says the command the repeats end on where the list speaks the moves"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_orca(),
            [reading_and_row("", 1, &idle).1, moved[4].clone()],
            "Orca speaks the first command Down reached, drops the rest, and speaks the status"
        );
        held(cx, &mut heard, &["up"; 5]);
        heard.wait(cx, 2000);
        heard.take();
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }

        // Down held past a command a write makes wait, and Up back onto it,
        // as the write starts.
        let at = row_a_write(true, &idle);
        let mut keys = vec!["down"; at + 1];
        keys.push("up");
        held(cx, &mut heard, &keys);
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        heard.frame(cx);
        heard.take();
        heard.take_renamed();
        heard.wait(cx, 499);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing before the rest"
        );
        heard.wait(cx, 1);
        let (waiting, waiting_row) = reading_and_row("", at, &availability(cx));
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&waiting),
            "the rest says the command the repeats end on, waiting for the write, once"
        );
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                [waiting_row],
                "beside the row's one rename"
            );
        }
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_orca().last(),
            Some(&waiting),
            "Orca speaks the status's reading last"
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
    }

    /// A move inside the query's rest is the change of selection that rest
    /// follows: a key, Down 200 ms later and, 200 ms after that, a key that
    /// keeps Down's command selected at another place. Where the list speaks
    /// its selection (Linux) and Orca surely spoke Down's change, the rest
    /// says the command's place alone, which the row Orca spoke does not
    /// say; elsewhere (macOS) the whole reading. Taken as the move's own,
    /// Down led to no rest, and the status said the command a second time.
    #[gpui::test]
    fn a_move_inside_the_query_rest_leads_it(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        heard.wait(cx, 1000);
        heard.take();
        let first = |query: &str| first_available(&matches(query), &availability);
        // A key, and a second one that keeps the command Down reaches from
        // the first key's command selected, at another place.
        let letters = || ('a'..='z').map(String::from);
        let (key, then) = letters()
            .flat_map(|key| letters().map(move |then| (key.clone(), then)))
            .find(|(key, then)| {
                let (found, query) = (matches(key), format!("{key}{then}"));
                let down = first(key) + 1;
                let kept = matches(&query);
                found.len() > down
                    && kept.get(first(&query)) == Some(&found[down])
                    && first(&query) != down
            })
            .expect("a key, and a second one that keeps Down's command");
        let query = format!("{key}{then}");
        cx.simulate_input(&key);
        heard.frame(cx);
        heard.wait(cx, 200);
        cx.simulate_keystrokes("down");
        heard.frame(cx);
        let (moved, row) = reading_and_row(&key, first(&key) + 1, &availability);
        assert_eq!(heard.row_name(), Some(row.as_str()), "Down reaches {row:?}");
        let own = if shortcuts::LIST_SPEAKS_SELECTION {
            Vec::new()
        } else {
            vec![moved]
        };
        assert_eq!(
            heard.take(),
            own,
            "Down is the list's where it speaks it, and the status's at once elsewhere"
        );
        heard.wait(cx, 200);
        cx.simulate_input(&then);
        heard.frame(cx);
        heard.wait(cx, 2000);
        let at = first(&query);
        let said = if shortcuts::LIST_SPEAKS_SELECTION {
            place_of(&query, at)
        } else {
            reading_and_row(&query, at, &availability).0
        };
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&said),
            "{query:?}, which keeps Down's command, rests on {said:?}"
        );
        #[cfg(target_os = "linux")]
        assert!(
            heard.take_orca().ends_with(&[row, said]),
            "Orca speaks the row Down reached and then its place"
        );
    }

    /// A selected row the wheel brings back into the list is a change of
    /// selection: it enters the window's tree already selected, which the
    /// AT-SPI adapter reports as a change of the list's selection, and Orca
    /// speaks it. A key that selects another command 50 ms after it makes a
    /// change Orca 51.1 drops as one of a burst, so the query's rest
    /// announces the whole reading of the command it selects. Untimed, the
    /// row's return left that key's change timed against the palette's
    /// opening, and the rest said the command's place alone, the command
    /// unheard.
    #[gpui::test]
    fn a_selected_row_brought_back_into_the_list_is_a_change(cx: &mut TestAppContext) {
        row_brought_back(cx, 800.);
    }

    /// As [`a_selected_row_brought_back_into_the_list_is_a_change`], in a
    /// list one row tall, as a short window at a large text size leaves it:
    /// the list at its top draws its first row alone, the range the toolkit
    /// also lays out each frame to measure that row, and the row's return
    /// is read from the rows drawn ([`Palette::note_drawn_rows`]).
    #[gpui::test]
    fn a_selected_row_brought_back_into_a_list_one_row_tall_is_a_change(cx: &mut TestAppContext) {
        row_brought_back(cx, 354.);
    }

    /// As [`a_selected_row_brought_back_into_a_list_one_row_tall_is_a_change`],
    /// with the second command selected: the wheel takes the list to its
    /// top, where it draws its first row alone, and back onto the second
    /// row, whose return the AT-SPI adapter reports and Orca speaks, and a
    /// key 50 ms later makes a change Orca drops, so the rest says the whole
    /// reading of the command the key selects. Told apart from the toolkit's
    /// measuring of the first row by their range, the rows drawn at the top
    /// read as a measuring call, so the return went untimed, and the rest
    /// said the place alone, Orca's last word the second command.
    #[gpui::test]
    fn a_second_row_brought_back_into_a_list_one_row_tall_is_a_change(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        cx.simulate_resize(size(px(1200.), px(354.)));
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        heard.wait(cx, 1000);
        heard.step(cx, "down");
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        let results = matches("");
        // The row Orca speaks, where the list speaks its selection.
        #[cfg(target_os = "linux")]
        let second = reading_and_row("", 1, &availability).1;
        let key = ["g", "b", "t", "h", "c", "s", "r", "p"]
            .into_iter()
            .find(|key| {
                let found = matches(key);
                found.get(first_available(&found, &availability)) != Some(&results[1])
            })
            .expect("a key that selects another command");
        let row = "palette-command-1";
        assert!(
            cx.debug_bounds(row).is_some(),
            "Down selects the second row"
        );
        let centre = cx
            .debug_bounds("command-palette-results")
            .expect("the list")
            .center();
        crate::tags::tests::scroll_down(
            cx,
            centre,
            gpui::ScrollDelta::Pixels(point(px(0.), px(600.))),
        );
        heard.frame(cx);
        assert!(
            cx.debug_bounds(row).is_none() && cx.debug_bounds("palette-command-0").is_some(),
            "the wheel takes the list to its top, where it draws its first row alone"
        );
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        crate::tags::tests::scroll_down(
            cx,
            centre,
            gpui::ScrollDelta::Pixels(point(px(0.), px(-60.))),
        );
        heard.frame(cx);
        assert!(
            cx.debug_bounds(row).is_some(),
            "and back onto the second row"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_listed(),
            std::slice::from_ref(&second),
            "the adapter reports the row's return as a change of the list's selection"
        );
        // A key that selects another command 50 ms later.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        cx.simulate_input(key);
        heard.frame(cx);
        heard.wait(cx, 2000);
        let found = matches(key);
        let whole = reading_and_row(key, first_available(&found, &availability), &availability).0;
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&whole),
            "the rest announces the command with its place"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_orca(),
            [second, whole],
            "Orca speaks the returned row, drops the key's change 50 ms after it, and speaks the \
             status's reading"
        );
    }

    /// A selected row the wheel takes out of the list is no change of
    /// selection, nor is any frame drawn while it stays out, as the query's
    /// caret draws them as it blinks: the list counts no change, and the
    /// rest, which a change it was not sure Orca spoke would have the status
    /// say whole, says nothing. Read from the toolkit's measuring of the
    /// first row, the selected first row read as drawn there and not drawn
    /// in the rows the list drew, a return made up on every frame, each
    /// restarting the rest, which then said the command whole.
    #[gpui::test]
    fn a_selected_row_scrolled_out_of_the_list_is_no_change(cx: &mut TestAppContext) {
        let (_app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        let palette = OPENED_PALETTE
            .with_borrow(|opened| opened.as_ref().and_then(WeakEntity::upgrade))
            .expect("the open palette");
        let changes =
            |cx: &mut VisualTestContext| cx.update(|_, cx| palette.read(cx).listed.changes());
        let row = "palette-command-0";
        assert!(
            cx.debug_bounds(row).is_some(),
            "the selected first row is drawn"
        );
        let centre = cx
            .debug_bounds("command-palette-results")
            .expect("the list")
            .center();
        let counted = changes(cx);
        crate::tags::tests::scroll_down(
            cx,
            centre,
            gpui::ScrollDelta::Pixels(point(px(0.), px(-600.))),
        );
        heard.frame(cx);
        assert!(
            cx.debug_bounds(row).is_none(),
            "the wheel takes the selected row out of the list"
        );
        // Frames 100 ms apart while it stays out, and the rest after them.
        for _ in 0..10 {
            heard.wait(cx, 100);
        }
        heard.wait(cx, 2000);
        assert!(cx.debug_bounds(row).is_none(), "the row stays out");
        assert_eq!(
            changes(cx),
            counted,
            "the list counts no change of selection as the row leaves, nor at any frame after"
        );
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "and the rest says nothing"
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                heard.take_listed(),
                Vec::<String>::new(),
                "the adapter reports no change of selection"
            );
            assert_eq!(
                heard.take_orca(),
                Vec::<String>::new(),
                "and Orca hears nothing"
            );
        }
    }

    /// Wheels the palette's selected first row out of its list and back, in
    /// a window `height` tall, and types a key 50 ms after its return.
    fn row_brought_back(cx: &mut TestAppContext, height: f32) {
        let (app, cx) = hub(cx);
        cx.simulate_resize(size(px(1200.), px(height)));
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        heard.wait(cx, 1000);
        heard.take();
        let results = matches("");
        assert_eq!(
            first_available(&results, &availability),
            0,
            "the whole list starts on its first command"
        );
        // The row Orca speaks, where the list speaks its selection.
        #[cfg(target_os = "linux")]
        let first = reading_and_row("", 0, &availability).1;
        let key = ["g", "b", "t", "h", "c", "s", "r", "p"]
            .into_iter()
            .find(|key| {
                let found = matches(key);
                found.get(first_available(&found, &availability)) != Some(&results[0])
            })
            .expect("a key that selects another command");
        let row = "palette-command-0";
        assert!(cx.debug_bounds(row).is_some(), "the selected row is drawn");
        if height < 400. {
            assert!(
                cx.debug_bounds("palette-command-1").is_none(),
                "the list draws its first row alone"
            );
        }
        let centre = cx
            .debug_bounds("command-palette-results")
            .expect("the list")
            .center();
        crate::tags::tests::scroll_down(
            cx,
            centre,
            gpui::ScrollDelta::Pixels(point(px(0.), px(-600.))),
        );
        heard.frame(cx);
        assert!(
            cx.debug_bounds(row).is_none(),
            "the wheel takes the selected row out of the list"
        );
        heard.wait(cx, 1000);
        heard.take();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
        }
        crate::tags::tests::scroll_down(
            cx,
            centre,
            gpui::ScrollDelta::Pixels(point(px(0.), px(600.))),
        );
        heard.frame(cx);
        assert!(cx.debug_bounds(row).is_some(), "and brings it back");
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_listed(),
            std::slice::from_ref(&first),
            "the adapter reports the row's return as a change of the list's selection"
        );
        // A key that selects another command 50 ms later.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        cx.simulate_input(key);
        heard.frame(cx);
        heard.wait(cx, 2000);
        let found = matches(key);
        let whole = reading_and_row(key, first_available(&found, &availability), &availability).0;
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&whole),
            "the rest announces the command with its place"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_orca(),
            [first, whole],
            "Orca speaks the returned row, drops the key's change 50 ms after it, and speaks the \
             status's reading"
        );
    }

    /// A search that takes away the command Orca last spoke leaves Orca
    /// nothing to say but the command each move reaches: the row it left
    /// stays in libatspi's cache and answers the list as its parent (the
    /// vendored AT-SPI adapter's patches), so Orca takes it as alive, finds
    /// the list they share and reads no window, dialog or list before the
    /// command, as it did in Keyboard Shortcuts once a search took away its
    /// row (QA-94).
    #[cfg(target_os = "linux")]
    #[gpui::test]
    fn a_search_that_takes_the_spoken_command_away_leaves_moves_bare(cx: &mut TestAppContext) {
        let (_app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        heard.locus = Some(None);
        cx.simulate_keystrokes("down");
        heard.frame(cx);
        let spoken = heard.take_listed();
        assert_eq!(
            heard.take_orca(),
            spoken,
            "Down speaks the command it reaches"
        );
        heard.wait(cx, 1000);
        let row = spoken.first().expect("Down selects a command");
        assert!(
            !matches("go")
                .iter()
                .any(|&command| COMMANDS[command].label == row.as_str()),
            "\"go\" takes away {row:?}"
        );
        for key in ["g", "o"] {
            cx.simulate_input(key);
            heard.frame(cx);
            heard.wait(cx, 60);
        }
        heard.wait(cx, 2000);
        heard.take();
        heard.take_listed();
        let typed = heard.take_orca();
        assert!(
            !typed.iter().any(|said| said == "<context>"),
            "nothing of the window, the dialog or the list while typing: {typed:?}"
        );
        cx.simulate_keystrokes("down");
        heard.frame(cx);
        let listed = heard.take_listed();
        assert_eq!(listed.len(), 1, "Down selects one command: {listed:?}");
        assert_eq!(
            heard.take_orca(),
            listed,
            "only the command Down reaches, with nothing before it"
        );
        cx.simulate_keystrokes("escape");
        heard.frame(cx);
    }

    /// Typing "go" into the palette's query 60 ms a key, as steady typing
    /// does, makes one announcement, 500 ms after the last key
    /// ([`shortcuts::COUNT_REST`]) and not at 499 ms, of the reading the
    /// query rests on, its first available command with its place, and
    /// nothing after it; on Linux Orca speaks that reading last, the command
    /// included though the list speaks its selection there: Orca drops the
    /// list's change of selection that "o" makes 60 ms after the one before
    /// it ([`Listed::spoken()`]), so a reading of the place alone would leave
    /// the command unheard. Each key's
    /// reading was announced at once, and Orca 51.1, whose spam filter drops
    /// an event of a type the application sent less than 100 ms before,
    /// spoke the reading for "g" and dropped the one for "go", sent 51 ms
    /// after it, so the last reading heard was stale (QA-94). Closing the
    /// palette while its query rests drops the rest, and nothing is said.
    #[gpui::test]
    fn a_burst_of_keys_announces_once_the_command_it_rests_on(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        let reading = |query: &str| {
            let first = first_available(&matches(query), &availability);
            reading_and_row(query, first, &availability).0
        };
        let settled = reading("go");
        assert_ne!(reading("g"), settled, "\"g\" selects another command");
        cx.simulate_input("g");
        heard.frame(cx);
        heard.wait(cx, 60);
        cx.simulate_input("o");
        heard.frame(cx);
        heard.wait(cx, 499);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing is announced while the query has rested under 500 ms"
        );
        heard.wait(cx, 1);
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&settled),
            "one announcement: the reading the query rests on"
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        #[cfg(target_os = "linux")]
        {
            let orca = heard.take_orca();
            assert_eq!(
                orca.last(),
                Some(&settled),
                "Orca speaks the reading the query rests on, last: {orca:?}"
            );
            assert!(
                !orca.contains(&reading("g")),
                "and no reading of a query the person typed past: {orca:?}"
            );
        }

        let palette = OPENED_PALETTE
            .with_borrow(|opened| opened.as_ref().and_then(WeakEntity::upgrade))
            .expect("the open palette");
        cx.simulate_input("x");
        heard.wait(cx, 100);
        assert!(
            cx.update(|_, cx| palette.read(cx).settling.is_some()),
            "the query rests"
        );
        cx.simulate_keystrokes("escape");
        heard.frame(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape closes the palette"
        );
        // Held here past its close, the palette no longer waits for the rest.
        assert!(
            cx.update(|_, cx| palette.read(cx).settling.is_none()),
            "closing drops the query's rest"
        );
        drop(palette);
        heard.wait(cx, 2000);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "a query whose rest the closing cut short says nothing"
        );
    }

    /// After Down reaches a command that a running write makes wait, the
    /// write's start, a single change of context, changes the command's
    /// reading, which is heard once, once the context has rested for 500 ms
    /// ([`shortcuts::COUNT_REST`]) and not at 499 ms; the write's end
    /// changes it back, heard once the same way. Where the selected row's
    /// name carries a change of context ([`ROW_CARRIES_CONTEXT`], Linux),
    /// the row is renamed once at each rest, which the AT-SPI adapter
    /// reports and Orca speaks of the list's selected item, and the status
    /// announces neither (QA-96); elsewhere the row takes each reading as the
    /// context changes, and the status announces each once, politely, even
    /// after a move it took quietly. The status stayed quiet
    /// after a move where the list speaks it, so a reason appearing or
    /// going as a Fetch ran went unheard; announced at once, a change of
    /// context followed by another lost the second to Orca 51.1's spam
    /// filter (QA-95,
    /// [`a_writes_end_and_its_reload_are_heard_once_as_the_reading_they_end_on`]).
    #[gpui::test]
    fn a_context_change_after_a_move_is_heard(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        let fetching = |cx: &mut VisualTestContext, busy: Option<&'static str>| {
            app.update(cx, |app, cx| {
                app.operation_busy = busy;
                cx.notify();
            });
        };
        let idle = availability(cx);
        let mut busy = idle.clone();
        busy.busy = Some("Fetching…");
        let results = matches("");
        let at = (1..results.len())
            .find(|&at| {
                COMMANDS[results[at]].id.reason(&idle) != COMMANDS[results[at]].id.reason(&busy)
            })
            .expect("a running write makes a command wait");
        for _ in 0..at {
            heard.step(cx, "down");
        }
        let (quiet, quiet_row) = reading_and_row("", at, &idle);
        #[cfg(target_os = "linux")]
        let command = COMMANDS[results[at]].label;
        assert_eq!(
            heard.named.as_deref(),
            Some(quiet.as_str()),
            "Down reaches it"
        );
        assert_eq!(
            heard.row_name(),
            Some(quiet_row.as_str()),
            "and selects its row"
        );
        if shortcuts::LIST_SPEAKS_SELECTION {
            assert_eq!(
                heard.take(),
                Vec::<String>::new(),
                "the moves are the list's"
            );
        } else {
            heard.take();
        }
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        {
            heard.take_orca();
            heard.take_listed();
            heard.take_items_renamed(command);
        }
        // Each change is heard once, at its rest: as the selected row's
        // rename where it carries the context, otherwise as the status's
        // announcement.
        let rests_on =
            |cx: &mut VisualTestContext, heard: &mut Heard, had: &str, ends: (&str, &str)| {
                let (reading, row) = ends;
                // Where the row does not carry the change (macOS), its name
                // took the live reading as the context changed.
                #[cfg(not(target_os = "linux"))]
                assert_eq!(
                    heard.take_renamed(),
                    [row],
                    "the change renames the row at once, to {row:?}"
                );
                heard.wait(cx, 499);
                assert_eq!(
                    heard.take(),
                    Vec::<String>::new(),
                    "{reading:?} is not announced before the context has rested 500 ms"
                );
                let named = if ROW_CARRIES_CONTEXT { had } else { row };
                assert_eq!(
                    heard.row_name(),
                    Some(named),
                    "before the rest, the row is named {named:?}"
                );
                heard.wait(cx, 1);
                assert_eq!(heard.named.as_deref(), Some(reading), "the status reads it");
                if ROW_CARRIES_CONTEXT {
                    assert_eq!(
                        heard.take_renamed(),
                        [row],
                        "the rest renames the row once, to {row:?}"
                    );
                    assert_eq!(
                        heard.take(),
                        Vec::<String>::new(),
                        "and the status announces none of it"
                    );
                } else {
                    assert_eq!(
                        heard.take(),
                        [reading],
                        "the status announces {reading:?} once it rests, once"
                    );
                    assert_eq!(
                        heard.take_renamed(),
                        Vec::<String>::new(),
                        "and the rest renames the row no more"
                    );
                }
                #[cfg(target_os = "linux")]
                {
                    assert_eq!(
                        heard.take_items_renamed(command),
                        [row],
                        "the AT-SPI adapter reports the row renamed once, to {row:?}"
                    );
                    assert_eq!(
                        heard.take_orca(),
                        Vec::<String>::new(),
                        "and no announcement nor change of selection"
                    );
                }
            };
        fetching(cx, Some("Fetching…"));
        heard.frame(cx);
        let (waiting, waiting_row) = reading_and_row("", at, &availability(cx));
        assert_ne!(waiting, quiet, "the write changes the command's reading");
        rests_on(
            cx,
            &mut heard,
            quiet_row.as_str(),
            (waiting.as_str(), waiting_row.as_str()),
        );
        fetching(cx, None);
        heard.frame(cx);
        rests_on(
            cx,
            &mut heard,
            waiting_row.as_str(),
            (quiet.as_str(), quiet_row.as_str()),
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        assert_eq!(heard.take_renamed(), Vec::<String>::new(), "nor any rename");
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_items_renamed(command),
            Vec::<String>::new(),
            "that the adapter reports"
        );
    }

    /// QA-95 and QA-96: with the palette on Browse stashes… while a Fetch
    /// runs, the Fetch's end changes the command's reading to wait for the
    /// file list to refresh, and the reload's end 40 ms later makes it
    /// available. The two are heard once, 500 ms after the reload's end
    /// ([`shortcuts::COUNT_REST`]) and not at 499 ms, as the command
    /// available, and nothing after it. Where the selected row's name
    /// carries a change of context ([`ROW_CARRIES_CONTEXT`], Linux), that
    /// name keeps the reading it had until then and takes the final one in
    /// one rename, the one the AT-SPI adapter reports, and the status
    /// announces nothing: renamed at each change, Orca 51.1 spoke both
    /// renames of the selected row, 57 ms apart, the transient "Wait for
    /// the file list to refresh" first, and then the status's announcement
    /// of the final reading a third time (QA-96). Elsewhere the row's name
    /// follows each change at once, and the status announces the command
    /// available with its place, once: announced at
    /// each change of context, Orca 51.1's spam filter, which drops an event
    /// of a type the application sent less than 100 ms before, dropped the
    /// second, so the command becoming available was never heard (QA-95).
    #[gpui::test]
    fn a_writes_end_and_its_reload_are_heard_once_as_the_reading_they_end_on(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::{tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        // The Fetch is held as the palette opens and finds Browse stashes….
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
        let mut heard = Heard::new(cx);
        let query = "bro";
        cx.simulate_input(query);
        heard.wait(cx, 2000);
        let at = matches(query)
            .iter()
            .position(|&index| COMMANDS[index].id == CommandId::Stashes)
            .expect("\"bro\" finds Browse stashes…");
        let palette = OPENED_PALETTE
            .with_borrow(|opened| opened.as_ref().and_then(WeakEntity::upgrade))
            .expect("the open palette");
        let selected = cx.update(|_, cx| palette.read(cx).selection.index);
        for _ in selected..at {
            heard.step(cx, "down");
        }
        assert_eq!(
            cx.update(|_, cx| palette.read(cx).selection.index),
            at,
            "Down reaches Browse stashes…"
        );
        drop(palette);
        let (fetching, fetching_row) = reading_and_row(query, at, &availability(cx));
        assert!(
            fetching.ends_with(crate::workspace::BUSY_REASON),
            "while the Fetch runs, Browse stashes… waits for it: {fetching:?}"
        );
        assert_eq!(
            heard.row_name(),
            Some(fetching_row.as_str()),
            "and its row says so"
        );
        heard.take();
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        {
            heard.take_orca();
            heard.take_listed();
            heard.take_items_renamed("Browse stashes…");
        }

        // The Fetch ends and its reload starts.
        app.update(cx, |app, cx| {
            app.operation_busy = None;
            app.status_task = Some(Task::ready(()));
            app.status_after_write = Some(app.work_generation);
            cx.notify();
        });
        heard.frame(cx);
        let (reloading, reloading_row) = reading_and_row(query, at, &availability(cx));
        assert!(
            reloading.ends_with(crate::workspace::REFRESHING_ROWS_REASON),
            "through the reload, Browse stashes… waits for it: {reloading:?}"
        );
        // Where the row does not carry the change (macOS), its name follows
        // the live context at once.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            std::slice::from_ref(&reloading_row),
            "the Fetch's end renames the row at once, to wait for the reload"
        );
        // The reload ends 40 ms later.
        heard.wait(cx, 40);
        app.update(cx, |app, cx| {
            app.status_task = None;
            app.status_after_write = None;
            cx.notify();
        });
        heard.frame(cx);
        let (available, available_row) = reading_and_row(query, at, &availability(cx));
        assert_eq!(
            available,
            format!("Browse stashes… · {} of {}", at + 1, matches(query).len()),
            "the reload's end makes Browse stashes… available"
        );
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            std::slice::from_ref(&available_row),
            "and the reload's end renames it at once, to the command available"
        );
        heard.wait(cx, 499);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing is announced while the context has rested under 500 ms"
        );
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                Vec::<String>::new(),
                "the row is renamed to none while the context has rested under 500 ms, \
                 {reloading_row:?} included"
            );
            assert_eq!(
                heard.row_name(),
                Some(fetching_row.as_str()),
                "and keeps the reading it had"
            );
        }
        heard.wait(cx, 1);
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                std::slice::from_ref(&available_row),
                "one rename: the command available"
            );
            assert_eq!(
                heard.take(),
                Vec::<String>::new(),
                "and the status announces none of it"
            );
            assert_eq!(
                heard.named.as_deref(),
                Some(available.as_str()),
                "which it takes quietly"
            );
        } else {
            assert_eq!(
                heard.take(),
                std::slice::from_ref(&available),
                "one announcement: the command available"
            );
            assert_eq!(heard.row_name(), Some(available_row.as_str()));
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                heard.take_items_renamed("Browse stashes…"),
                std::slice::from_ref(&available_row),
                "the AT-SPI adapter reports one rename of the row, to the command available"
            );
            assert_eq!(
                heard.take_orca(),
                Vec::<String>::new(),
                "and no announcement nor change of selection"
            );
        }
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        assert_eq!(heard.take_renamed(), Vec::<String>::new(), "nor any rename");
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_items_renamed("Browse stashes…"),
            Vec::<String>::new(),
            "that the adapter reports"
        );
        cx.simulate_keystrokes("escape");
        crate::tags::tests::draw(cx);
    }

    /// A change of context during the query's rest, as a Fetch starts
    /// 300 ms after "go" is typed and makes its command wait, restarts the
    /// one rest the query and the context share: nothing is heard at the
    /// query's 500 ms nor 499 ms after the change, and then the reading both
    /// end on is heard once, the command waiting, and nothing after it: the
    /// status announces it with its place, on every platform. Where the
    /// selected row's name carries a change of context
    /// ([`ROW_CARRIES_CONTEXT`], Linux), the row keeps the name the query
    /// selected it with until the rest and is then renamed once, to the
    /// command waiting, beside the status's announcement, which the query's
    /// change makes ([`Palette::queried`]): taken quietly at a rest that
    /// renamed the row, a query typed as a Fetch ended was heard as nothing
    /// where Orca had dropped the list's change of selection to the row and
    /// so ignored its rename (QA-97). Elsewhere the row's name takes the
    /// command waiting as the Fetch starts.
    #[gpui::test]
    fn a_context_change_while_the_query_rests_is_heard_once_as_the_final_reading(
        cx: &mut TestAppContext,
    ) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        let idle = availability(cx);
        let query = "go";
        let at = first_available(&matches(query), &idle);
        let (typed, typed_row) = reading_and_row(query, at, &idle);
        cx.simulate_input(query);
        heard.frame(cx);
        heard.wait(cx, 300);
        assert_eq!(
            heard.row_name(),
            Some(typed_row.as_str()),
            "the query selects its command"
        );
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        let command = COMMANDS[matches(query)[at]].label;
        #[cfg(target_os = "linux")]
        heard.take_items_renamed(command);
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        heard.frame(cx);
        let (waiting, waiting_row) = reading_and_row(query, at, &availability(cx));
        assert_ne!(waiting, typed, "the Fetch makes the command wait");
        // Where the row does not carry the change (macOS), its name follows
        // the live context at once.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            std::slice::from_ref(&waiting_row),
            "the Fetch renames the row at once, to the command waiting"
        );
        heard.wait(cx, 200);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "the change of context restarts the query's rest"
        );
        heard.wait(cx, 299);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing is announced while the context has rested under 500 ms"
        );
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.row_name(),
                Some(typed_row.as_str()),
                "the row keeps its name until the rest"
            );
        }
        heard.wait(cx, 1);
        assert_eq!(
            heard.named.as_deref(),
            Some(waiting.as_str()),
            "the status reads the reading the query and the context end on"
        );
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&waiting),
            "one announcement: the reading the query and the context end on, \
             the query's change making the rest heard"
        );
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                std::slice::from_ref(&waiting_row),
                "beside one rename of the row, at the rest"
            );
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                heard.take_items_renamed(command),
                std::slice::from_ref(&waiting_row),
                "which the AT-SPI adapter reports once"
            );
            let orca = heard.take_orca();
            assert_eq!(
                orca.last(),
                Some(&waiting),
                "Orca speaks the status's reading last: {orca:?}"
            );
            assert!(
                !orca.contains(&typed),
                "and nothing of the reading the Fetch replaced: {orca:?}"
            );
        }
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        assert_eq!(heard.take_renamed(), Vec::<String>::new(), "nor any rename");
    }

    /// QA-97: with the palette open while a Fetch runs, "bro" typed 60 ms a
    /// key selects Browse and manage tags…, which leads its results, every
    /// one of them waiting, and the Fetch's end 60 ms after the last key
    /// makes that command available within the query's rest; the reload's
    /// end 40 ms later changes only Browse stashes…. The one rest the query
    /// and the context share, 500 ms after the Fetch's end, announces the
    /// reading they end on, the command available with its place, once, on
    /// every platform, and nothing after it. Where the selected row's name
    /// carries a change of context ([`ROW_CARRIES_CONTEXT`], Linux), the row
    /// keeps the waiting name the query selected it with until the rest and
    /// is then renamed once, beside the announcement. With the status quiet
    /// at that rest, Orca 51.1 heard nothing of the query in 2 of 3 runs: it
    /// had dropped the list's change of selection to the row as one of a
    /// burst ("multiple instances in short time"), and ignored the row's
    /// rename as "not from the locus of focus". Elsewhere (macOS) the row's
    /// name takes the command available as the Fetch ends.
    #[gpui::test]
    fn a_query_resting_with_a_writes_end_on_its_command_is_announced(cx: &mut TestAppContext) {
        query_resting_with_a_writes_end(cx, 60);
    }

    /// As QA-97's, with "bro" typed 150 ms a key: where the list speaks its
    /// selection (Linux), Orca speaks the list's change of selection to
    /// Browse and manage tags…, waiting, as "r" makes it, and at the rest
    /// the row's rename says it is available and the status's announcement
    /// says only its place ("1 of 4"), what neither the row nor its rename
    /// says. Elsewhere (macOS) the status announces the whole reading.
    #[gpui::test]
    fn a_query_typed_slowly_resting_with_a_writes_end_announces_its_place(cx: &mut TestAppContext) {
        query_resting_with_a_writes_end(cx, 150);
    }

    /// "bro" typed `key` ms a key while a Fetch runs, the Fetch ending 60 ms
    /// after the last key and its reload 40 ms after that.
    fn query_resting_with_a_writes_end(cx: &mut TestAppContext, key: u64) {
        use crate::tags::tests::{tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1200.), px(800.)));
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        // The Fetch is held as the palette opens.
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
        let mut heard = Heard::new(cx);
        let query = "bro";
        let at = first_available(&matches(query), &availability(cx));
        assert_eq!(
            COMMANDS[matches(query)[at]].id,
            CommandId::Tags,
            "while the Fetch runs, {query:?} selects Browse and manage tags…"
        );
        let (waiting, waiting_row) = reading_and_row(query, at, &availability(cx));
        assert!(
            waiting.ends_with(crate::workspace::BUSY_REASON),
            "which waits for the Fetch: {waiting:?}"
        );
        for (typed, input) in ["b", "r", "o"].into_iter().enumerate() {
            if typed > 0 {
                heard.wait(cx, key);
            }
            cx.simulate_input(input);
            heard.frame(cx);
        }
        heard.wait(cx, 60);
        assert_eq!(
            heard.row_name(),
            Some(waiting_row.as_str()),
            "the query selects the command waiting"
        );
        // Typed slowly, the query's last change of selection is spoken where
        // the list speaks it, and the status says only the place.
        let said = |available: &str| {
            if key >= 150 && shortcuts::LIST_SPEAKS_SELECTION {
                place_of(query, at)
            } else {
                available.to_owned()
            }
        };
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing is announced while the query rests"
        );
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        heard.take_items_renamed("");

        // The Fetch ends and its reload starts.
        app.update(cx, |app, cx| {
            app.operation_busy = None;
            app.status_task = Some(Task::ready(()));
            app.status_after_write = Some(app.work_generation);
            cx.notify();
        });
        heard.frame(cx);
        let (available, available_row) = reading_and_row(query, at, &availability(cx));
        assert_eq!(
            available,
            format!(
                "Browse and manage tags… · {} of {}",
                at + 1,
                matches(query).len()
            ),
            "the Fetch's end makes Browse and manage tags… available"
        );
        // Where the row does not carry the change (macOS), its name follows
        // the live context at once.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            std::slice::from_ref(&available_row),
            "the Fetch's end renames the row at once, to the command available"
        );
        // The reload ends 40 ms later and leaves the command's reading.
        heard.wait(cx, 40);
        app.update(cx, |app, cx| {
            app.status_task = None;
            app.status_after_write = None;
            cx.notify();
        });
        heard.frame(cx);
        assert_eq!(
            reading_and_row(query, at, &availability(cx)).0,
            available,
            "the reload's end leaves Browse and manage tags… available"
        );
        heard.wait(cx, 459);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing is announced while the query and the context have rested under 500 ms"
        );
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.row_name(),
                Some(waiting_row.as_str()),
                "the row keeps the name the query selected it with until the rest"
            );
        }
        heard.wait(cx, 1);
        let said = said(&available);
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&said),
            "one announcement: the reading the query and the context end on"
        );
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                std::slice::from_ref(&available_row),
                "beside one rename of the row, at the rest"
            );
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                heard.take_items_renamed(COMMANDS[matches(query)[at]].label),
                std::slice::from_ref(&available_row),
                "which the AT-SPI adapter reports once"
            );
            let orca = heard.take_orca();
            if said == available {
                assert_eq!(
                    orca.last(),
                    Some(&available),
                    "Orca speaks the status's reading last: {orca:?}"
                );
            } else {
                assert!(
                    orca.ends_with(&[waiting_row.clone(), said.clone()]),
                    "Orca speaks the row the query selected, waiting, and after the row's rename \
                     the status's place: {orca:?}"
                );
            }
        }
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        assert_eq!(heard.take_renamed(), Vec::<String>::new(), "nor any rename");
        cx.simulate_keystrokes("escape");
        crate::tags::tests::draw(cx);
    }

    /// A row of the whole list after its first, by its place, whose reading
    /// a running write leaves as it is or, where it `changes` it, changes
    /// that of the row after it too.
    fn row_a_write(changes: bool, idle: &Availability) -> usize {
        let mut busy = idle.clone();
        busy.busy = Some("Fetching…");
        let results = matches("");
        let changed = |at: usize| {
            let id = COMMANDS[results[at]].id;
            id.reason(idle) != id.reason(&busy)
        };
        (1..results.len() - 1)
            .find(|&at| changed(at) == changes && (!changes || changed(at + 1)))
            .expect("such a row")
    }

    /// A change of context that leaves the selected command's reading as it
    /// was, while it renames other rows, renames the selected row at no
    /// time and announces nothing; nor does a change that the context undoes
    /// within the rest, as a Fetch starting and a second write's end 100 ms
    /// later would. Each rename of the selected row is spoken on Linux
    /// (QA-96), so one that came back to the reading heard was heard twice.
    /// Where the row's name does not carry a change of context (macOS), it
    /// follows the change and its undoing at once, and the status says
    /// nothing.
    #[gpui::test]
    fn a_context_change_that_leaves_the_selected_reading_renames_nothing(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let idle = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
        let fetching = |cx: &mut VisualTestContext, busy: Option<&'static str>| {
            app.update(cx, |app, cx| {
                app.operation_busy = busy;
                cx.notify();
            });
        };
        #[cfg(target_os = "linux")]
        let results = matches("");
        let flush = |heard: &mut Heard| {
            heard.take();
            heard.take_renamed();
            #[cfg(target_os = "linux")]
            {
                heard.take_orca();
                heard.take_listed();
                heard.take_items_renamed("");
            }
        };
        let unchanged = row_a_write(false, &idle);
        for _ in 0..unchanged {
            heard.step(cx, "down");
        }
        let named = reading_and_row("", unchanged, &idle);
        assert_eq!(heard.named.as_deref(), Some(named.0.as_str()));
        assert_eq!(heard.row_name(), Some(named.1.as_str()));
        flush(&mut heard);
        fetching(cx, Some("Fetching…"));
        heard.frame(cx);
        heard.wait(cx, 2000);
        assert_eq!(
            heard.row_name(),
            Some(named.1.as_str()),
            "the selected row reads as it did"
        );
        assert_eq!(
            heard.take_renamed(),
            Vec::<String>::new(),
            "a write that leaves its reading renames it at no time"
        );
        assert_eq!(heard.take(), Vec::<String>::new(), "nor announces anything");
        #[cfg(target_os = "linux")]
        {
            let renamed = std::mem::take(&mut heard.items_renamed);
            assert!(
                !renamed.is_empty(),
                "while it renames rows whose reading it changes"
            );
            assert!(
                !renamed
                    .iter()
                    .any(|name| shows(name, COMMANDS[results[unchanged]].label)),
                "the AT-SPI adapter reports the selected row renamed at no time: {renamed:?}"
            );
            assert_eq!(
                heard.take_orca(),
                Vec::<String>::new(),
                "and Orca hears nothing"
            );
        }

        // A change the context undoes within the rest.
        fetching(cx, None);
        heard.wait(cx, 2000);
        let changed = row_a_write(true, &idle);
        let key = if changed > unchanged { "down" } else { "up" };
        for _ in 0..changed.abs_diff(unchanged) {
            heard.step(cx, key);
        }
        let named = reading_and_row("", changed, &idle);
        assert_eq!(heard.named.as_deref(), Some(named.0.as_str()));
        assert_eq!(heard.row_name(), Some(named.1.as_str()));
        flush(&mut heard);
        fetching(cx, Some("Fetching…"));
        heard.frame(cx);
        // Where the row does not carry the change (macOS), its name follows
        // the live context at once, there and back.
        #[cfg(not(target_os = "linux"))]
        {
            let busy = cx.update(|window, cx| app.read(cx).palette_availability(window, cx));
            assert_eq!(
                heard.take_renamed(),
                [reading_and_row("", changed, &busy).1],
                "the Fetch renames the row at once"
            );
        }
        heard.wait(cx, 100);
        fetching(cx, None);
        heard.frame(cx);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            std::slice::from_ref(&named.1),
            "and its undoing renames the row back at once"
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.row_name(), Some(named.1.as_str()));
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                Vec::<String>::new(),
                "a change undone within the rest renames the selected row at no time"
            );
        }
        assert_eq!(heard.take(), Vec::<String>::new(), "and announces nothing");
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_items_renamed(COMMANDS[results[changed]].label),
            Vec::<String>::new(),
            "the AT-SPI adapter reports it renamed at no time"
        );
    }

    /// A move while a change of context rests reads the row it reaches at
    /// once, by its live reading, which on Linux the list's change of
    /// selection reports and Orca speaks. Where the selected row's name
    /// carries the change ([`ROW_CARRIES_CONTEXT`], Linux), the row it
    /// leaves keeps the name it held through the move's frame and takes its
    /// live reading at the rest, so the AT-SPI adapter reports the list's
    /// change of selection a frame before that row's rename, which Orca, no
    /// longer presenting the row, does not speak: the adapter reports a
    /// rename as it walks a frame's changes and the change of selection
    /// after them, so a row renamed as it was left was still the item Orca
    /// presented when its rename arrived. Elsewhere (macOS) the selected row
    /// takes the write's reading at once, as every row does. The rest
    /// announces nothing.
    #[gpui::test]
    fn a_move_during_the_rest_reads_the_live_reading_of_the_row_it_reaches(
        cx: &mut TestAppContext,
    ) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        let idle = availability(cx);
        let results = matches("");
        let at = row_a_write(true, &idle);
        for _ in 0..at {
            heard.step(cx, "down");
        }
        let left = format!("palette-command-{}", results[at]);
        let row_named = |id: &str| {
            crate::native_accessibility::written::get(id)
                .and_then(|row| row.node.label().map(str::to_owned))
        };
        assert_eq!(
            heard.row_name(),
            Some(reading_and_row("", at, &idle).1.as_str())
        );
        heard.take();
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        {
            heard.take_orca();
            heard.take_listed();
            heard.take_items_renamed("");
        }
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        heard.frame(cx);
        heard.wait(cx, 200);
        let busy = availability(cx);
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.row_name(),
                Some(reading_and_row("", at, &idle).1.as_str()),
                "the change of context rests"
            );
        }
        // Where the row does not carry the change (macOS), its name took the
        // write's reading as it began.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            [reading_and_row("", at, &busy).1],
            "the write renames the selected row at once"
        );
        // The rows it does not hold, the one Down reaches among them, took
        // the write's readings at once.
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_items_renamed(COMMANDS[results[at]].label),
            Vec::<String>::new(),
            "the selected row is held"
        );
        cx.simulate_keystrokes("down");
        heard.frame(cx);
        let (reached, reached_row) = reading_and_row("", at + 1, &busy);
        let (left_held, left_row) = (
            reading_and_row("", at, &idle).1,
            reading_and_row("", at, &busy).1,
        );
        assert_ne!(
            reached_row,
            reading_and_row("", at + 1, &idle).1,
            "the write changes the reading of the row Down reaches"
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                heard.take_listed(),
                std::slice::from_ref(&reached_row),
                "the AT-SPI adapter reports the list's selection changed, and Orca speaks the \
                 row's live reading"
            );
            let renamed = std::mem::take(&mut heard.items_renamed);
            assert!(
                !renamed.iter().any(|name| {
                    shows(name, COMMANDS[results[at]].label)
                        || shows(name, COMMANDS[results[at + 1]].label)
                }),
                "and renames neither the row left nor the row reached in that frame, so the \
                 change of selection comes before any rename of the row left: {renamed:?}"
            );
        }
        assert_eq!(
            heard.row_name(),
            Some(reached_row.as_str()),
            "the row Down reaches reads its live reading at once"
        );
        assert_eq!(heard.named.as_deref(), Some(reached.as_str()));
        let named = if ROW_CARRIES_CONTEXT {
            &left_held
        } else {
            &left_row
        };
        assert_eq!(
            row_named(&left).as_ref(),
            Some(named),
            "the row it left still reads {named:?} in the move's frame"
        );
        let own = if shortcuts::LIST_SPEAKS_SELECTION {
            Vec::new()
        } else {
            vec![reached.clone()]
        };
        assert_eq!(
            heard.take(),
            own,
            "the move is the list's where it speaks it"
        );
        heard.wait(cx, 2000);
        assert_eq!(
            row_named(&left),
            Some(left_row.clone()),
            "the row left takes its live reading by the rest"
        );
        assert_eq!(heard.row_name(), Some(reached_row.as_str()));
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "the rest announces nothing"
        );
        assert_eq!(
            heard.take_renamed(),
            Vec::<String>::new(),
            "and renames nothing selected"
        );
        #[cfg(target_os = "linux")]
        {
            let renamed = std::mem::take(&mut heard.items_renamed);
            assert_eq!(
                renamed
                    .iter()
                    .filter(|name| shows(name, COMMANDS[results[at]].label))
                    .collect::<Vec<_>>(),
                [&left_row],
                "the AT-SPI adapter reports the row left renamed once, frames after the change \
                 of selection"
            );
            assert!(
                !renamed
                    .iter()
                    .any(|name| shows(name, COMMANDS[results[at + 1]].label)),
                "and the row reached renamed at no time: {renamed:?}"
            );
        }
    }

    /// A move back onto the row holding its name, before the change of
    /// context rests, reads that row's live reading in the frame that
    /// reports the list's change of selection, which Orca speaks, and the
    /// rest renames nothing more. Its rename from the held reading lands in
    /// that frame, ahead of the change of selection, while Orca still
    /// presents the row Up leaves, so it is not spoken; kept held, the row
    /// would be selected again with the reading the write had replaced and
    /// renamed while selected at the rest. Where the row's name does not
    /// carry a change of context (macOS), it took the write's reading as
    /// the write began, before Down left it.
    #[gpui::test]
    fn a_move_back_onto_the_held_row_reads_its_live_reading(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        let idle = availability(cx);
        #[cfg(target_os = "linux")]
        let results = matches("");
        let at = row_a_write(true, &idle);
        for _ in 0..at {
            cx.simulate_keystrokes("down");
            heard.frame(cx);
        }
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        heard.frame(cx);
        heard.wait(cx, 100);
        cx.simulate_keystrokes("down");
        heard.frame(cx);
        heard.wait(cx, 50);
        let busy = availability(cx);
        let (back, back_row) = reading_and_row("", at, &busy);
        assert_ne!(
            back_row,
            reading_and_row("", at, &idle).1,
            "the write changes the held row's reading"
        );
        // Where the row does not carry the change (macOS), its name took the
        // write's reading as it began, before Down left it.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            std::slice::from_ref(&back_row),
            "the write renames the selected row at once"
        );
        heard.take();
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
            let renamed = std::mem::take(&mut heard.items_renamed);
            assert!(
                !renamed
                    .iter()
                    .any(|name| shows(name, COMMANDS[results[at]].label)),
                "the row Down left still holds its name: {renamed:?}"
            );
        }

        cx.simulate_keystrokes("up");
        heard.frame(cx);
        assert_eq!(
            heard.row_name(),
            Some(back_row.as_str()),
            "Up back onto the held row reads its live reading at once"
        );
        assert_eq!(heard.named.as_deref(), Some(back.as_str()));
        let own = if shortcuts::LIST_SPEAKS_SELECTION {
            Vec::new()
        } else {
            vec![back.clone()]
        };
        assert_eq!(
            heard.take(),
            own,
            "the move is the list's where it speaks it"
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            heard.take_listed(),
            std::slice::from_ref(&back_row),
            "the AT-SPI adapter reports the list's selection changed, and Orca speaks the row's \
             live reading"
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.row_name(), Some(back_row.as_str()));
        // Up came 50 ms after Down, a change Orca drops as one of a burst:
        // where the list speaks the moves, the rest says the command Up
        // reached, whole.
        let again = if shortcuts::LIST_SPEAKS_SELECTION {
            vec![back.clone()]
        } else {
            Vec::new()
        };
        assert_eq!(
            heard.take(),
            again,
            "the rest announces only the command Up reached, where Orca may have missed it"
        );
        assert_eq!(
            heard.take_renamed(),
            Vec::<String>::new(),
            "and renames the row selected at no time"
        );
        #[cfg(target_os = "linux")]
        {
            let renamed = std::mem::take(&mut heard.items_renamed);
            assert_eq!(
                renamed
                    .iter()
                    .filter(|name| shows(name, COMMANDS[results[at]].label))
                    .collect::<Vec<_>>(),
                [&back_row],
                "the adapter reports the row renamed once, in the frame Up reached it"
            );
            assert!(
                !renamed
                    .iter()
                    .any(|name| shows(name, COMMANDS[results[at + 1]].label)),
                "and the row Up left renamed at no time: {renamed:?}"
            );
            assert_eq!(
                heard.take_orca(),
                again,
                "and Orca hears the status say it, and nothing more"
            );
        }
    }

    /// A query typed while a change of context to the selected command
    /// rests, whose rest selects another command, is announced at that rest
    /// as the query's rest always is, the command it selects with its place,
    /// once: the row left holding its name is renamed at the rest, no longer
    /// selected, so no rename carries the reading. Where the selected row's
    /// name carries a change of context ([`ROW_CARRIES_CONTEXT`], Linux), the
    /// row left is renamed at no time while selected; taking the rest as one
    /// that renamed the selected row, the status would say nothing of the
    /// command the query selected. Elsewhere (macOS) the row takes the
    /// write's reading as the write begins, while still selected.
    #[gpui::test]
    fn a_query_that_selects_another_command_during_the_rest_is_announced(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        let mut heard = Heard::new(cx);
        let availability = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).palette_availability(window, cx))
        };
        let idle = availability(cx);
        let mut busy = idle.clone();
        busy.busy = Some("Fetching…");
        let held = matches("")[row_a_write(true, &idle)];
        // A query that keeps the held command among its first results,
        // where the list draws its row, and whose first command the write
        // leaves available is another: a word of the held command's label.
        let label = COMMANDS[held].label.to_lowercase();
        let (query, selected) = label
            .split_whitespace()
            .find_map(|query| {
                let results = matches(query);
                let at = first_available(&results, &busy);
                (results.iter().position(|&command| command == held)? < 4
                    && results[at] != held
                    && COMMANDS[results[at]].id.reason(&busy).is_none())
                .then_some((query, at))
            })
            .expect("a query that selects another command");
        let place = |query: &str| {
            matches(query)
                .iter()
                .position(|&command| command == held)
                .expect("the held command is among the query's results")
        };
        let (shown, place) = (place(query), place(""));
        for _ in 0..place {
            cx.simulate_keystrokes("down");
            heard.frame(cx);
        }
        assert_eq!(
            heard.row_name(),
            Some(reading_and_row("", place, &idle).1.as_str())
        );
        heard.take();
        heard.take_renamed();
        #[cfg(target_os = "linux")]
        {
            heard.take_listed();
            heard.take_orca();
            heard.take_items_renamed("");
        }
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            cx.notify();
        });
        heard.frame(cx);
        heard.wait(cx, 100);
        // Where the row does not carry the change (macOS), its name took the
        // write's reading as it began.
        #[cfg(not(target_os = "linux"))]
        assert_eq!(
            heard.take_renamed(),
            [reading_and_row("", place, &availability(cx)).1],
            "the write renames the selected row at once"
        );
        cx.simulate_input(query);
        heard.frame(cx);
        assert!(
            cx.debug_bounds(format!("palette-command-{shown}").leak())
                .is_some(),
            "{query:?}'s list draws the held command's row"
        );
        let reading = reading_and_row(query, selected, &availability(cx)).0;
        #[cfg(target_os = "linux")]
        let renamed_to = reading_and_row(query, shown, &availability(cx)).1;
        heard.wait(cx, 499);
        assert_eq!(
            heard.take(),
            Vec::<String>::new(),
            "nothing is announced before the query has rested 500 ms"
        );
        heard.wait(cx, 1);
        assert_eq!(
            heard.take(),
            std::slice::from_ref(&reading),
            "the query's rest announces the command it selects, once"
        );
        heard.wait(cx, 2000);
        assert_eq!(heard.take(), Vec::<String>::new(), "and nothing after it");
        if ROW_CARRIES_CONTEXT {
            assert_eq!(
                heard.take_renamed(),
                Vec::<String>::new(),
                "no row is renamed while selected"
            );
        }
        #[cfg(target_os = "linux")]
        {
            let orca = heard.take_orca();
            assert_eq!(
                orca.last(),
                Some(&reading),
                "Orca speaks the query's reading last: {orca:?}"
            );
            assert_eq!(
                heard.take_items_renamed(COMMANDS[held].label),
                [renamed_to],
                "the row left is renamed once, at the rest, no longer selected"
            );
        }
    }

    /// How a held key's repeats reach the window after its first press.
    #[derive(Clone, Copy, Debug)]
    enum Repeats {
        /// Marked held, as macOS and Wayland send them.
        Held,
        /// Fresh presses with no release between, as X11 sends them.
        Fresh,
        /// A release and a press together, as an input method on Wayland
        /// (fcitx5 on Omarchy) sends each repeat while a text field is
        /// focused.
        InputMethod,
    }

    fn enter_down(cx: &mut VisualTestContext, is_held: bool) {
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::PlatformInput::KeyDown(gpui_kit::KeyDownEvent {
                    keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
                    is_held,
                    prefer_character_input: false,
                }),
                cx,
            );
        });
    }

    fn enter_up(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::PlatformInput::KeyUp(gpui_kit::KeyUpEvent {
                    keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
                }),
                cx,
            );
        });
    }

    /// Three repeats of a held Return, as `repeats` arrive, each followed
    /// by the frames drawn before the next.
    fn repeat_enter(cx: &mut VisualTestContext, repeats: Repeats) {
        for _ in 0..3 {
            match repeats {
                Repeats::Held => enter_down(cx, true),
                Repeats::Fresh => enter_down(cx, false),
                Repeats::InputMethod => {
                    enter_up(cx);
                    enter_down(cx, false);
                }
            }
            crate::views::tests::draw(cx);
        }
    }

    /// Return held on a palette command that opens a dialog opens it once,
    /// and nothing takes the held key's repeats until it comes up: the
    /// dialog stays open with focus where it put it, nothing inside it acts
    /// and nothing behind it opens, however the platform sends the repeats.
    /// Then a fresh Return acts in the dialog as usual. In History, with
    /// focus on a commit's changed files, held on "Browse reflog and recover
    /// a commit…" under fcitx5, the first repeat closed the Reflog and the
    /// next opened Compare on the selected file (QA-105): the input method
    /// sends each repeat as a release and a press, and the release ended the
    /// press the Reflog waited out. Held on "Quick Open File…", any repeat
    /// reached Quick Open's query, which opened the highlighted file.
    #[gpui::test]
    async fn a_held_return_on_a_palette_command_opens_its_dialog_once(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_git, history_script, history_window_from};
        use gpui_kit::component::WindowExt as _;
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        // Checked out, so Quick Open lists the worktree's tracked file and
        // the Reflog holds HEAD's entry.
        fixture_git(
            &fixture.path().join("narrow"),
            &["reset", "--hard", "--quiet"],
        );
        cx.update(|_, cx| {
            // A dialog otherwise slides in on a wall-clock animation.
            cx.set_reduce_motion(true);
            crate::shortcuts::bind_keys(cx);
        });
        let files = app.read_with(cx, |app, _| {
            assert!(!app.files.is_empty(), "the selected commit's changed files");
            app.file_focus.clone()
        });
        let compare = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare)
        };
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));

        for repeats in [Repeats::Held, Repeats::Fresh, Repeats::InputMethod] {
            for (query, command, title) in [
                ("reflog", CommandId::Reflog, "reflog-title"),
                ("Quick Open", CommandId::QuickOpen, "quick-open-title"),
            ] {
                let at = format!("{query:?}, repeats {repeats:?}");
                cx.update(|window, cx| window.focus(&files, cx));
                draw(cx);
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.open_command_palette(window, cx))
                });
                draw(cx);
                cx.simulate_input(query);
                draw(cx);
                assert_eq!(
                    matches(query).first().map(|&index| COMMANDS[index].id),
                    Some(command),
                    "{at}: the highlighted command"
                );

                enter_down(cx, false);
                draw(cx);
                assert!(
                    cx.debug_bounds(title).is_some(),
                    "{at}: the press opens the dialog"
                );
                assert!(
                    cx.debug_bounds("palette-command-0").is_none(),
                    "{at}: the palette closed"
                );
                if command == CommandId::QuickOpen {
                    // Its paths land before the key repeats, as on a
                    // platform's repeat delay.
                    crate::revision_inspection::settle_quick_open(cx).await;
                    draw(cx);
                }
                let opened = focused(cx);
                assert!(
                    opened.is_some() && opened != Some(files.clone()),
                    "{at}: the dialog took focus"
                );

                repeat_enter(cx, repeats);
                assert!(
                    cx.debug_bounds(title).is_some(),
                    "{at}: the dialog stays open through the repeats"
                );
                assert_eq!(
                    focused(cx),
                    opened,
                    "{at}: focus stays where the dialog put it"
                );
                assert!(!compare(&app, cx), "{at}: nothing behind the dialog opens");

                enter_up(cx);
                draw(cx);
                assert!(
                    cx.debug_bounds(title).is_some(),
                    "{at}: the key-up leaves it open"
                );
                enter_down(cx, false);
                draw(cx);
                if command == CommandId::Reflog {
                    // Return in the Reflog's filter is the filter's own: it
                    // selects the first entry it matches and leaves the
                    // Reflog open (`reflog::ReflogBrowser::confirm_in_field`).
                    assert!(
                        cx.update(|window, cx| window.has_active_dialog(cx)),
                        "{at}: a fresh Return acts in the filter, the Reflog open"
                    );
                    enter_up(cx);
                    cx.simulate_keystrokes("escape");
                    draw(cx);
                    assert!(
                        !cx.update(|window, cx| window.has_active_dialog(cx)),
                        "{at}: Escape closes the Reflog"
                    );
                } else {
                    assert!(
                        !cx.update(|window, cx| window.has_active_dialog(cx)),
                        "{at}: a fresh Return acts in the dialog, which closes"
                    );
                    enter_up(cx);
                }
                draw(cx);
                if command == CommandId::QuickOpen {
                    assert!(compare(&app, cx), "{at}: Quick Open opened its file");
                    // Back returns to History's selected commit.
                    cx.simulate_keystrokes("escape");
                    draw(cx);
                }
                // The Reflog's Escape closed it, so History shows on.
                assert!(
                    app.read_with(cx, |app, _| app.mode == WorkspaceMode::History),
                    "{at}: History shows"
                );
            }
        }
    }

    /// A held Return on a palette command a write refuses runs it once per
    /// press, under X11 too: refused as the key goes down, the command does
    /// not run once the write ends during the hold, however the repeats
    /// arrive, and the palette stays open; after the key-up a new press
    /// runs it. The query's own `PressEnter`, which it sends for every Enter
    /// key-down, ran the command as well as the dialog's default action, so
    /// the first repeat after the write ended opened the Reflog.
    #[gpui::test]
    async fn a_held_return_runs_a_command_a_write_refused_once_per_press(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, history_script, history_window_from};
        use gpui_kit::component::WindowExt as _;
        let (_fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        cx.update(|_, cx| {
            cx.set_reduce_motion(true);
            crate::shortcuts::bind_keys(cx);
        });
        let reflog = |cx: &mut VisualTestContext| cx.debug_bounds("reflog-title").is_some();
        let palette = |cx: &mut VisualTestContext| cx.debug_bounds("palette-command-0").is_some();
        let busy = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext, busy: bool| {
            app.update(cx, |app, cx| {
                app.operation_busy = busy.then_some("Testing");
                cx.notify();
            });
            draw(cx);
        };
        for repeats in [Repeats::Held, Repeats::Fresh, Repeats::InputMethod] {
            let at = format!("repeats {repeats:?}");
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
            draw(cx);
            cx.simulate_input("reflog");
            draw(cx);
            assert_eq!(
                matches("reflog").first().map(|&index| COMMANDS[index].id),
                Some(CommandId::Reflog),
                "{at}: the highlighted command"
            );
            // A write runs as the key goes down and ends during the hold.
            busy(&app, cx, true);
            enter_down(cx, false);
            draw(cx);
            assert!(
                palette(cx) && !reflog(cx),
                "{at}: the write refuses the command"
            );
            busy(&app, cx, false);
            repeat_enter(cx, repeats);
            assert!(
                !reflog(cx),
                "{at}: the held key runs nothing once the write ends"
            );
            assert!(palette(cx), "{at}: the palette stays open");
            enter_up(cx);
            draw(cx);
            enter_down(cx, false);
            draw(cx);
            assert!(reflog(cx), "{at}: a new press runs the command");
            enter_up(cx);
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}: Escape closes the Reflog"
            );
        }
    }

    /// The command palette's query, a dialog's field, draws its caret in
    /// the frame that opens the palette on it, the one frame the opening
    /// draws, and in the first frame after Tab, a click or code focuses it
    /// (`caret_focus_tests::draws_its_caret_as_it_takes_focus`). The opening
    /// frame is the one drawn as the update that opens the palette ends:
    /// GPUI reports focus to the new query only once that update's effects
    /// have run, so a frame drawn inside the update reports none, and the
    /// caret it draws never starts blinking.
    #[gpui::test]
    async fn the_palette_s_query_draws_its_caret_as_it_takes_focus(cx: &mut TestAppContext) {
        use crate::caret_focus_tests::{draws_its_caret_as_it_takes_focus, painted_carets};
        use crate::views::tests::{draw, history_window};
        let (_fixture, app, cx) = history_window(cx).await;
        cx.update(|window, cx| {
            // A dialog otherwise slides in on a wall-clock animation.
            cx.set_reduce_motion(true);
            window.activate_window();
        });
        cx.run_until_parked();
        let frames = |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.draws.len());
        let before = frames(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
        assert_eq!(frames(cx) - before, 1, "the opening draws one frame");
        assert_eq!(
            cx.update(|window, cx| painted_carets(window, cx)),
            1,
            "the frame that opens the palette draws its caret"
        );
        draw(cx);
        let query = cx.update(|_, cx| {
            OPENED_PALETTE
                .with_borrow(|opened| opened.as_ref().and_then(WeakEntity::upgrade))
                .expect("the open palette")
                .read(cx)
                .query
                .clone()
        });
        draws_its_caret_as_it_takes_focus(cx, "the command palette's query", &query);
    }
}
