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
pub(super) struct ReturnContext {
    inspector_message: commit_message::State,
    files: Vec<FileChange>,
    file_filter: String,
    selected_file: Option<usize>,
    preferred_file: Option<PathBuf>,
    content: Option<Arc<Content>>,
    conflict_view: Option<Entity<conflicts::ConflictView>>,
    conflict_subscription: Option<Subscription>,
    patch_editor: Option<Entity<EditorState>>,
    patch_decoration: Option<editor_find::PatchDecorations>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    partial_subscription: Option<Subscription>,
    split_view: Option<Entity<split_diff::SplitView>>,
    markdown_view: Option<Entity<markdown_view::View>>,
    before_editor: Option<Entity<EditorState>>,
    after_editor: Option<Entity<EditorState>>,
    images: [Option<Arc<RenderImage>>; 2],
    image_scroll: ScrollHandle,
    image_comparison: image_compare::State,
    blame_visible: bool,
    zoom: f32,
    text_mode: TextMode,
    review: crate::text_review::State,
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
    previous: Option<Box<State>>,
}

impl State {
    pub(super) fn rescale_lists(&self, scales: settings::ListScales) {
        settings::rescale_list_scroll(&self.scroll, scales.lineage);
        if let Some(previous) = &self.previous {
            previous.rescale_lists(scales);
        }
    }
    pub(super) fn pause_for_tab(&mut self) {
        self.task = None;
        if let Some(lineage) = &mut self.lineage {
            lineage.generation = lineage.generation.wrapping_add(1);
            if lineage.pending.take().is_some() {
                lineage.error =
                    Some("History read paused on tab switch; use Retry to continue.".into());
            }
        }
        if let Some(retained) = &mut self.retained {
            retained.pause_for_tab();
        }
        if let Some(previous) = &mut self.previous {
            previous.pause_for_tab();
        }
    }

    pub(super) fn retained_bytes(&self) -> usize {
        self.retained
            .as_ref()
            .map_or(0, |context| context.retained_bytes())
            + self
                .previous
                .as_ref()
                .map_or(0, |previous| previous.retained_bytes())
            + self
                .lineage
                .as_ref()
                .and_then(|lineage| lineage.page.as_ref())
                .map_or(0, |page| {
                    page.entries
                        .iter()
                        .map(|entry| {
                            entry.commit.history_bytes()
                                + repository_tabs::file_bytes(&entry.change)
                        })
                        .sum()
                })
    }
    fn depth(&self) -> usize {
        usize::from(self.is_active())
            + self
                .previous
                .as_ref()
                .map_or(0, |previous| previous.depth())
    }

    pub(super) fn is_active(&self) -> bool {
        self.retained.is_some()
    }

    pub(super) fn rescale_code(&self, ratio: f32, cx: &mut App) {
        if let Some(previous) = &self.previous {
            previous.rescale_code(ratio, cx);
        }
        if let Some(retained) = &self.retained {
            retained.rescale_code(ratio, cx);
        }
    }

    pub(super) fn refresh_theme(&self, cx: &mut App) {
        if let Some(previous) = &self.previous {
            previous.refresh_theme(cx);
        }
        if let Some(retained) = &self.retained {
            retained.refresh_theme(cx);
        }
    }
}

impl ReturnContext {
    pub(super) fn pause_for_tab(&mut self) {
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
        self.image_comparison = self.image_comparison.clone();
    }

    pub(super) fn retained_bytes(&self) -> usize {
        // The preview accounts captured data, decoded pixels and presentations.
        // Reserve extra copies for native editors and uploaded image surfaces.
        self.content
            .as_ref()
            .map_or(0, |content| content.bytes().saturating_mul(4))
            + self
                .files
                .iter()
                .map(repository_tabs::file_bytes)
                .sum::<usize>()
            + self.file_filter.capacity()
            + self.inspector_message.retained_bytes()
    }
    pub(super) fn rescale_code(&self, ratio: f32, cx: &mut App) {
        for editor in [&self.patch_editor, &self.before_editor, &self.after_editor]
            .into_iter()
            .flatten()
        {
            text::rescale_editor(editor, ratio, cx);
        }
        if let Some(view) = &self.split_view {
            view.update(cx, |view, cx| view.rescale_code(ratio, cx));
        }
        if let Some(view) = &self.conflict_view {
            view.update(cx, |view, cx| view.rescale_code(ratio, cx));
        }
    }
    pub(super) fn refresh_theme(&self, cx: &mut App) {
        if let (Some(collection), Some(Content::Text { presentation, .. })) =
            (&self.patch_decoration, self.content.as_deref())
        {
            text::refresh_theme(collection, presentation, cx);
        }
        if let Some(split) = &self.split_view {
            split.update(cx, |view, cx| view.refresh_theme(cx));
        }
    }
}

