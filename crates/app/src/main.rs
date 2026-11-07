mod activity;
mod appearance;
mod authentication;
mod automatic_refresh;
mod blame;
mod branch_actions;
mod build_info;
#[cfg(test)]
mod caret_focus_tests;
mod columns;
mod command_palette;
mod commit_drafts;
mod commit_message;
mod conflicts;
mod copy_feedback;
mod desktop_text;
#[cfg(test)]
mod dialog_sides;
mod diff_view;
mod discard;
mod editor_find;
mod empty_state;
mod file_history;
#[cfg(test)]
mod find_bench;
mod focus_reveal;
mod focus_ring;
mod folder_picker;
mod gif_playback;
mod github;
mod github_view;
mod graph;
mod history_paging;
mod history_search;
mod history_updates;
mod ignore;
mod image_compare;
// Preview images: AVIF and JPEG 2000 in the confined helper on Linux, every
// other image (and every image on other targets) through `decode_image`.
mod image_helper;
mod image_lifetime;
mod in_frame;
mod integration;
mod interactive_rebase;
#[cfg(test)]
mod layout_grid;
mod lfs_download;
mod local_refresh;
mod markdown_view;
mod model_view;
mod motion;
mod native_accessibility;
mod navigation;
mod operations;
mod page_navigation;
mod partial_view;
mod path_filter;
// The confined helper process, its runner and the PDF client, which `pdf_view`
// renders Linux PDF pages through; `image_helper` shares the runner.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod pdf_helper;
mod pdf_view;
mod platform_polish;
mod preferences;
mod profiles;
mod project_library;
mod project_pane;
mod projects;
mod recovery;
mod recovery_drafts;
mod reflog;
mod repository_access;
mod repository_tabs;
mod revision_inspection;
mod rewrite_review;
mod rich_preview;
#[cfg(test)]
mod scroll_tests;
mod settings;
mod shortcuts;
mod split_diff;
mod system_preview;
mod tags;
#[cfg(test)]
mod test_support;
mod text;
mod text_cut;
mod text_review;
mod theme_editor;
mod views;
#[cfg(test)]
mod width_walk;
#[cfg(target_os = "linux")]
mod window_chrome;
mod worker;
mod working_selection;
mod workspace;
mod worktrees;

use appearance::palette;
use empty_state::EmptyState;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, Worktree};
use gpui_kit::component::{
    Disableable, Icon, Root, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogKind,
    input::{EditorState, Input, InputEvent, InputState, Textarea, TextareaState},
    resizable::{ResizableState, h_resizable, resizable_panel},
    tooltip::Tooltip,
};
use gpui_kit::*;
use native_accessibility::{AppContextMenu, AppMenu};
use operations::SerialExecutor;
use platform_polish::*;
use preferences::{AppSettings, CommitDraft, Preferences};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use worker::{Content, Job, Output, Worker};

gpui_kit::actions!(
    gitturtle,
    [
        CloseRepositoryTab,
        NextRepositoryTab,
        PreviousRepositoryTab,
        SelectRepositoryTab1,
        SelectRepositoryTab2,
        SelectRepositoryTab3,
        SelectRepositoryTab4,
        SelectRepositoryTab5,
        SelectRepositoryTab6,
        SelectRepositoryTab7,
        SelectRepositoryTab8,
        Quit,
        OpenRepository,
        Refresh,
        Search,
        FindInComparison,
        NextRow,
        PreviousRow,
        FirstRow,
        LastRow,
        NextPane,
        NextTextChange,
        PreviousTextChange,
        ClearSearch,
        ToggleSidebar,
        ToggleProjectPane,
        BackHistory,
        ShowProjects,
        ShowSettings,
        ShowChanges,
        QuickOpenFile,
        ShowCommandPalette,
        ShowActivity,
        ExtendNextWorking,
        ExtendPreviousWorking,
        SelectAllWorking,
        CompareRevisions
    ]
);

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets/icons/"]
struct EmbeddedAssets;
const APP_ICON_PATH: &str = "branding/app-icon.png";
/// The status of a History file's preview read, which Back abandons.
const PREVIEW_LABEL: &str = "Loading preview…";
/// The status of a captured comparison's preview read as its tab is
/// restored (`restore_captured_tab_comparison`), which Back abandons too.
const RESTORED_PREVIEW_LABEL: &str = "Restoring captured comparison…";

/// A preview read the window asked its repository worker for
/// ([`GitTurtle::request`]): a comparison's, a restored comparison's, a
/// tracked file's, a working file's, or a text review's preparation of the
/// comparison shown. Only one whose generation is still the window's, and
/// still loading, is in flight ([`GitTurtle::preview_in_flight`]): any other
/// read moves the generation on, and its own reply clears `loading`.
struct PreviewRead {
    generation: u64,
    /// What a comparison's read (`Job::Preview`) reads, the whole of its
    /// input: the repository, the changed file and the revision pair its
    /// documents resolve against. Text mode, zoom and the review's options
    /// apply as the reply is shown, not to the read. The other preview
    /// reads have none.
    target: Option<PreviewTarget>,
}

/// A comparison read's input ([`PreviewRead::target`]).
#[derive(PartialEq)]
struct PreviewTarget {
    repository: PathBuf,
    file: FileChange,
    origins: markdown_view::Origins,
}

impl PreviewRead {
    /// The preview read `job` is, if it is one, asked for at `generation`.
    fn of(job: &Job, generation: u64) -> Option<Self> {
        let target = match job {
            Job::Preview {
                repo,
                file,
                origins,
            } => Some(PreviewTarget {
                repository: repo.path().to_owned(),
                file: file.clone(),
                origins: origins.clone(),
            }),
            Job::TrackedPreview { .. } | Job::WorkingPreview { .. } | Job::ReviewText { .. } => {
                None
            }
            _ => return None,
        };
        Some(Self { generation, target })
    }
}
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == APP_ICON_PATH {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../../assets/branding/app-icon.png"
            ))));
        }
        if let Some(asset) = path.strip_prefix("icons/").and_then(EmbeddedAssets::get) {
            return Ok(Some(asset.data));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut result: Vec<SharedString> = EmbeddedAssets::iter()
            .map(|s| format!("icons/{s}"))
            .filter(|s| s.starts_with(path))
            .map(Into::into)
            .collect();
        if APP_ICON_PATH.starts_with(path) {
            result.push(APP_ICON_PATH.into());
        }
        result.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(result)
    }
}

