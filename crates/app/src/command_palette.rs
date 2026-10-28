//! A bounded registry of native workflows. Highlighting a command is inert;
//! activation rechecks the current context before invoking its existing handler.
use crate::shortcuts::ShortcutId;
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
        None
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
    busy: bool,
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
        if context.busy
            && !matches!(
                self,
                Self::Activity
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
            return Some("Wait for the current Git operation to finish");
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
        if self == Self::Branch && !context.branch {
            return Some("Check out a named local branch first");
        }
        if self == Self::Rebase && context.integration {
            return Some("Finish or abort the current Git operation first");
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
    #[cfg(any(target_os = "linux", test))]
    pub(super) fn menu_command_reason(
        &self,
        id: CommandId,
        window: &Window,
    ) -> Option<&'static str> {
        id.reason(&self.palette_availability(window))
    }

    #[cfg(target_os = "linux")]
    pub(super) fn run_menu_command(
        &mut self,
        id: CommandId,
        path: &Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if &self.path != path
            || id.reason(&self.palette_availability(window)).is_some()
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        self.run_palette_command(id, window, cx);
    }

    fn palette_context(&self, window: &Window) -> PaletteContext {
        PaletteContext {
            availability: self.palette_availability(window),
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
    fn palette_file_scope(&self) -> FileScope {
        if self.mode == WorkspaceMode::Working {
            return FileScope {
                working: self.working_selected.and_then(|(index, area)| {
                    self.work_status
                        .as_ref()?
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
    fn palette_availability(&self, window: &Window) -> Availability {
        let file_scope = self.palette_file_scope();
        Availability {
            repository: self.repository.is_some(),
            workspace: self.page == AppPage::Repository,
            busy: self.operation_busy.is_some(),
            branch: self
                .work_status
                .as_ref()
                .is_some_and(|status| status.branch.is_some()),
            file: file_scope.file.is_some(),
            working_file: file_scope.working.is_some(),
            working: self.mode == WorkspaceMode::Working,
            file_history: self.file_history.is_active(),
            blame: self.blame.is_visible(),
            integration: self.integration_state.is_some(),
            profile_saving: self.profile_save_pending(),
            narrow_history: self.mode == WorkspaceMode::History && !self.navigation_fits(window),
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
        let context = self.palette_context(window);
        let form =
            cx.new(|cx| Palette::new(owner, path, file_scope, context, return_focus, window, cx));
        let query_focus = form.read(cx).query.read(cx).focus_handle(cx);
        window.open_alert_dialog(cx, move |dialog, _, cx| {
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
                .width(px(660.))
                .bg(rgb(surface.raised()))
                .border_color(rgb(surface.raised_edge()))
                .child(form.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            button("cancel-command-palette", "Cancel", "", false).on_click(
                                move |_, window, cx| {
                                    cancel.update(cx, |form, cx| form.cancel(window, cx))
                                },
                            ),
                        )
                        .child(
                            button("run-palette-command", "Open command", "", true)
                                .disabled(!form.read(cx).can_activate())
                                .on_click(move |_, window, cx| {
                                    activate.update(cx, |form, cx| form.activate(window, cx))
                                }),
                        ),
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
    fn run_palette_command(&mut self, id: CommandId, window: &mut Window, cx: &mut Context<Self>) {
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
                if let Some(branch) = self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.branch.clone())
                {
                    self.open_contextual_branch(branch, false, window, cx);
                }
            }
            CommandId::Worktrees => self.open_worktree_manager(window, cx),
            CommandId::Tags => self.open_tags(window, cx),
            CommandId::GitHub => self.open_github(window, cx),
            CommandId::Reflog => self.open_reflog_browser(window, cx),
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
struct Palette {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    file_scope: FileScope,
    context: PaletteContext,
    _owner_subscription: Option<Subscription>,
    return_focus: Option<FocusHandle>,
    query: Entity<InputState>,
    results: Vec<usize>,
    selection: Selection,
    scroll: UniformListScrollHandle,
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
        let subscription =
            cx.subscribe_in(&query, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.results = matches(&this.query.read(cx).value());
                    this.highlight_first_available();
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate(window, cx),
                _ => {}
            });
        // Notifications run after the owner update has ended. Rendering only
        // reads this owned snapshot because the dialog layer itself is rendered
        // while GitTurtle is mutably borrowed.
        let owner_subscription = owner.upgrade().map(|owner| {
            cx.observe_in(&owner, window, |this, owner, window, cx| {
                if !this.selection.closed {
                    this.context = owner.read(cx).palette_context(window);
                    cx.notify();
                }
            })
        });
        let mut palette = Self {
            owner,
            path,
            file_scope,
            context,
            _owner_subscription: owner_subscription,
            return_focus,
            query,
            results: matches(""),
            selection: Selection::default(),
            scroll: UniformListScrollHandle::new(),
            error: None,
            _subscription: subscription,
        };
        palette.highlight_first_available();
        palette
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
            if let Some(reason) = command.reason(&owner.palette_availability(window)) {
                self.error = Some(reason.into());
                return;
            }
            if !self.selection.claim(true) {
                return;
            }
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
                let context = this.context.availability.clone();
                range
                    .map(|index| {
                        let colors = palette(cx);
                        let spec = COMMANDS[this.results[index]];
                        let disabled = spec.id.reason(&context);
                        let selected = this.selection.index == index;
                        let shortcut = spec.shortcut.map(shortcuts::label).unwrap_or_default();
                        div()
                            .id(("palette-command", index))
                            .debug_selector(move || format!("palette-command-{index}"))
                            .role(Role::ListBoxOption)
                            .aria_label(format!(
                                "{}{}",
                                spec.label,
                                disabled
                                    .map(|reason| format!(". Unavailable: {reason}"))
                                    .unwrap_or_default()
                            ))
                            .aria_selected(selected)
                            .aria_position_in_set(index + 1)
                            .aria_size_of_set(this.results.len())
                            .w_full()
                            .h(crate::appearance::ui_size(54.))
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap_1()
                            // A resting row shows the raised surface beneath
                            // it, as a menu's rows do.
                            .when(selected, |row| row.bg(rgb(colors.selected)))
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
                                            .children(appearance::weight_probe(move || {
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
                                    .child(disabled.unwrap_or(if spec.id.selected_file() {
                                        "Selected file · opens its existing inspection workflow"
                                    } else if spec.id.workspace() {
                                        "Current repository · opens the existing workflow"
                                    } else {
                                        "Application action"
                                    })),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.selection.index = index;
                                this.activate(window, cx);
                            }))
                            .into_any_element()
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
                this.selection.move_by(delta, this.results.len());
                this.scroll
                    .scroll_to_item(this.selection.index, ScrollStrategy::Center);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                div()
                    .id("command-palette-current-result")
                    .role(Role::Status)
                    .a11y_synthetic_children(native_accessibility::polite)
                    .aria_label(
                        self.results
                            .get(self.selection.index)
                            .map(|index| {
                                format!(
                                    "{} · {} of {}{}",
                                    COMMANDS[*index].label,
                                    self.selection.index + 1,
                                    self.results.len(),
                                    reason
                                        .map(|reason| format!(" · Unavailable: {reason}"))
                                        .unwrap_or_default()
                                )
                            })
                            .unwrap_or_else(|| "No matching commands".into()),
                    )
                    .h(px(1.))
                    .overflow_hidden(),
            )
            .child(
                Input::new(&self.query)
                    .aria_label("Search app commands")
                    .cleanable(true),
            )
            .child(
                div()
                    .id("command-palette-scope")
                    .role(Role::Label)
                    .aria_label(scope.clone())
                    .tooltip({
                        let scope = scope.clone();
                        move |window, cx| Tooltip::new(scope.clone()).build(window, cx)
                    })
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .truncate()
                    .child(scope),
            )
            .child(
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
                    .when(count > 0, |element| element.child(list))
                    .when(count == 0, |element| {
                        element.child(div().p_4().text_color(rgb(p.muted)).child(
                            if self.query.read(cx).value().len() > 4096 {
                                "The search is too long. Use up to 4,096 bytes."
                            } else {
                                "No matching commands. Try profiles, compare, branches, or themes."
                            },
                        ))
                    }),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(reason.map(str::to_owned).unwrap_or_else(|| {
                        format!(
                            "↑ / ↓ select · {} opens · Escape returns to your previous focus",
                            shortcuts::ENTER
                        )
                    })),
            )
            .children(
                self.error
                    .as_ref()
                    .map(|error| div().text_color(rgb(p.warning)).child(error.clone())),
            )
    }
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
        context.repository = true;
        assert!(CommandId::History.reason(&context).is_none());
        assert!(CommandId::QuickOpen.reason(&context).is_some());
        context.workspace = true;
        assert!(CommandId::QuickOpen.reason(&context).is_none());
        assert!(CommandId::Blame.reason(&context).is_some());
        context.working_file = true;
        assert!(CommandId::Blame.reason(&context).is_none());
        context.integration = true;
        assert!(CommandId::Rebase.reason(&context).is_some());
        context.busy = true;
        assert!(CommandId::Profiles.reason(&context).is_some());
        assert!(CommandId::Activity.reason(&context).is_none());
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
        snapshot.availability.busy = true;
        assert!(!snapshot.allows(&original, CommandId::QuickOpen));
        assert!(snapshot.allows(&original, CommandId::Activity));
    }
}

/// The palette as a window draws it.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::{activity, image_lifetime, recovery_drafts, repository_tabs};
    use core::prelude::v1::test;
    use gpui_kit::{Background, Bounds, ScaledPixels, component::Root, point};
    use std::{cell::RefCell, rc::Rc};

    /// The application on its Projects hub, with no repository open, so the
    /// repository's commands are unavailable, and dialogs opening at rest.
    fn hub(cx: &mut TestAppContext) -> (Entity<GitTurtle>, &mut VisualTestContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
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
    }

    fn painted(cx: &mut VisualTestContext) -> Painted {
        let form = crate::tags::tests::rendered(cx, "command-palette-form");
        let list = crate::tags::tests::rendered(cx, "command-palette-results");
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
                        inside(logical(quad.bounds, scale)) && !quad.background.is_transparent()
                    })
                    .map(|quad| quad.background)
                    .collect(),
            }
        })
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
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window));
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

    /// An unavailable command's title is regular and an available one's
    /// medium, so the weight marks an unavailable row as well as the disabled
    /// color does; it was medium either way. "show" on the Projects hub
    /// matches both kinds within the first screen of rows.
    #[gpui::test]
    fn an_unavailable_commands_title_is_regular(cx: &mut TestAppContext) {
        let (app, cx) = hub(cx);
        cx.simulate_input("show");
        crate::tags::tests::draw(cx);
        let availability = cx.update(|window, cx| app.read(cx).palette_availability(window));
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
}