impl GitTurtle {
    pub(super) fn file_history_entry(&self) -> Option<&FileHistoryEntry> {
        self.file_history.lineage.as_ref()?.entry()
    }
    pub(super) fn file_history_preview_origins(&self) -> Option<markdown_view::Origins> {
        let entry = self.file_history.lineage.as_ref()?.entry()?;
        Some(markdown_view::Origins::revisions(
            entry.commit.parents.first().cloned(),
            Some(entry.commit.oid.clone()),
        ))
    }
    pub(super) fn take_inspection_context(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ReturnContext {
        ReturnContext {
            inspector_message: std::mem::take(&mut self.inspector_message),
            files: std::mem::take(&mut self.files),
            file_filter: self.file_filter.read(cx).value().to_string(),
            selected_file: self.selected_file.take(),
            preferred_file: self.preferred_file.take(),
            content: self.content.take(),
            conflict_view: self.conflict_view.take(),
            conflict_subscription: self.conflict_subscription.take(),
            patch_editor: self.patch_editor.take(),
            patch_decoration: self.patch_decoration.take(),
            patch_view: self.patch_view.take(),
            partial_subscription: self.partial_subscription.take(),
            split_view: self.split_view.take(),
            markdown_view: self.markdown_view.take(),
            before_editor: self.before_editor.take(),
            after_editor: self.after_editor.take(),
            images: std::mem::take(&mut self.images),
            image_scroll: std::mem::take(&mut self.image_scroll),
            image_comparison: self.image_comparison.clone(),
            blame_visible: self.blame.is_visible(),
            zoom: self.zoom,
            text_mode: self.text_mode,
            review: std::mem::take(&mut self.review),
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
        }
    }

    pub(super) fn restore_inspection_context(
        &mut self,
        retained: ReturnContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_filter.update(cx, |input, cx| {
            input.set_value(retained.file_filter.clone(), window, cx)
        });
        self.inspector_message = retained.inspector_message;
        self.files = retained.files;
        self.refresh_file_filter(cx);
        self.selected_file = retained.selected_file;
        self.preferred_file = retained.preferred_file;
        self.content = retained.content;
        self.conflict_view = retained.conflict_view;
        self.conflict_subscription = retained.conflict_subscription;
        self.patch_editor = retained.patch_editor;
        self.patch_decoration = retained.patch_decoration;
        self.patch_view = retained.patch_view;
        self.partial_subscription = retained.partial_subscription;
        self.split_view = retained.split_view;
        self.markdown_view = retained.markdown_view;
        self.before_editor = retained.before_editor;
        self.after_editor = retained.after_editor;
        self.images = retained.images;
        self.image_scroll = retained.image_scroll;
        self.image_comparison = retained.image_comparison;
        self.blame.set_visible(retained.blame_visible);
        self.zoom = retained.zoom;
        self.text_mode = retained.text_mode;
        self.review = retained.review;
        self.rebind_text_partial(window, cx);
        self.mode = retained.mode;
        self.pane = retained.pane;
        self.sidebar = retained.sidebar;
        self.history_sidebar = retained.history_sidebar;
        self.status = retained.status;
        self.error = retained.error;
        if let Some(focus) = retained.focus {
            window.focus(&focus, cx);
        }
    }

    /// A page may finish while Settings or Projects is visible. Accept its
    /// metadata then, but construct/focus the selected preview only on return.
    pub(super) fn resume_file_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lineage) = &self.file_history.lineage else {
            return;
        };
        if self.page != AppPage::Repository || self.blame.is_visible() || lineage.pending.is_some()
        {
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
        if self.revision_inspection.is_active() {
            if let Some((anchor, path)) = self.inspection_history_target(index) {
                self.open_file_history_at(anchor, path, window, cx);
            } else {
                self.error = Some(if self.is_quick_source() {
                    "The current HEAD is unavailable. Refresh Working Changes before opening this tracked file's history."
                } else {
                    "This file is absent on the selected comparison side. Choose its existing side to inspect history."
                }.into());
                cx.notify();
            }
            return;
        }
        let (Some(commit), Some(file)) = (
            self.selected_commit
                .and_then(|index| self.commits.get(index)),
            self.files.get(index),
        ) else {
            return;
        };
        let anchor = commit.oid.clone();
        let path = file.path().to_owned();
        self.open_file_history_at(anchor, path, window, cx);
    }

