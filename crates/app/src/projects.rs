use gpui_kit::component::{
    Disableable, Icon, Selectable, Sizable, Theme,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// User intent only. The application executes repository work in the background.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectEvent {
    Open(PathBuf),
    Clone {
        source: String,
        destination: PathBuf,
    },
    Create {
        destination: PathBuf,
        branch: String,
    },
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectMode {
    Open,
    Clone,
    Create,
}

pub struct ProjectHub {
    recent: Vec<PathBuf>,
    filtered: Vec<PathBuf>,
    search: Entity<InputState>,
    source: Entity<InputState>,
    parent: Entity<InputState>,
    name: Entity<InputState>,
    branch: Entity<InputState>,
    branch_edited: bool,
    pending_default_branch: Option<String>,
    // Displaying a path must not lose filename bytes selected by the native picker.
    picked_parent: Option<(String, PathBuf)>,
    suggested_name: Option<String>,
    mode: ProjectMode,
    pending_mode: Option<ProjectMode>,
    busy: bool,
    picker_pending: bool,
    can_go_back: bool,
    error: Option<String>,
    recent_scroll: UniformListScrollHandle,
    subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProjectEvent> for ProjectHub {}

impl ProjectHub {
    pub fn new(
        recent: Vec<PathBuf>,
        default_branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder("Find a project by name or folder"));
        let source =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://host/owner/project.git"));
        let parent = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Choose where your project will live")
        });
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("my-project"));
        let branch = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("main")
                .default_value(default_branch)
        });
        let recent = unique_recent(recent);
        let mut this = Self {
            filtered: recent.clone(),
            recent,
            search: search.clone(),
            source: source.clone(),
            parent: parent.clone(),
            name: name.clone(),
            branch: branch.clone(),
            branch_edited: false,
            pending_default_branch: None,
            picked_parent: None,
            suggested_name: None,
            mode: ProjectMode::Open,
            pending_mode: None,
            busy: false,
            picker_pending: false,
            can_go_back: false,
            error: None,
            recent_scroll: UniformListScrollHandle::new(),
            subscriptions: Vec::new(),
        };
        this.subscriptions.push(cx.subscribe_in(
            &search,
            window,
            |this, _, event, _, cx| match event {
                InputEvent::Change => this.filter_recent(cx),
                InputEvent::PressEnter { .. } => {
                    if let Some(path) = this.filtered.first().cloned() {
                        this.emit_operation(ProjectEvent::Open(path), cx);
                    }
                }
                _ => {}
            },
        ));
        this.subscriptions.push(
            cx.subscribe_in(&source, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    this.suggest_project_name(window, cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(cx),
                _ => {}
            }),
        );
        for input in [&parent, &name] {
            this.subscriptions
                .push(
                    cx.subscribe_in(input, window, |this, _, event, _, cx| match event {
                        InputEvent::Change => {
                            this.error = None;
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.submit(cx),
                        _ => {}
                    }),
                );
        }
        this.subscriptions.push(cx.subscribe_in(
            &branch,
            window,
            |this, _, event, _, cx| match event {
                InputEvent::Change => {
                    this.branch_edited = true;
                    this.pending_default_branch = None;
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(cx),
                _ => {}
            },
        ));
        this
    }

    pub fn set_recent(&mut self, recent: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.recent = unique_recent(recent);
        self.filter_recent(cx);
    }

    pub fn set_default_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        if !self.branch_edited {
            // InputState needs a Window; apply this during the next render.
            self.pending_default_branch = Some(branch);
            cx.notify();
        }
    }

    /// The hub sets this before emitting an operation; the owner clears it when finished.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        if !busy {
            self.pending_mode = None;
        }
        cx.notify();
    }

    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    pub fn set_can_go_back(&mut self, can_go_back: bool, cx: &mut Context<Self>) {
        self.can_go_back = can_go_back;
        cx.notify();
    }

    fn unavailable(&self) -> bool {
        self.busy || self.picker_pending
    }

    fn busy_label(&self) -> &'static str {
        match self.pending_mode.unwrap_or(self.mode) {
            ProjectMode::Open => "Opening project…",
            ProjectMode::Clone => "Cloning project…",
            ProjectMode::Create => "Creating project…",
        }
    }

    fn filter_recent(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_lowercase();
        self.filtered = self
            .recent
            .iter()
            .filter(|path| path.to_string_lossy().to_lowercase().contains(&query))
            .cloned()
            .collect();
        self.recent_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unavailable() {
            return;
        }
        self.search.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.filter_recent(cx);
    }

    fn suggest_project_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.name.read(cx).value();
        if !current.is_empty() && self.suggested_name.as_deref() != Some(current.as_ref()) {
            return;
        }
        if let Some(name) = suggested_project_name(self.source.read(cx).value().as_ref()) {
            self.name
                .update(cx, |input, cx| input.set_value(name.clone(), window, cx));
            self.suggested_name = Some(name);
        }
    }

    fn change_mode(&mut self, mode: ProjectMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.unavailable() {
            return;
        }
        self.mode = mode;
        self.error = None;
        match mode {
            ProjectMode::Open => self.search.update(cx, |input, cx| input.focus(window, cx)),
            ProjectMode::Clone => self.source.update(cx, |input, cx| input.focus(window, cx)),
            ProjectMode::Create => self.name.update(cx, |input, cx| input.focus(window, cx)),
        }
        cx.notify();
    }

    fn emit_operation(&mut self, event: ProjectEvent, cx: &mut Context<Self>) {
        if self.unavailable() {
            return;
        }
        self.pending_mode = Some(match &event {
            ProjectEvent::Open(_) => ProjectMode::Open,
            ProjectEvent::Clone { .. } => ProjectMode::Clone,
            ProjectEvent::Create { .. } => ProjectMode::Create,
            ProjectEvent::Back => return,
        });
        self.error = None;
        self.busy = true;
        cx.emit(event);
        cx.notify();
    }

    fn choose_folder(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.unavailable() {
            return;
        }
        self.picker_pending = true;
        self.error = None;
        cx.notify();
        let response = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                if open {
                    "Open project"
                } else {
                    "Choose parent folder"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let response = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.picker_pending = false;
                match response {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            if open {
                                this.emit_operation(ProjectEvent::Open(path), cx);
                            } else {
                                let display = path.to_string_lossy().into_owned();
                                this.parent.update(cx, |input, cx| {
                                    input.set_value(display.clone(), window, cx)
                                });
                                this.picked_parent = Some((display, path));
                                this.name.update(cx, |input, cx| input.focus(window, cx));
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        this.error = Some(format!("Could not choose a folder: {error}"))
                    }
                    Err(_) => {
                        this.error =
                            Some("The folder picker closed unexpectedly. Please try again.".into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn destination(&self, cx: &App) -> Result<PathBuf, String> {
        let value = self.parent.read(cx).value();
        let parent = self
            .picked_parent
            .as_ref()
            .filter(|(display, _)| display == value.as_ref())
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| PathBuf::from(value.as_ref()));
        validate_destination(&parent, self.name.read(cx).value().as_ref())
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.unavailable() || self.mode == ProjectMode::Open {
            return;
        }
        let operation = (|| {
            let destination = self.destination(cx)?;
            match self.mode {
                ProjectMode::Clone => {
                    let source = self.source.read(cx).value().trim().to_string();
                    validate_source(&source)?;
                    Ok(ProjectEvent::Clone {
                        source,
                        destination,
                    })
                }
                ProjectMode::Create => {
                    let branch = self.branch.read(cx).value().trim().to_string();
                    validate_branch(&branch)?;
                    Ok(ProjectEvent::Create {
                        destination,
                        branch,
                    })
                }
                ProjectMode::Open => unreachable!(),
            }
        })();
        match operation {
            Ok(event) => self.emit_operation(event, cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn recent_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = Theme::global(cx).colors;
        let palette = crate::appearance::palette(cx);
        let path = self.filtered[index].clone();
        let display = path.display().to_string();
        let location = path.parent().unwrap_or(&path).display().to_string();
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        Button::new(("recent-project", index))
            .ghost()
            .h(crate::appearance::ui_size(76.))
            .w_full()
            .px_4()
            .rounded(px(12.))
            .disabled(self.unavailable())
            .accessibility_label(format!("Open {name}, {display}"))
            .tooltip(display.clone())
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(42.))
                            .flex_shrink_0()
                            .rounded(px(12.))
                            .bg(rgb(palette.selected))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(hub_icon("folder", 19., colors.primary)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_left()
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .text_color(colors.muted_foreground)
                                    .truncate()
                                    .child(location),
                            ),
                    )
                    .child(hub_icon("chevron", 14., colors.muted_foreground)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.emit_operation(ProjectEvent::Open(path.clone()), cx)
            }))
            .into_any_element()
    }

    fn render_recent(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = Theme::global(cx).colors;
        let query = self.search.read(cx).value();
        let searching = !query.trim().is_empty();
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .when(compact, |panel| panel.flex_none().w_full().h(px(360.)))
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div().flex().items_center().gap_2()
                            .child(hub_icon("clock", 16., colors.muted_foreground))
                            .child(div().text_size(crate::appearance::ui_text(15.)).font_weight(FontWeight::SEMIBOLD).child("Recently opened")),
                    )
                    .child(
                        div().px_2().py_0p5().rounded(px(6.))
                            .bg(colors.secondary).text_size(crate::appearance::ui_text(11.))
                            .text_color(colors.muted_foreground)
                            .child(if searching { format!("{} of {}", self.filtered.len(), self.recent.len()) } else { self.recent.len().to_string() }),
                    ),
            )
            .child(Input::new(&self.search).disabled(self.unavailable()).prefix(Icon::default().path("icons/search.svg").size(px(15.)))
                .when(!query.is_empty(), |input| input.suffix(
                    Button::new("hub-clear-search-input")
                        .icon(Icon::default().path("icons/close.svg"))
                        .text().xsmall()
                        .accessibility_label("Clear project search")
                        .tooltip("Clear project search")
                        .disabled(self.unavailable())
                        .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))),
                )))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .p_2()
                    .when(self.filtered.is_empty(), |panel| {
                        panel.child(
                            div()
                                .size_full()
                                .p_6()
                                .flex()
                                .flex_col()
                                .justify_center()
                                .items_center()
                                .gap_3()
                                .text_center()
                                .child(hub_icon("folder", 28., colors.muted_foreground))
                                .child(div().text_size(crate::appearance::ui_text(15.)).font_weight(FontWeight::MEDIUM).child(if self.recent.is_empty() { "Your next project starts here" } else { "No matching projects" }))
                                .child(div().max_w(px(240.)).text_size(crate::appearance::ui_text(12.)).line_height(relative(1.5)).text_color(colors.muted_foreground).child(if self.recent.is_empty() { "Open, clone, or create a repository. It will be waiting here next time." } else { "Try another project name or folder." }))
                                .when(searching, |empty| empty.child(Button::new("hub-clear-search").label("Clear search").disabled(self.unavailable()).on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))))),
                        )
                    })
                    .when(!self.filtered.is_empty(), |panel| {
                        panel.child(
                            uniform_list("recent-projects", self.filtered.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|index| this.recent_row(index, cx)).collect::<Vec<_>>()
                            }))
                            .size_full()
                            .track_scroll(&self.recent_scroll),
                        )
                    }),
            )
            .into_any_element()
    }

    fn field(&self, label: &'static str, input: &Entity<InputState>, cx: &App) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Theme::global(cx).colors.foreground)
                    .child(label),
            )
            .child(Input::new(input).disabled(self.unavailable()))
            .into_any_element()
    }

    fn render_action(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = Theme::global(cx).colors;
        let palette = crate::appearance::palette(cx);
        let (title, description, symbol, tint) = match self.mode {
            ProjectMode::Open => (
                "Open a repository",
                "Choose a project on your computer to explore its history and changes.",
                "folder",
                palette.accent,
            ),
            ProjectMode::Clone => (
                "Clone a repository",
                "Bring a repository from a remote URL or local path into a new folder.",
                "remote",
                palette.hunk,
            ),
            ProjectMode::Create => (
                "Create a repository",
                "Start a project in a new folder, ready for your first commit.",
                "branch-add",
                palette.renamed,
            ),
        };
        div()
            .w(px(400.))
            .flex_shrink_0()
            .h_full()
            .when(compact, |panel| panel.w_full().h_auto())
            .flex()
            .flex_col()
            .rounded(px(18.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.sidebar)
            .p_5()
            .gap_5()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .bg(colors.background)
                    .rounded(px(9.))
                    .p_1()
                    .children(
                        [
                            (ProjectMode::Open, "Open", "folder"),
                            (ProjectMode::Clone, "Clone", "remote"),
                            (ProjectMode::Create, "Create", "branch-add"),
                        ]
                        .map(|(mode, label, symbol)| {
                            Button::new(label)
                                .ghost()
                                .flex_1()
                                .h(crate::appearance::ui_size(32.))
                                .rounded(px(7.))
                                .label(label)
                                .icon(
                                    Icon::default()
                                        .path(format!("icons/{symbol}.svg"))
                                        .size(px(14.)),
                                )
                                .selected(self.mode == mode)
                                .toggled(self.mode == mode)
                                .disabled(self.unavailable())
                                .when(self.mode == mode && !self.unavailable(), |button| {
                                    button.hover(|style| style.opacity(0.9))
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.change_mode(mode, window, cx)
                                }))
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(42.))
                                    .flex_shrink_0()
                                    .rounded(px(12.))
                                    .bg(rgba((tint << 8) | 0x18))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(hub_icon(symbol, 21., rgb(tint).into())),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(19.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(12.))
                            .line_height(relative(1.55))
                            .text_color(colors.muted_foreground)
                            .child(description),
                    ),
            )
            .when(self.mode == ProjectMode::Open, |panel| {
                panel
                    .child(
                        Button::new("hub-open-folder")
                            .primary()
                            .h(crate::appearance::ui_size(40.))
                            .w_full()
                            .label(if self.busy {
                                self.busy_label()
                            } else {
                                "Choose a repository…"
                            })
                            .icon(Icon::default().path("icons/folder.svg"))
                            .disabled(self.unavailable())
                            .loading(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_folder(true, window, cx)
                            })),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(colors.muted_foreground)
                            .child("You can also open any project from the recent list."),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(colors.border)
                            .pt_5()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .children(
                                [
                                    (
                                        "clock",
                                        "Explore history",
                                        "Follow branches and inspect any commit.",
                                    ),
                                    (
                                        "changes",
                                        "Review and commit",
                                        "Compare files, stage changes, and commit.",
                                    ),
                                ]
                                .map(|(symbol, title, detail)| {
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap_3()
                                        .child(hub_icon(symbol, 16., colors.muted_foreground))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(crate::appearance::ui_text(12.))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::appearance::ui_text(11.))
                                                        .line_height(relative(1.5))
                                                        .text_color(colors.muted_foreground)
                                                        .child(detail),
                                                ),
                                        )
                                }),
                            ),
                    )
            })
            .when(self.mode != ProjectMode::Open, |panel| {
                panel
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .when(self.mode == ProjectMode::Clone, |form| {
                                form.child(self.field(
                                    "Repository URL or local path",
                                    &self.source,
                                    cx,
                                ))
                            })
                            .child(self.field("Project folder name", &self.name, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(crate::appearance::ui_text(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("Parent folder"),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .child(
                                                div().flex_1().min_w_0().child(
                                                    Input::new(&self.parent)
                                                        .disabled(self.unavailable()),
                                                ),
                                            )
                                            .child(
                                                Button::new("hub-choose-parent")
                                                    .label("Browse…")
                                                    .icon(
                                                        Icon::default()
                                                            .path("icons/folder.svg")
                                                            .size(px(14.)),
                                                    )
                                                    .disabled(self.unavailable())
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.choose_folder(false, window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .when(self.mode == ProjectMode::Create, |form| {
                                form.child(self.field("Initial branch", &self.branch, cx))
                            }),
                    )
                    .when_some(self.destination(cx).ok(), |panel, destination| {
                        let display = destination.display().to_string();
                        let tooltip = display.clone();
                        panel.child(
                            div()
                                .id("hub-destination-preview")
                                .tooltip(move |window, cx| {
                                    Tooltip::new(tooltip.clone()).build(window, cx)
                                })
                                .min_w_0()
                                .rounded(px(8.))
                                .bg(colors.background)
                                .px_3()
                                .py_2()
                                .flex()
                                .items_start()
                                .gap_2()
                                .child(hub_icon("folder", 14., colors.muted_foreground))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(crate::appearance::ui_text(10.))
                                                .text_color(colors.muted_foreground)
                                                .child("New repository folder"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::appearance::ui_text(11.))
                                                .truncate()
                                                .child(display),
                                        ),
                                ),
                        )
                    })
                    .child(
                        Button::new("hub-submit")
                            .primary()
                            .h(crate::appearance::ui_size(40.))
                            .w_full()
                            .icon(
                                Icon::default()
                                    .path(format!("icons/{symbol}.svg"))
                                    .size(px(16.)),
                            )
                            .disabled(self.unavailable())
                            .loading(self.busy)
                            .label(if self.busy {
                                self.busy_label()
                            } else if self.mode == ProjectMode::Clone {
                                "Clone project"
                            } else {
                                "Create project"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    )
            })
            .when_some(self.error.as_ref(), |panel, error| {
                panel.child(
                    div()
                        .id("project-hub-error")
                        .max_h(px(110.))
                        .overflow_y_scroll()
                        .flex_shrink_0()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(colors.danger)
                        .bg(rgb(palette.removed_background))
                        .p_3()
                        .line_height(relative(1.5))
                        .text_size(crate::appearance::ui_text(12.))
                        .text_color(colors.danger)
                        .child(error.clone()),
                )
            })
            .into_any_element()
    }
}

impl Render for ProjectHub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(branch) = self.pending_default_branch.take() {
            self.branch
                .update(cx, |input, cx| input.set_value(branch, window, cx));
        }
        let colors = Theme::global(cx).colors;
        let compact = window.viewport_size().width < px(900.);
        let narrow = window.viewport_size().width < px(640.);
        div()
            .size_full()
            .bg(colors.background)
            .text_color(colors.foreground)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(crate::appearance::ui_size(56.))
                    .px_4()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(colors.border)
                    .when(self.can_go_back, |header| {
                        header.child(Button::new("hub-back").ghost().label("Back to repository").icon(Icon::default().path("icons/arrow-left.svg").size(px(15.))).disabled(self.unavailable()).on_click(cx.listener(|this, _, _, cx| {
                            if !this.unavailable() {
                                cx.emit(ProjectEvent::Back);
                            }
                        })))
                    })
                    .child(crate::app_icon(32.))
                    .child(div().text_size(crate::appearance::ui_text(16.)).font_weight(FontWeight::SEMIBOLD).child("GitTurtle"))
                    .child(div().w(px(1.)).h(crate::appearance::ui_size(18.)).bg(colors.border))
                    .child(div().text_size(crate::appearance::ui_text(12.)).text_color(colors.muted_foreground).child("Projects"))
                    .child(div().flex_1()),
            )
            .child(
                div()
                    .id("project-hub-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px_6()
                    .py_8()
                    .when(narrow, |body| body.px_4().py_5())
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1060.))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap_6()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(div().text_size(crate::appearance::ui_text(32.)).font_weight(FontWeight::SEMIBOLD).child("Your projects"))
                                    .child(div().text_size(crate::appearance::ui_text(14.)).text_color(colors.muted_foreground).child("Pick up where you left off, or start something new.")),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(if self.error.is_some() { 750. } else if self.mode == ProjectMode::Open { 590. } else { 650. }))
                                    .flex_shrink_0()
                                    .flex()
                                    .gap_6()
                                    .when(compact, |body| body.flex_col().h_auto().child(self.render_action(true, cx)).child(self.render_recent(true, cx)))
                                    .when(!compact, |body| body.child(self.render_recent(false, cx)).child(self.render_action(false, cx))),
                            ),
                    ),
            )
    }
}

