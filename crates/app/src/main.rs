mod appearance;
mod columns;
mod diff_view;
mod graph;
mod navigation;
mod operations;
mod preferences;
mod projects;
mod settings;
mod text;
mod views;
mod worker;
mod workspace;

use appearance::palette;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, Worktree};
use gpui_kit::component::{
    Disableable, Icon, Root, Selectable, Sizable,
    button::{Button, ButtonVariants},
    input::{Editor, EditorState, Input, InputEvent, InputState, Textarea, TextareaState},
    resizable::{h_resizable, resizable_panel},
    tooltip::Tooltip,
};
use gpui_kit::*;
use operations::SerialExecutor;
use preferences::{AppSettings, Preferences};
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
        Quit,
        OpenRepository,
        Refresh,
        Search,
        NextRow,
        PreviousRow,
        FirstRow,
        LastRow,
        NextPane,
        ClearSearch,
        ToggleSidebar,
        BackHistory,
        ShowProjects,
        ShowSettings,
        ShowChanges
    ]
);

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets/icons/"]
struct EmbeddedAssets;
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
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
#[derive(Clone, Copy, PartialEq)]
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
    Before,
    After,
}
#[derive(Clone, Copy, PartialEq)]
enum NavMode {
    Local,
    Remote,
    Worktrees,
}

struct GitTurtle {
    retained_history_files: Option<(Vec<FileChange>, Option<usize>)>,
    commit_drafts: HashMap<PathBuf, String>,
    draft_repository: Option<PathBuf>,
    settings: AppSettings,
    preferences_writer: SerialExecutor,
    operations: SerialExecutor,
    operation_busy: Option<&'static str>,
    operation_error: Option<String>,
    operation_notice: Option<String>,
    operation_task: Option<Task<()>>,
    status_task: Option<Task<()>>,
    work_generation: u64,
    work_status: Option<gitturtle_core::RepositoryStatus>,
    profile: Option<gitturtle_core::GitProfile>,
    remotes: Vec<gitturtle_core::Remote>,
    working_rows: Vec<workspace::WorkingRow>,
    working_selected: Option<(usize, gitturtle_core::ChangeArea)>,
    working_scroll: UniformListScrollHandle,
    page: AppPage,
    hub: Entity<projects::ProjectHub>,
    commit_message: Entity<TextareaState>,
    identity_name: Entity<InputState>,
    identity_email: Entity<InputState>,
    branch_name: Entity<InputState>,
    remote_name: Entity<InputState>,
    remote_branch: Entity<InputState>,
    settings_branch: Entity<InputState>,
    column_drag: Option<(columns::ColumnId, Pixels, f32)>,
    column_menu: bool,
    git_actions_open: bool,
    history_horizontal: ScrollHandle,
    worker: Worker,
    task: Option<Task<()>>,
    generation: u64,
    repository: Option<GitRepository>,
    path: Option<PathBuf>,
    scope: Option<(String, worker::Scope)>,
    limit: usize,
    branches: Vec<Branch>,
    worktrees: Vec<Worktree>,
    nav_rows: Vec<NavRow>,
    nav_mode: NavMode,
    expanded_folders: HashSet<String>,
    seed_folders: bool,
    commits: Vec<Commit>,
    visible: Vec<usize>,
    graph: Vec<graph::GraphRow>,
    graph_lanes: usize,
    graph_notice: Option<String>,
    refs: HashMap<String, Vec<String>>,
    selected_commit: Option<usize>,
    selected_file: Option<usize>,
    parent: usize,
    files: Vec<FileChange>,
    content: Option<Arc<Content>>,
    patch_editor: Option<Entity<EditorState>>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    before_editor: Option<Entity<EditorState>>,
    after_editor: Option<Entity<EditorState>>,
    images: [Option<Arc<RenderImage>>; 2],
    text_mode: TextMode,
    zoom: f32,
    image_scroll: ScrollHandle,
    image_drag: Option<(Point<Pixels>, Point<Pixels>)>,
    search: Entity<InputState>,
    nav_search: Entity<InputState>,
    subscriptions: Vec<Subscription>,
    app_focus: FocusHandle,
    focus: FocusHandle,
    file_focus: FocusHandle,
    pane: Pane,
    history_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
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
    details: bool,
}