#[derive(Clone)]
enum NavRow {
    Section(&'static str, usize),
    All,
    Folder {
        key: String,
        label: String,
        depth: usize,
        count: usize,
        expanded: bool,
    },
    Branch(usize, usize),
    Worktree(usize),
}
#[derive(Clone, Copy, PartialEq)]
enum WorkspaceMode {
    History,
    Compare,
    Working,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppPage {
    Repository,
    Projects,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
enum Pane {
    History,
    Files,
}
#[derive(Clone, Copy, PartialEq)]
enum TextMode {
    Unified,
    Split,
    Before,
    After,
    Diagrams,
    Markdown,
}
#[derive(Clone, Copy, PartialEq)]
enum NavMode {
    Local,
    Remote,
    Worktrees,
}

/// Branch filters and write targets belong to the last successfully opened
/// worktree, which can differ from an in-flight requested path or its alias.
#[derive(Default)]
struct BranchInputScope {
    repository: Option<PathBuf>,
}

impl BranchInputScope {
    /// Called only for a successful, generation-accepted Open snapshot.
    fn opened(&mut self, repository: &std::path::Path) -> bool {
        let changed = self
            .repository
            .as_deref()
            .is_some_and(|previous| previous != repository);
        self.repository = Some(repository.to_owned());
        changed
    }
}

struct GitTurtle {
    activity: activity::State,
    working_selection: working_selection::Selection,
    working_filter: Entity<InputState>,
    file_filter: Entity<InputState>,
    file_paths: path_filter::State,
    revision_inspection: revision_inspection::State,
    authentication: authentication::State,
    blame: blame::State,
    tag_actions: tags::State,
    worktree_management: worktrees::State,
    interactive_rebase: interactive_rebase::State,
    ignore_actions: ignore::State,
    discard_actions: discard::State,
    /// What the native menus were last built for
    /// (`GitTurtle::update_native_menus`).
    menu_state: Option<platform_polish::MenuState>,
    #[cfg(target_os = "linux")]
    primary_menu: Entity<PrimaryMenu>,
    dialog_layer_subscription: Option<Subscription>,
    /// The window this view draws in, for work that needs it asked for
    /// with none at hand: Stashes cancels Review restore…'s preparation from
    /// its own listeners and as it is released
    /// (`GitTurtle::cancel_recovery_preparation`).
    window_handle: AnyWindowHandle,
    modal_was_open: bool,
    modal_return_focus: Option<FocusHandle>,
    /// The control the open modal would give focus back to as it opened,
    /// where the page behind it was restored while it showed: a control of
    /// the replaced page, which its close gives way to `modal_return_focus`
    /// (`GitTurtle::return_modal_to`).
    modal_replaced_focus: Option<FocusHandle>,
    modal_focus_generation: u64,
    settings_editor: Entity<InputState>,
    /// The Settings page's own view, embedded cached by `render`
    /// (`settings::SettingsPage`).
    settings_page: Entity<settings::SettingsPage>,
    /// Where the Settings page's last build placed the picker cards, whose
    /// bodies `settings::CardLayer` draws after the page every frame.
    picker_cards: settings::CardSlots,
    history_search: history_search::State,
    history_updates: history_updates::State,
    file_history: file_history::State,
    automatic: automatic_refresh::State,
    branch_actions: branch_actions::State,
    recovery: recovery::State,
    page_return_focus: Option<FocusHandle>,
    page_origin: AppPage,
    content_panels: Entity<ResizableState>,
    history_panels: Entity<ResizableState>,
    project_panels: Entity<ResizableState>,
    retained_history_files: Option<(Vec<FileChange>, Option<usize>)>,
    commit_drafts: HashMap<PathBuf, CommitDraft>,
    /// Client-only project names, keyed by canonical worktree root. The Git
    /// repository and its folder never see these.
    project_names: HashMap<PathBuf, String>,
    rename_project: Option<Entity<projects::RenameProjectForm>>,
    /// Known projects and their user-defined groups, shown by the left pane.
    project_library: project_library::ProjectLibrary,
    /// Saved custom themes, which a custom `settings.theme` selection resolves against.
    custom_themes: Vec<appearance::custom::CustomTheme>,
    /// One miniature per picker card, built-in and custom, so the Settings
    /// picker can reuse the preview bodies that a palette change does not
    /// alter (`settings::ThemePreviewBody`). Each is stored with the selection
    /// it draws and found through `GitTurtle::theme_preview_body`, never by
    /// position: `ThemeChoice::ALL` is in display order, not discriminant
    /// order, and a custom id is not an index. `GitTurtle::set_custom_themes`
    /// is the only writer of `custom_themes` and keeps the custom entries in
    /// step: a saved palette is pushed into its existing body, which notifies
    /// only when it changed, and a deleted theme's body is dropped.
    theme_previews: Vec<(
        appearance::custom::ThemeSelection,
        Entity<settings::ThemePreviewBody>,
    )>,
    /// The picker cards' focus handles, one per built-in and one per saved
    /// custom theme, each stored with the selection its card draws. The app
    /// owns them, as it owns the miniatures, so they outlive a replayed page,
    /// and each card's button tracks its own (the patched kit
    /// `Button::track_focus`), so the picker finds the focused card for its
    /// focus ring;
    /// `GitTurtle::set_custom_themes` keeps the custom entries in step.
    theme_card_focus: Vec<(appearance::custom::ThemeSelection, FocusHandle)>,
    /// The picker cards' cached bodies (`settings::ThemeCardBody`), one per
    /// built-in and one per saved custom theme, each stored with the
    /// selection its card draws and kept in step as `theme_card_focus` is.
    /// `GitTurtle::sync_theme_cards` sets what each shows.
    theme_card_bodies: Vec<(
        appearance::custom::ThemeSelection,
        Entity<settings::ThemeCardBody>,
    )>,
    /// Test-only: the debug selector and accessible name of every card the
    /// last picker render built, in order
    /// (`theme_editor::tests::custom_themes_form_a_third_picker_group`).
    #[cfg(test)]
    card_names: std::cell::RefCell<Vec<(String, String)>>,
    /// Linux: the `appearance::omarchy` revision this window last took, or
    /// `None` before it took any (`GitTurtle::omarchy_changed`).
    #[cfg(target_os = "linux")]
    omarchy_revision: Option<u64>,
    /// Linux: the Omarchy card's choice waiting for its read
    /// (`GitTurtle::choose_omarchy`).
    #[cfg(target_os = "linux")]
    omarchy_choice: settings::OmarchyChoice,
    /// Test-only: whether the last Settings build drew Follow system on, off
    /// and disabled for the Omarchy theme, and its description
    /// (`settings::picker_tests::the_omarchy_card_selects_and_locks_follow_system`).
    #[cfg(test)]
    follow_system_row: std::cell::RefCell<Option<(bool, bool, SharedString)>>,
    /// Test-only: the debug selector and accessible name of every Your themes
    /// action the last Settings build drew: New theme…, Import… and each drawn
    /// row's Edit…, Export… and Delete…
    /// (`theme_editor::tests::your_themes_actions_carry_their_accessible_names`).
    #[cfg(test)]
    theme_action_names: std::cell::RefCell<Vec<(String, String)>>,
    /// Test-only: the debug selector and tooltip of New theme… and Import…
    /// as the last Settings build drew them
    /// (`settings::picker_tests::bound_tooltips_say_what_to_delete`).
    #[cfg(test)]
    theme_action_tooltips: std::cell::RefCell<[(String, String); 2]>,
    /// Test-only: the accessible name, tooltip and disabled state of the
    /// History rail's navigation button as the last draw built it
    /// (`views::tests::narrow_history_keeps_the_navigation_choice`).
    #[cfg(test)]
    rail_navigation: std::cell::Cell<Option<(&'static str, &'static str, bool)>>,
    /// Test-only: the refusal count and accessible name of the History rail's
    /// polite status node as the last draw of the rail built it, or `None`
    /// when it drew none
    /// (`views::tests::narrow_history_announces_the_refused_toggle`).
    #[cfg(test)]
    rail_announcement: std::cell::Cell<Option<(usize, &'static str)>>,
    /// Test-only: a line drawn above the Working inspector's list/composer
    /// body, as "Filtering working paths…" once was, and where each frame
    /// lays the list and the composer out (`workspace::WorkingProbe`,
    /// `motion::tests::the_list_keeps_its_bounds_in_every_frame_of_a_write`).
    #[cfg(test)]
    working_probe: workspace::WorkingProbe,
    /// Test-only: the palette every draw of this view saw. A Settings theme
    /// switch and a live-preview edit each cost exactly one draw, which
    /// already shows the new palette; see
    /// `theme_editor::tests::an_edit_and_a_switch_each_draw_the_window_once`.
    #[cfg(test)]
    draws: Vec<appearance::Palette>,
    project_pane: project_pane::State,
    draft_saver: commit_drafts::DraftSaver,
    recovery_drafts: recovery_drafts::State,
    draft_save_error: Option<String>,
    draft_repository: Option<PathBuf>,
    settings: AppSettings,
    preferences_writer: SerialExecutor,
    operations: SerialExecutor,
    operation_busy: Option<&'static str>,
    /// The look of the current busy spell, which waits a moment
    /// (`workspace::BusyLook`).
    busy_look: workspace::BusyLook,
    operation_repository: Option<PathBuf>,
    operation_outcomes: operations::RepositoryOutcomes,
    operation_error: Option<String>,
    /// Refusals reported in the window's error
    /// (`GitTurtle::refuse_while_paused`). Each draws the error's line as a
    /// fresh node, which assistive technology announces as it is added, so
    /// the same refusal asked for again is heard again: the line kept its
    /// node and its text, and a second Enter in a paused commit menu was
    /// silent. Window-lived; it only names the node.
    error_refusals: usize,
    operation_notice: Option<String>,
    /// This showing of the preview, which a line beneath an open control
    /// belongs to: [`Self::clear_preview`] and a restored inspection start a
    /// new one, so the line goes when another file is shown.
    preview_showing: system_preview::Showing,
    operation_task: Option<Task<()>>,
    status_task: Option<Task<()>>,
    /// The status reads waiting in the operation executor's queue, one for
    /// each repository at most, whichever tab or route asked
    /// (`GitTurtle::refresh_worktree`). Window-lived, as the executor is.
    status_reads: operations::WaitingReads<PathBuf, worker::WorkingState>,
    /// The `work_generation` of the status read a write that changes only
    /// the working state asked for as it ended (Stage, Unstage, Discard,
    /// Ignore, Delete untracked file…, Resolve, a stash's recovery), or of
    /// the read that replaced it, as Refresh does (`refresh_worktree`):
    /// menus refuse writes while that read runs
    /// (`GitTurtle::write_unsettled`).
    status_after_write: Option<u64>,
    /// The repository whose working-state read was asked for while a write
    /// or a review's preparation ran, and waits for it to end
    /// (`GitTurtle::read_deferred_working`).
    working_read_deferred: Option<PathBuf>,
    work_generation: u64,
    /// The working-state reads this window has asked for, explicit and
    /// quiet alike: each takes the next number, which its reply carries
    /// (`GitTurtle::request_status_read`), so a read asked for before a
    /// refusal is told from one asked for after it. Window-lived, as the
    /// readers are, so numbers only grow across tabs.
    status_requests: u64,
    work_status: Option<Arc<gitturtle_core::RepositoryStatus>>,
    working_paths: working_selection::FilterState,
    integration_state: Option<gitturtle_core::OperationState>,
    integration_task: Option<Task<()>>,
    /// The busy state of a review's preparation that a page or tab switch
    /// dropped while its read may still run on the operation executor, and
    /// the wait for a marker queued behind that read
    /// (`GitTurtle::end_preparations`): reads asked for meanwhile are noted
    /// rather than queued behind it, and the marker's end reads them.
    dropped_preparation: Option<page_navigation::DroppedPreparation>,
    profile: Option<gitturtle_core::GitProfile>,
    profiles: profiles::State,
    theme_editor: theme_editor::State,
    remotes: Vec<gitturtle_core::Remote>,
    working_rows: Vec<workspace::WorkingRow>,
    working_selected: Option<(usize, gitturtle_core::ChangeArea)>,
    working_scroll: UniformListScrollHandle,
    /// A row moving between Staged and Unstaged after its Stage or Unstage.
    working_travel: motion::RowTravel,
    /// The pointer where a row's Stage or Unstage was pressed, until it moves.
    still_pointer: Option<motion::StillPointer>,
    page: AppPage,
    hub: Entity<projects::ProjectHub>,
    commit_title: Entity<InputState>,
    commit_message: Entity<TextareaState>,
    identity_name: Entity<InputState>,
    identity_email: Entity<InputState>,
    branch_name: Entity<InputState>,
    branch_input_scope: BranchInputScope,
    remote_name: Entity<InputState>,
    remote_branch: Entity<InputState>,
    settings_branch: Entity<InputState>,
    settings_drafts: settings::DraftState,
    column_drag: Option<(columns::ColumnId, Pixels, f32)>,
    column_menu: bool,
    git_actions_open: bool,
    layout_trace: Option<(
        Size<Pixels>,
        appearance::custom::ThemeSelection,
        appearance::Density,
        u8,
        u8,
        bool,
    )>,
    history_horizontal: ScrollHandle,
    worker: Worker,
    task: Option<Task<()>>,
    generation: u64,
    /// The generation of the preview read that the person's explicit choice
    /// of its file started or waits on: a click on its row, or Enter on the
    /// row selected. Only its landing activates the preview, where a
    /// conflict's result draft takes focus ([`Self::ensure_conflict_view`]);
    /// any other landing, as a write's status read selecting the row again
    /// or an arrow key moving the selection, leaves focus where it is.
    preview_activation: Option<u64>,
    /// The last preview read asked for, which Back abandons whatever its
    /// label while it is in flight, and which choosing its file again keeps
    /// (`Self::preview_in_flight`).
    preview_read: Option<PreviewRead>,
    repository: Option<GitRepository>,
    path: Option<PathBuf>,
    scope: Option<(String, worker::Scope)>,
    limit: usize,
    history_paging: history_paging::State,
    repository_tabs: repository_tabs::State,
    branches: Vec<Branch>,
    worktrees: Vec<Worktree>,
    nav_rows: Vec<NavRow>,
    /// Each of `nav_rows`' element IDs, from what the row shows
    /// ([`GitTurtle::nav_row_id`]), made as the rows are built.
    nav_ids: Vec<ElementId>,
    nav_cursor: Option<usize>,
    nav_focus: FocusHandle,
    nav_mode: NavMode,
    expanded_folders: HashSet<String>,
    seed_folders: bool,
    commits: Vec<Commit>,
    visible: Vec<usize>,
    graph: Vec<graph::GraphRow>,
    graph_lanes: usize,
    graph_offset: usize,
    graph_notice: Option<String>,
    refs: worker::HistoryReferences,
    selected_commit: Option<usize>,
    selected_file: Option<usize>,
    parent: usize,
    files: Vec<FileChange>,
    content: Option<Arc<Content>>,
    patch_editor: Option<Entity<EditorState>>,
    patch_decoration: Option<editor_find::PatchDecorations>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    partial_subscription: Option<Subscription>,
    split_view: Option<Entity<split_diff::SplitView>>,
    markdown_view: Option<Entity<markdown_view::View>>,
    conflict_view: Option<Entity<conflicts::ConflictView>>,
    conflict_subscription: Option<Subscription>,
    conflict_drafts: HashMap<(PathBuf, PathBuf), conflicts::Draft>,
    before_editor: Option<Entity<EditorState>>,
    after_editor: Option<Entity<EditorState>>,
    images: [Option<Arc<RenderImage>>; 2],
    text_mode: TextMode,
    review: text_review::State,
    zoom: f32,
    image_comparison: image_compare::State,
    image_scroll: ScrollHandle,
    image_drag: Option<(Point<Pixels>, Point<Pixels>)>,
    search: Entity<InputState>,
    nav_search: Entity<InputState>,
    subscriptions: Vec<Subscription>,
    _display_preferences_task: Option<Task<()>>,
    /// Whether More contrast was on when this window last applied its theme
    /// (`apply_appearance`), so a change of it, from any path, applies the
    /// theme again once (`GitTurtle::show_display_preferences`).
    applied_contrast: bool,
    app_focus: FocusHandle,
    focus: FocusHandle,
    file_focus: FocusHandle,
    /// The cursor row's menu in the changed-file or Working Changes list,
    /// opened from the keyboard (`native_accessibility::RowActions`).
    file_menu: Option<native_accessibility::KeyboardMenu>,
    /// The Working Changes row `file_menu` was opened on, while the menu is
    /// that list's: the file its items were last built from, by path and
    /// group, whether the row had its Stage or Unstage control, and whether
    /// they were built refusing writes. Installed rows move the menu with the
    /// row, build it again where the file's status changed, or close it where
    /// the row went, and a write's start or end builds it again from the file
    /// as it then stands (`follow_working_menu`).
    file_menu_row: Option<workspace::WorkingMenu>,
    /// The Working Changes file rows' element IDs, made once per row of the
    /// status the rows show (`working_row_key`).
    working_row_ids: std::cell::RefCell<workspace::WorkingRowIds>,
    /// The selected commit's menu in History's list, opened from the
    /// keyboard (`native_accessibility::RowActions`).
    commit_menu: Option<native_accessibility::KeyboardMenu>,
    /// The cursor row's menu in the navigator, opened from the keyboard
    /// (`native_accessibility::ManageNavigation`).
    nav_menu: Option<native_accessibility::KeyboardMenu>,
    /// The header's Pull requests button, full or compact: the GitHub panel it
    /// opens returns focus to it on close.
    pull_requests_focus: FocusHandle,
    /// Which list draws its keyboard focus ring.
    focus_ring: focus_ring::FocusRing,
    /// The dialog controls whose keys the status bar names.
    dialog_hints: shortcuts::DialogHints,
    pane: Pane,
    history_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    history_list_layout: Option<(Size<Pixels>, Pixels)>,
    /// The Working inspector's layout as last laid out
    /// (`working_inspector_layout`).
    working_layout: std::cell::Cell<Option<Entity<workspace::WorkingInspectorLayout>>>,
    file_list_layout: Option<(Size<Pixels>, Pixels)>,
    nav_scroll: UniformListScrollHandle,
    loading: Option<&'static str>,
    error: Option<String>,
    status: String,
    /// The parts the status was last joined from where one comes from user
    /// text (`views::StatusParts`): the status line reads them only while
    /// it still says that text, so a status set since, or restored, as File
    /// History's return restores one, needs nothing reset here.
    status_parts: views::StatusParts,
    /// A Git write's outcome, which the status line keeps over the reads
    /// that follow it for as long as a control confirms in place
    /// (`workspace::StatusOutcome`).
    status_outcome: Option<workspace::StatusOutcome>,
    sidebar: bool,
    history_sidebar: bool,
    /// Toggle Sidebar presses refused because History is too narrow for its
    /// navigation. The rail announces each one with a fresh polite status
    /// node, so a repeated press is heard again. Window-lived and never
    /// retained: render clears it once narrow History is not drawn, so
    /// returning there announces nothing stale.
    navigation_refusals: usize,
    history_width: f32,
    /// History's table insets and narrow History's subject floor, measured
    /// from the window's font and sizes by the last History render
    /// (`views::HistoryMetrics`), so the lane commands between frames lay the
    /// columns out as that frame did. Never saved or retained by a tab, and
    /// zero until History first renders.
    history_metrics: std::cell::Cell<views::HistoryMetrics>,
    mode: WorkspaceMode,
    restore_commit: Option<String>,
    preferred_file: Option<PathBuf>,
    interaction_started: Option<Instant>,
    inspector_message: commit_message::State,
}

impl GitTurtle {
    fn new(
        initial: Option<PathBuf>,
        preferences: Preferences,
        tab_session: repository_tabs::Session,
        activity: activity::State,
        recovery_drafts: recovery_drafts::State,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(views::SEARCH_PLACEHOLDERS[0]));
        let nav_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(views::NAV_FILTER_PLACEHOLDERS[0]));
        let settings = preferences.settings.clone();
        appearance::apply_text_sizes(
            settings.interface_text_size,
            settings.code_text_size,
            window,
            cx,
        );
        let hub = cx.new(|cx| {
            projects::ProjectHub::new(
                preferences.recent_repositories.clone(),
                preferences.project_names.clone(),
                settings.default_branch.clone(),
                window,
                cx,
            )
        });
        let commit_title =
            cx.new(|cx| InputState::new(window, cx).placeholder(workspace::TITLE_PLACEHOLDER));
        let commit_message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(workspace::DESCRIPTION_PLACEHOLDER)
                .rows(5)
        });
        let identity_name = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
        let identity_email =
            cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com"));
        let branch_name = cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-change"));
        let remote_name = cx.new(|cx| InputState::new(window, cx).placeholder("Remote"));
        let remote_branch = cx.new(|cx| InputState::new(window, cx).placeholder("Remote branch"));
        let settings_branch =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.default_branch.clone()));
        let settings_drafts = settings::DraftState::new(settings.default_branch.clone());
        let settings_editor = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.external_editor.clone())
                .placeholder(if cfg!(target_os = "macos") {
                    "Visual Studio Code"
                } else {
                    "code"
                })
        });
        // A built-in's miniature never changes: it draws one built-in
        // palette. A custom miniature draws its saved palette until that theme
        // is saved again. Their entities outlive a palette change so its frame
        // can reuse them.
        let theme_previews: Vec<(_, Entity<settings::ThemePreviewBody>)> =
            appearance::ThemeChoice::ALL
                .into_iter()
                .map(|choice| {
                    (
                        appearance::custom::ThemeSelection::BuiltIn(choice),
                        choice.palette(),
                    )
                })
                .chain(preferences.custom_themes.iter().map(|theme| {
                    (
                        appearance::custom::ThemeSelection::Custom(theme.id),
                        theme.palette,
                    )
                }))
                .map(|(selection, palette)| {
                    (
                        selection,
                        cx.new(|_| settings::ThemePreviewBody::new(palette)),
                    )
                })
                .collect();
        let theme_card_bodies = theme_previews
            .iter()
            .map(|(selection, miniature)| {
                let miniature = miniature.entity_id();
                (
                    *selection,
                    cx.new(|_| settings::ThemeCardBody::new(miniature)),
                )
            })
            .collect();
        let theme_card_focus = appearance::ThemeChoice::ALL
            .into_iter()
            .map(appearance::custom::ThemeSelection::BuiltIn)
            .chain(
                preferences
                    .custom_themes
                    .iter()
                    .map(|theme| appearance::custom::ThemeSelection::Custom(theme.id)),
            )
            .map(|selection| (selection, cx.focus_handle()))
            .collect();
        let file_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(views::CHANGED_FILTER_PLACEHOLDER));
        let working_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(workspace::FILTER_PLACEHOLDER));
        // The Settings page's own view; `render` embeds it cached and the app
        // notifies it at every site that alters the page
        // (`settings::SettingsPage`).
        let settings_page = {
            let owner = cx.entity().downgrade();
            cx.new(|cx| {
                settings::SettingsPage::new(
                    owner,
                    [
                        &settings_branch,
                        &settings_editor,
                        &identity_name,
                        &identity_email,
                    ],
                    cx,
                )
            })
        };
        let mut this = Self {
            activity,
            recovery_drafts,
            working_selection: working_selection::Selection::default(),
            working_filter: working_filter.clone(),
            file_filter: file_filter.clone(),
            file_paths: path_filter::State::default(),
            revision_inspection: revision_inspection::State::default(),
            authentication: authentication::State::default(),
            blame: blame::State::default(),
            tag_actions: tags::State::default(),
            worktree_management: worktrees::State::default(),
            interactive_rebase: interactive_rebase::State::default(),
            ignore_actions: ignore::State::default(),
            discard_actions: discard::State::default(),
            menu_state: None,
            #[cfg(target_os = "linux")]
            primary_menu: {
                let owner = cx.entity().downgrade();
                cx.new(|_| PrimaryMenu::new(owner))
            },
            dialog_layer_subscription: None,
            window_handle: window.window_handle(),
            modal_was_open: false,
            modal_return_focus: None,
            modal_replaced_focus: None,
            modal_focus_generation: 0,
            settings_editor,
            settings_page,
            picker_cards: settings::CardSlots::default(),
            history_search: history_search::State::default(),
            history_updates: history_updates::State::default(),
            file_history: file_history::State::default(),
            automatic: automatic_refresh::State::default(),
            branch_actions: branch_actions::State::default(),
            recovery: recovery::State::default(),
            page_return_focus: None,
            page_origin: AppPage::Projects,
            content_panels: cx.new(|_| ResizableState::default()),
            history_panels: cx.new(|_| ResizableState::default()),
            project_panels: cx.new(|_| ResizableState::default()),
            retained_history_files: None,
            commit_drafts: preferences.commit_drafts,
            project_names: preferences.project_names.clone(),
            project_library: preferences.project_library.clone(),
            custom_themes: preferences.custom_themes.clone(),
            theme_previews,
            theme_card_focus,
            theme_card_bodies,
            #[cfg(test)]
            draws: Vec::new(),
            #[cfg(test)]
            card_names: Default::default(),
            #[cfg(target_os = "linux")]
            omarchy_revision: None,
            #[cfg(target_os = "linux")]
            omarchy_choice: Default::default(),
            #[cfg(test)]
            follow_system_row: Default::default(),
            #[cfg(test)]
            theme_action_names: Default::default(),
            #[cfg(test)]
            theme_action_tooltips: Default::default(),
            #[cfg(test)]
            rail_navigation: Default::default(),
            #[cfg(test)]
            rail_announcement: Default::default(),
            #[cfg(test)]
            working_probe: Default::default(),
            project_pane: project_pane::State::new(cx),
            rename_project: None,
            draft_saver: commit_drafts::DraftSaver::default(),
            draft_save_error: None,
            draft_repository: None,
            settings,
            preferences_writer: SerialExecutor::new("gitturtle-preferences"),
            operations: SerialExecutor::new("gitturtle-operations"),
            operation_busy: None,
            busy_look: workspace::BusyLook::default(),
            operation_repository: None,
            operation_outcomes: operations::RepositoryOutcomes::default(),
            operation_error: None,
            error_refusals: 0,
            operation_notice: None,
            preview_showing: system_preview::Showing::next(),
            operation_task: None,
            status_task: None,
            status_reads: Default::default(),
            status_after_write: None,
            working_read_deferred: None,
            work_generation: 0,
            status_requests: 0,
            work_status: None,
            working_paths: working_selection::FilterState::default(),
            integration_state: None,
            integration_task: None,
            dropped_preparation: None,
            profile: None,
            profiles: profiles::State::default(),
            theme_editor: theme_editor::State::default(),
            remotes: Vec::new(),
            working_rows: Vec::new(),
            working_selected: None,
            working_scroll: UniformListScrollHandle::new(),
            working_travel: motion::RowTravel::Idle,
            still_pointer: None,
            page: if initial.is_some() {
                AppPage::Repository
            } else {
                AppPage::Projects
            },
            hub: hub.clone(),
            commit_title,
            commit_message,
            identity_name,
            identity_email,
            branch_name,
            branch_input_scope: BranchInputScope::default(),
            remote_name,
            remote_branch,
            settings_branch,
            settings_drafts,
            column_drag: None,
            column_menu: false,
            git_actions_open: false,
            layout_trace: None,
            history_horizontal: ScrollHandle::new(),
            worker: Worker::new(),
            task: None,
            generation: 0,
            preview_activation: None,
            preview_read: None,
            repository: None,
            path: None,
            scope: None,
            limit: 500,
            history_paging: history_paging::State::default(),
            repository_tabs: repository_tabs::State::from_session(tab_session),
            branches: vec![],
            worktrees: vec![],
            nav_rows: vec![],
            nav_ids: vec![],
            nav_cursor: Some(0),
            nav_focus: cx.focus_handle(),
            nav_mode: NavMode::Local,
            expanded_folders: HashSet::new(),
            seed_folders: true,
            commits: vec![],
            visible: vec![],
            graph: vec![],
            graph_lanes: 1,
            graph_offset: 0,
            graph_notice: None,
            refs: Default::default(),
            selected_commit: None,
            selected_file: None,
            parent: 0,
            files: vec![],
            content: None,
            patch_editor: None,
            patch_decoration: None,
            patch_view: None,
            partial_subscription: None,
            split_view: None,
            markdown_view: None,
            conflict_view: None,
            conflict_subscription: None,
            conflict_drafts: HashMap::new(),
            before_editor: None,
            after_editor: None,
            images: [None, None],
            text_mode: TextMode::Unified,
            review: text_review::State::default(),
            zoom: 0.,
            image_comparison: image_compare::State::default(),
            image_scroll: ScrollHandle::new(),
            image_drag: None,
            search: search.clone(),
            nav_search: nav_search.clone(),
            subscriptions: vec![],
            _display_preferences_task: None,
            applied_contrast: appearance::more_contrast(cx),
            app_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            file_menu: None,
            file_menu_row: None,
            working_row_ids: Default::default(),
            commit_menu: None,
            nav_menu: None,
            pull_requests_focus: cx.focus_handle(),
            focus_ring: focus_ring::FocusRing::default(),
            dialog_hints: shortcuts::DialogHints::default(),
            pane: Pane::History,
            history_scroll: UniformListScrollHandle::new(),
            file_scroll: UniformListScrollHandle::new(),
            history_list_layout: None,
            working_layout: Default::default(),
            file_list_layout: None,
            nav_scroll: UniformListScrollHandle::new(),
            loading: None,
            error: None,
            status: "Open a repository to explore its history".into(),
            status_parts: views::StatusParts::default(),
            status_outcome: None,
            sidebar: true,
            history_sidebar: true,
            navigation_refusals: 0,
            history_width: 900.,
            history_metrics: Default::default(),
            mode: WorkspaceMode::History,
            restore_commit: None,
            preferred_file: None,
            interaction_started: None,
            inspector_message: commit_message::State::default(),
        };
        this.rebuild_project_rows();
        this.load_profiles(window, cx);
        this.install_draft_quit_observer(cx);
        this.install_tab_quit_observer(window, cx);
        #[cfg(target_os = "linux")]
        this.subscriptions
            .push(cx.observe_button_layout_changed(window, |_, window, cx| {
                window.refresh();
                cx.notify();
            }));
        this.draft_saver.install_quit_observer(cx);
        this.subscriptions.push(cx.subscribe_in(
            &file_filter,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.change_file_filter(window, cx);
                }
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &working_filter,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filter_working_paths(window, cx);
                }
            },
        ));
        this.subscriptions
            .push(cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.follow_system {
                    this.apply_appearance(window, cx);
                }
            }));
        this.subscriptions.push(
            cx.subscribe_in(&search, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.history_query_changed(window, cx);
                    cx.notify();
                }
            }),
        );
        this.subscriptions.push(
            cx.subscribe_in(&nav_search, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) && !this.repository_tabs.switching {
                    this.rebuild_navigation(cx);
                    this.nav_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
            }),
        );
        this.subscriptions
            .push(cx.subscribe_in(&hub, window, |this, _, event, window, cx| {
                this.project_event(event, window, cx);
            }));
        this.subscriptions.push(cx.subscribe_in(
            &this.commit_message.clone(),
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.persist_commit_draft(window, cx);
                }
                cx.notify();
            },
        ));
        this.subscriptions.push(cx.subscribe_in(
            &this.commit_title.clone(),
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.persist_commit_draft(window, cx);
                }
                cx.notify();
            },
        ));
        for input in [&this.branch_name, &this.remote_name, &this.remote_branch] {
            this.subscriptions
                .push(cx.subscribe_in(input, window, |_, _, event, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }));
        }
        for panels in [
            &this.content_panels,
            &this.history_panels,
            &this.project_panels,
        ] {
            this.subscriptions
                .push(cx.observe(panels, |_, _, cx| cx.notify()));
        }
        this.subscribe_settings_inputs(window, cx);
        // Linux: the desktop's Omarchy theme, read at launch and followed while
        // it is selected, reaches this window through its one owner.
        #[cfg(target_os = "linux")]
        {
            this.subscriptions.push(
                cx.observe_global_in::<appearance::omarchy::Omarchy>(window, |this, window, cx| {
                    this.omarchy_changed(window, cx)
                }),
            );
            // What launch already read, applied once the window is assembled.
            cx.defer_in(window, |this, window, cx| this.omarchy_changed(window, cx));
            // The desktop's answer, which its settings-portal observer has
            // already resolved with the person's choices (`desktop_text`,
            // `native_accessibility::sync_preferences`), applies this window's
            // theme again when More contrast changed and pauses its playback
            // while motion is reduced, as the macOS display-change
            // notification does.
            this.subscriptions.push(
                cx.observe_global_in::<native_accessibility::DesktopDisplay>(
                    window,
                    |this, window, cx| {
                        let arrived = native_accessibility::desktop_arrived(cx);
                        this.show_display_preferences(
                            arrived.filter(|_| trace_enabled()),
                            window,
                            cx,
                        );
                        cx.notify();
                    },
                ),
            );
        }
        this._display_preferences_task =
            native_accessibility::observe_display_preferences(window, cx);
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    native_accessibility::sync_preferences(cx);
                    desktop_text::refresh(cx);
                    #[cfg(target_os = "linux")]
                    appearance::omarchy::refresh(cx);
                    this.show_display_preferences(None, window, cx);
                }
                if window.is_window_active() && this.repository.is_some() {
                    this.ensure_local_watcher(true, window, cx);
                    this.queue_automatic_refresh(
                        local_refresh::LocalChange {
                            rescan: true,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                }
            }));
        window.focus(
            if initial.is_some() {
                &this.focus
            } else {
                &this.app_focus
            },
            cx,
        );
        if let Some(path) = initial {
            // Saved tabs consult the window's dialog/sheet root while opening.
            // The caller installs Root only after this constructor returns.
            cx.defer_in(window, move |this, window, cx| {
                this.open(path, None, window, cx);
            });
        }
        this
    }

    fn request(
        &mut self,
        job: Job,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pause_history_search_for_read();
        self.cancel_automatic_read();
        self.generation += 1;
        let generation = self.generation;
        self.loading = Some(label);
        self.error = None;
        self.preview_read = PreviewRead::of(&job, generation);
        let reviewing = matches!(&job, Job::ReviewText { .. });
        let opening = matches!(&job, Job::Open { .. }).then(|| {
            self.operation_outcomes
                .begin_open(&self.operation_error, &self.operation_notice)
        });
        let response = self.worker.submit(job);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let reply = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.generation {
                    return;
                }
                this.loading = None;
                this.task = None;
                // A read that ends with no reply to use, failed or dropped,
                // ends a cold tab's restore waiting for it, as a read that
                // replaces it does (`end_replaced_restore`): the tab, left,
                // stays warm with what it shows. A restore that would have
                // gone on to Compare stays in History then, where History's
                // search asked for meanwhile opens (`run_pending_search`);
                // held for that Compare, it was lost.
                if !matches!(&reply, Ok(Ok(_))) && this.repository_tabs.restoring.take().is_some() {
                    this.run_pending_search(window, cx);
                }
                // Dropped unanswered by the worker, the read is over all the
                // same: left loading, it held quiet reads off, a write's
                // reload with them, and every menu that waits for that
                // reload stayed refused. An Open so dropped fails as an
                // interrupted one: ended alone, it left the window on the
                // repository page with nothing read and nothing to say why,
                // and the hub busy behind it, its actions refused.
                let result = match reply {
                    Ok(result) => result,
                    Err(_) if opening.is_some() => Err(anyhow::anyhow!(
                        "Repository read was interrupted. Open the repository again to retry."
                    )),
                    Err(_) => {
                        this.release_text_review(cx);
                        this.try_automatic_refresh(window, cx);
                        cx.notify();
                        return;
                    }
                };
                match result {
                    Ok(output) => {
                        if let (Some(opening), Output::Snapshot(snapshot)) = (opening, &output) {
                            this.operation_outcomes.opened(
                                snapshot.repository.path(),
                                opening,
                                &mut this.operation_error,
                                &mut this.operation_notice,
                            );
                        }
                        this.receive(output, window, cx);
                    }
                    Err(error) => {
                        this.release_text_review(cx);
                        this.error = Some(format!("{error:#}"));
                        if reviewing {
                            this.text_review_failed();
                        }
                        // Only the repository's own read speaks for it: a
                        // failed Open marks its tab and is the opening error
                        // the Projects page shows. Any other read's failure,
                        // a preview's or a review's, stays in its pane; set
                        // on the hub as well, a review past its limit met
                        // the person who closed the tab as Projects' alert.
                        let opening_failed = opening.is_some();
                        if opening_failed {
                            this.tab_opening_failed();
                        }
                        this.status = "Read could not complete".into();
                        this.hub.update(cx, |hub, cx| {
                            hub.set_busy(false, cx);
                            if opening_failed {
                                hub.set_error(this.error.clone(), cx);
                            }
                        });
                        if this.repository.is_none() {
                            this.page = AppPage::Projects;
                        }
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The preview read in flight, if the window's read under way is one:
    /// asked for by `request` at the current generation and not yet
    /// answered, whatever label it shows.
    fn preview_in_flight(&self) -> Option<&PreviewRead> {
        self.preview_read
            .as_ref()
            .filter(|read| read.generation == self.generation && self.loading.is_some())
    }

    fn open(
        &mut self,
        path: PathBuf,
        scope: Option<(String, worker::Scope)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // History's search held for a snapshot read earlier is for that
        // read alone (`run_pending_search`).
        self.repository_tabs.drop_pending_search();
        if self.tab_open_requested(&path, &scope, window, cx) {
            return;
        }
        // A repository opened again in place, as History's Refresh opens
        // it, drops what it was preparing, and waits for its read as a
        // switch that drops it does, or holds the wait for a preparation a
        // switch dropped, whichever tab asked for it, since that read still
        // runs on the executor every tab shares
        // (`wait_for_cancelled_preparation`): the snapshot's working read
        // waits for the wait's end rather than queue behind that read.
        // One opened in a new tab or a tab switched to has none shown here:
        // the tab left dropped what it was preparing (`retain_active_tab`),
        // and the window waits for that read as a warm tab's switch does.
        // Cancelled here too, a cold tab's switch, unlike a warm one's, let
        // the next tab queue its reads behind the read.
        if self.repository.is_some() {
            let busy = self.operation_busy;
            self.cancel_branch_action();
            self.cancel_recovery_read();
            self.cancel_tag_action();
            self.cancel_interactive_rebase_action();
            self.cancel_ignore_action();
            self.cancel_discard_action();
            self.wait_for_cancelled_preparation(busy, window, cx);
        }
        // The working read Changes asked for while a preparation cancelled
        // above ran is dropped: the snapshot opened below reads the working
        // state too. As History's Refresh cancelled Discard…'s review, the
        // read once stayed noted, and a later preparation's end read unasked.
        self.working_read_deferred = None;
        self.close_inspections(window, cx);
        self.close_blame(window, cx);
        self.blame = blame::State::default();
        self.discard_history_search();
        self.forget_page_return();
        self.restore_commit = if self.path.as_ref() == Some(&path) && self.scope == scope {
            self.selected_commit.map(|i| self.commits[i].oid.clone())
        } else {
            None
        };
        if self.path.as_ref() != Some(&path) {
            self.inspector_message
                .replace(commit_message::State::default(), cx);
            self.file_filter
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.automatic.reset();
            self.retained_history_files = None;
            self.repository = None;
            self.work_generation += 1;
            self.work_status = None;
            self.working_paths.reset();
            self.integration_state = None;
            self.profile = None;
            self.remotes.clear();
            self.working_rows.clear();
            self.working_selected = None;
            self.working_selection.clear();
            self.working_filter
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.remote_name
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.remote_branch
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.branches.clear();
            self.worktrees.clear();
            self.nav_rows.clear();
            self.nav_ids.clear();
            self.expanded_folders.clear();
            self.seed_folders = true;
            self.preferred_file = None;
            self.nav_scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.back_to_history(window, cx);
        self.history_updates.reset_history();
        if self.path.as_ref() != Some(&path) {
            self.history_updates.committed = None;
        }
        self.automatic.retained_commit = None;
        self.path = Some(path.clone());
        // A wait for a preparation another tab dropped says where it runs.
        self.hold_dropped_preparation();
        self.refs = Default::default();
        self.scope = scope;
        self.clear_preview();
        self.files.clear();
        self.file_paths.reset();
        self.selected_file = None;
        self.selected_commit = None;
        self.commits.clear();
        self.visible.clear();
        self.graph.clear();
        self.graph_notice = None;
        // A tab opened before finds its own repository again or none; only a
        // new tab's explicit open may land on the work tree enclosing `path`.
        let root = self
            .repository_tabs
            .reopen_root(&path, self.scope.as_ref().map(|s| &s.1));
        self.request(
            Job::Open {
                path,
                scope: self.scope.as_ref().map(|s| s.1.clone()),
                limit: self.limit,
                root,
            },
            "Reading local history…",
            window,
            cx,
        );
    }

    fn clear_preview(&mut self) {
        self.preview_showing = system_preview::Showing::next();
        self.repository_tabs.document_restore = None;
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
        self.review = text_review::State::default();
        self.image_drag = None;
        self.image_comparison = image_compare::State::default();
        self.content = None;
        self.patch_editor = None;
        self.patch_decoration = None;
        self.patch_view = None;
        self.partial_subscription = None;
        self.split_view = None;
        self.markdown_view = None;
        self.conflict_view = None;
        self.conflict_subscription = None;
        self.before_editor = None;
        self.after_editor = None;
        self.images = [None, None];
    }

    fn receive(&mut self, output: Output, window: &mut Window, cx: &mut Context<Self>) {
        match output {
            Output::HistoryReleased => {}
            Output::HistoryPage(result) => self.receive_history_page(result, window, cx),
            Output::ReviewText(content, elapsed) => {
                self.status = format!(
                    "Text review prepared · {:.1} ms",
                    elapsed.as_secs_f64() * 1000.
                );
                self.receive_text_review(content, window, cx);
            }
            Output::RevisionComparison(_)
            | Output::RevisionTargets(_)
            | Output::TrackedPaths(_) => {} // Modal-owned read results.
            Output::Blame(_)
            | Output::LineHistory(_)
            | Output::SearchHistory(_)
            | Output::FileHistory(_) => {} // Applied by their own bounded subscriptions.
            Output::QuietRefresh(_) => {} // Applied by the quiet-read subscription.
            Output::WorkingPreview(file, content, elapsed) => {
                if self.mode != WorkspaceMode::Working {
                    return;
                }
                self.files = vec![file];
                self.refresh_file_filter(window, cx);
                self.selected_file = Some(0);
                self.receive(Output::Preview(content, elapsed), window, cx);
            }
            Output::Snapshot(snapshot) => {
                if self.tab_snapshot_accepted(&snapshot.repository, window, cx) {
                    return;
                }
                self.accept_snapshot_scope(&snapshot);
                self.history_updates.captured(&snapshot);
                self.history_paging = history_paging::State::from_snapshot(&snapshot);
                self.status = format!(
                    "Local snapshot · {:.0} ms · {} · {}",
                    snapshot.elapsed.as_secs_f64() * 1000.,
                    counted(snapshot.branches.len(), "branch", "branches"),
                    counted(snapshot.worktrees.len(), "worktree", "worktrees")
                );
                self.refs = snapshot.refs;
                self.branches = snapshot.branches;
                if self.seed_folders {
                    self.expanded_folders = navigation::current_ancestors(&self.branches);
                    self.seed_folders = false;
                }
                self.worktrees = snapshot.worktrees;
                self.commits = snapshot.commits;
                self.graph = snapshot.graph;
                self.graph_notice = snapshot.graph_notice;
                self.graph_lanes = self.graph.iter().map(|r| r.width).max().unwrap_or(1);
                let resolved = snapshot.repository.path().to_owned();
                if self.branch_input_scope.opened(&resolved) {
                    self.nav_search
                        .update(cx, |input, cx| input.set_value("", window, cx));
                    self.branch_name
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                self.restore_commit_draft(resolved, window, cx);
                self.repository_tabs
                    .resolve_pending_search(self.path.as_deref(), snapshot.repository.path());
                self.path = Some(snapshot.repository.path().to_owned());
                self.repository = Some(snapshot.repository);
                self.ensure_local_watcher(false, window, cx);
                self.hub.update(cx, |hub, cx| {
                    hub.set_busy(false, cx);
                    hub.set_error(None, cx);
                    hub.set_can_go_back(true, cx);
                });
                self.remember_repository(window, cx);
                self.refresh_worktree(window, cx);
                self.rebuild_navigation(cx);
                self.filter_history(cx);
                self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
                let selected = self
                    .restore_commit
                    .take()
                    .and_then(|oid| {
                        self.visible
                            .iter()
                            .copied()
                            .find(|&i| self.commits[i].oid == oid)
                    })
                    .or_else(|| self.visible.first().copied());
                if self.repository_tabs.restoring.is_some() {
                    self.finish_tab_snapshot(window, cx);
                    // History's search asked for meanwhile opens now where
                    // the restore stays in History, and as it lands where it
                    // goes on to Compare (`land_restore_focus`).
                    if !self.repository_tabs.restore_lands_in_compare() {
                        self.run_pending_search(window, cx);
                    }
                    return;
                }
                if self.mode == WorkspaceMode::Working {
                    self.selected_commit = selected;
                } else if let Some(index) = selected {
                    self.select_commit(index, window, cx);
                    if let Some(position) = self.visible.iter().position(|&i| i == index) {
                        self.history_scroll
                            .scroll_to_item(position, ScrollStrategy::Center);
                    }
                }
                if !self.search.read(cx).value().is_empty() {
                    self.history_query_changed(window, cx);
                }
                self.run_pending_search(window, cx);
            }
            Output::Changes(files, elapsed) => {
                self.status = format!(
                    "{} · file list {:.1} ms",
                    changed_files(files.len()),
                    elapsed.as_secs_f64() * 1000.
                );
                self.files = files;
                self.refresh_file_filter(window, cx);
                if !self.files.is_empty() {
                    let index = self
                        .preferred_file
                        .as_ref()
                        .and_then(|path| self.files.iter().position(|f| f.path() == path))
                        .unwrap_or(0);
                    self.file_scroll
                        .scroll_to_item(index, ScrollStrategy::Center);
                    self.selected_file = Some(index);
                    self.preferred_file = Some(self.files[index].path().to_owned());
                    if self.mode == WorkspaceMode::Compare {
                        self.load_file(index, window, cx);
                    }
                }
                if self.mode == WorkspaceMode::History {
                    self.trace_frame("commit_files_frame_ms", window, cx);
                }
                self.restore_tab_file(window, cx);
            }
            Output::Preview(content, elapsed) => {
                // A reply for a comparison that is no longer shown is
                // dropped, with the document bookmark that waited for it,
                // should its generation still match: Back cancels the
                // comparison's read, restored with its tab or not, but a
                // restored one's reply once landed after Back, wrote "… ·
                // content read" over History's status line and kept a
                // comparison nothing showed. Another page shown over Compare
                // keeps it, for the return.
                if !matches!(self.mode, WorkspaceMode::Compare | WorkspaceMode::Working) {
                    self.repository_tabs.document_restore = None;
                    return;
                }
                // A working preview's `files` is the one previewed file.
                let summary = if self.mode == WorkspaceMode::Working {
                    self.working_summary()
                } else {
                    changed_files(self.files.len())
                };
                self.status = format!(
                    "{summary} · content read {:.1} ms",
                    elapsed.as_secs_f64() * 1000.
                );
                match content.as_ref() {
                    Content::Text {
                        diagrams, markdown, ..
                    } => {
                        if let (Some(view), Some(markdown)) = (&self.markdown_view, markdown) {
                            view.update(cx, |view, cx| view.replace(markdown.clone(), cx));
                        }
                        if (self.text_mode == TextMode::Diagrams && diagrams.is_none())
                            || (self.text_mode == TextMode::Markdown && markdown.is_none())
                        {
                            self.text_mode = if self.is_quick_source() {
                                TextMode::After
                            } else {
                                TextMode::Unified
                            };
                        }
                    }
                    Content::Images { old, new } => {
                        self.images = [old.render.clone(), new.render.clone()];
                    }
                    Content::Notice(_) | Content::Conflict(_) | Content::Rich(_) => {}
                }
                self.content = Some(content);
                // Compare or Working, as checked above; on the repository
                // page, an editor for it is built now.
                if self.page == AppPage::Repository {
                    // Only the read the person's choice of the file started
                    // activates it. A write's status read selecting the row
                    // again took focus into a conflict's draft, closing a
                    // row's keyboard menu that the selection had kept open.
                    // And only while focus stays on the list, where the
                    // choice left it: moved on before the read landed, into
                    // the filter field or the row's menu, it lost focus to
                    // the draft.
                    let activated = self.preview_activation.take() == Some(self.generation)
                        && self.file_focus.is_focused(window);
                    self.prepare_editor(activated, window, cx);
                    self.restore_tab_documents(cx);
                    self.trace_frame(
                        if self.mode == WorkspaceMode::Working {
                            "working_preview_frame_ms"
                        } else {
                            "file_preview_frame_ms"
                        },
                        window,
                        cx,
                    );
                }
            }
        }
    }

    fn ensure_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_editor(true, window, cx);
    }

    /// Builds the shown preview's editor, unless built. A conflict's result
    /// draft takes focus as its view opens only where `activated`, the
    /// preview answering the person's choice of its file
    /// ([`Self::ensure_conflict_view`]).
    fn prepare_editor(&mut self, activated: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.blame.is_visible() {
            return;
        }
        let Some(content) = &self.content else {
            return;
        };
        if let Content::Conflict(presentation) = content.as_ref() {
            self.ensure_conflict_view(Arc::clone(presentation), activated, window, cx);
            return;
        }
        let Content::Text {
            patch,
            old,
            new,
            presentation,
            split,
            partial,
            ..
        } = content.as_ref()
        else {
            return;
        };
        let language = self
            .selected_file
            .and_then(|i| self.files[i].path().extension())
            .and_then(|s| s.to_str())
            .map(language_for)
            .unwrap_or("text");
        if self.text_mode == TextMode::Markdown {
            if self.markdown_view.is_none()
                && let Content::Text {
                    markdown: Some(markdown),
                    ..
                } = content.as_ref()
            {
                let markdown = markdown.clone();
                let quick = self.is_quick_source();
                self.markdown_view =
                    Some(cx.new(|cx| markdown_view::View::new(markdown, quick, cx)));
            }
            return;
        }
        if self.text_mode == TextMode::Diagrams {
            return;
        }
        if self.text_mode == TextMode::Split {
            if self.split_view.is_none() {
                self.split_view = Some(split_diff::new(
                    Arc::clone(split),
                    language,
                    self.focus_ring.clone(),
                    window,
                    cx,
                ));
            }
            return;
        }
        let (slot, value, language, diff) = match self.text_mode {
            TextMode::Split | TextMode::Diagrams | TextMode::Markdown => unreachable!(),
            TextMode::Unified => (&mut self.patch_editor, patch, "diff", true),
            TextMode::Before => (&mut self.before_editor, old, language, false),
            TextMode::After => (&mut self.after_editor, new, language, false),
        };
        if slot.is_none() {
            let (editor, decoration) = text::editor_with_decorations(
                value,
                language,
                diff.then_some(presentation.as_ref()),
                window,
                cx,
            );
            *slot = Some(editor);
            if diff {
                self.patch_decoration = decoration;
            }
        }
        if diff
            && self.patch_view.is_none()
            && let Some(editor) = &self.patch_editor
        {
            let view = if let Some(partial) = partial {
                diff_view::new_partial(
                    editor.clone(),
                    presentation,
                    Arc::clone(partial),
                    window,
                    cx,
                )
            } else {
                diff_view::new(editor.clone(), presentation, window, cx)
            };
            self.bind_text_partial(&view, window, cx);
            self.patch_view = Some(view);
        }
    }

    fn filter_history(&mut self, cx: &App) {
        self.filter_history_retaining_scroll(cx);
        self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
    fn filter_history_retaining_scroll(&mut self, _cx: &App) {
        if self.history_search_active() {
            self.visible = (0..self.commits.len()).collect();
            return;
        }
        self.visible = self
            .commits
            .iter()
            .enumerate()
            .filter(|(index, _)| self.automatic.retained_commit != Some(*index))
            .map(|(i, _)| i)
            .collect();
    }
    fn rebuild_navigation(&mut self, cx: &App) {
        // What the cursor row showed, from the rows as they were built: the
        // branches they index may already be the new ones.
        let followed = self
            .nav_cursor
            .and_then(|index| self.nav_ids.get(index).cloned());
        let query = self.nav_search.read(cx).value().trim().to_lowercase();
        self.nav_rows = vec![NavRow::All];
        if self.nav_mode != NavMode::Worktrees {
            let remote = self.nav_mode == NavMode::Remote;
            let count = self
                .branches
                .iter()
                .filter(|b| {
                    b.remote == remote
                        && (query.is_empty() || b.name.to_lowercase().contains(&query))
                })
                .count();
            self.nav_rows.push(NavRow::Section(
                if remote {
                    "REMOTE BRANCHES"
                } else {
                    "LOCAL BRANCHES"
                },
                count,
            ));
            self.nav_rows.extend(
                navigation::branch_rows(&self.branches, remote, &query, &self.expanded_folders)
                    .into_iter()
                    .map(|row| match row {
                        navigation::Row::Folder {
                            key,
                            label,
                            depth,
                            count,
                            expanded,
                        } => NavRow::Folder {
                            key,
                            label,
                            depth,
                            count,
                            expanded,
                        },
                        navigation::Row::Branch { index, depth } => NavRow::Branch(index, depth),
                    }),
            );
        } else {
            let matching: Vec<_> = self
                .worktrees
                .iter()
                .enumerate()
                .filter(|(_, w)| {
                    w.path.to_string_lossy().to_lowercase().contains(&query)
                        || self.project_name(&w.path).to_lowercase().contains(&query)
                        || w.branch
                            .as_ref()
                            .is_some_and(|b| b.to_lowercase().contains(&query))
                })
                .map(|(i, _)| NavRow::Worktree(i))
                .collect();
            self.nav_rows
                .push(NavRow::Section("WORKTREES", matching.len()));
            self.nav_rows.extend(matching);
        }
        self.nav_ids = self
            .nav_rows
            .iter()
            .map(|row| self.nav_row_id(row))
            .collect();
        // The cursor stays on what its row showed, wherever rows that came or
        // went before it moved that row; kept at its place, it passed to
        // whichever branch moved in, as one created above it did.
        let followed = followed.and_then(|id| self.nav_ids.iter().position(|row| *row == id));
        // Where its row left the tree, as a branch deleted under its menu,
        // the cursor goes to the row that took its place, or the one before
        // where it was the last, past a section's heading: it once went to
        // the top.
        self.nav_cursor = followed
            .or_else(|| {
                let at = self.nav_cursor?.min(self.nav_rows.len().checked_sub(1)?);
                (0..=at)
                    .rev()
                    .find(|index| !matches!(self.nav_rows[*index], NavRow::Section(..)))
            })
            .or(Some(0));
        // A keyboard's menu, opened on the cursor row, is drawn beside that
        // row where it now stands.
        if let (Some(row), Some(menu)) = (followed, self.nav_menu.as_mut()) {
            menu.follow_row(row);
        }
    }

    /// A navigator row's element ID, from what it shows: its branch's name
    /// and whether it is remote, its worktree's path or its folder's key. The
    /// state GPUI keeps for the row, its open right-click menu and tooltip
    /// among it, stays with that branch, worktree or folder as rows come or
    /// go around it, and goes with the row when it leaves the tree; keyed by
    /// its place, a menu open on a branch passed to the branch that moved
    /// into that place. Made as the rows are built (`nav_ids`), so a frame
    /// clones each rather than formatting it.
    fn nav_row_id(&self, row: &NavRow) -> ElementId {
        match row {
            NavRow::Section(label, _) => ElementId::Name((*label).into()),
            NavRow::All => ElementId::Name("nav-all".into()),
            NavRow::Folder { key, .. } => ElementId::Name(format!("nav-folder:{key}").into()),
            NavRow::Branch(branch, _) => {
                let branch = &self.branches[*branch];
                let side = if branch.remote { "remote" } else { "local" };
                ElementId::Name(format!("nav-{side}:{}", branch.name).into())
            }
            NavRow::Worktree(tree) => {
                ElementId::Path(Arc::from(self.worktrees[*tree].path.as_path()))
            }
        }
    }

    /// The element ID of the navigator's `index`th row (`nav_ids`).
    fn nav_row_key(&self, index: usize) -> ElementId {
        self.nav_ids
            .get(index)
            .cloned()
            .unwrap_or_else(|| ElementId::named_usize("nav", index))
    }

    fn trace_frame(&mut self, metric: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.interaction_started.take() else {
            return;
        };
        if !trace_enabled() {
            return;
        }
        let generation = self.generation;
        let mode = self.mode;
        let view = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = view.update(cx, |this, _| {
                if this.generation == generation && this.mode == mode {
                    eprintln!(
                        "gitturtle.{metric}={:.3}",
                        start.elapsed().as_secs_f64() * 1000.
                    );
                }
            });
        });
    }

    /// Print `gitturtle.<metric>` from a handler-captured `start` at the
    /// window's next frame callback. Unlike `trace_frame`, the traced work
    /// changes neither content generation nor mode, so no staleness guard or
    /// shared `interaction_started` slot applies; each call prints once.
    fn trace_next_frame(metric: &'static str, start: Instant, window: &mut Window) {
        window.on_next_frame(move |_, _| {
            eprintln!(
                "gitturtle.{metric}={:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
        });
    }

    /// An explicit History destination exits every retained inspection. Back
    /// remains a one-level return so comparisons keep their navigation context.
    fn show_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository.is_none() {
            self.forget_page_return();
            self.show_projects(window, cx);
            return;
        }
        self.close_inspections(window, cx);
        self.forget_page_return();
        self.back_to_history(window, cx);
        self.repaint_page(window, cx);
    }

    fn back_to_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
        // A preview still being read is abandoned whatever its label, the
        // comparison's own, one restored with its tab, a tracked or working
        // file's, or a text review's preparation of the comparison shown:
        // cancelling the read now also stops what it prepares, such as the
        // PDF helper rendering a first page or an image's decode, at its
        // next check. A restored comparison's read once ran on after Back,
        // its helper to the 10 s deadline, only for `receive` to drop the
        // reply, and a text review's preparation ran on as long as Back
        // checked only the comparison's labels. The comparison it was for
        // is not kept, nor the bookmark that waited for its reply: a review
        // abandoned half prepared left its partial actions suspended for
        // the return.
        if self.preview_in_flight().is_some() {
            self.invalidate_read();
            self.clear_preview();
        }
        if self.close_blame(window, cx) {
            return;
        }
        if self.close_file_history(window, cx) {
            return;
        }
        if self.close_revision_inspection(window, cx) {
            return;
        }
        let was_working = self.mode == WorkspaceMode::Working;
        if self.mode != WorkspaceMode::History {
            self.mode = WorkspaceMode::History;
            self.sidebar = self.history_sidebar;
        }
        if was_working {
            self.invalidate_read();
            self.clear_preview();
            let (files, selected) = self.retained_history_files.take().unwrap_or_default();
            self.files = files;
            self.refresh_file_filter(window, cx);
            self.selected_file = selected;
            self.status = changed_files(self.files.len());
            // A plan History asked for that Changes dropped as it opened
            // opens here as its preparation ends: it once stayed dropped.
            // What Changes was preparing, such as Discard…'s review, is
            // dropped: its confirmation once opened over History.
            self.end_preparations(page_navigation::PageSwitch::ToHistory, window, cx);
        }
        if was_working
            && self.files.is_empty()
            && let (Some(repo), Some(commit)) = (
                &self.repository,
                self.selected_commit.and_then(|i| self.commits.get(i)),
            )
        {
            self.request(
                Job::Changes {
                    repo: repo.clone(),
                    oid: commit.oid.clone(),
                    parent: self.parent,
                },
                "Reading changed files…",
                window,
                cx,
            );
        }
        self.page = AppPage::Repository;
        self.interaction_started = None;
        self.pane = Pane::History;
        window.focus(&self.focus, cx);
        self.try_automatic_refresh(window, cx);
        cx.notify();
    }
    fn select_commit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_blame(window, cx);
        self.close_inspections(window, cx);
        if self.mode == WorkspaceMode::Working {
            self.mode = WorkspaceMode::History;
            self.working_selected = None;
        }
        if index >= self.commits.len() {
            return;
        }
        self.interaction_started = Some(Instant::now());
        self.preferred_file = self
            .selected_file
            .map(|i| self.files[i].path().to_owned())
            .or_else(|| self.preferred_file.take());
        self.inspector_message.select(&self.commits[index].oid, cx);
        self.selected_commit = Some(index);
        self.reveal_graph_lane(index);
        self.parent = 0;
        self.files.clear();
        self.file_paths.reset();
        self.selected_file = None;
        self.clear_preview();
        if let Some(repo) = &self.repository {
            self.request(
                Job::Changes {
                    repo: repo.clone(),
                    oid: self.commits[index].oid.clone(),
                    parent: 0,
                },
                "Reading changed files…",
                window,
                cx,
            );
        }
    }
    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.show_file(index, true, window, cx);
    }
    /// Shows changed file `index` in Compare as choosing it does, focusing
    /// the changed-file list where `focus`. Where not, as for a tab's
    /// restore (`restore_tab_file`), which then lands focus by its own rule
    /// (`land_restore_focus`), the window names that list's pane and leaves
    /// focus where it is.
    fn show_file(
        &mut self,
        index: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_blame(window, cx);
        if index >= self.files.len() {
            return;
        }
        // The file whose preview is still being read, chosen again, keeps
        // that read, timed from the choice that started it: the same file of
        // the same revision pair reads nothing new. Chosen again, it was read
        // again from the start, a slow image's decode with it.
        let reading = self.selected_file == Some(index) && self.reads_preview_of(index);
        if !reading {
            self.interaction_started = Some(Instant::now());
        }
        if self.mode == WorkspaceMode::History {
            self.history_sidebar = self.sidebar;
            self.sidebar = false;
            self.mode = WorkspaceMode::Compare;
        }
        if focus {
            self.focus_compare_files(window, cx);
        } else {
            self.pane = Pane::Files;
        }
        if reading {
            cx.notify();
        } else if self.selected_file == Some(index) && self.content.is_some() {
            self.ensure_editor(window, cx);
            self.trace_frame("file_preview_frame_ms", window, cx);
            cx.notify();
        } else {
            self.load_file(index, window, cx);
        }
    }
    /// Puts focus where entering Compare puts it: on the changed-file list,
    /// whose selected file the comparison shows and from which Escape goes
    /// back to History. Choosing a file focuses it, and so does a cold tab's
    /// restore that lands in Compare (`GitTurtle::land_restore_focus`).
    fn focus_compare_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
    }
    fn load_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.files.len() {
            return;
        }
        self.selected_file = Some(index);
        self.preferred_file = Some(self.files[index].path().to_owned());
        self.clear_preview();
        self.zoom = 0.;
        self.image_scroll.set_offset(point(px(0.), px(0.)));
        if let Some(repo) = &self.repository {
            let origins = self.preview_origins();
            self.request(
                Job::Preview {
                    origins,
                    repo: repo.clone(),
                    file: self.files[index].clone(),
                },
                PREVIEW_LABEL,
                window,
                cx,
            );
        }
    }
    /// The revision pair a changed file's comparison is read against, for
    /// the documents it shows: File history's or an inspection's, else the
    /// selected commit and its compared parent.
    fn preview_origins(&self) -> markdown_view::Origins {
        self.file_history_preview_origins()
            .or_else(|| self.inspection_preview_origins())
            .unwrap_or_else(|| {
                let commit = self
                    .selected_commit
                    .and_then(|index| self.commits.get(index));
                markdown_view::Origins::revisions(
                    commit
                        .and_then(|commit| commit.parents.get(self.parent))
                        .cloned(),
                    commit.map(|commit| commit.oid.clone()),
                )
            })
    }
    /// Whether the preview read in flight reads changed file `index` as
    /// `load_file` would read it now: the same file of the same repository
    /// against the same revision pair ([`PreviewTarget`]).
    fn reads_preview_of(&self, index: usize) -> bool {
        let (Some(target), Some(repo), Some(file)) = (
            self.preview_in_flight()
                .and_then(|read| read.target.as_ref()),
            &self.repository,
            self.files.get(index),
        ) else {
            return false;
        };
        target.repository == repo.path()
            && target.file == *file
            && target.origins == self.preview_origins()
    }
    fn change_parent(&mut self, parent: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.close_inspections(window, cx);
        let (Some(index), Some(repo)) = (self.selected_commit, &self.repository) else {
            return;
        };
        let job = Job::Changes {
            repo: repo.clone(),
            oid: self.commits[index].oid.clone(),
            parent,
        };
        self.interaction_started = Some(Instant::now());
        self.parent = parent;
        self.files.clear();
        self.file_paths.reset();
        self.selected_file = None;
        self.clear_preview();
        self.request(job, "Reading comparison…", window, cx);
    }
    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        if !self.file_history.is_active() && self.refresh_revision_inspection(window, cx) {
            return;
        }
        self.close_inspections(window, cx);
        if self.mode == WorkspaceMode::Working {
            self.refresh_worktree(window, cx);
        } else if let Some(path) = self.path.clone() {
            self.open(path, self.scope.clone(), window, cx);
        }
    }
    fn choose_repository(
        &mut self,
        _: &OpenRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let response = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Git repository".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let response = folder_picker::selected_path(response).await;
            let _ = this.update_in(cx, |this, window, cx| match response {
                Ok(Some(path)) => {
                    this.open_repository_tab(path, window, cx);
                }
                Ok(None) => {}
                Err(error) => window.open_alert_dialog(cx, move |dialog, window, cx| {
                    dialog
                        .kind(DialogKind::Alert)
                        .max_h(appearance::dialog_max_height(window))
                        .title("Could not open repository picker")
                        .child(
                            native_accessibility::static_text(
                                div().id("repository-picker-error"),
                                error.clone(),
                            )
                            .child(folder_picker::styled_message(&error, palette(cx).muted)),
                        )
                }),
            });
        })
        .detach();
    }
    fn move_selection(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository {
            return;
        }
        if self.move_blame_line(direction, edge, window, cx) {
            return;
        }
        if self.move_file_history_revision(direction, edge, window, cx) {
            return;
        }
        if self.mode == WorkspaceMode::Working {
            self.move_working_selection(direction, edge, window, cx);
            return;
        }
        // Keyboard focus can arrive through Tab without a mouse-down handler.
        self.pane = if self.file_focus.is_focused(window) {
            Pane::Files
        } else {
            Pane::History
        };
        if self.pane == Pane::Files {
            let visible = self.filtered_file_indices(cx);
            if visible.is_empty() {
                return;
            }
            let current = self
                .selected_file
                .and_then(|index| visible.iter().position(|item| *item == index))
                .unwrap_or(0);
            let position = if edge {
                if direction < 0 { 0 } else { visible.len() - 1 }
            } else {
                (current as i32 + direction).clamp(0, visible.len() as i32 - 1) as usize
            };
            self.select_file(visible[position], window, cx);
            self.file_scroll
                .scroll_to_item(position, ScrollStrategy::Center);
        } else {
            if self.visible.is_empty() {
                return;
            }
            let position = self
                .selected_commit
                .and_then(|selected| self.visible.iter().position(|&i| i == selected))
                .unwrap_or(0);
            let position = if edge {
                if direction < 0 {
                    0
                } else {
                    self.visible.len() - 1
                }
            } else {
                (position as i32 + direction).clamp(0, self.visible.len() as i32 - 1) as usize
            };
            self.select_commit(self.visible[position], window, cx);
            self.history_scroll
                .scroll_to_item(position, ScrollStrategy::Center);
        }
    }
    fn search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        // While the tab's snapshot is read, History has nothing to show:
        // the search opens as the tab lands (`defer_search`).
        if self.defer_search(window, cx) {
            return;
        }
        self.show_history(window, cx);
        // With no repository at all, History shows Projects, whose own
        // control takes focus: History's search field is drawn on no page
        // then, and focus on it reached no key until a click.
        if self.repository.is_some() {
            window.focus(&self.search.read(cx).focus_handle(cx), cx);
        }
    }
    /// Whether Compare is open, which the window's key context names
    /// (`shortcuts::COMPARE`), so the platform's Find shortcut finds in the
    /// comparison from anywhere in the window.
    fn comparing(&self) -> bool {
        self.page == AppPage::Repository && self.mode == WorkspaceMode::Compare
    }
    /// Opens Find on the text Compare shows, with its field focused, from
    /// wherever focus is in the window, as the shortcut does from the text
    /// itself: from the Changed files list it left Compare for History's
    /// search, and from a toolbar control or the inspector it did nothing
    /// (QA-90, decided by the coordinator). Where Compare shows no text, an
    /// image, PDF pages, a rendered document or diagram, a notice, Blame or
    /// an error, it does nothing and keeps the key: a Find key never leaves
    /// Compare for History's search, which would lose the comparison, and
    /// Back is the way out (decided by the coordinator).
    fn find_in_comparison(
        &mut self,
        _: &FindInComparison,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.comparing() || window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            cx.propagate();
            return;
        }
        let Some(text) = self.comparison_find_focus(cx) else {
            return;
        };
        // The text's own Find opens on it, as the key does from the text
        // (`editor_find::Editor`), and takes focus into its field.
        window.focus(&text, cx);
        window.dispatch_action(Box::new(gpui_kit::component::input::Search), cx);
    }
    /// The focus of the text Find opens on in Compare, the text drawn this
    /// frame (`shown_text`): the patch, Before or After, and in Split the
    /// side whose find bar is open, else After, or Before where After has no
    /// text, as a deleted file's. None where no text is drawn, as with Blame
    /// open or an error shown, where an editor kept from before is hidden:
    /// focused, it took the keyboard where nothing showed it.
    fn comparison_find_focus(&self, cx: &App) -> Option<FocusHandle> {
        let editor = match self.shown_text()? {
            views::ShownText::Editor(editor) => editor.clone(),
            views::ShownText::Split(view) => {
                let [before, after] = view.read(cx).editors().clone();
                let open =
                    |editor: &Entity<EditorState>| editor_find::panel_height(editor, cx) > px(0.);
                let no_after = matches!(
                    self.content.as_deref(),
                    Some(Content::Text { new, .. }) if new.is_empty()
                );
                if open(&before) || (no_after && !open(&after)) {
                    before
                } else {
                    after
                }
            }
        };
        Some(gpui_kit::Focusable::focus_handle(editor.read(cx), cx))
    }
    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        if self.column_menu {
            self.column_menu = false;
            cx.notify();
            return;
        }
        if self.page != AppPage::Repository {
            self.return_from_page(window, cx);
            return;
        }
        if self.mode != WorkspaceMode::History
            || self.blame.is_visible()
            || self.file_history.is_active()
            || self.revision_inspection.is_active()
        {
            self.back_to_history(window, cx);
            self.repaint_page(window, cx);
            return;
        }
        self.clear_history_search(window, cx);
    }
}