    pub(super) fn file_history_blame_target(&self) -> Option<gitturtle_core::BlameTarget> {
        let entry = self.file_history.lineage.as_ref()?.entry()?;
        Some(gitturtle_core::BlameTarget::Committed {
            oid: entry.commit.oid.clone(),
            path: entry.change.path().to_owned(),
        })
    }

    pub(super) fn open_file_history_at(
        &mut self,
        anchor: String,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository || self.operation_busy.is_some() {
            return;
        }
        if self.file_history.depth() >= 4 {
            self.error = Some(
                "Four file-history comparisons are already open. Use Back before opening another."
                    .into(),
            );
            cx.notify();
            return;
        }
        let Some(repo) = &self.repository else {
            return;
        };
        let lineage = Lineage::new(repo.path().to_owned(), anchor, path);
        self.invalidate_read();
        let retained = self.take_inspection_context(window, cx);
        self.blame.hide();
        self.clear_preview();
        let previous = self
            .file_history
            .is_active()
            .then(|| Box::new(std::mem::take(&mut self.file_history)));
        self.file_history = State {
            lineage: Some(lineage),
            previous,
            retained: Some(Box::new(retained)),
            ..Default::default()
        };
        self.mode = WorkspaceMode::Compare;
        self.sidebar = false;
        self.pane = Pane::Files;
        window.focus(&self.file_focus, cx);
        self.load_file_history_page(0, Position::First, window, cx);
    }

