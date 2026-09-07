mod diff_view;
mod graph;
mod navigation;
mod preferences;
mod text;
mod views;
mod worker;

use gitturtle_core::{Branch, Commit, FileChange, GitRepository, Worktree};
use gpui_kit::component::{
    Disableable, Icon, Root, Selectable, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Editor, EditorState, Input, InputEvent, InputState},
    resizable::{h_resizable, resizable_panel},
    tooltip::Tooltip,
};
use gpui_kit::*;
use preferences::Preferences;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use worker::{Content, Job, Output, Worker};

const CANVAS: u32 = 0x0f171c;
const PANEL: u32 = 0x121e24;
const HOVER: u32 = 0x19272e;
const BORDER: u32 = 0x293b43;
const TEXT: u32 = 0xdee9ed;
const MUTED: u32 = 0x9aaeb8;
const MINT: u32 = 0x7adfb4;
const SELECTED: u32 = 0x1c3b36;

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
        BackHistory
    ]
);

#[derive(rust_embed::RustEmbed)]
#[folder = "../../assets/"]
struct EmbeddedAssets;
struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(asset) = EmbeddedAssets::get(path) {
            return Ok(Some(asset.data));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut result: Vec<SharedString> = EmbeddedAssets::iter()
            .filter(|s| s.starts_with(path))
            .map(|s| s.to_string().into())
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
    graph_width: f32,
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
    search: Entity<InputState>,
    nav_search: Entity<InputState>,
    subscriptions: Vec<Subscription>,
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
    fn new(initial: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search loaded commits, author, hash…")
        });
        let nav_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter branches & worktrees"));
        let mut this = Self {
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
            graph_width: 112.,
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
            search: search.clone(),
            nav_search: nav_search.clone(),
            subscriptions: vec![],
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
        window.focus(&this.focus, cx);
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
        self.restore_commit = if self.path.as_ref() == Some(&path) && self.scope == scope {
            self.selected_commit.map(|i| self.commits[i].oid.clone())
        } else {
            None
        };
        if self.path.as_ref() != Some(&path) {
            self.repository = None;
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
        self.content = None;
        self.patch_editor = None;
        self.patch_view = None;
        self.before_editor = None;
        self.after_editor = None;
        self.images = [None, None];
    }

    fn receive(&mut self, output: Output, window: &mut Window, cx: &mut Context<Self>) {
        match output {
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
                self.graph_width = (self.graph_lanes as f32 * 12. + 24.).clamp(84., 230.);
                self.path = Some(snapshot.repository.path().to_owned());
                self.repository = Some(snapshot.repository);
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
                if let Some(index) = selected {
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
                if self.mode == WorkspaceMode::Compare {
                    self.ensure_editor(window, cx);
                    self.trace_frame("file_preview_frame_ms", window, cx);
                }
            }
        }
    }

    fn ensure_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(content) = &self.content else {
            return;
        };
        let Content::Text { patch, old, new } = content.as_ref() else {
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
            *slot = Some(text::editor(value, language, diff, window, cx));
        }
        if diff && self.patch_view.is_none() {
            self.patch_view = self
                .patch_editor
                .as_ref()
                .map(|editor| diff_view::new(editor.clone(), patch, window, cx));
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
                .filter(|b| b.remote == remote && b.name.to_lowercase().contains(&query))
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
        if self.mode == WorkspaceMode::Compare {
            self.mode = WorkspaceMode::History;
            self.sidebar = self.history_sidebar;
        }
        self.interaction_started = None;
        self.pane = Pane::History;
        window.focus(&self.focus, cx);
        cx.notify();
    }
    fn select_commit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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
        if let Some(path) = self.path.clone() {
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
        if self.mode == WorkspaceMode::Compare {
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
        .text_size(px(11.));
    if label.is_empty() {
        button = button.accessibility_label(format!("{} file path", symbol));
    } else {
        button = button.label(label);
    }
    if !symbol.is_empty() {
        button = button.icon(
            Icon::default()
                .path(format!("icons/{symbol}.svg"))
                .size(px(14.)),
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
        .child(
            div()
                .text_size(px(14.))
                .text_color(rgb(TEXT))
                .child(title.to_owned()),
        )
        .child(
            div()
                .max_w(px(600.))
                .text_center()
                .text_size(px(12.))
                .text_color(rgb(MUTED))
                .child(detail.to_owned()),
        )
        .into_any_element()
}

fn checkerboard() -> AnyElement {
    canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let tile = 12.;
            let rows = (f32::from(bounds.size.height) / tile).ceil() as i32;
            let cols = (f32::from(bounds.size.width) / tile).ceil() as i32;
            for row in 0..rows {
                for col in 0..cols {
                    let color = if (row + col) % 2 == 0 {
                        0x172229
                    } else {
                        0x1c2930
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
    let initial = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| preferences.last_repository());
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        {
            let theme = Theme::global_mut(cx);
            theme.colors.background = rgb(CANVAS).into();
            theme.colors.foreground = rgb(TEXT).into();
            theme.colors.muted_foreground = rgb(MUTED).into();
            theme.colors.primary = rgb(MINT).into();
            theme.colors.border = rgb(BORDER).into();
            theme.colors.input = rgb(BORDER).into();
            theme.colors.selection = rgb(SELECTED).into();
            let syntax = Arc::make_mut(&mut theme.highlight_theme);
            syntax.style.editor_background = Some(rgb(CANVAS).into());
            syntax.style.editor_gutter_background = Some(rgb(CANVAS).into());
            syntax.style.editor_active_line = Some(rgb(PANEL).into());
            syntax.style.editor_line_number = Some(rgb(0x8199a4).into());
            syntax.style.editor_foreground = Some(rgb(TEXT).into());
            theme.font_size = px(13.);
            theme.mono_font_size = px(12.);
            theme.radius = px(5.);
        }
        Theme::sync_base(cx);
        let primary = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.bind_keys([
            KeyBinding::new(&format!("{primary}-q"), Quit, None),
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
                MenuItem::action("Refresh Local State", Refresh),
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
                    let view = cx.new(|cx| GitTurtle::new(initial, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("open GitTurtle window");
        })
        .detach();
    });
}