fn app_icon(dimension: f32) -> Img {
    app_icon_img(app_icon_source())
        .size(px(dimension))
        .object_fit(ObjectFit::Contain)
        .flex_shrink_0()
}

/// The app icon's image: the embedded asset, by its static path, which GPUI
/// takes as it is. Given the path as a `&str`, GPUI parsed it as a URL and
/// copied it into a new string on every frame that drew the icon.
fn app_icon_source() -> ImageSource {
    ImageSource::Resource(gpui::Resource::Embedded(SharedString::new_static(
        APP_ICON_PATH,
    )))
}

/// The app icon's image from `source`, noted for a test with what making
/// it an image source allocated ([`APP_ICON_DRAWN`]).
fn app_icon_img(source: impl Into<ImageSource>) -> Img {
    #[cfg(test)]
    let before = test_support::allocations();
    let source = source.into();
    #[cfg(test)]
    {
        let made = test_support::allocations() - before;
        APP_ICON_DRAWN.with_borrow_mut(|drawn| *drawn = Some((source.clone(), made)));
    }
    img(source)
}

#[cfg(test)]
thread_local! {
    /// The image source the app icon was last drawn from, and the
    /// allocations making it took ([`app_icon_img`]), for a test.
    static APP_ICON_DRAWN: std::cell::RefCell<Option<(ImageSource, u64)>> =
        const { std::cell::RefCell::new(None) };
}

