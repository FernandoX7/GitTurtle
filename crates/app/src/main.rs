mod activity;
mod appearance;
mod authentication;
mod automatic_refresh;
mod blame;
mod branch_actions;
mod build_info;
mod columns;
mod command_palette;
mod commit_drafts;
mod commit_message;
mod conflicts;
mod desktop_text;
mod diff_view;
mod discard;
mod editor_find;
mod file_history;
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
mod image_lifetime;
mod integration;
mod interactive_rebase;
mod lfs_download;
mod local_refresh;
mod markdown_view;
mod model_view;
mod native_accessibility;
mod navigation;
mod operations;
mod page_navigation;
mod partial_view;
mod path_filter;
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
mod tags;
mod text;
mod text_review;
mod theme_editor;
mod views;
#[cfg(target_os = "linux")]
mod window_chrome;
mod worker;
mod working_selection;
mod workspace;
mod worktrees;

use appearance::palette;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, Worktree};
use gpui_kit::component::{
    Disableable, Icon, Root, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    input::{EditorState, Input, InputEvent, InputState, Textarea, TextareaState},
    resizable::{ResizableState, h_resizable, resizable_panel},
    tooltip::Tooltip,
};
use gpui_kit::*;
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
    menu_state: Option<(bool, bool)>,
    #[cfg(target_os = "linux")]
    primary_menu: Entity<PrimaryMenu>,
    dialog_layer_subscription: Option<Subscription>,
    modal_was_open: bool,
    modal_return_focus: Option<FocusHandle>,
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
    operation_repository: Option<PathBuf>,
    operation_outcomes: operations::RepositoryOutcomes,
    operation_error: Option<String>,
    operation_notice: Option<String>,
    operation_task: Option<Task<()>>,
    status_task: Option<Task<()>>,
    work_generation: u64,
    work_status: Option<Arc<gitturtle_core::RepositoryStatus>>,
    working_paths: working_selection::FilterState,
    integration_state: Option<gitturtle_core::OperationState>,
    integration_task: Option<Task<()>>,
    profile: Option<gitturtle_core::GitProfile>,
    profiles: profiles::State,
    theme_editor: theme_editor::State,
    remotes: Vec<gitturtle_core::Remote>,
    working_rows: Vec<workspace::WorkingRow>,
    working_selected: Option<(usize, gitturtle_core::ChangeArea)>,
    working_scroll: UniformListScrollHandle,
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
    repository: Option<GitRepository>,
    path: Option<PathBuf>,
    scope: Option<(String, worker::Scope)>,
    limit: usize,
    history_paging: history_paging::State,
    repository_tabs: repository_tabs::State,
    branches: Vec<Branch>,
    worktrees: Vec<Worktree>,
    nav_rows: Vec<NavRow>,
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
    refs: HashMap<String, Vec<String>>,
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
    app_focus: FocusHandle,
    focus: FocusHandle,
    file_focus: FocusHandle,
    pane: Pane,
    history_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    history_list_layout: Option<(Size<Pixels>, Pixels)>,
    file_list_layout: Option<(Size<Pixels>, Pixels)>,
    nav_scroll: UniformListScrollHandle,
    loading: Option<&'static str>,
    error: Option<String>,
    status: String,
    sidebar: bool,
    history_sidebar: bool,
    history_width: f32,
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
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search repository history, author, hash…")
        });
        let nav_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter branches & worktrees"));
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
            cx.new(|cx| InputState::new(window, cx).placeholder("Summarize the change"));
        let commit_message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Explain why this change is needed…")
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
                .placeholder("Visual Studio Code")
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
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter changed paths…"));
        let working_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter working paths…"));
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
            modal_was_open: false,
            modal_return_focus: None,
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
            #[cfg(test)]
            theme_action_names: Default::default(),
            #[cfg(test)]
            theme_action_tooltips: Default::default(),
            project_pane: project_pane::State::new(cx),
            rename_project: None,
            draft_saver: commit_drafts::DraftSaver::default(),
            draft_save_error: None,
            draft_repository: None,
            settings,
            preferences_writer: SerialExecutor::new("gitturtle-preferences"),
            operations: SerialExecutor::new("gitturtle-operations"),
            operation_busy: None,
            operation_repository: None,
            operation_outcomes: operations::RepositoryOutcomes::default(),
            operation_error: None,
            operation_notice: None,
            operation_task: None,
            status_task: None,
            work_generation: 0,
            work_status: None,
            working_paths: working_selection::FilterState::default(),
            integration_state: None,
            integration_task: None,
            profile: None,
            profiles: profiles::State::default(),
            theme_editor: theme_editor::State::default(),
            remotes: Vec::new(),
            working_rows: Vec::new(),
            working_selected: None,
            working_scroll: UniformListScrollHandle::new(),
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
            repository: None,
            path: None,
            scope: None,
            limit: 500,
            history_paging: history_paging::State::default(),
            repository_tabs: repository_tabs::State::from_session(tab_session),
            branches: vec![],
            worktrees: vec![],
            nav_rows: vec![],
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
            refs: HashMap::new(),
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
            app_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            pane: Pane::History,
            history_scroll: UniformListScrollHandle::new(),
            file_scroll: UniformListScrollHandle::new(),
            history_list_layout: None,
            file_list_layout: None,
            nav_scroll: UniformListScrollHandle::new(),
            loading: None,
            error: None,
            status: "Open a repository to explore its history".into(),
            sidebar: true,
            history_sidebar: true,
            history_width: 900.,
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
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.change_file_filter(cx);
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
        this._display_preferences_task = native_accessibility::observe_display_preferences(cx);
        this.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    native_accessibility::sync_preferences(cx);
                    desktop_text::refresh(cx);
                    this.apply_motion_preferences(cx);
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
        let opening = matches!(&job, Job::Open { .. }).then(|| {
            self.operation_outcomes
                .begin_open(&self.operation_error, &self.operation_notice)
        });
        let response = self.worker.submit(job);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = response.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.generation {
                    return;
                }
                this.loading = None;
                this.task = None;
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
                        this.error = Some(format!("{error:#}"));
                        if let Some(index) = this.repository_tabs.active
                            && let Some(tab) = this.repository_tabs.tabs.get_mut(index)
                        {
                            tab.error = this.error.clone();
                        }
                        this.status = "Read could not complete".into();
                        this.hub.update(cx, |hub, cx| {
                            hub.set_busy(false, cx);
                            hub.set_error(this.error.clone(), cx);
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

    fn open(
        &mut self,
        path: PathBuf,
        scope: Option<(String, worker::Scope)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_open_requested(&path, &scope, window, cx) {
            return;
        }
        self.cancel_branch_action();
        self.cancel_recovery_read();
        self.cancel_tag_action();
        self.cancel_interactive_rebase_action();
        self.cancel_ignore_action();
        self.cancel_discard_action();
        self.close_inspections(window, cx);
        self.close_blame(window, cx);
        self.blame = blame::State::default();
        self.discard_history_search();
        self.page_return_focus = None;
        self.restore_commit = if self.path.as_ref() == Some(&path) && self.scope == scope {
            self.selected_commit.map(|i| self.commits[i].oid.clone())
        } else {
            None
        };
        if self.path.as_ref() != Some(&path) {
            self.inspector_message = commit_message::State::default();
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
        self.refs.clear();
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
        self.request(
            Job::Open {
                path,
                scope: self.scope.as_ref().map(|s| s.1.clone()),
                limit: self.limit,
            },
            "Reading local history…",
            window,
            cx,
        );
    }

    fn clear_preview(&mut self) {
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
                self.refresh_file_filter(cx);
                self.selected_file = Some(0);
                self.receive(Output::Preview(content, elapsed), window, cx);
            }
            Output::Snapshot(snapshot) => {
                if self.tab_snapshot_accepted(&snapshot.repository, window, cx) {
                    return;
                }
                self.history_updates
                    .clear_scope_error(&mut self.operation_error);
                self.history_updates.captured(&snapshot);
                self.history_paging = history_paging::State::from_snapshot(&snapshot);
                self.status = format!(
                    "Local snapshot · {:.0} ms · {} branches · {} worktrees",
                    snapshot.elapsed.as_secs_f64() * 1000.,
                    snapshot.branches.len(),
                    snapshot.worktrees.len()
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
            }
            Output::Changes(files, elapsed) => {
                self.status = format!(
                    "{} changed files · file list {:.1} ms",
                    files.len(),
                    elapsed.as_secs_f64() * 1000.
                );
                self.files = files;
                self.refresh_file_filter(cx);
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
                self.status = format!(
                    "{} changed files · content read {:.1} ms",
                    self.files.len(),
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
                if self.page == AppPage::Repository
                    && matches!(self.mode, WorkspaceMode::Compare | WorkspaceMode::Working)
                {
                    self.ensure_editor(window, cx);
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
        if self.blame.is_visible() {
            return;
        }
        let Some(content) = &self.content else {
            return;
        };
        if let Content::Conflict(presentation) = content.as_ref() {
            self.ensure_conflict_view(Arc::clone(presentation), window, cx);
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
                self.split_view = Some(split_diff::new(Arc::clone(split), language, window, cx));
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
            let generation = self.generation;
            let path = self.path.clone();
            self.partial_subscription =
                Some(
                    cx.subscribe_in(&view, window, move |this, _, event, window, cx| {
                        if this.generation != generation
                            || this.path != path
                            || this.mode != WorkspaceMode::Working
                        {
                            return;
                        }
                        let diff_view::DiffViewEvent::ApplyPartial { diff, selection } = event;
                        this.write(
                            gitturtle_core::WriteCommand::ApplyPartial {
                                diff: Arc::clone(diff),
                                selection: selection.clone(),
                            },
                            if diff.area == gitturtle_core::ChangeArea::Unstaged {
                                "Staging selected changes…"
                            } else {
                                "Unstaging selected changes…"
                            },
                            window,
                            cx,
                        );
                    }),
                );
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
        self.nav_cursor = self
            .nav_cursor
            .filter(|index| {
                *index < self.nav_rows.len()
                    && !matches!(self.nav_rows[*index], NavRow::Section(..))
            })
            .or(Some(0));
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
            self.show_projects(window, cx);
            return;
        }
        self.close_inspections(window, cx);
        self.back_to_history(window, cx);
        self.repaint_page(window, cx);
    }

    fn back_to_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
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
            self.refresh_file_filter(cx);
            self.selected_file = selected;
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
        self.inspector_message.select(&self.commits[index].oid);
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
        self.close_blame(window, cx);
        if index >= self.files.len() {
            return;
        }
        self.interaction_started = Some(Instant::now());
        if self.mode == WorkspaceMode::History {
            self.history_sidebar = self.sidebar;
            self.sidebar = false;
            self.mode = WorkspaceMode::Compare;
        }
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        if self.selected_file == Some(index) && self.content.is_some() {
            self.ensure_editor(window, cx);
            self.trace_frame("file_preview_frame_ms", window, cx);
            cx.notify();
        } else {
            self.load_file(index, window, cx);
        }
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
            let origins = self
                .file_history_preview_origins()
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
                });
            self.request(
                Job::Preview {
                    origins,
                    repo: repo.clone(),
                    file: self.files[index].clone(),
                },
                "Loading preview…",
                window,
                cx,
            );
        }
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
                Err(error) => window.open_alert_dialog(cx, move |dialog, _, cx| {
                    dialog.title("Could not open repository picker").child(
                        div()
                            .id("repository-picker-error")
                            .role(Role::Label)
                            .aria_label(error.clone())
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
        self.show_history(window, cx);
        window.focus(&self.search.read(cx).focus_handle(cx), cx);
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
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.history_query_changed(window, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

fn app_icon(dimension: f32) -> Img {
    img(APP_ICON_PATH)
        .size(px(dimension))
        .object_fit(ObjectFit::Contain)
        .flex_shrink_0()
}

fn icon(name: &str, dimension: f32, color: u32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
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
    let label = label.into();
    // Transparent at rest like the kit's ghost, with the palette's hover and
    // pressed fills: the ghost's own hover darkens a hovered row. Selected, it
    // keeps the `selected` surface.
    let mut button = Button::new(id)
        .small()
        .with_variant(appearance::control_button_variant(active))
        .h(appearance::ui_size(28.))
        .min_w(appearance::ui_size(28.))
        .px(appearance::ui_size(if label.is_empty() { 6. } else { 10. }))
        .gap(appearance::ui_size(6.))
        .rounded(appearance::ui_size(7.))
        .selected(active)
        .text_size(crate::appearance::ui_text(12.));
    if active {
        // The kit omits variant hover styles for selected controls. Keep their
        // selected surface and expose gentle pointer feedback explicitly.
        button = button.hover(|style| style.opacity(0.9));
    }
    if label.is_empty() {
        button = button
            .w(appearance::ui_size(28.))
            .accessibility_label(format!("{} file path", symbol));
    } else {
        button = button.label(label);
    }
    if !symbol.is_empty() {
        button = button.icon(
            Icon::default()
                .path(format!("icons/{symbol}.svg"))
                .size(appearance::ui_size(16.)),
        );
    }
    button
}
fn empty(title: &str, detail: &str) -> AnyElement {
    div()
        .id(SharedString::from(format!("empty-state:{title}")))
        .role(Role::Label)
        .aria_label(format!("{title}. {detail}"))
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
        )
        .into_any_element()
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
    let preferences = Preferences::load();
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
        native_accessibility::sync_preferences(cx);
        let desktop_text = desktop_text::start(cx);
        native_accessibility::bind_keys(cx);
        image_lifetime::init(cx);
        interactive_rebase::init(cx);
        theme_editor::init(cx);
        preferences
            .settings
            .resolved_theme(cx.window_appearance(), &preferences.custom_themes)
            .apply(None, cx);
        shortcuts::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &HideApplication, cx| cx.hide());
        cx.on_action(|_: &HideOtherApplications, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAllApplications, cx| cx.unhide_other_apps());
        platform_polish::menus(false, false, cx);
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
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("GitTurtle".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(1000.), px(680.))),
                    app_id: Some("com.gitturtle.desktop".into()),
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
