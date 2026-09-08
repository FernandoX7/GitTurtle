//! Contextual revision browsing with immutable anchors and retained UI context.
//! Git history and preview preparation use the existing replaceable worker.
use crate::*;
use gitturtle_core::{FileHistoryEntry, FileHistoryPage};
use gpui_kit::prelude::FluentBuilder;

const PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    First,
    Last,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageRequest {
    generation: u64,
    offset: usize,
    position: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Revision(usize),
    Page(usize, Position),
}

struct Lineage {
    repository: PathBuf,
    anchor: String,
    path: PathBuf,
    page: Option<FileHistoryPage>,
    offset: usize,
    selected: Option<usize>,
    generation: u64,
    pending: Option<PageRequest>,
    last_request: Option<PageRequest>,
    error: Option<String>,
}

impl Lineage {
    fn new(repository: PathBuf, anchor: String, path: PathBuf) -> Self {
        Self {
            repository,
            anchor,
            path,
            page: None,
            offset: 0,
            selected: None,
            generation: 0,
            pending: None,
            last_request: None,
            error: None,
        }
    }

    fn begin(&mut self, offset: usize, position: Position) -> PageRequest {
        self.generation = self.generation.wrapping_add(1);
        let request = PageRequest {
            generation: self.generation,
            offset,
            position,
        };
        self.pending = Some(request);
        self.last_request = Some(request);
        self.error = None;
        request
    }

    fn accept(&mut self, request: PageRequest, page: FileHistoryPage) -> bool {
        if self.pending != Some(request) || page.anchor != self.anchor || page.path != self.path {
            return false;
        }
        self.selected = match request.position {
            Position::First => (!page.entries.is_empty()).then_some(0),
            Position::Last => page.entries.len().checked_sub(1),
        };
        self.offset = request.offset;
        self.page = Some(page);
        self.pending = None;
        self.error = None;
        true
    }

    fn fail(&mut self, request: PageRequest, message: String) {
        if self.pending == Some(request) {
            self.pending = None;
            self.error = Some(message);
        }
    }

    fn entry(&self) -> Option<&FileHistoryEntry> {
        self.page.as_ref()?.entries.get(self.selected?)
    }

    fn destination(&self, direction: i32, edge: bool) -> Option<Destination> {
        if self.pending.is_some() {
            return None;
        }
        let page = self.page.as_ref()?;
        if page.entries.is_empty() {
            return None;
        }
        let current = self.selected.unwrap_or(0);
        if edge {
            return Some(Destination::Revision(if direction < 0 {
                0
            } else {
                page.entries.len() - 1
            }));
        }
        if direction < 0 {
            if current > 0 {
                Some(Destination::Revision(current - 1))
            } else {
                (self.offset > 0).then(|| {
                    Destination::Page(self.offset.saturating_sub(PAGE_SIZE), Position::Last)
                })
            }
        } else if current + 1 < page.entries.len() {
            Some(Destination::Revision(current + 1))
        } else {
            page.next_offset
                .map(|offset| Destination::Page(offset, Position::First))
        }
    }
}

/// Holds the existing editors and scroll handles, so closing revision history
/// restores selection and viewport without rebuilding the previous preview.
struct ReturnContext {
    files: Vec<FileChange>,
    selected_file: Option<usize>,
    preferred_file: Option<PathBuf>,
    content: Option<Arc<Content>>,
    patch_editor: Option<Entity<EditorState>>,
    patch_decoration: Option<editor_find::PatchDecorations>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    partial_subscription: Option<Subscription>,
    split_view: Option<Entity<split_diff::SplitView>>,
    before_editor: Option<Entity<EditorState>>,
    after_editor: Option<Entity<EditorState>>,
    images: [Option<Arc<RenderImage>>; 2],
    image_scroll: ScrollHandle,
    zoom: f32,
    text_mode: TextMode,
    mode: WorkspaceMode,
    pane: Pane,
    sidebar: bool,
    history_sidebar: bool,
    focus: Option<FocusHandle>,
    status: String,
    error: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    lineage: Option<Lineage>,
    retained: Option<Box<ReturnContext>>,
    task: Option<Task<()>>,
    scroll: UniformListScrollHandle,
}