/// What the shared helpers draw for an icon's `symbol`, formatted once per
/// symbol ([`symbol_text`]).
#[derive(Clone)]
struct SymbolText {
    /// The icon's asset, `icons/<symbol>.svg`.
    path: SharedString,
    /// An icon-only control's name until its caller names it
    /// ([`button_frame`]): `<symbol> file path`.
    name: SharedString,
}

thread_local! {
    /// [`SymbolText`] for each symbol drawn, by symbol: the app's own icon
    /// names, a fixed set.
    static SYMBOL_TEXT: std::cell::RefCell<HashMap<Box<str>, SymbolText>> =
        std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
thread_local! {
    /// How many times a symbol's text was formatted ([`symbol_text`]), for a
    /// test.
    static SYMBOL_TEXT_FORMATTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times a symbol's text was asked for, for a test.
    static SYMBOL_TEXT_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What `read` takes from the text the shared helpers draw for `symbol`
/// ([`button`], [`button_frame`], [`control_icon`], [`icon`]), formatted the
/// first time the symbol is drawn and kept, so a frame at rest formats none
/// of it. Each Button formatted its icon's path, and an icon-only one its
/// placeholder name, on every frame.
fn symbol_text<R>(symbol: &str, read: impl FnOnce(&SymbolText) -> R) -> R {
    #[cfg(test)]
    SYMBOL_TEXT_LOOKUPS.set(SYMBOL_TEXT_LOOKUPS.get() + 1);
    SYMBOL_TEXT.with_borrow_mut(|kept| {
        if let Some(text) = kept.get(symbol) {
            return read(text);
        }
        #[cfg(test)]
        SYMBOL_TEXT_FORMATTED.set(SYMBOL_TEXT_FORMATTED.get() + 1);
        let text = SymbolText {
            path: format!("icons/{symbol}.svg").into(),
            name: format!("{symbol} file path").into(),
        };
        read(kept.entry(symbol.into()).or_insert(text))
    })
}

/// The icon asset `symbol` names, `icons/<symbol>.svg` ([`symbol_text`]).
fn icon_path(symbol: &str) -> SharedString {
    symbol_text(symbol, |text| text.path.clone())
}

/// Empties [`SYMBOL_TEXT`], for a test that counts what it formats: a GPUI
/// test's seeds share the thread and its cache.
#[cfg(test)]
fn forget_symbol_text() {
    SYMBOL_TEXT.with_borrow_mut(HashMap::clear);
}

fn icon(name: &str, dimension: f32, color: u32) -> Svg {
    svg()
        .path(icon_path(name))
        .size(px(dimension))
        .text_color(rgb(color))
        .flex_shrink_0()
}
fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    symbol: &str,
    active: bool,
) -> Button {
    let (id, label) = (id.into(), label.into());
    let path = (!symbol.is_empty()).then(|| icon_path(symbol));
    if let Some(path) = &path {
        // Only tests read the icon back.
        views::note_control_icon(&id, path);
    }
    let mut button = button_frame(id, &label, symbol, active);
    if !label.is_empty() {
        button = button.label(label);
    }
    if let Some(path) = path {
        button = button.icon(control_icon_at(path));
    }
    button
}

/// The space beside a labelled compact Button's label, at the default
/// interface size: text beside or below such a Button that takes it lines up
/// with the Button's label.
const LABEL_INSET: f32 = 10.;

/// How far a ghost Button, a labelled control of the [`button`] helper,
/// transparent at rest, standing at a dialog's or a Settings card's text edge
/// stands its frame before that edge: its label inset, on the device pixel
/// layout places that padding on, so its content, its leading icon, else its
/// label, lands on the edge, its hover fill and focus ring spreading into the
/// padding before it (DESIGN.md, "One left edge").
pub(crate) fn ghost_pull(window: &Window) -> Pixels {
    window.pixel_snap(appearance::ui_size(LABEL_INSET))
}

/// The line height of an [`edge_button`]'s label, of its text size: looser
/// than a one-line Button's 1, so a wrapped label's lines keep their
/// ascenders and descenders apart, and tighter than the dialog's text, so
/// its lines read as one label.
const EDGE_LABEL_LINE: f32 = 1.25;

/// The space above and below an [`edge_button`]'s label at the default
/// interface size: on one line the 28 px frame [`button`] gives a Button
/// holds it, and each further line grows the frame by its line.
const EDGE_LABEL_PADDING: f32 = 6.;

/// A ghost Button with a label and no icon standing at a dialog's text edge,
/// drawn as the shared [`button`] helper draws it, whose label wraps rather
/// than run past the room its line gives it (DESIGN.md, "One left edge"):
/// it never runs past the dialog's text column. Its caller stands it in a
/// row pulled before the edge by [`ghost_pull`] ([`edge_row`]), so
/// its label starts on the edge; where its label is wider than that row,
/// it shrinks to the row, its frame ending on the column's right side, and
/// its label wraps onto more lines, each starting where the first does, at
/// word boundaries, or inside a word, such as a branch name, wider than the
/// row; its frame grows by each line. On one line it is 28 px tall at the
/// default size, with its label where `button` draws one. Its accessible
/// name is its label. The profile form's Use current repository signing
/// settings, about 389 px at 18 pt, ended in an ellipsis in a 461 px window,
/// stretched across the form, and ran 26 px past the column standing as
/// wide as its label.
pub(crate) fn edge_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    let label = label.into();
    crate::button_frame(id, &label, "", false)
        .h_auto()
        .min_h(appearance::ui_size(28.))
        .py(appearance::ui_size(EDGE_LABEL_PADDING))
        // The kit's Button keeps its width in a row; this one gives way to
        // the row's.
        .flex_shrink(1.)
        .accessibility_label(label.clone())
        .child(
            // It fills the frame, so a wrapped label's lines start at its
            // inset rather than centred in it.
            div()
                .flex_grow(1.)
                .min_w_0()
                .whitespace_normal()
                .line_height(relative(EDGE_LABEL_LINE))
                .child(label),
        )
}