    /// An explicit target change exits the entire transient navigation stack.
    /// Back itself still pops one view, retaining the prior selection/viewport.
    pub(super) fn close_inspections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        loop {
            if self.close_blame(window, cx) {
                continue;
            }
            if self.close_file_history(window, cx) {
                continue;
            }
            if self.close_revision_inspection(window, cx) {
                continue;
            }
            break;
        }
        self.blame = blame::State::default();
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
        self.file_history = self
            .file_history
            .previous
            .take()
            .map(|previous| *previous)
            .unwrap_or_default();
        self.clear_preview();
        let retained = *retained;
        self.restore_inspection_context(retained, window, cx);
        if self.mode == WorkspaceMode::Compare
            && !self.blame.is_visible()
            && self.content.is_none()
            && self.error.is_none()
            && !self.reload_tracked_inspection(window, cx)
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
        self.close_blame(window, cx);
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
        self.inspector_message.select(&entry.commit.oid);
        let change = entry.change.clone();
        lineage.selected = Some(index);
        lineage.error = None;
        self.file_history
            .scroll
            .scroll_to_item(index, ScrollStrategy::Center);
        self.files = vec![change];
        self.refresh_file_filter(cx);
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
                    .id("file-history-header")
                    .max_h(relative(0.30))
                    .overflow_y_scroll()
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
                                    .text_size(crate::appearance::ui_text(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("File history"),
                            )
                            .child(
                                button("close-file-history", "Back", "arrow-left", false).debug_selector(|| "close-file-history".into())
                                    .accessibility_label(
                                        "Close file history and restore previous view",
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.back_to_history(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("file-history-anchor-path")
                            .min_w_0()
                            .truncate()
                            .text_size(crate::appearance::ui_text(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .tooltip(move |window, cx| Tooltip::new(path.clone()).build(window, cx))
                            .child(copy_path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(crate::appearance::ui_text(10.))
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
                            .text_size(crate::appearance::ui_text(10.))
                            .text_color(rgb(p.muted))
                            .child("Follows renames and the first parent of each merge."),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
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
                                    .text_size(crate::appearance::ui_text(10.))
                                    .text_color(rgb(p.muted))
                                    .child(lineage.selected.map_or(String::new(), |index| {
                                        format!("Revision {}", lineage.offset + index + 1)
                                    })),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().flex().flex_col()
                .children(entry.map(|entry| self.render_commit_message(&entry.commit, true, cx)))
            .children(lineage.error.as_ref().map(|error| {
                div()
                    .id("file-history-failure")
                    .max_h(relative(0.35))
                    .overflow_y_scroll()
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
                            .text_size(crate::appearance::ui_text(11.))
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
                    .id("file-revision-list").debug_selector(|| "file-revision-list".into())
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
            )
            .child(
                div()
                    .id("file-history-footer")
                    .max_h(relative(0.25))
                    .overflow_y_scroll()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(10.))
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
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                button("newer-file-page", "Newer page", "", false).debug_selector(|| "newer-file-page".into())
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
                                button("older-file-page", "Older page", "", false).debug_selector(|| "older-file-page".into())
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

    pub(super) fn render_revision_metadata(
        &self,
        entry: &FileHistoryEntry,
        cx: &App,
    ) -> AnyElement {
        let p = palette(cx);
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
            .min_w_0()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(appearance::ui_text(11.))
            .text_color(rgb(p.muted))
            .child(format!(
                "{} · {}",
                commit_message::summary(&entry.commit.author),
                full_date(entry.commit.timestamp)
            ))
            .child(format!(
                "{} · {}",
                short_oid(&entry.commit.oid),
                entry.change.status.label()
            ))
            .child(entry.parent_oid.as_ref().map_or(
                "Root revision · compared with an empty tree".into(),
                |parent| format!("Compared with first parent {}", short_oid(parent)),
            ))
            .child(div().text_color(rgb(p.text)).child(if before == after {
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
            commit_message::summary(&entry.commit.author)
        );
        let subject = commit_message::summary(&entry.commit.subject);
        let tooltip = format!("{subject}\n{detail}\n{path}");
        div()
            .id(("file-revision", index))
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .aria_label(format!("{subject} · {detail}"))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .h(crate::appearance::ui_size(68.))
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
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(subject),
            )
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(detail),
            )
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
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
    use super::*;
    use core::prelude::v1::test;
    use gitturtle_core::{ChangeStatus, Commit, FileChange, FileHistoryEntry, FileHistoryPage};
    use std::path::PathBuf;

    #[test]
    fn scaling_preserves_nested_file_history_list_rows() {
        use crate::settings::ListScales;
        use gpui_kit::{point, px};
        let state = super::State {
            previous: Some(Box::default()),
            ..Default::default()
        };
        state
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-6800.)));
        let previous = state.previous.as_ref().unwrap();
        previous
            .scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(-13600.)));
        state.rescale_lists(ListScales {
            history: 1.,
            files: 1.,
            navigation: 1.,
            lineage: 85. / 68.,
        });
        assert_eq!(state.scroll.0.borrow().base_handle.offset().y, px(-8500.));
        assert_eq!(
            previous.scroll.0.borrow().base_handle.offset().y,
            px(-17000.)
        );
    }

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

    #[gpui::test]
    fn rendered_messages_leave_revision_navigation_and_copy_the_selected_entry(
        cx: &mut TestAppContext,
    ) {
        use crate::commit_message::tests::{commit, draw, fixture};
        let (_, app, cx) = fixture(cx, vec![commit("a", "Original history", "Original body")]);
        let expected = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let retained = app.take_inspection_context(window, cx);
                let mut lineage = lineage();
                let request = lineage.begin(0, Position::First);
                let mut page = page(&lineage, 20, Some(PAGE_SIZE));
                page.entries[0].commit.subject = "Wrapped revision title 東京 🐢 ".repeat(100);
                page.entries[0].commit.body = "Paragraph.\n\n".repeat(1000);
                let expected = commit_message::full_message(&page.entries[0].commit);
                assert!(lineage.accept(request, page));
                app.file_history = State {
                    lineage: Some(lineage),
                    retained: Some(Box::new(retained)),
                    ..State::default()
                };
                expected
            })
        });
        for font in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(font, 12, window, cx));
            draw(cx);
            let revision_list = cx.debug_bounds("file-revision-list").unwrap();
            assert!(
                revision_list.size.height >= appearance::ui_size(68.),
                "at least one entire revision must remain usable: {revision_list:?}"
            );
            let inspector = cx.debug_bounds("inspector-test").unwrap();
            for id in [
                "copy-message",
                "copy-commit",
                "close-file-history",
                "newer-file-page",
                "older-file-page",
            ] {
                let control = cx.debug_bounds(id).unwrap();
                assert!(
                    control.left() >= inspector.left() && control.right() <= inspector.right(),
                    "{id}: {control:?}"
                );
            }
            let button = cx.debug_bounds("copy-message").unwrap();
            cx.simulate_click(button.center(), Modifiers::default());
            cx.read(|cx| {
                assert!(
                    cx.read_from_clipboard().unwrap().text().unwrap() == expected,
                    "copy must preserve the selected file revision message"
                )
            });
        }
        cx.update(|window, cx| {
            appearance::apply_text_sizes(13, 12, window, cx);
            app.update(cx, |app, cx| app.back_to_history(window, cx));
        });
        draw(cx);
        let copy = cx.debug_bounds("copy-message").unwrap();
        cx.simulate_click(copy.center(), Modifiers::default());
        cx.read(|cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "Original history\n\nOriginal body"
            )
        });
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
