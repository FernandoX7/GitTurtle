mod graph;
mod preferences;
mod text;
mod worker;

use gitturtle_core::{Branch, Commit, FileChange, GitRepository, Worktree};
use gpui_kit::component::{
    Disableable, Icon, Root, Selectable, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Editor, EditorState, Input, InputEvent, InputState},
    resizable::{h_resizable, resizable_panel, v_resizable},
    tooltip::Tooltip,
};
use gpui_kit::*;
use preferences::Preferences;
use std::{borrow::Cow, collections::HashMap, path::PathBuf, sync::Arc, time::Instant};
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
        ToggleSidebar
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
    Branch(usize),
    Worktree(usize),
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
        self.path = Some(path.clone());
        self.repository = None;
        self.branches.clear();
        self.worktrees.clear();
        self.nav_rows.clear();
        self.refs.clear();
        self.scope = scope;
        self.clear_preview();
        self.files.clear();
        self.selected_file = None;
        self.selected_commit = None;
        self.commits.clear();
        self.visible.clear();
        self.graph.clear();
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
                    self.load_file(index, window, cx);
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
                self.ensure_editor(window, cx);
                if let Some(start) = self.interaction_started.take() {
                    let generation = self.generation;
                    let view = cx.entity().downgrade();
                    window.on_next_frame(move |_, cx| {
                        let _ = view.update(cx, |this, _| {
                            if this.generation == generation
                                && std::env::var_os("GITTURTLE_TRACE").is_some()
                            {
                                eprintln!(
                                    "gitturtle.selection_frame_ms={:.3}",
                                    start.elapsed().as_secs_f64() * 1000.
                                );
                            }
                        });
                    });
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
        let query = self.nav_search.read(cx).value().to_lowercase();
        self.nav_rows = vec![NavRow::All];
        for (remote, label) in [(false, "LOCAL BRANCHES"), (true, "REMOTE BRANCHES")] {
            if (remote && self.nav_mode != NavMode::Remote)
                || (!remote && self.nav_mode != NavMode::Local)
            {
                continue;
            }
            let matching: Vec<_> = self
                .branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.remote == remote && b.name.to_lowercase().contains(&query))
                .map(|(i, _)| NavRow::Branch(i))
                .collect();
            self.nav_rows.push(NavRow::Section(label, matching.len()));
            self.nav_rows.extend(matching);
        }
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
        if self.nav_mode == NavMode::Worktrees {
            self.nav_rows
                .push(NavRow::Section("WORKTREES", matching.len()));
            self.nav_rows.extend(matching);
        }
        self.nav_scroll.scroll_to_item(0, ScrollStrategy::Top);
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
        self.interaction_started = Some(Instant::now());
        self.load_file(index, window, cx);
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
        window.focus(&self.search.read(cx).focus_handle(cx), cx);
    }
    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.filter_history(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h(px(52.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(icon("turtle", 25., MINT))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(265.))
                    .overflow_hidden()
                    .child(
                        div().font_weight(FontWeight::SEMIBOLD).child(
                            self.repository
                                .as_ref()
                                .map(|r| r.name())
                                .unwrap_or("GitTurtle".into()),
                        ),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(
                                self.path
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or("A clearer view of your code".into()),
                            ),
                    ),
            )
            .child(
                button("open", "Open", "folder", false).on_click(cx.listener(
                    |this, _, window, cx| this.choose_repository(&OpenRepository, window, cx),
                )),
            )
            .child(div().flex_1())
            .child(
                div()
                    .w(px(330.))
                    .child(Input::new(&self.search).text_size(px(12.))),
            )
            .child(
                button("refresh", "Refresh", "refresh", false).on_click(
                    cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(MINT))
                    .px_2()
                    .py_1()
                    .bg(rgb(SELECTED))
                    .rounded(px(4.))
                    .child("READ ONLY"),
            )
            .into_any_element()
    }
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div().flex().gap_1().px_2().pt_2().children(
                    [
                        (NavMode::Local, "Local"),
                        (NavMode::Remote, "Remote"),
                        (NavMode::Worktrees, "Worktrees"),
                    ]
                    .map(|(mode, name)| {
                        button(name, name, "", self.nav_mode == mode).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.nav_mode = mode;
                                this.rebuild_navigation(cx);
                                cx.notify();
                            },
                        ))
                    }),
                ),
            )
            .child(
                div()
                    .px_3()
                    .py_3()
                    .child(Input::new(&self.nav_search).text_size(px(11.))),
            )
            .child(
                uniform_list(
                    "navigation",
                    self.nav_rows.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|i| this.render_nav_row(i, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h_0()
                .track_scroll(&self.nav_scroll),
            )
            .child(
                div()
                    .p_3()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child("Remote branches reflect local refs.\nRefresh never fetches."),
            )
            .into_any_element()
    }
    fn render_nav_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = self.nav_rows[index].clone();
        let (name, symbol, active, path) = match &row {
            NavRow::Section(label, count) => {
                return div()
                    .w_full()
                    .h(px(30.))
                    .px_3()
                    .pt_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(*label)
                    .child(count.to_string())
                    .into_any_element();
            }
            NavRow::All => ("All history".into(), "commit", self.scope.is_none(), None),
            NavRow::Branch(i) => {
                let b = &self.branches[*i];
                (
                    format!("{}{}", if b.current { "• " } else { "" }, b.name),
                    if b.remote { "remote" } else { "branch" },
                    self.scope.as_ref().is_some_and(|s| s.0 == b.name),
                    None,
                )
            }
            NavRow::Worktree(i) => {
                let w = &self.worktrees[*i];
                (
                    w.path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    "worktree",
                    self.path.as_ref() == Some(&w.path),
                    Some(w.path.clone()),
                )
            }
        };
        let tooltip = match &row {
            NavRow::Worktree(i) => {
                let w = &self.worktrees[*i];
                format!(
                    "{}\n{}{}{}{}",
                    w.path.display(),
                    w.branch.as_deref().unwrap_or("Detached HEAD"),
                    if w.locked { " · locked" } else { "" },
                    if w.prunable { " · prunable" } else { "" },
                    if w.detached { " · detached" } else { "" }
                )
            }
            _ => name.clone(),
        };
        div()
            .id(("nav", index))
            .role(Role::ListBoxOption)
            .aria_label(tooltip.clone())
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .aria_selected(active)
            .w_full()
            .h(px(30.))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .overflow_hidden()
            .cursor_pointer()
            .bg(rgb(if active { SELECTED } else { PANEL }))
            .hover(|s| s.bg(rgb(HOVER)))
            .text_color(rgb(if active { MINT } else { TEXT }))
            .child(icon(symbol, 15., if active { MINT } else { MUTED }))
            .child(div().flex_1().truncate().child(name))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.limit = 500;
                match &row {
                    NavRow::All => {
                        if let Some(path) = this.path.clone() {
                            this.open(path, None, window, cx);
                        }
                    }
                    NavRow::Branch(i) => {
                        if let Some(path) = this.path.clone() {
                            let b = &this.branches[*i];
                            this.open(
                                path,
                                Some((
                                    b.name.clone(),
                                    worker::Scope::Branch {
                                        name: b.name.clone(),
                                        remote: b.remote,
                                    },
                                )),
                                window,
                                cx,
                            );
                        }
                    }
                    NavRow::Worktree(i) => {
                        let w = &this.worktrees[*i];
                        let scope = Some((
                            w.branch.clone().unwrap_or("Detached worktree".into()),
                            worker::Scope::Worktree {
                                path: w.path.clone(),
                            },
                        ));
                        this.open(path.clone().unwrap(), scope, window, cx);
                    }
                    NavRow::Section(..) => {}
                }
            }))
            .into_any_element()
    }
    fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let scope = self
            .scope
            .as_ref()
            .map(|s| s.0.clone())
            .unwrap_or("All history".into());
        let history = if let Some(error) = &self.error {
            if self.commits.is_empty() {
                empty("Could not open repository", error)
            } else {
                uniform_list(
                    "commits",
                    self.visible.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|i| this.render_commit_row(i, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .track_scroll(&self.history_scroll)
                .into_any_element()
            }
        } else if self.visible.is_empty() {
            empty(
                if self.loading.is_some() {
                    "Reading local history…"
                } else if self.repository.is_none() {
                    "Your history, at a glance"
                } else if !self.commits.is_empty() {
                    "No commits match this search"
                } else {
                    "No commits yet"
                },
                if self.repository.is_none() {
                    "Open a Git repository to explore branches, worktrees and changes."
                } else {
                    "Search applies to the loaded history."
                },
            )
        } else {
            uniform_list(
                "commits",
                self.visible.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|i| this.render_commit_row(i, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .size_full()
            .track_scroll(&self.history_scroll)
            .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .child(
                div()
                    .h(px(38.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_4()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(icon("branch", 15., MINT))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .max_w(px(450.))
                            .truncate()
                            .child(scope),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "{} / {} commits",
                                self.visible.len(),
                                self.commits.len()
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        button(
                            "load-more",
                            if self.limit >= 10000 {
                                "10,000 loaded limit"
                            } else {
                                "Load more"
                            },
                            "chevron",
                            false,
                        )
                        .disabled(
                            self.repository.is_none()
                                || self.commits.len() < self.limit
                                || self.limit >= 10000,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.limit = (this.limit + 500).min(10000);
                            if let Some(path) = this.path.clone() {
                                this.open(path, this.scope.clone(), window, cx);
                            }
                        })),
                    ),
            )
            .children(self.graph_notice.as_ref().map(|notice| {
                div()
                    .px_3()
                    .py_1()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(notice.clone())
            }))
            .child(
                div()
                    .h(px(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(div().w(px(self.graph_width)).flex_shrink_0().child(
                        if self.graph_notice.is_some() {
                            "GRAPH · NODES ONLY"
                        } else {
                            "GRAPH"
                        },
                    ))
                    .child(div().flex_1().child("COMMIT"))
                    .child(div().w(px(140.)).child("AUTHOR"))
                    .child(div().w(px(72.)).child("DATE"))
                    .child(div().w(px(65.)).child("SHA")),
            )
            .child(
                div()
                    .id("history-pane")
                    .role(Role::ListBox)
                    .aria_label("Commit history")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.focus)
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
                    .child(history),
            )
            .into_any_element()
    }
    fn render_commit_row(&self, position: usize, cx: &mut Context<Self>) -> AnyElement {
        let index = self.visible[position];
        let commit = &self.commits[index];
        let active = self.selected_commit == Some(index);
        let mut summary = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_1()
            .min_w_0()
            .overflow_hidden();
        if let Some(names) = self.refs.get(&commit.oid) {
            for name in names.iter().take(2) {
                summary = summary.child(
                    div()
                        .max_w(px(150.))
                        .truncate()
                        .text_size(px(10.))
                        .text_color(rgb(MINT))
                        .bg(rgb(SELECTED))
                        .px_1()
                        .rounded(px(3.))
                        .child(name.clone()),
                );
            }
            if names.len() > 2 {
                summary = summary.child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(MUTED))
                        .child(format!("+{}", names.len() - 2)),
                );
            }
        }
        summary = summary.child(div().flex_1().truncate().child(commit.subject.clone()));
        div()
            .id(("commit", index))
            .role(Role::ListBoxOption)
            .aria_label(format!(
                "{} · {} · {}",
                commit.subject,
                commit.author,
                short_oid(&commit.oid)
            ))
            .aria_selected(active)
            .w_full()
            .h(px(34.))
            .flex()
            .items_center()
            .px_3()
            .gap_0()
            .bg(rgb(if active { SELECTED } else { CANVAS }))
            .border_l_2()
            .border_color(rgb(if active { MINT } else { CANVAS }))
            .hover(|s| s.bg(rgb(HOVER)))
            .cursor_pointer()
            .child(graph::render(
                self.graph[index].clone(),
                self.graph_width,
                self.graph_lanes,
                active,
                commit.parents.len() > 1,
                self.visible.len() != self.commits.len() || self.graph_notice.is_some(),
            ))
            .child(summary)
            .child(
                div()
                    .w(px(140.))
                    .flex_shrink_0()
                    .pl_3()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(commit.author.clone()),
            )
            .child(
                div()
                    .w(px(72.))
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(short_date(commit.timestamp)),
            )
            .child(
                div()
                    .w(px(65.))
                    .flex_shrink_0()
                    .font_family(mono())
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(short_oid(&commit.oid)),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_commit(index, window, cx)))
            .into_any_element()
    }
    fn render_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(index) = self.selected_commit else {
            return empty(
                "Inspect a commit",
                "Select a commit to see its files, code and images.",
            );
        };
        let commit = &self.commits[index];
        let oid = commit.oid.clone();
        let message = format!("{}\n\n{}", commit.subject, commit.body);
        let mut parents = div().flex().gap_1();
        for (i, parent) in commit.parents.iter().enumerate() {
            parents = parents.child(
                button(
                    ("parent", i),
                    format!("Parent {} · {}", i + 1, short_oid(parent)),
                    "commit",
                    self.parent == i,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.change_parent(i, window, cx)),
                ),
            );
        }
        if commit.parents.is_empty() {
            parents = parents.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("Root commit · compared with empty tree"),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(commit.subject.clone()),
                            )
                            .child(button("message", "Message", "", self.details).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.details = !this.details;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                button("copy-commit", short_oid(&commit.oid), "copy", false)
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            oid.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "{}  ·  {}",
                                commit.author,
                                full_date(commit.timestamp)
                            ))
                            .child(div().flex_1())
                            .child(parents),
                    ),
            )
            .children(self.details.then(|| {
                div()
                    .id("commit-message")
                    .max_h(px(130.))
                    .overflow_y_scroll()
                    .p_3()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(12.))
                    .child(
                        button("copy-message", "Copy full message", "copy", false).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(message.clone()))
                            },
                        ),
                    )
                    .child(div().child(if commit.body.is_empty() {
                        "No extended commit message.".into()
                    } else {
                        commit.body.chars().take(8192).collect::<String>()
                    }))
            }))
            .child(
                h_resizable("inspector-panes")
                    .child(
                        resizable_panel()
                            .size(px(250.))
                            .size_range(px(180.)..px(600.))
                            .child(self.render_files(cx)),
                    )
                    .child(resizable_panel().child(self.render_preview(cx))),
            )
            .into_any_element()
    }
    fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(format!("CHANGED FILES   {}", self.files.len())),
            )
            .child(
                div()
                    .id("files-pane")
                    .role(Role::ListBox)
                    .aria_label("Changed files")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.file_focus)
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.pane = Pane::Files;
                            window.focus(&this.file_focus, cx);
                        }),
                    )
                    .child(if self.files.is_empty() {
                        empty(
                            self.loading.unwrap_or("No file changes"),
                            "Compared against the selected parent.",
                        )
                    } else {
                        uniform_list(
                            "files",
                            self.files.len(),
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|i| this.render_file_row(i, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.file_scroll)
                        .into_any_element()
                    }),
            )
            .into_any_element()
    }
    fn render_file_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let file = &self.files[index];
        let active = self.selected_file == Some(index);
        let path = file.path();
        let color = match file.status.letter() {
            "A" => MINT,
            "D" => 0xf29aa2,
            "R" => 0x9cb9f2,
            _ => 0xe9c17e,
        };
        div()
            .id(("file", index))
            .role(Role::ListBoxOption)
            .aria_label(format!("{} · {}", path.display(), file.status.label()))
            .aria_selected(active)
            .w_full()
            .h(px(48.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .bg(rgb(if active { SELECTED } else { PANEL }))
            .border_l_2()
            .border_color(rgb(if active { MINT } else { PANEL }))
            .hover(|s| s.bg(rgb(HOVER)))
            .cursor_pointer()
            .child(icon(
                if gitturtle_preview::is_image_path(path) {
                    "image"
                } else {
                    "code"
                },
                16.,
                MUTED,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().truncate().text_size(px(12.)).child(
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(
                                path.parent()
                                    .filter(|p| !p.as_os_str().is_empty())
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or("Repository root".into()),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(color))
                    .child(file.status.letter()),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, window, cx)))
            .into_any_element()
    }
    fn render_preview(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(index) = self.selected_file else {
            return empty(
                self.loading.unwrap_or("Choose a file"),
                "Text diffs and image comparisons appear here.",
            );
        };
        let file = &self.files[index];
        let path = file.path().to_string_lossy().into_owned();
        let copy_path = path.clone();
        let mut toolbar = div()
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(div().flex_1().truncate().text_size(px(11.)).child(path))
            .child(
                button("copy-path", "", "copy", false).on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()))
                }),
            );
        let content = if let Some(error) = &self.error {
            empty("Preview unavailable", error)
        } else if let Some(content) = &self.content {
            match content.as_ref() {
                Content::Text { patch, .. } => {
                    for (mode, name) in [
                        (TextMode::Unified, "Diff"),
                        (TextMode::Before, "Before"),
                        (TextMode::After, "After"),
                    ] {
                        toolbar =
                            toolbar.child(button(name, name, "", self.text_mode == mode).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    this.text_mode = mode;
                                    this.ensure_editor(window, cx);
                                    cx.notify();
                                }),
                            ));
                    }
                    let editor = match self.text_mode {
                        TextMode::Unified => &self.patch_editor,
                        TextMode::Before => &self.before_editor,
                        TextMode::After => &self.after_editor,
                    };
                    if self.text_mode == TextMode::Unified && patch.is_empty() {
                        empty("Content unchanged", "Only the file mode or path changed.")
                    } else if let Some(editor) = editor {
                        Editor::new(editor)
                            .h(relative(1.))
                            .readonly(true)
                            .bordered(false)
                            .aria_label("Read-only file comparison")
                            .text_size(px(12.))
                            .into_any_element()
                    } else {
                        empty("Loading text…", "")
                    }
                }
                Content::Images { old, new } => {
                    for (name, zoom) in [("Fit", 0.), ("50%", 0.5), ("100%", 1.), ("200%", 2.)] {
                        toolbar =
                            toolbar.child(button(name, name, "", self.zoom == zoom).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.zoom = zoom;
                                    cx.notify();
                                }),
                            ));
                    }
                    div()
                        .size_full()
                        .flex()
                        .child(self.render_image_side(0, old))
                        .child(self.render_image_side(1, new))
                        .into_any_element()
                }
                Content::Notice(message) => empty("File information", message),
            }
        } else {
            empty(self.loading.unwrap_or("No preview"), "")
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .child(toolbar)
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
            .child(
                div()
                    .h(px(24.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child(format!(
                        "{}   ·   {} → {}   ·   {} → {}",
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
                        file.old_oid.as_deref().map(short_oid).unwrap_or("—".into()),
                        file.new_oid.as_deref().map(short_oid).unwrap_or("—".into())
                    )),
            )
            .into_any_element()
    }
    fn render_image_side(&self, index: usize, side: &worker::ImageSide) -> AnyElement {
        let name = if index == 0 { "BEFORE" } else { "AFTER" };
        let details = side
            .image
            .as_ref()
            .map(|i| {
                format!(
                    "{} × {} · {} · {}{}",
                    i.original_width,
                    i.original_height,
                    i.format,
                    format_bytes(side.bytes),
                    if i.width != i.original_width || i.height != i.original_height {
                        format!(" · preview {} × {}", i.width, i.height)
                    } else {
                        String::new()
                    }
                )
            })
            .unwrap_or_default();
        let view = if let Some(image) = &self.images[index] {
            let preview = side.image.as_ref().unwrap();
            if self.zoom == 0. {
                div()
                    .size_full()
                    .p_4()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(image.clone())
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            } else {
                div()
                    .id(("image-scroll", index))
                    .size_full()
                    .overflow_scroll()
                    .track_scroll(&self.image_scroll)
                    .child(
                        div()
                            .w(px(self
                                .images
                                .iter()
                                .flatten()
                                .map(|i| i.size(0).width.0)
                                .max()
                                .unwrap_or(preview.width as i32)
                                as f32
                                * self.zoom))
                            .h(px(self
                                .images
                                .iter()
                                .flatten()
                                .map(|i| i.size(0).height.0)
                                .max()
                                .unwrap_or(preview.height as i32)
                                as f32
                                * self.zoom))
                            .child(
                                img(image.clone())
                                    .w(px(preview.width as f32 * self.zoom))
                                    .h(px(preview.height as f32 * self.zoom))
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .into_any_element()
            }
        } else {
            empty(
                if side.message.is_some() {
                    "Image unavailable"
                } else if index == 0 {
                    "Added image"
                } else {
                    "Deleted image"
                },
                side.message.as_deref().unwrap_or("This side has no image."),
            )
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(name)
                    .child(details),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .child(checkerboard())
                    .child(div().absolute().inset_0().child(view)),
            )
            .into_any_element()
    }
}

impl Render for GitTurtle {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_resizable("history-inspector")
            .child(
                resizable_panel()
                    .size(px(360.))
                    .size_range(px(130.)..px(1600.))
                    .child(self.render_history(cx)),
            )
            .child(resizable_panel().child(self.render_inspector(cx)));
        let workspace = if self.sidebar {
            h_resizable("workspace")
                .child(
                    resizable_panel()
                        .size(px(240.))
                        .size_range(px(190.)..px(480.))
                        .child(self.render_sidebar(cx)),
                )
                .child(resizable_panel().child(content))
                .into_any_element()
        } else {
            content.into_any_element()
        };
        div()
            .id("gitturtle")
            .key_context("GitTurtle")
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(CANVAS))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .on_action(cx.listener(Self::choose_repository))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::search))
            .on_action(cx.listener(Self::clear_search))
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
                this.pane = if this.pane == Pane::History {
                    Pane::Files
                } else {
                    Pane::History
                };
                window.focus(
                    if this.pane == Pane::History {
                        &this.focus
                    } else {
                        &this.file_focus
                    },
                    cx,
                );
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar = !this.sidebar;
                cx.notify();
            }))
            .child(self.render_header(cx))
            .child(div().flex_1().min_h_0().child(workspace))
            .child(
                div()
                    .h(px(26.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(PANEL))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(
                        div()
                            .size(px(5.))
                            .rounded_full()
                            .bg(rgb(if self.error.is_some() { 0xf29aa2 } else { MINT })),
                    )
                    .child(
                        div().flex_1().truncate().child(
                            self.error
                                .as_deref()
                                .unwrap_or(self.loading.unwrap_or(&self.status))
                                .to_string(),
                        ),
                    )
                    .child(format!(
                        "{}O Open   {}R Refresh   {}F Search   ↑↓ Navigate",
                        primary_label(),
                        primary_label(),
                        primary_label()
                    )),
            )
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