/// A row of ghost Buttons at a dialog's text edge, wrapping onto lines of
/// their own where they do not fit beside each other: pulled before the
/// edge by their label inset ([`ghost_pull`]), so each line's first
/// Button stands its label, or its leading icon, on the edge and its frame
/// and hover fill in the padding before it, and as wide as the text column
/// past that, so none runs past the column ([`edge_button`]). A row holding
/// a Button with a fill or an outline at rest, as a chosen toggle, is not
/// one: such a row keeps its frames at the edge.
pub(crate) fn edge_row(window: &Window) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_2()
        .ml(-crate::ghost_pull(window))
}

/// The kit's own padding at a dialog's sides, which [`dialog_ghost_room`]
/// keeps where a ghost Button takes less.
const DIALOG_SIDE_PADDING: Pixels = px(16.);

/// The room a dialog's sides keep for such a Button: its pull and the room
/// its focus ring takes outside its frame, at least the kit's own 16 px,
/// rounded up to the device pixel. The kit pads a dialog's sides by the
/// larger of this and its 16 px (`set_dialog_side_room`), which is this
/// unchanged, so the ring stays whole inside the dialog: 16 px at 13 pt,
/// where the two take 13, and 17 px at 18 pt, where they take 14 and 3. On
/// the device pixel, layout draws the padding at exactly this width, so what
/// reads it back through the kit's `dialog_side_padding`, the GitHub panel's
/// Files fit and Keyboard Shortcuts' `help_width`, measures what is drawn;
/// rounded up, the sides keep at least that room where it falls between two
/// device pixels: 26 of them, 17.33 px, at scale 1.5 and 18 pt, where the
/// sum, 25.5, lay between the two layout can draw, and 20, 16.67 px, at 1.2
/// and 13 pt, where the kit's 16 px is 19.2. With only the pull and ring
/// rounded, the kit's 16 px, larger at 13 pt, lay between two device pixels
/// at scales 1.2, 4/3, 1.6, 5/3 and 1.8.
pub(crate) fn dialog_ghost_room(window: &Window, cx: &App) -> Pixels {
    let scale = window.scale_factor();
    let room = (ghost_pull(window) + appearance::button_ring_room(cx)).max(DIALOG_SIDE_PADDING);
    // Less a rounding error's worth, so a room already on a device pixel stays
    // on it.
    px((f32::from(room) * scale - 0.01).ceil() / scale)
}