fn hub_icon(name: &str, dimension: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(dimension))
        .text_color(color)
        .flex_shrink_0()
}

fn unique_recent(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn suggested_project_name(source: &str) -> Option<String> {
    let source = source
        .trim()
        .split(['?', '#'])
        .next()?
        .trim_end_matches('/');
    let name = source.rsplit(['/', ':']).next()?.trim_end_matches(".git");
    if name.is_empty() || matches!(name, "." | "..") || name.chars().any(char::is_control) {
        None
    } else {
        Some(name.to_string())
    }
}

fn validate_destination(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if parent.as_os_str().is_empty() {
        return Err("Choose a parent folder for your project.".into());
    }
    if !parent.is_absolute() {
        return Err("Use an absolute parent-folder path, or choose one with Browse.".into());
    }
    if name.trim().is_empty() {
        return Err("Give your project folder a name.".into());
    }
    if matches!(name, "." | "..") || name.contains('/') || name.chars().any(char::is_control) {
        return Err("Use a single folder name without slashes or control characters.".into());
    }
    Ok(parent.join(name))
}

fn validate_source(source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Err("Enter a repository URL or local path to clone.".into());
    }
    if source.starts_with('-') || source.chars().any(char::is_control) {
        return Err(
            "Enter a repository URL or path without control characters or a leading dash.".into(),
        );
    }
    Ok(())
}