impl GitTurtle {
    fn new(
        initial: Option<PathBuf>,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search loaded commits, author, hash…")
        });
        let nav_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter branches & worktrees"));
        let settings = preferences.settings.clone();
        let hub = cx.new(|cx| {
            projects::ProjectHub::new(
                preferences.recent_repositories.clone(),
                settings.default_branch.clone(),
                window,
                cx,
            )
        });
        let commit_message = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Summarize your changes…\n\nAdd a description (optional)")
                .rows(3)
        });
        let identity_name = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
        let identity_email =
            cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com"));
        let branch_name = cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-change"));
        let remote_name = cx.new(|cx| InputState::new(window, cx).placeholder("Remote"));
        let remote_branch = cx.new(|cx| InputState::new(window, cx).placeholder("Remote branch"));
        let settings_branch =
            cx.new(|cx| InputState::new(window, cx).default_value(settings.default_branch.clone()));
        let mut this = Self {
            retained_history_files: None,
            commit_drafts: HashMap::new(),
            draft_repository: None,
            settings,
            preferences_writer: SerialExecutor::new("gitturtle-preferences"),
            operations: SerialExecutor::new("gitturtle-operations"),
            operation_busy: None,
            operation_error: None,
            operation_notice: None,
            operation_task: None,
            status_task: None,
            work_generation: 0,
            work_status: None,
            profile: None,
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
            commit_message,
            identity_name,
            identity_email,
            branch_name,
            remote_name,
            remote_branch,
            settings_branch,
            column_drag: None,
            column_menu: false,
            git_actions_open: true,
            history_horizontal: ScrollHandle::new(),
            worker: Worker::new(),
            task: None,
            generation: 0,
            repository: None,
            path: None,
            scope: None,
            limit: 500,
            branches: vec![],
            worktrees: vec![],
            nav_rows: vec![],
            nav_mode: NavMode::Local,
            expanded_folders: HashSet::new(),
            seed_folders: true,
            commits: vec![],
            visible: vec![],
            graph: vec![],
            graph_lanes: 1,
            graph_notice: None,
            refs: HashMap::new(),
            selected_commit: None,
            selected_file: None,
            parent: 0,
            files: vec![],
            content: None,
            patch_editor: None,
            patch_view: None,
            before_editor: None,
            after_editor: None,
            images: [None, None],
            text_mode: TextMode::Unified,
            zoom: 0.,
            image_scroll: ScrollHandle::new(),
            image_drag: None,
            search: search.clone(),
            nav_search: nav_search.clone(),
            subscriptions: vec![],
            app_focus: cx.focus_handle(),
            focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            pane: Pane::History,
            history_scroll: UniformListScrollHandle::new(),
            file_scroll: UniformListScrollHandle::new(),
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
            details: false,
        };
        this.subscriptions
            .push(cx.subscribe_in(&search, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filter_history(cx);
                    cx.notify();
                }
            }));
        this.subscriptions.push(
            cx.subscribe_in(&nav_search, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
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
            |_, _, _, _, cx| cx.notify(),
        ));
        for input in [&this.branch_name, &this.remote_name, &this.remote_branch] {
            this.subscriptions
                .push(cx.subscribe_in(input, window, |_, _, event, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }));
        }
        this.subscribe_settings_inputs(window, cx);
        window.focus(
            if initial.is_some() {
                &this.focus
            } else {
                &this.app_focus
            },
            cx,
        );
        if let Some(path) = initial {
            this.open(path, None, window, cx);
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
        self.generation += 1;
        let generation = self.generation;
        self.loading = Some(label);
        self.error = None;
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
                match result {
                    Ok(output) => this.receive(output, window, cx),
                    Err(error) => {
                        this.error = Some(format!("{error:#}"));
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
        if self.operation_busy.is_some() {
            return;
        }
        self.restore_commit = if self.path.as_ref() == Some(&path) && self.scope == scope {
            self.selected_commit.map(|i| self.commits[i].oid.clone())
        } else {
            None
        };
        if self.path.as_ref() != Some(&path) {
            self.operation_notice = None;
            self.retained_history_files = None;
            self.repository = None;
            self.work_generation += 1;
            self.work_status = None;
            self.profile = None;
            self.remotes.clear();
            self.working_rows.clear();
            self.working_selected = None;
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
        self.path = Some(path.clone());
        self.refs.clear();
        self.scope = scope;
        self.clear_preview();
        self.files.clear();
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
        self.image_drag = None;
        self.content = None;
        self.patch_editor = None;
        self.patch_view = None;
        self.before_editor = None;
        self.after_editor = None;
        self.images = [None, None];
    }

    fn receive(&mut self, output: Output, window: &mut Window, cx: &mut Context<Self>) {
        match output {
            Output::WorkingPreview(file, content, elapsed) => {
                if self.mode != WorkspaceMode::Working {
                    return;
                }
                self.files = vec![file];
                self.selected_file = Some(0);
                self.receive(Output::Preview(content, elapsed), window, cx);
            }
            Output::Snapshot(snapshot) => {
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
                if self.draft_repository.as_ref() != Some(&resolved) {
                    if let Some(previous) = self.draft_repository.take() {
                        self.commit_drafts
                            .insert(previous, self.commit_message.read(cx).value().to_string());
                    }
                    let draft = self
                        .commit_drafts
                        .get(&resolved)
                        .cloned()
                        .unwrap_or_default();
                    self.commit_message
                        .update(cx, |input, cx| input.set_value(draft, window, cx));
                    self.draft_repository = Some(resolved);
                }
                self.path = Some(snapshot.repository.path().to_owned());
                self.repository = Some(snapshot.repository);
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
                if self.mode == WorkspaceMode::Working {
                    self.selected_commit = selected;
                } else if let Some(index) = selected {
                    self.select_commit(index, window, cx);
                    if let Some(position) = self.visible.iter().position(|&i| i == index) {
                        self.history_scroll
                            .scroll_to_item(position, ScrollStrategy::Center);
                    }
                }
            }
            Output::Changes(files, elapsed) => {
                self.status = format!(
                    "{} changed files · file list {:.1} ms",
                    files.len(),
                    elapsed.as_secs_f64() * 1000.
                );
                self.files = files;
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
            }
            Output::Preview(content, elapsed) => {
                self.status = format!(
                    "{} changed files · content read {:.1} ms",
                    self.files.len(),
                    elapsed.as_secs_f64() * 1000.
                );
                match content.as_ref() {
                    Content::Text { .. } => {}
                    Content::Images { old, new } => {
                        self.images = [old.render.clone(), new.render.clone()];
                    }
                    Content::Notice(_) => {}
                }
                self.content = Some(content);
                if self.page == AppPage::Repository
                    && matches!(self.mode, WorkspaceMode::Compare | WorkspaceMode::Working)
                {
                    self.ensure_editor(window, cx);
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
        let Some(content) = &self.content else {
            return;
        };
        let Content::Text {
            patch,
            old,
            new,
            presentation,
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
        let (slot, value, language, diff) = match self.text_mode {
            TextMode::Unified => (&mut self.patch_editor, patch, "diff", true),
            TextMode::Before => (&mut self.before_editor, old, language, false),
            TextMode::After => (&mut self.after_editor, new, language, false),
        };
        if slot.is_none() {
            *slot = Some(text::editor(
                value,
                language,
                diff.then_some(presentation.as_ref()),
                window,
                cx,
            ));
        }
        if diff && self.patch_view.is_none() {
            self.patch_view = self
                .patch_editor
                .as_ref()
                .map(|editor| diff_view::new(editor.clone(), presentation, window, cx));
        }
    }

    fn filter_history(&mut self, cx: &App) {
        let query = self.search.read(cx).value().to_lowercase();
        self.visible = self
            .commits
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                query.is_empty()
                    || c.subject.to_lowercase().contains(&query)
                    || c.author.to_lowercase().contains(&query)
                    || c.oid.contains(&query)
                    || c.body.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        self.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
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
    }

    fn trace_frame(&mut self, metric: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(start) = self.interaction_started.take() else {
            return;
        };
        if std::env::var_os("GITTURTLE_TRACE").is_none() {
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

    fn back_to_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was_working = self.mode == WorkspaceMode::Working;
        if was_working {
            self.invalidate_read();
            self.clear_preview();
            let (files, selected) = self.retained_history_files.take().unwrap_or_default();
            self.files = files;
            self.selected_file = selected;
        }
        if self.mode != WorkspaceMode::History {
            self.mode = WorkspaceMode::History;
            self.sidebar = self.history_sidebar;
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
        cx.notify();
    }
    fn select_commit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        self.selected_commit = Some(index);
        self.parent = 0;
        self.files.clear();
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
            self.request(
                Job::Preview {
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
        self.selected_file = None;
        self.clear_preview();
        self.request(job, "Reading comparison…", window, cx);
    }
    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
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
            if let Ok(Ok(Some(paths))) = response.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.limit = 500;
                    this.open(path, None, window, cx);
                });
            }
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
            if self.files.is_empty() {
                return;
            }
            let index = if edge {
                if direction < 0 {
                    0
                } else {
                    self.files.len() - 1
                }
            } else {
                (self.selected_file.unwrap_or(0) as i32 + direction)
                    .clamp(0, self.files.len() as i32 - 1) as usize
            };
            self.select_file(index, window, cx);
            self.file_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
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
        self.back_to_history(window, cx);
        window.focus(&self.search.read(cx).focus_handle(cx), cx);
    }
    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository {
            self.page = if self.repository.is_some() {
                AppPage::Repository
            } else {
                AppPage::Projects
            };
            if self.page == AppPage::Repository && self.mode != WorkspaceMode::History {
                self.ensure_editor(window, cx);
            }
            window.focus(
                if self.page != AppPage::Repository {
                    &self.app_focus
                } else if self.mode == WorkspaceMode::History {
                    &self.focus
                } else {
                    &self.file_focus
                },
                cx,
            );
            cx.notify();
            return;
        }
        if self.mode != WorkspaceMode::History {
            self.back_to_history(window, cx);
            return;
        }
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.filter_history(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }
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
    let mut button = Button::new(id)
        .small()
        .ghost()
        .selected(active)
        .text_size(px(12.));
    if active {
        // The kit omits variant hover styles for selected controls. Keep their
        // selected surface and expose gentle pointer feedback explicitly.
        button = button.secondary().hover(|style| style.opacity(0.9));
    }
    if label.is_empty() {
        button = button.accessibility_label(format!("{} file path", symbol));
    } else {
        button = button.label(label);
    }
    if !symbol.is_empty() {
        button = button.icon(
            Icon::default()
                .path(format!("icons/{symbol}.svg"))
                .size(px(16.)),
        );
    }
    button
}
fn empty(title: &str, detail: &str) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p_6()
        .gap_2()
        .child(div().text_size(px(14.)).child(title.to_owned()))
        .child(
            div()
                .max_w(px(600.))
                .text_center()
                .text_size(px(12.))
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
    match extension {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" => "javascript",
        "json" => "json",
        "css" => "css",
        "md" => "markdown",
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
    let preferences = Preferences::load();
    let initial = std::env::args_os().nth(1).map(PathBuf::from).or_else(|| {
        preferences
            .settings
            .reopen_last
            .then(|| preferences.last_repository())
            .flatten()
    });
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        preferences.settings.theme.apply(None, cx);
        let primary = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.bind_keys([
            KeyBinding::new(&format!("{primary}-q"), Quit, None),
            KeyBinding::new(&format!("{primary}-,"), ShowSettings, Some("GitTurtle")),
            KeyBinding::new(
                &format!("{primary}-shift-o"),
                ShowProjects,
                Some("GitTurtle"),
            ),
            KeyBinding::new(&format!("{primary}-2"), ShowChanges, Some("GitTurtle")),
            KeyBinding::new(&format!("{primary}-o"), OpenRepository, Some("GitTurtle")),
            KeyBinding::new(&format!("{primary}-r"), Refresh, Some("GitTurtle")),
            KeyBinding::new(&format!("{primary}-f"), Search, Some("GitTurtleList")),
            KeyBinding::new(&format!("{primary}-b"), ToggleSidebar, Some("GitTurtle")),
            KeyBinding::new(&format!("{primary}-["), BackHistory, Some("GitTurtle")),
            KeyBinding::new("down", NextRow, Some("GitTurtleList")),
            KeyBinding::new("up", PreviousRow, Some("GitTurtleList")),
            KeyBinding::new("home", FirstRow, Some("GitTurtleList")),
            KeyBinding::new("end", LastRow, Some("GitTurtleList")),
            KeyBinding::new("enter", NextPane, Some("GitTurtleList")),
            KeyBinding::new("escape", ClearSearch, Some("GitTurtle")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "GitTurtle".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Open Repository…", OpenRepository),
                MenuItem::action("Projects…", ShowProjects),
                MenuItem::action("Working Changes", ShowChanges),
                MenuItem::action("Refresh Local State", Refresh),
                MenuItem::action("Settings…", ShowSettings),
                MenuItem::separator(),
                MenuItem::action("Quit GitTurtle", Quit),
            ],
        }]);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1480.), px(980.)), cx);
        cx.activate(true);
        cx.spawn(async move |cx| {
            cx.open_window(
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
                    let view = cx.new(|cx| GitTurtle::new(initial, preferences, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("open GitTurtle window");
        })
        .detach();
    });
}