/// A compact Button's label size at the default interface size, which
/// [`button_frame`] sets.
const LABEL_TEXT: f32 = 12.;

/// The space beside an icon-only compact Button's 16 px icon, at the default
/// interface size: its 28 px square less the icon, halved.
const ICON_INSET: f32 = 6.;

/// The compact frame [`button`] and `copy_feedback::copy_button` share,
/// without content: 28 px high, with 10 px of padding beside a label and a
/// 6 px icon/label gap, or, when `label` is empty, a 28 px square named after
/// its `symbol` until the caller names it. A control that draws its own
/// content, as Stashes' Review restore… draws its label and "Preparing…" in
/// one cell, takes the frame alone, so it matches the controls beside it.
fn button_frame(id: impl Into<ElementId>, label: &str, symbol: &str, active: bool) -> Button {
    // Transparent at rest like the kit's ghost, with the palette's hover and
    // pressed fills: the ghost's own hover darkens a hovered row. Selected, it
    // keeps the `selected` surface.
    let mut button = Button::new(id)
        .small()
        .with_variant(appearance::control_button_variant(active))
        .h(appearance::ui_size(28.))
        .min_w(appearance::ui_size(28.))
        .px(appearance::ui_size(if label.is_empty() {
            ICON_INSET
        } else {
            LABEL_INSET
        }))
        .gap(appearance::ui_size(6.))
        .rounded(appearance::ui_size(7.))
        .selected(active)
        .text_size(crate::appearance::ui_text(LABEL_TEXT));
    if active {
        // The kit omits variant hover styles for selected controls. Tint their
        // selected surface for gentle pointer feedback that leaves the focus
        // ring alone.
        button = button.hover(appearance::control_selected_hover);
    }
    if label.is_empty() {
        button = button
            .w(appearance::ui_size(28.))
            .accessibility_label(symbol_text(symbol, |text| text.name.clone()));
    }
    button
}

/// The width [`button`] gives a labelled control with an icon, and
/// `copy_feedback::copy_button` one whose label is the longer of its two
/// contents, as layout places it on this display's device pixels
/// ([`placed_button_width`]): `label` at 12 pt, laid out once per font and
/// size in `cache`, after the icon. A layout that would rather show such a
/// control as its icon than cut it compares this with the room it has.
/// Summed unrounded, its 10 px insets, 16 px icon and 6 px gap at the
/// interface size and the label in whole pixels, it missed the drawn
/// control by up to two device pixels, so at 18 pt an open control was
/// drawn labelled in a room two device pixels narrower than itself at scale
/// 1.2, and as its icon in a room that held it labelled at 1.25.
fn labelled_button_width(
    label: &str,
    cache: &'static std::thread::LocalKey<views::TextWidth>,
    window: &Window,
) -> Pixels {
    placed_button_width(label_text_width(label, cache, window), true, window)
}

/// `label` at a compact Button's 12 pt in whole pixels, laid out once per
/// font and size in `cache` ([`views::line_widths`]).
fn label_text_width(
    label: &str,
    cache: &'static std::thread::LocalKey<views::TextWidth>,
    window: &Window,
) -> Pixels {
    views::cached_text_width(cache, label, window, || {
        views::line_widths(&[(label, appearance::ui_text(LABEL_TEXT))], window)
    })
}

/// The width layout gives [`button`] reading a label `text` wide in whole
/// pixels ([`views::line_widths`]), after its icon where `icon`, on this
/// display's device pixels: its insets, the 16 px icon and its 6 px gap
/// each on a whole device pixel, halves toward zero (`Window::pixel_snap`),
/// as GPUI rounds a length before layout, and the label rounded up to a
/// device pixel as its text is measured
/// ([`views::laid_out_text_width`]), at least its 28 px square. Drawn this
/// wide, the label is never cut, and a row of such controls measured from
/// these widths is as wide as layout places it. Summed unrounded, a width
/// fell up to two device pixels short of its label at scale 1.2, so the PDF
/// page view's "Independent pages" was drawn "Independent pag…", the 3D
/// model's Reset and Edges, drawn that wide, were cut a device pixel short
/// of their labels at 13 pt and two short at 18 pt, and a row measured from
/// such sums fit a line that layout wrapped. Rounded up to a device pixel
/// after, as the find bar sized Close, Previous and Next, such a sum still
/// fell a device pixel short at scales 1.1 to 1.75.
fn placed_button_width(text: Pixels, icon: bool, window: &Window) -> Pixels {
    let snap = |length: Pixels| window.pixel_snap(length);
    let icon = if icon {
        snap(appearance::ui_size(16.)) + snap(appearance::ui_size(6.))
    } else {
        Pixels::ZERO
    };
    image_compare::on_device_pixel(
        (snap(appearance::ui_size(LABEL_INSET)) * 2.
            + icon
            + views::laid_out_text_width(text, window))
        .max(snap(appearance::ui_size(28.))),
        window,
    )
}

/// The width [`button`] gives a control that reads one of `labels` without
/// an icon, at the widest of them, as layout places it on this display's
/// device pixels ([`placed_button_width`]): the widest label at 12 pt, laid
/// out once per font and size in `cache`. A control whose label changes
/// with its state, as a toggle's or a page number's does, so takes one form
/// whatever it reads.
fn text_button_width(
    labels: &[&str],
    cache: &'static std::thread::LocalKey<views::TextWidth>,
    window: &Window,
) -> Pixels {
    placed_button_width(
        views::cached_text_width(cache, &labels.join("\n"), window, || {
            widest_label_now(labels, window)
        }),
        false,
        window,
    )
}

/// A menu's trigger reading a label `text` wide in whole pixels
/// ([`views::line_widths`]), as layout places it on this display's device
/// pixels: the label's control ([`placed_button_width`]), then the shared
/// `button` helper's 6 px gap and the kit's 0.875 rem caret, each on its
/// device pixel, halves toward zero (`Window::pixel_snap`).
fn placed_menu_width(text: Pixels, window: &Window) -> Pixels {
    image_compare::on_device_pixel(
        placed_button_width(text, false, window)
            + window.pixel_snap(appearance::ui_size(6.))
            + window.pixel_snap(window.rem_size() * 0.875),
        window,
    )
}

#[cfg(test)]
thread_local! {
    /// The labels [`widest_label_now`] last laid out, for a test to
    /// read what a width was measured from.
    static LAID_OUT_LABELS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The labels [`widest_label_now`] last laid out.
#[cfg(test)]
fn laid_out_labels() -> Vec<String> {
    LAID_OUT_LABELS.with_borrow(Clone::clone)
}

/// The widest of `labels` at a compact Button's 12 pt in whole pixels
/// ([`views::line_widths`]), laid out now, for a caller that keeps it
/// itself and places it as it draws ([`placed_button_width`]), as a side
/// header's toggle and the 3D model's menus do
/// (`system_preview::HeaderControl`), and the find bar's Close, Previous and
/// Next (`editor_find::Widths::measure`): a placed width depends on the
/// display's scale, which the font and sizes it is kept by do not.
fn widest_label_now(labels: &[&str], window: &Window) -> Pixels {
    #[cfg(test)]
    LAID_OUT_LABELS.with_borrow_mut(|laid_out| {
        *laid_out = labels.iter().map(|label| label.to_string()).collect();
    });
    labels
        .iter()
        .map(|label| views::line_widths(&[(label, appearance::ui_text(LABEL_TEXT))], window))
        .fold(px(0.), Pixels::max)
}

/// A control's 16 px icon, `icons/<symbol>.svg`, tinted with its text.
fn control_icon(symbol: &str) -> Icon {
    control_icon_at(icon_path(symbol))
}

/// [`control_icon`] for the asset at `path`.
fn control_icon_at(path: SharedString) -> Icon {
    Icon::default().path(path).size(appearance::ui_size(16.))
}
/// A pane's title and explanation, as loading and failures keep them, read
/// as one static text.
fn empty(title: &str, detail: &str) -> AnyElement {
    let read = format!("{title}. {detail}");
    let pane = div()
        .id(SharedString::from(format!("empty-state:{title}")))
        .debug_selector(|| "empty-state".into())
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p_6()
        .gap_2()
        .child(
            div()
                .text_size(crate::appearance::ui_text(14.))
                .child(title.to_owned()),
        )
        .child(
            div()
                .max_w(px(600.))
                .text_center()
                .text_size(crate::appearance::ui_text(12.))
                .opacity(0.75)
                .child(detail.to_owned()),
        );
    native_accessibility::static_text(pane, read).into_any_element()
}

fn checkerboard(colors: appearance::Palette) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let tile = 12.;
            let rows = (f32::from(bounds.size.height) / tile).ceil() as i32;
            let cols = (f32::from(bounds.size.width) / tile).ceil() as i32;
            for row in 0..rows {
                for col in 0..cols {
                    let color = if (row + col) % 2 == 0 {
                        colors.canvas
                    } else {
                        colors.subtle
                    };
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + point(px(col as f32 * tile), px(row as f32 * tile)),
                            size(px(tile), px(tile)),
                        ),
                        rgb(color),
                    ));
                }
            }
        },
    )
    .size_full()
    .into_any_element()
}
fn short_oid(oid: &str) -> String {
    oid.chars().take(7).collect()
}
fn short_date(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%b %d").to_string())
        .unwrap_or_default()
}
fn full_date(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%b %-d, %Y at %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}
fn language_for(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "json" | "jsonc" => "json",
        "css" => "css",
        "md" | "markdown" | "mdx" => "markdown",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "py" | "pyw" => "python",
        "go" => "go",
        "sh" | "bash" | "zsh" => "bash",
        _ => "text",
    }
}
fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}
/// The status bar's count of a commit's or comparison's changed files.
fn changed_files(count: usize) -> String {
    counted(count, "changed file", "changed files")
}
/// The " · "-separated phrases of `text`, each but the last ending in its
/// separator, for a line laid out as one wrapping item per phrase: it breaks
/// between two phrases, and inside one only where that phrase alone is wider
/// than the line, so no word of a phrase is left alone on a line.
fn phrase_runs(text: &str) -> Vec<SharedString> {
    let mut parts: Vec<&str> = text.split(" · ").collect();
    let last = parts.pop().unwrap_or_default().to_owned();
    parts
        .into_iter()
        .map(|phrase| format!("{phrase} · ").into())
        .chain([last.into()])
        .collect()
}
/// `count` with the noun that agrees with it: `one` for exactly one, `many`
/// otherwise, as in "1 match", "0 matches" and "2 matches". Every count shown
/// or announced with its noun is built here, so none reads "1 shortcuts" or
/// "1 matches", as Keyboard Shortcuts and Quick Open once did; a noun phrase
/// can carry its verb ("path differs", "paths differ").
fn counted<T>(count: T, one: &str, many: &str) -> String
where
    T: std::fmt::Display + PartialEq + From<u8>,
{
    format!("{count} {}", if count == T::from(1) { one } else { many })
}
fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else {
        "DejaVu Sans Mono"
    }
}
fn primary_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

fn main() {
    // Exactly `--gitturtle-pdf-helper <version> <parent pid>` runs the
    // confined PDF helper and never returns; anything else starts the app.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    pdf_helper::run_if_requested();
    if let Some(code) = gitturtle_core::run_askpass_if_requested() {
        std::process::exit(code);
    }
    if build_info::handle_cli() {
        return;
    }
    #[cfg(target_os = "linux")]
    if gpui_kit::guess_compositor() == "Headless" {
        eprintln!(
            "GitTurtle needs a Wayland or X11 desktop session. Launch it from your desktop's application menu or a terminal inside that session. No display was selected: WAYLAND_DISPLAY or DISPLAY must identify the session, and ZED_HEADLESS must be unset."
        );
        std::process::exit(1);
    }
    let preferences = Preferences::load_at_launch();
    let system_code_font = preferences.settings.system_code_font;
    let activity = activity::State::load();
    let recovery_drafts = recovery_drafts::State::load();
    let tab_session = repository_tabs::Session::load();
    let initial = std::env::args_os().nth(1).map(PathBuf::from).or_else(|| {
        preferences
            .settings
            .reopen_last
            .then(|| {
                tab_session
                    .tabs
                    .get(tab_session.active)
                    .map(|tab| tab.path.path())
                    .or_else(|| preferences.last_repository())
            })
            .flatten()
    });
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        desktop_text::register_bundled_fonts(cx);
        // The person's Reduce motion and Increase contrast, before the first
        // resolution, so the first frame already has them.
        native_accessibility::set_display_choices(&preferences.settings, cx);
        native_accessibility::sync_preferences(cx);
        let desktop_text = desktop_text::start(system_code_font, cx);
        #[cfg(target_os = "linux")]
        let omarchy = appearance::omarchy::start(
            preferences.settings.theme == appearance::custom::ThemeSelection::Omarchy,
            cx,
        );
        native_accessibility::bind_keys(cx);
        native_accessibility::set_overlay_room(cx);
        image_lifetime::init(cx);
        interactive_rebase::init(cx);
        theme_editor::init(cx);
        system_preview::init(cx);
        preferences
            .settings
            .effective_theme(cx, &preferences.custom_themes)
            .apply(None, cx);
        let applied_contrast = appearance::more_contrast(cx);
        shortcuts::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &HideApplication, cx| cx.hide());
        cx.on_action(|_: &HideOtherApplications, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAllApplications, cx| cx.unhide_other_apps());
        platform_polish::menus(MenuState::default(), StashesItem::Refused, cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1480.), px(980.)), cx);
        cx.activate(true);
        cx.spawn(async move |cx| {
            desktop_text::ready(desktop_text, cx).await;
            // The window's first frame shows a selected Omarchy theme when its
            // read finishes in time (`GitTurtle::new` applies it).
            #[cfg(target_os = "linux")]
            appearance::omarchy::ready(omarchy, cx).await;
            // The desktop's first answer, when it arrived during that wait and
            // changed More contrast, before any window could apply it: the
            // theme applies again, so the first frame has it.
            cx.update(|cx| {
                if appearance::more_contrast(cx) != applied_contrast {
                    preferences
                        .settings
                        .effective_theme(cx, &preferences.custom_themes)
                        .apply(None, cx);
                }
            });
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("GitTurtle".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(1000.), px(680.))),
                    app_id: Some("com.gitturtle.GitTurtle".into()),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| {
                        GitTurtle::new(
                            initial,
                            preferences,
                            tab_session,
                            activity,
                            recovery_drafts,
                            window,
                            cx,
                        )
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            #[cfg(target_os = "linux")]
            if let Err(error) = opened {
                eprintln!(
                    "Could not open the GitTurtle window: {error:#}\nCheck that this process can access your Wayland or X11 session and that a working Vulkan driver is installed. See docs/linux.md for startup troubleshooting."
                );
                std::process::exit(1);
            }
            #[cfg(not(target_os = "linux"))]
            opened.expect("open GitTurtle window");
        })
        .detach();
    });
}

/// Opt-in `gitturtle.*_frame_ms` interaction traces; see
/// `docs/benchmarks/metrics.md` for each metric's boundary.
fn trace_enabled() -> bool {
    std::env::var_os("GITTURTLE_TRACE").is_some()
}

/// The font that text beside `probe` in `container` is shaped with
/// (`window.text_style().font()`, as the kit's text elements read it) while
/// the code family is "Desktop Mono".
#[cfg(test)]
fn shaped_code_font(
    cx: &mut TestAppContext,
    container: impl Fn(AnyElement, &App) -> AnyElement + 'static,
) -> Font {
    use std::{cell::RefCell, rc::Rc};

    struct Probe<F>(F, Rc<RefCell<Option<Font>>>);
    impl<F: Fn(AnyElement, &App) -> AnyElement + 'static> Render for Probe<F> {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let seen = self.1.clone();
            let probe = canvas(
                move |_, window, _| *seen.borrow_mut() = Some(window.text_style().font()),
                |_, _, _, _| {},
            )
            .w(px(1.))
            .h(px(1.));
            (self.0)(probe.into_any_element(), cx)
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::global_mut(cx).mono_font_family = "Desktop Mono".into();
    });
    let seen = Rc::new(RefCell::new(None));
    let probe = Probe(container, seen.clone());
    let (_, cx) = cx.add_window_view(move |_, _| probe);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    seen.borrow().clone().expect("the probe painted")
}

#[cfg(test)]
mod late_preview_tests {
    use super::*;
    use core::prelude::v1::test;