fn validate_branch(branch: &str) -> Result<(), String> {
    if branch.is_empty() {
        return Err("Enter a name for the initial branch.".into());
    }
    if branch.starts_with('-')
        || matches!(branch, "@" | "HEAD")
        || branch.ends_with('.')
        || branch.contains("..")
        || branch.contains("@{")
        || branch
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        || branch
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
    {
        return Err("Use a valid branch name, such as main or feature/start.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn clone_names_support_https_ssh_and_local_sources() {
        for source in [
            "https://example.com/team/turtle.git",
            "git@example.com:team/turtle.git",
            "/tmp/turtle.git/",
            "https://example.com/team/turtle.git?ref=main",
        ] {
            assert_eq!(suggested_project_name(source).as_deref(), Some("turtle"));
        }
        assert_eq!(suggested_project_name("../.."), None);
    }

    #[test]
    fn clone_source_accepts_git_transports_and_rejects_empty_or_hidden_input() {
        for source in [
            "https://example.com/team/turtle.git",
            "git@example.com:team/turtle.git",
            "/tmp/local repo",
        ] {
            assert!(validate_source(source).is_ok());
        }
        for source in [
            "",
            "--upload-pack=command",
            "host/repo\nother",
            "host/repo\0",
        ] {
            assert!(validate_source(source).is_err());
        }
    }

    #[test]
    fn destination_keeps_the_project_under_the_chosen_parent() {
        assert_eq!(
            validate_destination(Path::new("/tmp/projects"), "my project").unwrap(),
            PathBuf::from("/tmp/projects/my project")
        );
        for name in ["", " ", ".", "..", "../outside", "/absolute", "bad\0name"] {
            assert!(validate_destination(Path::new("/tmp/projects"), name).is_err());
        }
        assert!(validate_destination(Path::new("relative"), "project").is_err());
    }

    #[test]
    fn initial_branch_rejects_invalid_refs_before_submission() {
        for branch in ["main", "feature/start", "release-1.0"] {
            assert!(validate_branch(branch).is_ok());
        }
        for branch in [
            "",
            "HEAD",
            "@",
            "-main",
            "two words",
            "main..next",
            "main@{1}",
            ".hidden",
            "feature/.hidden",
            "main.lock",
            "one//two",
            "one/",
            "one\\two",
        ] {
            assert!(validate_branch(branch).is_err(), "{branch}");
        }
    }

    #[test]
    fn recent_projects_keep_order_without_duplicate_paths() {
        let recent = unique_recent(vec!["/tmp/b".into(), "/tmp/a".into(), "/tmp/b".into()]);
        assert_eq!(
            recent,
            vec![PathBuf::from("/tmp/b"), PathBuf::from("/tmp/a")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn chosen_destination_preserves_non_utf8_parent_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let parent = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/project-\xff".to_vec()));
        assert_eq!(
            validate_destination(&parent, "child").unwrap(),
            parent.join("child")
        );
    }
}