impl State {
    pub(super) fn is_active(&self) -> bool {
        self.retained.is_some()
    }

    pub(super) fn refresh_theme(&self, cx: &mut App) {
        let Some(retained) = &self.retained else {
            return;
        };
        if let (Some(collection), Some(Content::Text { presentation, .. })) =
            (&retained.patch_decoration, retained.content.as_deref())
        {
            text::refresh_theme(collection, presentation, cx);
        }
        if let Some(split) = &retained.split_view {
            split.update(cx, |view, cx| view.refresh_theme(cx));
        }
    }
}

impl GitTurtle {
    /// A page may finish while Settings or Projects is visible. Accept its
    /// metadata then, but construct/focus the selected preview only on return.
    pub(super) fn resume_file_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lineage) = &self.file_history.lineage else {
            return;
        };
        if self.page != AppPage::Repository || lineage.pending.is_some() {
            return;
        }
        if let (Some(index), Some(entry)) = (lineage.selected, lineage.entry())
            && (self.files.first() != Some(&entry.change) || self.content.is_none())
        {
            self.select_file_revision(index, window, cx);
        }
    }

    pub(super) fn open_file_history(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository
            || self.mode == WorkspaceMode::Working
            || self.file_history.is_active()
            || self.operation_busy.is_some()
        {
            return;
        }
        let (Some(repo), Some(commit), Some(file)) = (
            self.repository.as_ref(),
            self.selected_commit
                .and_then(|index| self.commits.get(index)),
            self.files.get(index),
        ) else {
            return;
        };
        let lineage = Lineage::new(
            repo.path().to_owned(),
            commit.oid.clone(),
            file.path().to_owned(),
        );
        self.invalidate_read();
        let retained = ReturnContext {
            files: std::mem::take(&mut self.files),
            selected_file: self.selected_file.take(),
            preferred_file: self.preferred_file.take(),
            content: self.content.take(),
            patch_editor: self.patch_editor.take(),
            patch_decoration: self.patch_decoration.take(),
            patch_view: self.patch_view.take(),
            partial_subscription: self.partial_subscription.take(),
            split_view: self.split_view.take(),
            before_editor: self.before_editor.take(),
            after_editor: self.after_editor.take(),
            images: std::mem::take(&mut self.images),
            image_scroll: std::mem::take(&mut self.image_scroll),
            zoom: self.zoom,
            text_mode: self.text_mode,
            mode: self.mode,
            pane: self.pane,
            sidebar: self.sidebar,
            history_sidebar: self.history_sidebar,
            focus: if self.app_focus.contains_focused(window, cx) {
                window.focused(cx)
            } else {
                // A contextual popup owns temporary focus outside the app's
                // tree; retain the underlying list instead of that popup.
                Some(if self.pane == Pane::Files {
                    self.file_focus.clone()
                } else {
                    self.focus.clone()
                })
            },
            status: self.status.clone(),
            error: self.error.take(),
        };
        self.clear_preview();
        self.file_history = State {
            lineage: Some(lineage),
            retained: Some(Box::new(retained)),
            ..Default::default()
        };
        self.mode = WorkspaceMode::Compare;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.load_file_history_page(0, Position::First, window, cx);
    }

    /// Returns true if Back handled a transient file-history context.
    pub(super) fn close_file_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(retained) = self.file_history.retained.take() else {
            return false;
        };
        self.invalidate_read();
        self.file_history = State::default();
        self.clear_preview();
        let retained = *retained;
        self.files = retained.files;
        self.selected_file = retained.selected_file;
        self.preferred_file = retained.preferred_file;
        self.content = retained.content;
        self.patch_editor = retained.patch_editor;
        self.patch_decoration = retained.patch_decoration;
        self.patch_view = retained.patch_view;
        self.partial_subscription = retained.partial_subscription;
        self.split_view = retained.split_view;
        self.before_editor = retained.before_editor;
        self.after_editor = retained.after_editor;
        self.images = retained.images;
        self.image_scroll = retained.image_scroll;
        self.zoom = retained.zoom;
        self.text_mode = retained.text_mode;
        self.mode = retained.mode;
        self.pane = retained.pane;
        self.sidebar = retained.sidebar;
        self.history_sidebar = retained.history_sidebar;
        self.status = retained.status;
        self.error = retained.error;
        if let Some(focus) = retained.focus {
            window.focus(&focus, cx);
        }
        if self.mode == WorkspaceMode::Compare
            && self.content.is_none()
            && self.error.is_none()
            && let Some(index) = self.selected_file
        {
            self.load_file(index, window, cx);
        }
        // Page transitions and explicit writes may close this pane too. Wait
        // until their current handler finishes before resuming queued changes.
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| this.try_automatic_refresh(window, cx));
        });
        cx.notify();
        true
    }

    fn load_file_history_page(
        &mut self,
        offset: usize,
        position: Position,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let Some(lineage) = self.file_history.lineage.as_ref() else {
            return;
        };
        if repo.path() != lineage.repository || self.page != AppPage::Repository {
            return;
        }
        self.invalidate_read();
        let lineage = self.file_history.lineage.as_mut().unwrap();
        let request = lineage.begin(offset, position);
        let repository = lineage.repository.clone();
        let anchor = lineage.anchor.clone();
        let path = lineage.path.clone();
        let generation = self.generation;
        let response = self.worker.submit(Job::FileHistory {
            repo,
            anchor: anchor.clone(),
            path: path.clone(),
            offset,
            limit: PAGE_SIZE,
        });
        self.file_history.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!("File history was interrupted. Retry this page."))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                let Some(lineage) = this.file_history.lineage.as_mut() else {
                    return;
                };
                if this.path.as_ref() != Some(&repository)
                    || lineage.repository != repository
                    || lineage.anchor != anchor
                    || lineage.path != path
                    || lineage.pending != Some(request)
                {
                    return;
                }
                this.file_history.task = None;
                let selected = match result {
                    Ok(Output::FileHistory(page)) => {
                        if lineage.accept(request, page) {
                            lineage.selected
                        } else {
                            lineage.fail(request, "The returned file history did not match this revision. Retry this page.".into());
                            None
                        }
                    }
                    Ok(_) => {
                        lineage.fail(request, "The file-history read returned a different result. Retry this page.".into());
                        None
                    }
                    Err(error) => {
                        lineage.fail(request, format!("{error:#}"));
                        None
                    }
                };
                if let Some(selected) = selected && this.page == AppPage::Repository {
                    this.select_file_revision(selected, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_file_revision(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lineage) = self.file_history.lineage.as_mut() else {
            return;
        };
        if lineage.pending.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(entry) = lineage
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
        else {
            return;
        };
        let change = entry.change.clone();
        lineage.selected = Some(index);
        lineage.error = None;
        self.file_history
            .scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.files = vec![change];
        self.selected_file = None;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.load_file(0, window, cx);
    }

    pub(super) fn move_file_history_revision(
        &mut self,
        direction: i32,
        edge: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.file_history.is_active() {
            return false;
        }
        let destination = self
            .file_history
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.destination(direction, edge));
        match destination {
            Some(Destination::Revision(index)) => self.select_file_revision(index, window, cx),
            Some(Destination::Page(offset, position)) => {
                self.load_file_history_page(offset, position, window, cx)
            }
            None => {}
        }
        true
    }

    pub(super) fn render_file_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(lineage) = self.file_history.lineage.as_ref() else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let busy = lineage.pending.is_some();
        let newer = lineage.destination(-1, false).is_some();
        let older = lineage.destination(1, false).is_some();
        let count = lineage.page.as_ref().map_or(0, |page| page.entries.len());
        let next_page = lineage.page.as_ref().and_then(|page| page.next_offset);
        let previous_page = lineage.offset.checked_sub(PAGE_SIZE);
        let retry = lineage.last_request;
        let entry = lineage.entry();
        let copy_path = lineage.path.display().to_string();
        let path = copy_path.clone();
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(p.panel))
            .border_l_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("File history"),
                            )
                            .child(
                                button("close-file-history", "Back", "arrow-left", false)
                                    .accessibility_label(
                                        "Close file history and restore previous view",
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_file_history(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("file-history-anchor-path")
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                            .child(copy_path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(10.))
                                    .text_color(rgb(p.muted))
                                    .child(format!(
                                        "From {} · first-parent lineage",
                                        short_oid(&lineage.anchor)
                                    )),
                            )
                            .child(
                                button("copy-history-path", "", "copy", false)
                                    .tooltip("Copy file history's anchor path")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            copy_path.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(p.muted))
                            .child("Follows renames and the first parent of each merge."),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                button("newer-file-revision", "Newer", "", false)
                                    .disabled(!newer)
                                    .accessibility_label("Previous newer file revision")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.move_file_history_revision(-1, false, window, cx);
                                    })),
                            )
                            .child(
                                button("older-file-revision", "Older", "", false)
                                    .disabled(!older)
                                    .accessibility_label("Next older file revision")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.move_file_history_revision(1, false, window, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(10.))
                                    .text_color(rgb(p.muted))
                                    .child(lineage.selected.map_or(String::new(), |index| {
                                        format!("Revision {}", lineage.offset + index + 1)
                                    })),
                            ),
                    ),
            )
            .children(entry.map(|entry| self.render_revision_details(entry, cx)))
            .children(lineage.error.as_ref().map(|error| {
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .bg(rgb(p.removed_background))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .id("file-history-error")
                            .max_h(px(100.))
                            .overflow_y_scroll()
                            .text_size(px(11.))
                            .text_color(rgb(p.warning))
                            .child(error.clone()),
                    )
                    .child(
                        button("retry-file-history", "Retry page", "refresh", false)
                            .disabled(busy || retry.is_none())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(request) = retry {
                                    this.load_file_history_page(
                                        request.offset,
                                        request.position,
                                        window,
                                        cx,
                                    );
                                }
                            })),
                    )
            }))
            .child(
                div()
                    .id("file-revision-list")
                    .role(Role::ListBox)
                    .aria_label("File revisions, newest first")
                    .tab_stop(true)
                    .key_context("GitTurtleList")
                    .track_focus(&self.file_focus)
                    .flex_1()
                    .min_h_0()
                    .child(if count == 0 {
                        empty(
                            if busy {
                                "Reading file history…"
                            } else {
                                "No revisions in this lineage"
                            },
                            "History starts at the chosen commit and follows its first parents.",
                        )
                    } else {
                        uniform_list(
                            "file-revisions",
                            count,
                            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| this.render_revision_row(index, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .track_scroll(&self.file_history.scroll)
                        .into_any_element()
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(p.muted))
                            .child(if busy {
                                "Reading the requested page…".into()
                            } else if count == 0 {
                                "No revisions loaded".into()
                            } else {
                                format!(
                                    "Revisions {}–{}{}",
                                    lineage.offset + 1,
                                    lineage.offset + count,
                                    if next_page.is_some() {
                                        " · more available"
                                    } else {
                                        " · end of lineage"
                                    }
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                button("newer-file-page", "Newer page", "", false)
                                    .disabled(busy || previous_page.is_none())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(offset) = previous_page {
                                            this.load_file_history_page(
                                                offset,
                                                Position::First,
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .child(
                                button("older-file-page", "Older page", "", false)
                                    .disabled(busy || next_page.is_none())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(offset) = next_page {
                                            this.load_file_history_page(
                                                offset,
                                                Position::First,
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_revision_details(&self, entry: &FileHistoryEntry, cx: &App) -> AnyElement {
        let p = palette(cx);
        let oid = entry.commit.oid.clone();
        let before = entry
            .change
            .old_path
            .as_ref()
            .map_or("Absent".into(), |path| path.display().to_string());
        let after = entry
            .change
            .new_path
            .as_ref()
            .map_or("Absent (deleted)".into(), |path| path.display().to_string());
        div()
            .id("file-revision-details")
            .max_h(px(210.))
            .overflow_y_scroll()
            .flex_shrink_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .border_b_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(entry.commit.subject.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        button("copy-file-revision", short_oid(&oid), "copy", false)
                            .tooltip("Copy revision commit hash")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(oid.clone()))
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(p.muted))
                            .child(entry.change.status.label()),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "{} · {}",
                        entry.commit.author,
                        full_date(entry.commit.timestamp)
                    )),
            )
            .child(div().text_size(px(10.)).text_color(rgb(p.muted)).child(
                entry.parent_oid.as_ref().map_or(
                    "Root revision · compared with an empty tree".into(),
                    |parent| format!("Compared with first parent {}", short_oid(parent)),
                ),
            ))
            .child(div().text_size(px(11.)).child(if before == after {
                before
            } else {
                format!("Before: {before}\nAfter: {after}")
            }))
            .into_any_element()
    }

    fn render_revision_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(lineage) = self.file_history.lineage.as_ref() else {
            return div().into_any_element();
        };
        let Some(entry) = lineage
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
        else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let active = lineage.selected == Some(index);
        let enabled = lineage.pending.is_none();
        let path = entry.change.path().display().to_string();
        let detail = format!(
            "{} · {} · {}",
            short_oid(&entry.commit.oid),
            entry.change.status.label(),
            entry.commit.author
        );
        let tooltip = format!("{}\n{}\n{}", entry.commit.subject, detail, path);
        div()
            .id(("file-revision", index))
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .aria_label(format!("{} · {}", entry.commit.subject, detail))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .h(px(68.))
            .w_full()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .border_l_2()
            .border_color(rgb(if active { p.accent } else { p.panel }))
            .bg(rgb(if active { p.selected } else { p.panel }))
            .hover(|style| style.bg(rgb(p.row_hover(active))))
            .when(enabled, |element| element.cursor_pointer())
            .child(
                div()
                    .truncate()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(entry.commit.subject.clone()),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(10.))
                    .text_color(rgb(p.muted))
                    .child(detail),
            )
            .child(
                div()
                    .truncate()
                    .text_size(px(10.))
                    .text_color(rgb(p.muted))
                    .child(path),
            )
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.select_file_revision(index, window, cx)
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Destination, Lineage, PAGE_SIZE, Position};
    use gitturtle_core::{ChangeStatus, Commit, FileChange, FileHistoryEntry, FileHistoryPage};
    use std::path::PathBuf;

    fn change(old: Option<PathBuf>, new: Option<PathBuf>) -> FileChange {
        FileChange {
            old_oid: old.as_ref().map(|_| "a".repeat(40)),
            new_oid: new.as_ref().map(|_| "b".repeat(40)),
            status: if new.is_none() {
                ChangeStatus::Deleted
            } else if old.is_none() {
                ChangeStatus::Added
            } else if old != new {
                ChangeStatus::Renamed
            } else {
                ChangeStatus::Modified
            },
            old_mode: if old.is_some() { "100644" } else { "000000" }.into(),
            new_mode: if new.is_some() { "100644" } else { "000000" }.into(),
            old_path: old,
            new_path: new,
        }
    }

    fn page(lineage: &Lineage, count: usize, next_offset: Option<usize>) -> FileHistoryPage {
        FileHistoryPage {
            anchor: lineage.anchor.clone(),
            path: lineage.path.clone(),
            entries: (0..count)
                .map(|index| FileHistoryEntry {
                    commit: Commit {
                        oid: format!("{index:040x}"),
                        parents: vec!["c".repeat(40)],
                        author: "Author".into(),
                        timestamp: 0,
                        subject: format!("Revision {index}"),
                        body: String::new(),
                    },
                    parent_oid: Some("c".repeat(40)),
                    change: change(Some(lineage.path.clone()), Some(lineage.path.clone())),
                })
                .collect(),
            next_offset,
        }
    }

    fn lineage() -> Lineage {
        Lineage::new("/fixture".into(), "f".repeat(40), "current.txt".into())
    }

    #[test]
    fn superseded_pages_cannot_replace_the_latest_navigation() {
        let mut state = lineage();
        let stale = state.begin(0, Position::First);
        let current = state.begin(PAGE_SIZE, Position::Last);
        assert!(!state.accept(stale, page(&state, PAGE_SIZE, Some(PAGE_SIZE))));
        state.fail(stale, "obsolete failure".into());
        assert_eq!(state.pending, Some(current));
        assert!(state.error.is_none());
        assert!(state.accept(current, page(&state, 3, None)));
        assert_eq!(state.offset, PAGE_SIZE);
        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn revision_arrows_cross_pages_without_skipping_boundary_entries() {
        let mut state = lineage();
        let request = state.begin(0, Position::Last);
        assert!(state.accept(request, page(&state, PAGE_SIZE, Some(PAGE_SIZE))));
        assert_eq!(
            state.destination(1, false),
            Some(Destination::Page(PAGE_SIZE, Position::First))
        );
        let next = state.begin(PAGE_SIZE, Position::First);
        assert_eq!(state.destination(-1, false), None);
        assert!(state.accept(next, page(&state, 3, None)));
        assert_eq!(state.selected, Some(0));
        assert_eq!(
            state.destination(-1, false),
            Some(Destination::Page(0, Position::Last))
        );
        assert_eq!(state.destination(1, false), Some(Destination::Revision(1)));
        state.selected = Some(2);
        assert_eq!(state.destination(1, false), None);
    }

    #[test]
    fn failed_page_keeps_the_previous_revision_and_retry_offset() {
        let mut state = lineage();
        let first = state.begin(0, Position::Last);
        assert!(state.accept(first, page(&state, PAGE_SIZE, Some(PAGE_SIZE))));
        let selected_oid = state.entry().unwrap().commit.oid.clone();
        let failed = state.begin(PAGE_SIZE, Position::First);
        state.fail(failed, "missing local object".into());
        assert_eq!(state.entry().unwrap().commit.oid, selected_oid);
        assert_eq!(state.offset, 0);
        assert_eq!(state.last_request, Some(failed));
        assert!(state.pending.is_none());
    }

    #[test]
    fn anchor_mismatch_and_empty_pages_do_not_invent_a_revision() {
        let mut state = lineage();
        let request = state.begin(0, Position::First);
        let mut wrong = page(&state, 2, None);
        wrong.anchor = "a".repeat(40);
        assert!(!state.accept(request, wrong));
        assert!(state.page.is_none());
        assert!(state.accept(request, page(&state, 0, None)));
        assert!(state.entry().is_none());
        assert_eq!(state.destination(1, false), None);
        assert_eq!(state.destination(-1, true), None);
    }

    #[test]
    fn revision_selection_preserves_rename_and_absent_deletion_sides() {
        let mut state = lineage();
        let request = state.begin(0, Position::First);
        let mut revisions = page(&state, 2, None);
        let before = PathBuf::from("older/name.txt");
        let after = state.path.clone();
        revisions.entries[0].change = change(Some(after.clone()), None);
        revisions.entries[1].change = change(Some(before.clone()), Some(after.clone()));
        assert!(state.accept(request, revisions));
        let deletion = &state.entry().unwrap().change;
        assert_eq!(deletion.old_path.as_ref(), Some(&after));
        assert!(deletion.new_path.is_none());
        assert!(deletion.new_oid.is_none());
        state.selected = Some(1);
        let rename = &state.entry().unwrap().change;
        assert_eq!(rename.old_path.as_ref(), Some(&before));
        assert_eq!(rename.new_path.as_ref(), Some(&after));
    }

    #[cfg(unix)]
    #[test]
    fn visually_identical_byte_paths_cannot_receive_each_others_pages() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let mut state = lineage();
        state.path = PathBuf::from(OsString::from_vec(b"file-\xff.txt".to_vec()));
        let request = state.begin(0, Position::First);
        let mut wrong = page(&state, 1, None);
        wrong.path = PathBuf::from(OsString::from_vec(b"file-\xfe.txt".to_vec()));
        assert_eq!(state.path.to_string_lossy(), wrong.path.to_string_lossy());
        assert!(!state.accept(request, wrong));
        assert!(state.page.is_none());
    }
}