    /// The bookmark a session file keeps for `file` of `commit` shown in
    /// Compare, as that file holds it.
    fn compared(commit: &Commit, file: &FileChange) -> repository_tabs::Bookmark {
        let text = |path: &Option<PathBuf>| {
            path.as_deref()
                .and_then(std::path::Path::to_str)
                .map(str::to_owned)
        };
        serde_json::from_value(serde_json::json!({
            "selected_oid": commit.oid,
            "comparison": {
                "old_path": text(&file.old_path),
                "new_path": text(&file.new_path),
                "old_oid": file.old_oid,
                "new_oid": file.new_oid,
                "old_mode": file.old_mode,
                "new_mode": file.new_mode,
                "status": file.status.letter(),
            },
            "origins": [commit.parents.first(), commit.oid],
            "selected_file": text(&file.new_path),
            "compare": true,
        }))
        .expect("a saved comparison")
    }

    /// Back cancels the preview read of a comparison restored with its
    /// tab, as it does the comparison's own: the read leaves the reader's
    /// queue unrun, nothing is left loading, History keeps the status line
    /// Back left, no comparison is kept and the document bookmark that
    /// waited for the reply goes with it. The read once ran on after Back,
    /// a PDF side's helper to its 10 s deadline, and its reply, dropped
    /// since, once wrote "1 changed file · content read N ms" over
    /// History's status line and kept the comparison nothing showed. Not
    /// left, the same restore shows its comparison.
    #[gpui::test]
    async fn back_cancels_a_restored_comparisons_read(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let (bookmark, status) = app.read_with(cx, |app, _| {
            let commit = &app.commits[app.selected_commit.expect("a selected commit")];
            (compared(commit, &app.files[0]), app.status.clone())
        });
        // The reader takes nothing until released, so the restored read is
        // still queued when Back comes. Quiet reads are held for the whole
        // test: Back, which leaves nothing loading, rightly takes up a change
        // the fixture's watcher left pending, and a quiet read's submission
        // replaces whatever waits in the reader's queue and cancels what
        // runs, so it would drop the restored read whether Back cancelled it
        // or not, and land on History's status line. The reader is idle
        // first: it marks its last read done only after sending the reply
        // the fixture's window awaited, so held at once it could still count
        // that read as running.
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(true);
                app.repository_tabs.restoring = Some(bookmark.clone());
                app.restore_tab_selection(window, cx);
                assert!(app.mode == WorkspaceMode::Compare);
                assert_eq!(app.loading, Some(RESTORED_PREVIEW_LABEL));
                let submitted = app.worker.submissions();
                app.back_to_history(window, cx);
                assert_eq!(app.loading, None, "Back cancels the restored read");
                assert!(app.task.is_none(), "nothing waits for its reply");
                assert_eq!(
                    app.worker.submissions(),
                    submitted,
                    "Back submits no preview read, the restored one or another"
                );
            })
        });
        // No preview read is queued or running: the restored read was
        // dropped unrun. Left queued, it fails this wait at once.
        app.read_with(cx, |app, _| {
            app.worker.wait_idle_within(std::time::Duration::ZERO);
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History);
            assert_eq!(app.loading, None);
            assert_eq!(app.status, status, "History's status line is Back's");
            assert!(app.content.is_none(), "no comparison is kept");
            assert!(app.images.iter().all(Option::is_none));
            assert!(app.repository_tabs.document_restore.is_none());
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.repository_tabs.restoring = Some(bookmark);
                app.restore_tab_selection(window, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Compare);
            assert!(app.content.is_some(), "the restored comparison shows");
            assert!(app.status.contains("content read"), "{}", app.status);
            assert!(app.repository_tabs.document_restore.is_none());
        });
    }

    /// Back abandons a preview read whatever label it shows: a text
    /// review's preparation of the comparison shown, and Quick Open's read
    /// of a tracked file. Each read leaves the reader's queue unrun as Back
    /// returns, nothing is left loading, and once the reader is released no
    /// late reply reopens Compare or keeps a comparison. Back once cancelled
    /// only the comparison's own labels, so a review's preparation ran on
    /// to its end after Back, and so would a decode started under any other.
    /// The text review's label tests that fix. The tracked file's passed
    /// without it, since Back closes the inspection Quick Open opened, whose
    /// close already cancels its read (`close_revision_inspection`): it
    /// guards that label against a regression. A read the reader has
    /// already taken stops as the worker's contract has it
    /// (`worker::tests::explicit_cancel_invalidates_active_and_pending_without_replacement`,
    /// `worker::tests::replacement_and_explicit_cancel_signal_an_active_core_history_token`).
    #[gpui::test]
    async fn back_cancels_a_preview_read_under_any_label(cx: &mut TestAppContext) {
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        // The fixture's history is imported: its file is checked out for a
        // tracked source of its own.
        let path = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::tags::tests::git(&path, &["reset", "--quiet", "--hard"]);
        for label in ["Preparing text review…", "Reading tracked file…"] {
            if label == "Preparing text review…" {
                cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
                crate::views::tests::settle(&app, cx).await;
                app.read_with(cx, |app, _| {
                    assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
                });
            }
            // Quiet reads are held: one submitted replaces what waits in
            // the reader's queue, whether Back cancelled it or not.
            let held = app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.automatic.hold_quiet_reads(true);
                    if label == "Preparing text review…" {
                        let options = text_review::Options {
                            hide_whitespace: true,
                            ..Default::default()
                        };
                        app.set_text_review(options, window, cx);
                    } else {
                        let repo = app.repository.clone().expect("a repository");
                        let paths = repo
                            .search_tracked_paths(
                                &PathScope::Worktree,
                                "notes",
                                &HistoryCancellation::default(),
                            )
                            .expect("tracked paths");
                        let entry = paths.entries.first().expect("the file").clone();
                        app.show_tracked_file(paths.scope.clone(), entry, window, cx);
                    }
                    assert!(app.mode == WorkspaceMode::Compare, "{label}");
                    assert_eq!(app.loading, Some(label));
                    app.back_to_history(window, cx);
                    assert_eq!(app.loading, None, "{label}: Back cancels the read");
                    assert!(app.task.is_none(), "{label}: nothing waits for its reply");
                })
            });
            // Left queued, the read fails this wait at once.
            app.read_with(cx, |app, _| {
                app.worker.wait_idle_within(std::time::Duration::ZERO);
            });
            drop(held);
            crate::views::tests::settle(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert!(
                    app.mode == WorkspaceMode::History,
                    "{label}: Compare stays closed"
                );
                assert_eq!(app.loading, None, "{label}");
                assert!(app.content.is_none(), "{label}: no comparison is kept");
            });
            cx.update(|_, cx| app.update(cx, |app, _| app.automatic.hold_quiet_reads(false)));
        }
    }

    /// Choosing the file whose preview is still being read keeps that read:
    /// the same file of the same revision pair reads nothing new, so a
    /// second click on its row, or Enter on it, adds no preview read and the
    /// read under way lands. A changed target is read in its place: another
    /// file chosen while one is read, and the first chosen back while the
    /// other is read, are two reads, and so is the same file chosen against
    /// another revision pair while it is read: the first read is cancelled
    /// and one against that pair starts. A choice made again once restarted
    /// the read, a slow image's decode from its start.
    #[gpui::test]
    async fn choosing_a_file_still_read_keeps_its_read(cx: &mut TestAppContext) {
        // The newest commit adds notes.txt again as the oldest added it, the
        // same blob: the same changed file in two revision pairs.
        let (_fixture, app, cx) = crate::views::tests::history_window_from(
            cx,
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline notes.txt\ndata 6\nnotes\nM 100644 inline other.txt\ndata 6\nother\n\ncommit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000100 +0000\ndata 12\nremove notes\nD notes.txt\n\ncommit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000200 +0000\ndata 13\nrestore notes\nM 100644 inline notes.txt\ndata 6\nnotes\nM 100644 inline other.txt\ndata 8\nother 2\n\ndone\n",
        )
        .await;
        let index = |app: &GitTurtle, name: &str| {
            app.files
                .iter()
                .position(|file| file.path() == std::path::Path::new(name))
                .expect("the commit changes it")
        };
        // The oldest commit's changed files, as choosing it reads them, and
        // the newest selected again.
        let oldest = app.read_with(cx, |app, _| {
            app.commits
                .iter()
                .position(|commit| commit.subject == "fixture notes")
                .expect("the oldest commit")
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(oldest, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        let oldest_files = app.read_with(cx, |app, _| app.files.clone());
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.commits[0].subject, "restore notes", "the newest commit");
            assert_eq!(app.selected_commit, Some(0));
        });
        let shown = |app: &GitTurtle| match app.content.as_deref() {
            Some(Content::Text { new, .. }) => Some(new.clone()),
            _ => None,
        };
        // Held, the reader leaves each read queued. Quiet reads are held
        // too: only the reads the choices ask for are counted.
        let hold = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            })
        };
        let held = hold(cx);
        let reads = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.automatic.hold_quiet_reads(true);
                let notes = index(app, "notes.txt");
                let reads = app.worker.previews();
                app.select_file(notes, window, cx);
                assert!(app.mode == WorkspaceMode::Compare);
                assert_eq!(app.loading, Some(PREVIEW_LABEL));
                let generation = app.generation;
                app.select_file(notes, window, cx);
                assert_eq!(app.generation, generation, "the read under way goes on");
                assert_eq!(app.loading, Some(PREVIEW_LABEL));
                assert_eq!(
                    app.worker.previews(),
                    reads + 1,
                    "chosen again while it is read, it reads nothing new"
                );
                reads
            })
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(
                shown(app).as_deref(),
                Some("notes\n"),
                "the kept read lands"
            );
            assert_eq!(app.worker.previews(), reads + 1, "one read");
        });
        let held = hold(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let (notes, other) = (index(app, "notes.txt"), index(app, "other.txt"));
                app.select_file(other, window, cx);
                assert_eq!(app.loading, Some(PREVIEW_LABEL));
                app.select_file(notes, window, cx);
                assert_eq!(
                    app.worker.previews(),
                    reads + 3,
                    "each changed target is read: two reads"
                );
            })
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(
                shown(app).as_deref(),
                Some("notes\n"),
                "the file chosen last shows"
            );
            assert_eq!(app.selected_file, Some(index(app, "notes.txt")));
        });
        // While the newest commit's notes.txt is read, the window comes to
        // compare the oldest commit, whose changed files list the same
        // notes.txt, and that file is chosen: the read under way is for the
        // newest commit's pair, so it is cancelled and the oldest's starts.
        let held = hold(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let notes = index(app, "notes.txt");
                let file = app.files[notes].clone();
                app.clear_preview();
                app.select_file(notes, window, cx);
                assert_eq!(app.worker.previews(), reads + 4, "the newest's read");
                let (generation, newest) = (app.generation, app.preview_origins());
                app.selected_commit = Some(oldest);
                app.files = oldest_files.clone();
                app.refresh_file_filter(window, cx);
                let notes = index(app, "notes.txt");
                assert!(app.files[notes] == file, "the same changed file");
                assert!(app.preview_origins() != newest, "another revision pair");
                app.select_file(notes, window, cx);
                assert_ne!(app.generation, generation, "the first read is cancelled");
                assert_eq!(app.loading, Some(PREVIEW_LABEL));
                assert_eq!(app.worker.previews(), reads + 5, "and another starts");
                assert!(
                    app.preview_in_flight()
                        .and_then(|read| read.target.as_ref())
                        .is_some_and(|target| target.origins == app.preview_origins()),
                    "against the oldest commit's pair"
                );
            })
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.loading, None);
            assert_eq!(shown(app).as_deref(), Some("notes\n"), "its read lands");
            assert_eq!(app.worker.previews(), reads + 5);
        });
    }

    /// A comparison restored with its tab whose reply lands while Settings
    /// or Projects is shown restores its documents, its zoom among them, as
    /// the window returns to it and builds its editor, as one that lands on
    /// the repository page does then. One still being read as the window
    /// returns keeps its bookmark for its reply, and so does one whose read
    /// ended without a reply, which shows nothing to restore. The reply kept its
    /// comparison but never its documents' places: the bookmark stayed
    /// unused until some later preview took it.
    #[gpui::test]
    async fn a_restored_comparison_landing_under_another_page_restores_on_return(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let mut bookmark = app.read_with(cx, |app, _| {
            let commit = &app.commits[app.selected_commit.expect("a selected commit")];
            compared(commit, &app.files[0])
        });
        bookmark.zoom = 2.5;
        for page in [AppPage::Settings, AppPage::Projects] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.clear_preview();
                    app.zoom = 1.;
                    app.repository_tabs.restoring = Some(bookmark.clone());
                    app.restore_tab_selection(window, cx);
                    match page {
                        AppPage::Settings => app.show_settings(window, cx),
                        _ => app.show_projects(window, cx),
                    }
                    assert!(app.page == page);
                })
            });
            crate::views::tests::settle(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert!(app.content.is_some(), "the reply landed under {page:?}");
            });
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            crate::views::tests::settle(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert!(app.page == AppPage::Repository && app.mode == WorkspaceMode::Compare);
                assert_eq!(
                    app.zoom, 2.5,
                    "the documents are restored on the return from {page:?}"
                );
                assert!(app.repository_tabs.document_restore.is_none());
            });
        }
        // Returned to before the reply, the bookmark waits for it.
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.clear_preview();
                app.zoom = 1.;
                app.repository_tabs.restoring = Some(bookmark.clone());
                app.restore_tab_selection(window, cx);
                app.show_settings(window, cx);
                app.return_from_page(window, cx);
                assert_eq!(app.loading, Some(RESTORED_PREVIEW_LABEL));
                assert!(
                    app.repository_tabs.document_restore.is_some(),
                    "the bookmark waits for its reply"
                );
            })
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.content.is_some());
            assert_eq!(app.zoom, 2.5, "the reply restores the documents");
            assert!(app.repository_tabs.document_restore.is_none());
        });
        // A restored read that ended without a reply, dropped as a newer
        // submission drops it, shows nothing to restore the places onto:
        // a return from Settings neither applies them to nothing nor drops
        // them.
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.clear_preview();
                app.zoom = 1.;
                app.repository_tabs.restoring = Some(bookmark.clone());
                app.restore_tab_selection(window, cx);
                assert_eq!(app.loading, Some(RESTORED_PREVIEW_LABEL));
                app.worker.cancel();
            })
        });
        drop(held);
        cx.executor().run_until_parked();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.loading, None, "the read ended");
                assert!(app.content.is_none(), "with no reply");
                app.show_settings(window, cx);
                app.return_from_page(window, cx);
                assert!(app.mode == WorkspaceMode::Compare);
                assert_eq!(app.zoom, 1., "nothing takes the places");
                assert!(
                    app.repository_tabs
                        .document_restore
                        .as_ref()
                        .is_some_and(|saved| saved.zoom == 2.5),
                    "the places are kept"
                );
            })
        });
    }

    /// Saved places restore only the comparison they were saved with:
    /// places waiting in the window while another comparison shows are
    /// dropped as the window returns to it from Settings or Projects, and
    /// its zoom stays as it was. Restored on any return with nothing
    /// loading, another tab's places reached the comparison shown.
    #[gpui::test]
    async fn saved_places_restore_only_their_own_comparison(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        let mut other = app.read_with(cx, |app, _| {
            let commit = &app.commits[app.selected_commit.expect("a selected commit")];
            let mut file = app.files[0].clone();
            file.new_path = Some(PathBuf::from("elsewhere.txt"));
            compared(commit, &file)
        });
        other.zoom = 2.5;
        for page in [AppPage::Settings, AppPage::Projects] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
                    app.zoom = 1.5;
                    app.repository_tabs.document_restore = Some(other.clone());
                    match page {
                        AppPage::Settings => app.show_settings(window, cx),
                        _ => app.show_projects(window, cx),
                    }
                    app.return_from_page(window, cx);
                    assert!(app.page == AppPage::Repository);
                    assert_eq!(
                        app.zoom, 1.5,
                        "another comparison's places stay unused on the return from {page:?}"
                    );
                    assert!(
                        app.repository_tabs.document_restore.is_none(),
                        "they belong to nothing shown"
                    );
                })
            });
        }
    }
}

#[cfg(test)]
mod repository_branch_input_tests {
    use super::{BranchInputScope, GitRepository};

    #[test]
    fn branch_inputs_reset_on_canonical_switch_but_survive_aliases_and_failed_opens() {
        let fixture = tempfile::tempdir().unwrap();
        let first = GitRepository::init(fixture.path().join("first"), "main").unwrap();
        let second = GitRepository::init(fixture.path().join("second"), "main").unwrap();
        let nested = first.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let mut scope = BranchInputScope::default();

        assert!(!scope.opened(first.path()));
        // Refresh and a nested/aliased request resolve to the existing worktree.
        assert!(!scope.opened(GitRepository::open(first.path()).unwrap().path()));
        assert!(!scope.opened(GitRepository::open(&nested).unwrap().path()));
        #[cfg(unix)]
        {
            let alias = fixture.path().join("alias");
            std::os::unix::fs::symlink(first.path(), &alias).unwrap();
            assert!(!scope.opened(GitRepository::open(alias).unwrap().path()));
        }

        // Failed discovery has no accepted snapshot and must not change scope.
        let failed = GitRepository::open(fixture.path().join("missing"));
        assert!(
            failed
                .map(|repository| scope.opened(repository.path()))
                .is_err()
        );
        assert!(!scope.opened(first.path()));

        assert!(scope.opened(second.path()));
        assert!(!scope.opened(second.path()));
        assert!(scope.opened(first.path()));
    }
}
#[cfg(test)]
mod shared_button_tests {
    use super::*;
    use core::prelude::v1::test;

    struct Probe {
        icon: FocusHandle,
        label: FocusHandle,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .p_4()
                .flex()
                .gap_2()
                .child(
                    button("probe-icon", "", "copy", false)
                        .track_focus(&self.icon)
                        .debug_selector(|| "probe-icon".into()),
                )
                .child(
                    button("probe-label", "Fetch", "", true)
                        .track_focus(&self.label)
                        .debug_selector(|| "probe-label".into()),
                )
        }
    }

    /// The bordered quads the last frame painted wholly outside `element`, the
    /// ring a focused Button draws, as outer and inner bounds in logical pixels
    /// and border color. A border-only quad is painted once per side.
    fn rings_around(
        cx: &mut VisualTestContext,
        element: Bounds<Pixels>,
    ) -> Vec<(Bounds<Pixels>, Bounds<Pixels>, Hsla)> {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            let mut rings = Vec::new();
            for quad in window.painted_quads() {
                let (bounds, widths) = (quad.bounds, quad.border_widths);
                let outer = Bounds::from_corners(
                    point(logical(bounds.left()), logical(bounds.top())),
                    point(logical(bounds.right()), logical(bounds.bottom())),
                );
                let inner = Bounds::from_corners(
                    point(
                        outer.left() + logical(widths.left),
                        outer.top() + logical(widths.top),
                    ),
                    point(
                        outer.right() - logical(widths.right),
                        outer.bottom() - logical(widths.bottom),
                    ),
                );
                let ring = (outer, inner, quad.border_color);
                if inner != outer
                    && outer.left() < element.left()
                    && outer.top() < element.top()
                    && outer.right() > element.right()
                    && outer.bottom() > element.bottom()
                    && !rings.contains(&ring)
                {
                    rings.push(ring);
                }
            }
            rings
        })
    }

    /// The shared helpers, each once: an icon-only Button named by its
    /// symbol's placeholder, `symbol`, a labelled Button with an icon, a
    /// control's bare icon, an inline icon and the app icon.
    struct Helpers {
        symbol: &'static str,
        /// How many times it was rendered.
        renders: usize,
    }

    impl Render for Helpers {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            div()
                .flex()
                .gap_2()
                .child(button("helpers-icon", "", self.symbol, false))
                .child(button("helpers-labelled", "Refresh", "refresh", false))
                .child(control_icon("copy"))
                .child(icon("copy", 16., 0))
                .child(app_icon(28.))
        }
    }

    /// The shared helpers format a symbol's icon path and placeholder name
    /// once, the first time the symbol is drawn ([`symbol_text`]), and the
    /// app icon's source allocates nothing ([`app_icon_source`]): frames at
    /// rest format none of them, though each takes every helper's text from
    /// its symbol's and draws the app icon from its static source, allocating
    /// nothing to make it, another symbol formats its own text once,
    /// and a symbol drawn before finds its text kept. The icon drawn is the
    /// symbol's own asset. Every frame formatted each Button's icon path and
    /// an icon-only Button's name, and parsed the app icon's path as a URL
    /// and copied it.
    #[gpui::test]
    fn the_shared_helpers_format_a_symbol_s_text_once(cx: &mut TestAppContext) {
        // Empty, as a GPUI test's seeds share the thread and its cache.
        forget_symbol_text();
        let first = SYMBOL_TEXT_FORMATTED.get();
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_, _| Helpers {
            symbol: "copy",
            renders: 0,
        });
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        };
        let counts = || (SYMBOL_TEXT_FORMATTED.get(), SYMBOL_TEXT_LOOKUPS.get());
        let drawn_icon = || {
            views::CONTROL_ICONS.with(|icons| {
                icons
                    .borrow()
                    .get("helpers-icon")
                    .map(|path| path.to_string())
            })
        };
        let at_rest = |at: &str, cx: &mut VisualTestContext| {
            let (formatted, lookups) = counts();
            let renders = view.read_with(cx, |view, _| view.renders);
            for _ in 0..3 {
                APP_ICON_DRAWN.set(None);
                view.update(cx, |_, cx| cx.notify());
                draw(cx);
                // Drawn from its embedded asset, by the static path, which
                // made nothing.
                let drawn = APP_ICON_DRAWN.take();
                assert!(
                    matches!(
                        &drawn,
                        Some((ImageSource::Resource(gpui::Resource::Embedded(path)), 0))
                            if &**path == APP_ICON_PATH
                    ),
                    "{at}: the app icon is drawn from its embedded asset, allocating nothing"
                );
            }
            let (formatted_after, lookups_after) = counts();
            let renders = view.read_with(cx, |view, _| view.renders) - renders;
            assert!(renders >= 3, "{at}: each frame renders the helpers");
            // A render asks for each helper's text once: the icon-only
            // Button's path and name, the labelled Button's path, the bare
            // control icon's and the inline icon's.
            assert_eq!(
                lookups_after - lookups,
                renders * 5,
                "{at}: each render at rest draws every helper's text from its symbol's"
            );
            assert_eq!(
                formatted_after - formatted,
                0,
                "{at}: frames at rest format no symbol's text"
            );
        };
        draw(cx);
        assert_eq!(
            counts().0 - first,
            2,
            "the first frames format `copy`'s and `refresh`'s text once each"
        );
        assert_eq!(drawn_icon().as_deref(), Some("icons/copy.svg"));
        at_rest("copy", cx);

        let (formatted, _) = counts();
        view.update(cx, |view, cx| {
            view.symbol = "folder";
            cx.notify()
        });
        draw(cx);
        assert_eq!(
            counts().0 - formatted,
            1,
            "another symbol formats its text once"
        );
        assert_eq!(drawn_icon().as_deref(), Some("icons/folder.svg"));
        at_rest("folder", cx);

        let (formatted, _) = counts();
        view.update(cx, |view, cx| {
            view.symbol = "copy";
            cx.notify()
        });
        draw(cx);
        assert_eq!(
            counts().0,
            formatted,
            "a symbol drawn before finds its text kept"
        );
        assert_eq!(drawn_icon().as_deref(), Some("icons/copy.svg"));
        assert_eq!(
            symbol_text("folder", |text| (
                text.path.to_string(),
                text.name.to_string()
            )),
            ("icons/folder.svg".to_owned(), "folder file path".to_owned()),
            "a symbol's kept path and placeholder name"
        );

        let before = crate::test_support::allocations();
        let source = app_icon_source();
        assert_eq!(
            crate::test_support::allocations() - before,
            0,
            "the app icon's source allocates nothing"
        );
        assert!(
            matches!(
                &source,
                ImageSource::Resource(gpui::Resource::Embedded(path)) if &**path == APP_ICON_PATH
            ),
            "the app icon is the embedded asset"
        );
    }

    /// A focused helper, at rest or hovered and selected or not, keeps the box
    /// it had unfocused and draws the palette's ring, 2 px of full accent 1 px
    /// outside its edge: hovering a selected helper does not dim the ring.
    #[gpui::test]
    fn focusing_the_shared_button_keeps_its_box(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            appearance::ThemeChoice::Porcelain.apply(None, cx);
        });
        let (probe, cx) = cx.add_window_view(|_, cx| Probe {
            icon: cx.focus_handle(),
            label: cx.focus_handle(),
        });
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        };
        let ring = cx.read(|cx| gpui_kit::component::Theme::global(cx).colors.ring);
        let device = cx.update(|window, _| px(1. / window.scale_factor()));
        let near = |drawn: Bounds<Pixels>, expected: Bounds<Pixels>| {
            [
                (drawn.left(), expected.left()),
                (drawn.top(), expected.top()),
                (drawn.right(), expected.right()),
                (drawn.bottom(), expected.bottom()),
            ]
            .into_iter()
            .all(|(edge, expected)| (edge - expected).abs() <= device)
        };
        draw(cx);
        for selector in ["probe-icon", "probe-label"] {
            let resting = cx.debug_bounds(selector).expect("rendered helper");
            assert!(rings_around(cx, resting).is_empty(), "{selector} unfocused");
            let handle = cx.read(|cx| {
                let probe = probe.read(cx);
                if selector == "probe-icon" {
                    probe.icon.clone()
                } else {
                    probe.label.clone()
                }
            });
            cx.update(|window, cx| window.focus(&handle, cx));
            draw(cx);
            assert!(
                cx.update(|window, _| handle.is_focused(window)),
                "{selector}"
            );
            for state in ["focused", "focused and hovered"] {
                if state == "focused and hovered" {
                    cx.simulate_mouse_move(resting.center(), None, Modifiers::default());
                    draw(cx);
                }
                assert_eq!(
                    cx.debug_bounds(selector),
                    Some(resting),
                    "{selector} {state}"
                );
                let rings = rings_around(cx, resting);
                let [(outer, inner, color)] = rings.as_slice() else {
                    panic!("{selector} {state} draws one ring, drew {rings:?}");
                };
                assert!(
                    near(*outer, resting.dilate(px(3.))) && near(*inner, resting.dilate(px(1.))),
                    "{selector} {state}: ring from {inner:?} to {outer:?} around {resting:?}"
                );
                assert_eq!(*color, ring, "{selector} {state}");
            }
            cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
            draw(cx);
        }
    }
}

/// Every icon a source names literally must resolve through `Assets`: a name
/// neither the embedded set nor the kit bundle carries draws an empty slot.
#[cfg(test)]
mod icon_symbol_tests {
    use super::Assets;
    use gpui_kit::AssetSource;
    use std::path::{Path, PathBuf};

    #[derive(Debug, PartialEq)]
    enum Token {
        Ident(String),
        Str(String),
        Punct(char),
    }

    /// Rust tokens enough to find calls: identifiers, string literal contents
    /// and punctuation, with comments, char literals and lifetimes skipped.
    fn tokens(source: &str) -> Vec<Token> {
        let chars: Vec<char> = source.chars().collect();
        let mut tokens = Vec::new();
        let mut i = 0;
        let quoted = |chars: &[char], mut i: usize| -> (String, usize) {
            let mut text = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if let Some(&c) = chars.get(i) {
                    text.push(c);
                }
                i += 1;
            }
            (text, i + 1)
        };
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            if c == '/' && next == Some('/') {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            } else if c == '/' && next == Some('*') {
                let mut depth = 0;
                while i < chars.len() {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            } else if c == '"' {
                let (text, end) = quoted(&chars, i + 1);
                tokens.push(Token::Str(text));
                i = end;
            } else if c == '\'' {
                // A char literal ('x', '\n', '\''); otherwise a lifetime.
                if next == Some('\\') {
                    i += 3;
                    while i < chars.len() && chars[i] != '\'' {
                        i += 1;
                    }
                    i += 1;
                } else if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else {
                    i += 1;
                }
            } else if c.is_alphabetic() || c == '_' {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                let raw =
                    matches!(ident.as_str(), "r" | "br") && matches!(chars.get(i), Some('"' | '#'));
                if raw {
                    let hashes = chars[i..].iter().take_while(|&&c| c == '#').count();
                    let open = i + hashes;
                    if chars.get(open) == Some(&'"') {
                        let close: String = std::iter::once('"')
                            .chain("#".repeat(hashes).chars())
                            .collect();
                        let rest: String = chars[open + 1..].iter().collect();
                        let length = rest.find(&close).unwrap_or(rest.len());
                        tokens.push(Token::Str(rest[..length].to_owned()));
                        i = open + 1 + rest[..length].chars().count() + close.chars().count();
                        continue;
                    }
                }
                tokens.push(Token::Ident(ident));
            } else if !c.is_whitespace() {
                tokens.push(Token::Punct(c));
                i += 1;
            } else {
                i += 1;
            }
        }
        tokens
    }

    /// The literal icon names in `source`: the symbol argument of each
    /// `button(id, label, symbol, active)` call, the argument of each
    /// `control_icon(symbol)` call (as `copy_feedback::copy_button` draws its
    /// copy and check icons), the icon of each side header control, the
    /// fifth argument of `HeaderControl::new` and `HeaderControl::toggle`
    /// (`system_preview::HeaderControl`), and each `icons/<name>.svg` path
    /// literal. Method calls (`.button(`) and definitions are not calls of
    /// the helper; an empty symbol draws no icon.
    fn literal_icon_symbols(source: &str) -> Vec<String> {
        let tokens = tokens(source);
        let mut symbols = Vec::new();
        for (index, token) in tokens.iter().enumerate() {
            if let Token::Str(text) = token
                && let Some(name) = text
                    .strip_prefix("icons/")
                    .and_then(|path| path.strip_suffix(".svg"))
                && !name.contains('{')
            {
                symbols.push(name.to_owned());
            }
            // The symbol's argument position in each helper.
            let header_control = || {
                index >= 3
                    && tokens[index - 3..index]
                        == [
                            Token::Ident("HeaderControl".to_owned()),
                            Token::Punct(':'),
                            Token::Punct(':'),
                        ]
            };
            let position = match token {
                Token::Ident(name) if name == "button" => 2,
                Token::Ident(name) if name == "control_icon" => 0,
                Token::Ident(name) if (name == "new" || name == "toggle") && header_control() => 4,
                _ => continue,
            };
            let call = tokens.get(index + 1) == Some(&Token::Punct('('))
                && !matches!(
                    index.checked_sub(1).map(|i| &tokens[i]),
                    Some(Token::Punct('.')) | Some(Token::Ident(_))
                );
            if !call {
                continue;
            }
            let mut arguments: Vec<Vec<&Token>> = vec![Vec::new()];
            let mut depth = 0usize;
            for token in &tokens[index + 1..] {
                match token {
                    Token::Punct('(' | '[' | '{') => {
                        depth += 1;
                        if depth == 1 {
                            continue;
                        }
                    }
                    Token::Punct(')' | ']' | '}') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Token::Punct(',') if depth == 1 => {
                        arguments.push(Vec::new());
                        continue;
                    }
                    _ => {}
                }
                arguments.last_mut().unwrap().push(token);
            }
            if let Some([Token::Str(symbol)]) = arguments.get(position).map(Vec::as_slice)
                && !symbol.is_empty()
            {
                symbols.push(symbol.clone());
            }
        }
        symbols
    }

    fn sources(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    #[test]
    fn scanner_reads_only_helper_calls_and_icon_paths() {
        let source = r#"
            fn button(id: &str, label: &str, symbol: &str, active: bool) {}
            // button("commented", "", "commented-out", false)
            let a = button("one", if x { "a, b" } else { "c" }, "first", false);
            let b = crate::button(("pair", 1), format!("{}", 'x'), "second", true)
                .on_click(|this: &'static str| this);
            let c = toolbar.button("method", "", "method-call", false);
            let d = button("empty", "Label", "", false);
            let e = button("computed", "", symbol, false);
            let f = Icon::default().path("icons/third.svg");
            let g = svg().path(format!("icons/{name}.svg"));
            let h = copy_feedback::copy_button(mark, "Copy", "a tooltip, not an icon", cx);
            fn control_icon(symbol: &str) -> Icon {}
            let i = div().child(control_icon("fourth")).child(control_icon(icon));
            let j = crate::system_preview::HeaderControl::new(
                ("clip", index), format!("clip-{index}"), "Default pose", name, "fifth", widest,
            );
            let k = HeaderControl::toggle(id, selector, ["Off", "On"], name, "sixth", on, window);
            let l = Self::new(id, selector, readings[0], name, icon, widest);
            let m = Theme::new(id, selector, reading, name, "not-a-header-control", widest);
        "#;
        assert_eq!(
            literal_icon_symbols(source),
            ["first", "second", "third", "fourth", "fifth", "sixth"]
        );
    }

    #[test]
    fn every_literal_icon_symbol_resolves_through_the_app_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&root, &mut files);
        files.sort();
        let mut checked = 0;
        let mut missing = Vec::new();
        for file in &files {
            let source = std::fs::read_to_string(file).unwrap();
            for symbol in literal_icon_symbols(&source) {
                checked += 1;
                let path = format!("icons/{symbol}.svg");
                if !matches!(Assets.load(&path), Ok(Some(_))) {
                    let file = file.strip_prefix(&root).unwrap_or(file).display();
                    missing.push(format!("{file}: {path}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "icons that draw an empty slot: {missing:#?}"
        );
        let reflog = std::fs::read_to_string(root.join("reflog.rs")).unwrap();
        assert!(
            literal_icon_symbols(&reflog)
                .iter()
                .any(|symbol| symbol == "refresh"),
            "the scanner must see Read log's helper call"
        );
        // A renamed asset of a side header control drew an empty 28 px
        // square with every test passing while the scanner missed them.
        for (file, symbol) in [
            ("pdf_view.rs", "page"),
            ("model_view.rs", "edges"),
            ("model_view/playback.rs", "clip"),
        ] {
            let source = std::fs::read_to_string(root.join(file)).unwrap();
            assert!(
                literal_icon_symbols(&source)
                    .iter()
                    .any(|seen| seen == symbol),
                "the scanner must see the side header control icon {symbol} in {file}"
            );
        }
        assert!(checked > 50, "only {checked} literal icon names were found");
    }
}
