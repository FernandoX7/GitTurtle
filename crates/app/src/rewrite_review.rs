//! A retained native commit-series inspector and a separate reviewed lease push.
use crate::*;
use gitturtle_core::{
    LeasedPublishPlan, OperationControl, PublicationInspection, RewriteReview, WriteCommand,
};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use gpui_kit::prelude::FluentBuilder;

impl GitTurtle {
    pub(super) fn open_rewrite_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_series_review(window, cx);
    }

    /// Opens the rewritten series' review, as [`Self::open_rewrite_review`]
    /// does, and returns it, which a test waits on for its read
    /// (`focus_ring::sweep::Opened`).
    pub(super) fn open_series_review(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<SeriesBrowser>> {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return None;
        }
        let repo = self.repository.clone()?;
        let owner = cx.entity().downgrade();
        let remote = self.remote_name.read(cx).value().trim().to_owned();
        let branch = self.remote_branch.read(cx).value().trim().to_owned();
        let form = cx.new(|cx| SeriesBrowser::new(owner, repo, remote, branch, window, cx));
        open_series_dialog(form.clone(), window, cx);
        Some(form)
    }
}

/// The focus sweep (`focus_ring::sweep`) walks the rewritten series' review,
/// and the width walk (`width_walk`) reads it, once its series has been
/// read. The read runs on the review's own thread, which neither
/// `run_until_parked` nor the drain of the app's executor waits for: a walk
/// that began while it ran read "Reading captured local history…" with
/// Refresh series disabled, which the reply then replaced.
#[cfg(test)]
impl crate::focus_ring::sweep::Opened for Option<Entity<SeriesBrowser>> {
    fn finish(self, cx: &mut gpui::VisualTestContext) {
        let Some(review) = self else {
            return;
        };
        // Every read the review submitted has answered once its thread
        // drains, and the reply lands on this thread when it next runs.
        review.read_with(cx, |review, _| review.metadata.drain());
        cx.executor().run_until_parked();
        assert!(
            review.read_with(cx, |review, _| !review.pending),
            "the rewritten series' read lands"
        );
    }
}

/// Opens the series review's dialog around `form`.
fn open_series_dialog(form: Entity<SeriesBrowser>, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let close = form.clone();
        let cancel = form.clone();
        dialog
            .title("Review rewritten series")
            .width(appearance::dialog_width(px(980.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone())
            .footer(DialogFooter::new().child(
                button("close-series-review", "Back to repository", "", false).on_click(
                    move |_, window, cx| close.update(cx, |form, cx| form.close(window, cx)),
                ),
            ))
            .on_cancel(move |_, window, cx| {
                cancel.update(cx, |form, cx| form.back(window, cx));
                false
            })
    });
}

/// The series list's key context, where Return is the list's own
/// (`interactive_rebase::init`): it inspects the selected commit's files
/// rather than taking the review dialog's default action, which closed the
/// review.
pub(crate) const SERIES_CONTEXT: &str = "RewriteSeries";

pub(crate) struct SeriesBrowser {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    metadata: SerialExecutor,
    worker: Worker,
    control: OperationControl,
    network_control: Option<OperationControl>,
    generation: u64,
    /// This showing of the file preview, which a line beneath an open
    /// control belongs to; [`Self::clear_content`] starts a new one.
    showing: crate::system_preview::Showing,
    task: Option<Task<()>>,
    review: Option<Arc<RewriteReview>>,
    selected: usize,
    focus: FocusHandle,
    /// Back to series, which takes focus as the inspector replaces the
    /// series list: left on the list, focus rested on a control no longer
    /// drawn, so keys reached the window's root and Escape never reached
    /// the review.
    back: FocusHandle,
    rows_scroll: UniformListScrollHandle,
    files_scroll: UniformListScrollHandle,
    inspecting: bool,
    messages: Option<[Option<Entity<EditorState>>; 2]>,
    notice: Option<String>,
    inspect_label: String,
    files: Vec<FileChange>,
    selected_file: Option<usize>,
    content: Option<Arc<Content>>,
    editor: Option<Entity<EditorState>>,
    /// The patch editor's decorations, kept for its life like Compare's.
    decoration: Option<crate::editor_find::PatchDecorations>,
    patch: Option<Entity<diff_view::DiffView>>,
    text_mode: usize,
    pending: bool,
    network_pending: bool,
    error: Option<String>,
    remote: Entity<InputState>,
    remote_branch: Entity<InputState>,
    publication: Option<LeasedPublishPlan>,
    _subscriptions: Vec<Subscription>,
    closed: bool,
}

impl Drop for SeriesBrowser {
    fn drop(&mut self) {
        self.control.cancel();
        self.worker.cancel();
        if let Some(control) = &self.network_control {
            control.cancel();
        }
    }
}

impl SeriesBrowser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        remote: String,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            owner,
            repo,
            metadata: SerialExecutor::new("rewrite-series-review"),
            worker: Worker::new(),
            control: OperationControl::default(),
            network_control: None,
            generation: 0,
            showing: crate::system_preview::Showing::next(),
            task: None,
            review: None,
            selected: 0,
            focus: cx.focus_handle().tab_stop(true),
            back: cx.focus_handle().tab_stop(true),
            rows_scroll: UniformListScrollHandle::new(),
            files_scroll: UniformListScrollHandle::new(),
            inspecting: false,
            messages: None,
            notice: None,
            inspect_label: String::new(),
            files: vec![],
            selected_file: None,
            content: None,
            editor: None,
            decoration: None,
            patch: None,
            text_mode: 0,
            pending: false,
            network_pending: false,
            error: None,
            remote: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(remote)
                    .placeholder("Named remote")
            }),
            remote_branch: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(branch)
                    .placeholder("Remote branch")
            }),
            publication: None,
            _subscriptions: vec![],
            closed: false,
        };
        for input in [this.remote.clone(), this.remote_branch.clone()] {
            this._subscriptions.push(cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    if this.publication.take().is_some() { this.notice = Some("Destination changed. Check the named remote branch again before publishing.".into()); }
                    cx.notify();
                }
            }));
        }
        this.load(window, cx);
        this
    }
    fn current(&self, cx: &App) -> bool {
        !self.closed
            && self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                owner.page == AppPage::Repository && owner.path.as_deref() == Some(self.repo.path())
            })
    }
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.cancel_read();
        if let Some(control) = &self.network_control {
            control.cancel();
        }
        window.close_dialog(cx);
        if let Some(owner) = self.owner.upgrade() {
            let focus = owner.read(cx).app_focus.clone();
            focus.focus(window, cx);
        }
        window.refresh();
    }
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inspecting {
            self.inspecting = false;
            self.cancel_read();
            self.focus.focus(window, cx);
            cx.notify();
        } else {
            self.close(window, cx);
        }
    }
    fn cancel_read(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.control.cancel();
        self.worker.cancel();
        self.task = None;
        self.pending = false;
    }
    fn clear_content(&mut self) {
        self.showing = crate::system_preview::Showing::next();
        self.content = None;
        self.editor = None;
        self.decoration = None;
        self.patch = None;
        self.selected_file = None;
    }
    /// Opens captured bytes for this showing's open controls through the
    /// window's `GitTurtle`, whose keyboard focus ring it draws.
    fn opener(&self, cx: &App) -> crate::system_preview::Opener {
        let ring = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).focus_ring.clone())
            .unwrap_or_default();
        crate::system_preview::Opener::new(self.owner.clone(), self.showing).ringed_by(ring)
    }
    /// CPU bytes the open preview retains: its prepared content, which includes the patch
    /// presentation, with the same reserve for native editors as a repository tab, plus the
    /// patch editor's decorations. This transient review has no retained-bytes bound of its
    /// own, and a modal stays out of the repository tabs' bound because tabs cannot switch
    /// while it is open; its preview worker's cache bounds the content separately.
    #[cfg_attr(not(test), allow(dead_code))]
    fn preview_retained_bytes(&self) -> usize {
        self.content
            .as_ref()
            .map_or(0, |content| content.bytes().saturating_mul(4))
            + self
                .decoration
                .as_ref()
                .map_or(0, crate::editor_find::PatchDecorations::retained_bytes)
    }
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_read();
        self.clear_content();
        self.review = None;
        self.publication = None;
        self.notice = None;
        self.error = None;
        self.pending = true;
        self.control = OperationControl::default();
        let generation = self.generation;
        let repo = self.repo.clone();
        let response = self
            .metadata
            .submit_controlled(self.control.clone(), move || {
                let capture = recovery_drafts::latest_rewrite(repo.path())?;
                repo.rewrite_review(&capture.base, &capture.original, &capture.branch)
            });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Series review ended without a result")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || !this.current(cx) {
                    return;
                }
                this.pending = false;
                this.task = None;
                match result {
                    Ok(review) => {
                        this.review = Some(Arc::new(review));
                        this.selected = 0;
                        this.focus.focus(window, cx);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
    }
    fn select(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .review
            .as_ref()
            .is_none_or(|review| row >= review.rows.len())
        {
            return;
        }
        self.selected = row;
        self.focus.focus(window, cx);
        self.rows_scroll.scroll_to_item(row, ScrollStrategy::Center);
        cx.notify();
    }
    fn inspect(&mut self, original: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = &self.review else {
            return;
        };
        let Some(row) = review.rows.get(self.selected) else {
            return;
        };
        let commit = if original {
            row.original.map(|index| &review.original[index].commit)
        } else {
            row.rewritten.map(|index| &review.rewritten[index].commit)
        };
        let Some(commit) = commit else {
            return;
        };
        let oid = commit.oid.clone();
        self.inspect_label = format!(
            "{} {} · {}",
            if original { "Original" } else { "Rewritten" },
            short_oid(&oid),
            commit.subject
        );
        self.cancel_read();
        self.clear_content();
        self.inspecting = true;
        self.back.focus(window, cx);
        self.messages = None;
        self.files.clear();
        self.error = None;
        self.pending = true;
        let generation = self.generation;
        let response = self.worker.submit(Job::Changes {
            repo: self.repo.clone(),
            oid,
            parent: 0,
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation || !this.current(cx) || !this.inspecting { return; }
                this.pending = false; this.task = None;
                match result { Ok(Ok(Output::Changes(files, _))) => { this.files = files; this.files_scroll.scroll_to_item(0, ScrollStrategy::Top); }, Ok(Err(error)) => this.error = Some(format!("{error:#}")), _ => this.error = Some("Changed-file read was interrupted. Return to the series and select the commit again.".into()) }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// Return's inspection: the selected row's rewritten commit, or its
    /// original where the row has none, as a dropped commit has only its
    /// original side, so Return always inspects the row's commit.
    fn inspect_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let original = self
            .review
            .as_ref()
            .and_then(|review| review.rows.get(self.selected))
            .is_some_and(|row| row.rewritten.is_none());
        self.inspect(original, window, cx);
    }
    fn inspect_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = &self.review else {
            return;
        };
        let Some(row) = review.rows.get(self.selected) else {
            return;
        };
        let messages = [
            row.original
                .map(|index| review.original[index].commit.message.clone()),
            row.rewritten
                .map(|index| review.rewritten[index].commit.message.clone()),
        ];
        self.cancel_read();
        self.clear_content();
        self.inspecting = true;
        self.back.focus(window, cx);
        self.inspect_label = "Original and rewritten commit messages".into();
        self.messages = Some(messages.map(|message| {
            message.map(|message| text::editor(&message, "text", None, window, cx))
        }));
        cx.notify();
    }
    fn file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.files.get(index).cloned() else {
            return;
        };
        self.cancel_read();
        self.clear_content();
        self.selected_file = Some(index);
        self.pending = true;
        self.error = None;
        let generation = self.generation;
        let response = self.worker.submit(Job::Preview {
            origins: Default::default(),
            repo: self.repo.clone(),
            file,
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || !this.current(cx) || !this.inspecting {
                    return;
                }
                this.pending = false;
                this.task = None;
                match result {
                    Ok(Ok(Output::Preview(content, _))) => {
                        this.content = Some(content);
                        this.prepare_editor(window, cx);
                    }
                    Ok(Err(error)) => this.error = Some(format!("{error:#}")),
                    _ => this.error = Some("File preview was interrupted. Select it again.".into()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn prepare_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode == 3 {
            return;
        }
        let Some(Content::Text {
            patch,
            old,
            new,
            presentation,
            ..
        }) = self.content.as_deref()
        else {
            return;
        };
        let value = [patch, old, new][self.text_mode];
        let (editor, decoration) = text::editor_with_decorations(
            value,
            if self.text_mode == 0 { "diff" } else { "text" },
            (self.text_mode == 0).then_some(presentation.as_ref()),
            window,
            cx,
        );
        self.decoration = decoration;
        self.patch =
            (self.text_mode == 0).then(|| diff_view::new(editor.clone(), presentation, window, cx));
        self.editor = Some(editor);
    }
    fn check_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = self.review.clone() else {
            return;
        };
        if self.network_pending || !self.current(cx) {
            return;
        }
        let remote = self.remote.read(cx).value().trim().to_owned();
        let branch = self.remote_branch.read(cx).value().trim().to_owned();
        let repo = self.repo.clone();
        let form = cx.entity().downgrade();
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.operation_busy.is_some() { return; }
            owner.operation_busy = Some("Checking rewrite publication destination…");
            let control = owner.begin_operation_control(window, cx); self.network_control = Some(control.clone()); self.network_pending = true; self.publication = None; self.error = None;
            let response = owner.operations.submit_controlled(control, move || repo.inspect_rewrite_publication(&review, &remote, &branch));
            owner.operation_task = Some(cx.spawn_in(window, async move |owner, cx| {
                let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Remote inspection ended without a result. Check it again explicitly before publishing")));
                let _ = owner.update_in(cx, |owner, window, cx| {
                    owner.operation_busy = None; owner.finish_operation_control(window, cx); owner.read_deferred_working(window, cx);
                    let path = owner.path.clone(); let page = owner.page;
                    if let Some(form) = form.upgrade() { form.update(cx, |form, cx| {
                        form.network_pending = false; form.network_control = None;
                        if form.closed || page != AppPage::Repository || path.as_deref() != Some(form.repo.path()) { return; }
                        match result {
                            Ok(PublicationInspection::Ready(plan)) => { form.publication = Some(plan); form.notice = None; },
                            Ok(PublicationInspection::RemoteChanged(review)) => { form.review = Some(Arc::new(review)); form.selected = 0; form.publication = None; form.notice = Some("The remote changed. Original now shows its newly inspected series. Review these commits and files, then check the destination again to prepare a fresh lease.".into()); },
                            Ok(PublicationInspection::AlreadyPublished { remote, remote_ref, oid }) => { form.publication = None; form.notice = Some(format!("{remote} · {remote_ref} already points to {oid}. No publication is needed.")); },
                            Err(error) => form.error = Some(format!("{error:#}"))
                        }
                        cx.notify();
                    }); }
                    cx.notify();
                });
            })); cx.notify();
        });
        cx.notify();
    }
    fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.publication.clone() else {
            return;
        };
        if self.network_pending
            || !self.current(cx)
            || plan.remote != self.remote.read(cx).value().trim()
            || plan.remote_ref
                != format!("refs/heads/{}", self.remote_branch.read(cx).value().trim())
        {
            return;
        }
        self.close(window, cx);
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path.as_deref() == Some(plan.root.as_path()) {
                owner.write(
                    WriteCommand::PublishRewrite(Arc::new(plan)),
                    "Publishing rewritten branch…",
                    window,
                    cx,
                );
            }
        });
    }
    /// The inspected file, `width` wide as its box is in the frame it is
    /// drawn.
    fn render_content(&self, width: Pixels, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if let Some(messages) = &self.messages {
            return div()
                .size_full()
                .flex()
                .gap_2()
                .children(messages.iter().enumerate().map(|(side, editor)| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .child(if side == 0 {
                            "Original message"
                        } else {
                            "Rewritten message"
                        })
                        .child(editor.as_ref().map_or_else(
                            || {
                                div()
                                    .child("Commit absent in this series")
                                    .into_any_element()
                            },
                            |editor| {
                                editor_find::Editor::new(editor)
                                    .readonly(true)
                                    .size_full()
                                    .aria_label(if side == 0 {
                                        "Original commit message"
                                    } else {
                                        "Rewritten commit message"
                                    })
                                    .into_any_element()
                            },
                        ))
                }))
                .into_any_element();
        }
        match self.content.as_deref() {
            Some(Content::Text { diagrams, .. }) if self.text_mode == 3 => {
                diagrams.as_ref().map_or_else(
                    || {
                        div()
                            .child("No Mermaid diagrams on this file; use Source.")
                            .into_any_element()
                    },
                    |preview| {
                        rich_preview::render_comparison(
                            preview,
                            false,
                            self.opener(cx),
                            width,
                            window,
                            cx,
                        )
                    },
                )
            }
            Some(Content::Text { .. }) if self.text_mode == 0 => self.patch.as_ref().map_or_else(
                || div().into_any_element(),
                |patch| patch.clone().into_any_element(),
            ),
            Some(Content::Text { .. }) => self.editor.as_ref().map_or_else(
                || div().into_any_element(),
                |editor| {
                    editor_find::Editor::new(editor)
                        .readonly(true)
                        .size_full()
                        .aria_label("Commit file source")
                        .into_any_element()
                },
            ),
            Some(Content::Rich(preview)) => {
                rich_preview::render_comparison(preview, false, self.opener(cx), width, window, cx)
            }
            Some(Content::Images { old, new }) => div()
                .size_full()
                .flex()
                .children(
                    [("Before", old), ("After", new)]
                        .into_iter()
                        .map(|(label, side)| {
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .p_2()
                                .child(if side.animation.is_some() {
                                    format!("{label} · first frame")
                                } else {
                                    label.into()
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .when_some(side.render.clone(), |element, image| {
                                            element.child(crate::gif_playback::static_image(image))
                                        })
                                        .when(side.render.is_none(), |element| {
                                            element.child(
                                                side.message.clone().unwrap_or("Absent".into()),
                                            )
                                        }),
                                )
                        }),
                )
                .into_any_element(),
            Some(Content::Notice(message)) => div().p_3().child(message.clone()).into_any_element(),
            _ => div()
                .p_3()
                .child(if self.files.is_empty() && !self.pending {
                    "This commit has no changed files."
                } else {
                    "Select a file to inspect its captured commit bytes."
                })
                .into_any_element(),
        }
    }
    fn row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(review) = &self.review else {
            return div().into_any_element();
        };
        let Some(row) = review.rows.get(index) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = index == self.selected;
        let label = format!(
            "{}{}{}",
            row.change.label(),
            if row.reordered { " · reordered" } else { "" },
            if row.ambiguous {
                " · inspect correspondence"
            } else {
                ""
            }
        );
        let commit_labels = [
            (row.original, &review.original, "Original"),
            (row.rewritten, &review.rewritten, "Rewritten"),
        ]
        .map(|(commit_index, commits, side)| {
            commit_index.map_or_else(
                || (format!("{side}: absent"), "—".to_owned()),
                |commit_index| {
                    let commit = &commits[commit_index].commit;
                    let text = format!(
                        "{} · {} {}",
                        commit_index + 1,
                        short_oid(&commit.oid),
                        commit.subject
                    );
                    (format!("{side}: {text}"), text)
                },
            )
        });
        div()
            .id(("series-row", index))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .when(selected, |row| row.aria_active_descendant())
            .aria_position_in_set(index + 1)
            .aria_size_of_set(review.rows.len())
            .aria_label(format!(
                "{label}. {}. {}",
                commit_labels[0].0, commit_labels[1].0
            ))
            .w_full()
            .min_w_0()
            .h(appearance::ui_size(68.))
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(rgb(p.border))
            .bg(rgb(if selected { p.selected } else { p.panel }))
            .hover(|style| style.bg(rgb(p.row_hover(selected))))
            .cursor_pointer()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_size(appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .truncate()
                    .child(label),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .gap_3()
                    .children(commit_labels.into_iter().map(|(_, text)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(appearance::ui_text(12.))
                            .child(text)
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select(index, window, cx)))
            .into_any_element()
    }
}

impl Render for SeriesBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let height = (f32::from(window.viewport_size().height) - 250.).clamp(240., 690.);
        // The body scrolls, so it clips its controls on both axes. It keeps
        // the focus ring's room beside and below them, giving the sides back
        // through its margin, so the inputs and buttons at its edges draw
        // their rings whole, as the interactive rebase form does.
        let room = appearance::button_ring_room(cx);
        // The series draws the window's keyboard ring on its own 1 px edge,
        // after a key, until a pointer press.
        let ringed = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).focus_ring.shows(&self.focus));
        let series = cx.entity().downgrade();
        let row_count = self.review.as_ref().map_or(0, |review| review.rows.len());
        let row_height = appearance::ui_size(68.);
        let list_height = (row_height * row_count.clamp(1, 6) as f32 + px(2.))
            .min((px(height) - appearance::ui_size(210.)).max(row_height + px(2.)));
        let selected = self
            .review
            .as_ref()
            .and_then(|review| review.rows.get(self.selected))
            .cloned();
        let publication_valid = self.publication.as_ref().is_some_and(|plan| {
            plan.remote == self.remote.read(cx).value().trim()
                && plan.remote_ref
                    == format!("refs/heads/{}", self.remote_branch.read(cx).value().trim())
        });
        crate::native_accessibility::clips_children(div().id("rewrite-review-body").debug_selector(|| "rewrite-review-body".into()).min_w_0().px(room).mx(-room).pb(room).max_h(px(height) + room).when(self.inspecting, |element| element.h(px(height))).overflow_y_scroll().flex().flex_col().gap_2()
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.back(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.back(window, cx); cx.stop_propagation(); }))
            .when(!self.inspecting, |element| element
                .child(div().text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Original → rewritten · unique patch fingerprints identify likely correspondence. Changed patches may match by unique subject only. Ambiguous, combined and empty commits remain explicit; inspect both sides before publishing."))
                .when_some(self.review.as_ref(), |element, review| element.child(div().text_size(appearance::ui_text(11.)).child(format!("{} · original {} → rewritten {} · base {}", review.branch, short_oid(&review.original_head), short_oid(&review.rewritten_head), short_oid(&review.base)))))
                .child(crate::native_accessibility::clips_children(div().id("series-focus-list").w_full().min_w_0().h(list_height).flex_shrink_0().border_1().border_color(rgb(if ringed { p.accent } else { p.border })).rounded(px(6.)).overflow_hidden().track_focus(&self.focus).key_context(SERIES_CONTEXT).role(Role::ListBox).aria_label("Original and rewritten commits").aria_description("Up and Down select a commit pair. Return inspects its rewritten files, or the original's where it has no rewritten commit.")
                    // Return inspects as it goes down, never on a held key's
                    // repeats, as the dialog's own Enter acts.
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| { match event.keystroke.key.as_str() { "up" => this.select(this.selected.saturating_sub(1), window, cx), "down" => this.select(this.selected + 1, window, cx), "enter" if !event.is_held && gpui_kit::base::enter_press_began_on(&this.focus, window, cx) => this.inspect_selected(window, cx), "enter" => {}, _ => return } cx.stop_propagation(); }))
                    .child(uniform_list("rewrite-series-list", row_count, cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| this.row(index, cx)).collect::<Vec<_>>())).size_full().track_scroll(&self.rows_scroll))))
                .child(div().flex().flex_wrap().gap_2()
                    .child(button("inspect-series-messages", "Review messages", "", false).disabled(selected.is_none()).on_click(cx.listener(|this, _, window, cx| this.inspect_messages(window, cx))))
                    .child(button("inspect-original-commit", "Inspect original files", "", false).disabled(selected.as_ref().is_none_or(|row| row.original.is_none())).on_click(cx.listener(|this, _, window, cx| this.inspect(true, window, cx))))
                    .child(button("inspect-rewritten-commit", "Inspect rewritten files", "", false).disabled(selected.as_ref().is_none_or(|row| row.rewritten.is_none())).on_click(cx.listener(|this, _, window, cx| this.inspect(false, window, cx))))
                    .child(button("reload-series", "Refresh series", "", false).disabled(self.pending || self.network_pending).on_click(cx.listener(|this, _, window, cx| this.load(window, cx)))))
                .child(div().flex().flex_wrap().items_center().gap_2()
                    .child(div().w(px(150.)).child(Input::new(&self.remote).aria_label("Publication remote").disabled(self.network_pending)))
                    .child(div().w(px(170.)).child(Input::new(&self.remote_branch).aria_label("Publication remote branch").disabled(self.network_pending)))
                    .child(button("inspect-publication-remote", if self.network_pending { "Checking remote…" } else { "Check publication destination…" }, "", false).disabled(self.review.is_none() || self.network_pending || self.pending).on_click(cx.listener(|this, _, window, cx| this.check_remote(window, cx))))
                    .when(self.network_pending, |element| element.child(button("cancel-remote-inspection", "Cancel remote check", "", false).on_click(cx.listener(|this, _, _, cx| { if let Some(control) = &this.network_control { control.cancel(); } cx.notify(); })))))
                .when_some(self.notice.as_ref(), |element, notice| element.child(div().text_size(appearance::ui_text(12.)).text_color(rgb(p.warning)).child(notice.clone())))
                .when_some(self.publication.as_ref(), |element, plan| element.child(div().p_3().flex_shrink_0().flex().flex_col().gap_2().border_1().border_color(rgb(p.warning)).rounded(px(6.))
                    .child(div().text_size(appearance::ui_text(12.)).child(format!("Publish to {} · {}\n{}\nExpected remote: {}\nReviewed replacement: {}", plan.remote, plan.remote_ref, workspace::display_remote_url(&plan.remote_url), plan.expected_remote_oid, plan.new_oid)))
                    .child(div().text_size(appearance::ui_text(11.)).child("This replaces the named remote branch's history. Collaborators may need to update their local branches. The exact lease refuses intervening remote updates; failures and uncertain outcomes are never retried automatically."))
                    .child(div().flex().gap_2().child(button("publish-reviewed-rewrite", "Publish with exact lease", "", true).disabled(!publication_valid || self.network_pending).on_click(cx.listener(|this, _, window, cx| this.publish(window, cx))))
                        .child(button("cancel-publication-review", "Cancel publication review", "", false).on_click(cx.listener(|this, _, _, cx| { this.publication = None; cx.notify(); }))))))
            )
            .when(self.inspecting, |element| element
                .child(div().flex().flex_wrap().items_center().gap_2().child(button("back-rewrite-series", "Back to series", "arrow-left", false).track_focus(&self.back).on_click(cx.listener(|this, _, window, cx| this.back(window, cx)))).child(div().text_size(appearance::ui_text(12.)).child(self.inspect_label.clone())))
                .when(self.messages.is_none(), |element| element.child(div().debug_selector(|| "series-files".into()).h(px(130.)).flex_shrink_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden().child(uniform_list("series-files", self.files.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| {
                    let file = &this.files[index]; button(("series-file", index), file.path().to_string_lossy().into_owned(), "", this.selected_file == Some(index)).w_full().h(appearance::ui_size(30.)).on_click(cx.listener(move |this, _, window, cx| this.file(index, window, cx))).into_any_element()
                }).collect::<Vec<_>>())).size_full().track_scroll(&self.files_scroll))))
                .when(matches!(self.content.as_deref(), Some(Content::Text { .. })), |element| element.child(div().flex().gap_2().children(["Patch", "Before source", "After source", "Diagrams"].into_iter().enumerate().filter(|(mode,_)| *mode != 3 || matches!(self.content.as_deref(), Some(Content::Text {diagrams:Some(_),..}))).map(|(mode, label)| button(("series-text-mode", mode), label, "", self.text_mode == mode).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.prepare_editor(window, cx); cx.notify(); }))))))
                // Built for the box's width in the frame it is drawn
                // (`in_frame`), so the open controls take the form that
                // width leaves them in the first frame after a resize.
                .child(div().debug_selector(|| "series-file-preview".into()).flex_1().min_h(appearance::ui_size(100.)).flex().flex_col().border_1().border_color(rgb(p.border)).overflow_hidden().child(crate::in_frame::in_frame(move |bounds, window, cx| series.update(cx, |this, cx| this.render_content(bounds.size.width, window, cx)).unwrap_or_else(|_| div().into_any_element())).named("rewrite-comparison"))))
            .when(self.pending, |element| element.child(div().text_color(rgb(p.muted)).child("Reading captured local history…")))
            .when_some(self.error.as_ref(), |element, error| element.child(div().max_h(appearance::ui_size(100.)).id("series-review-error").overflow_y_scroll().text_size(appearance::ui_text(12.)).text_color(rgb(p.warning)).child(error.clone()))))
    }
}

/// Presses Check publication destination… in a series review of `app`'s
/// repository that names only its worktree, for the regression of the reads
/// a check's end runs in `page_navigation::tests`. The check needs a
/// remote the fixture has none of, so Git refuses it as soon as it runs.
#[cfg(test)]
pub(crate) fn check_publication(app: &Entity<GitTurtle>, window: &mut Window, cx: &mut App) {
    let repo = app.read(cx).repository.clone().expect("a repository");
    let review = RewriteReview {
        root: repo.path().to_owned(),
        branch: "main".into(),
        base: String::new(),
        original_head: String::new(),
        rewritten_head: String::new(),
        original: Vec::new(),
        rewritten: Vec::new(),
        rows: Vec::new(),
    };
    let browser = cx.new(|cx| {
        SeriesBrowser::new(
            app.downgrade(),
            repo,
            "origin".into(),
            "main".into(),
            window,
            cx,
        )
    });
    browser.update(cx, |browser, cx| {
        browser.review = Some(Arc::new(review));
        browser.check_remote(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    /// The series review's unified patch keeps its decorations for the editor's life, and its
    /// preview figure counts them along with the presentation they were built from.
    #[gpui::test]
    async fn series_patch_preview_counts_its_decorations_and_presentation(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
        cx.update(gpui_kit::init);
        let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<SeriesBrowser>>>> =
            Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    WeakEntity::new_invalid(),
                    repository,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            *output.borrow_mut() = Some(browser.clone());
            gpui_kit::component::Root::new(browser, window, cx)
        });
        cx.run_until_parked();
        let browser = captured.borrow().as_ref().unwrap().clone();
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.content = Some(Arc::new(text::decorated_patch_content()));
                browser.text_mode = 0;
                browser.prepare_editor(window, cx);
                let Some(Content::Text { presentation, .. }) = browser.content.as_deref() else {
                    unreachable!()
                };
                let presentation = presentation.retained_bytes();
                let decoration = browser.decoration.clone().expect("kept with the editor");
                let decorated = decoration.retained_bytes();
                assert!(decorated > 0, "a patch with changes has decorations");

                let with = browser.preview_retained_bytes();
                browser.decoration = None;
                let without = browser.preview_retained_bytes();
                assert_eq!(with - without, decorated);
                browser.content = None;
                assert!(without - browser.preview_retained_bytes() >= presentation);

                // A source mode replaces the patch editor and releases its decorations.
                browser.content = Some(Arc::new(text::decorated_patch_content()));
                browser.decoration = Some(decoration);
                browser.text_mode = 1;
                browser.prepare_editor(window, cx);
                assert!(browser.decoration.is_none());
                browser.clear_content();
                assert_eq!(browser.preview_retained_bytes(), 0);
            })
        });
    }

    /// An inspected commit's document, in the series review's file box, which
    /// keeps 100 px, says why a click on Open in default app opened nothing on
    /// one whole line beneath its header's controls, its whole answer in the
    /// line's tooltip, and the page beneath gives way below its minimum for
    /// it: in a 461 × 490 window the room beneath the header held no line
    /// above that minimum, so the line drew none and the click changed
    /// nothing on screen.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_document_says_why_on_one_whole_line(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
            std::fs::write(&document, pdf(text)).unwrap();
            git(repo.path(), &["add", "document.pdf"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        // The rewritten commit's files, as Inspect rewritten files lists
        // them, and its document chosen.
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("system-preview-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        crate::system_preview::tests::a_failed_open_answers_on_one_line(
            cx,
            "the series review at 13 pt in 461 × 490",
            [
                "system-preview-1",
                "system-preview-1/note",
                "rich-sides/body",
            ],
            &["series-file-preview", "rewrite-review-body"],
        );
    }

    /// A model the series review shows, an inspected commit's 3D file,
    /// focused from the keyboard keeps its canvas's accent ring while the
    /// pointer moves over it and away, at 13 pt in a 1,480 × 980 window, as
    /// Compare's does: the review's opener carries the window's focus ring
    /// (`Opener::ringed_by`), which the canvas asks.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_models_canvas_keeps_its_ring_while_the_pointer_moves(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
        let shape = repo.path().join("shape.obj");
        for (scale, message) in [("1", "Add a model"), ("2", "Grow it")] {
            std::fs::write(&shape, model(scale)).unwrap();
            git(repo.path(), &["add", "shape.obj"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("model-canvas-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        crate::focus_ring::sweep::prepare(&app, cx);
        crate::model_view::tests::assert_the_canvas_keeps_its_ring(
            cx,
            "model-canvas-1",
            "the series review",
        );
    }

    /// An inspected document's comparison takes its open controls' form
    /// from its box's width in the frame it is drawn ([`crate::in_frame`]):
    /// every frame drawn as the window narrows and widens between 461 and
    /// 1,481 px at 13 and 18 pt lays the comparison out as the frame it
    /// settles in, and the walk meets After's control labelled and as its
    /// icon. Built from the box's width a frame before, the first frame after
    /// a resize drew the form of the width before.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_comparison_takes_its_form_in_the_frame_it_is_drawn(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        for (text, message) in [("first", "Add a document"), ("second", "Rewrite it")] {
            std::fs::write(&document, pdf(text)).unwrap();
            git(repo.path(), &["add", "document.pdf"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1481.), px(980.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
                browser.file(0, window, cx);
            })
        });
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("system-preview-1").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let met = crate::system_preview::tests::open_controls_take_their_form_in_the_frame(
            cx,
            "the series review",
            "rewrite-comparison",
        );
        assert_eq!(
            met.into_iter().collect::<Vec<_>>(),
            ["icon", "labelled"],
            "the walk meets both forms"
        );
    }

    /// A rewritten commit's 3D model and PDF, in the series review's file
    /// box, which keeps 100 px at the interface size, keep each side's own
    /// header at its floor, one control whole with the larger of its ring's
    /// room and the header's padding above and below, beneath which the
    /// model or page gives way, and Tab reveals each of the header's
    /// controls whole with its ring there, in a 461 × 490 window, at 13 and
    /// 18 pt. The floor once counted the ring's room and the padding
    /// together, about 6 px more than either needed. The box once kept an
    /// unscaled 100 px, as the error box beneath it did, which at 18 pt left
    /// the model's side 44 px, less than its header's floor and rule, so the
    /// header took the side and the box's clip cut its controls' rings.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn an_inspected_side_header_keeps_its_floor_in_the_file_box(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::focus_reveal::tests::control;
        use crate::system_preview::tests::{Shown, assert_a_header_keeps_its_floor};
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        let model = repo.path().join("shape.obj");
        for (text, scale, message) in [("first", "2", "Add them"), ("second", "1", "Rewrite them")]
        {
            std::fs::write(&document, pdf(text)).unwrap();
            std::fs::write(
                &model,
                format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n"),
            )
            .unwrap();
            git(repo.path(), &["add", "document.pdf", "shape.obj"]);
            git(repo.path(), &["commit", "--quiet", "-m", message]);
        }
        // A fresh repository keeps its branch as a loose reference.
        let head = std::fs::read_to_string(repo.path().join(".git/refs/heads/main")).unwrap();
        let files = repo.changes_with_renames(head.trim(), 0).unwrap();
        let index = |path: &str| {
            files
                .iter()
                .position(|file| file.path() == std::path::Path::new(path))
                .expect("the commit changes it")
        };
        let (document, model) = (index("document.pdf"), index("shape.obj"));
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let repo = app
                .read(cx)
                .repository
                .clone()
                .expect("the window's repository");
            let browser = cx.new(|cx| {
                SeriesBrowser::new(
                    app.downgrade(),
                    repo,
                    "origin".into(),
                    "main".into(),
                    window,
                    cx,
                )
            });
            open_series_dialog(browser.clone(), window, cx);
            browser
        });
        cx.update(|_, cx| {
            browser.update(cx, |browser, _| {
                browser.inspecting = true;
                browser.inspect_label = "Rewritten".into();
                browser.files = files;
            })
        });
        let open = |cx: &mut gpui_kit::VisualTestContext, index: usize, selector: &'static str| {
            cx.update(|window, cx| {
                browser.update(cx, |browser, cx| browser.file(index, window, cx))
            });
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let view = browser.entity_id();
        let mut cameras = vec![control("model-external", 1)];
        cameras.extend((5..10).map(|index| control("model-camera", index)));
        cameras.extend([control("model-views", 1), control("model-link", 1)]);
        open(cx, model, "model-side-1/header");
        for (text, floor) in [(13, 34.), (18, 48.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            assert_a_header_keeps_its_floor(
                cx,
                &format!("the model's After at {text} pt"),
                (view, "model-side-1"),
                (px(floor), Shown::Floor),
                &control("model-source", 1),
                &cameras,
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        draw(cx);
        open(cx, document, "system-preview-1");
        browser.update(cx, |browser, cx| {
            let Some(Content::Rich(preview)) = browser.content.as_deref() else {
                panic!("a changed PDF is a document comparison");
            };
            browser.content = Some(Arc::new(crate::pdf_view::tests::paged(preview)));
            cx.notify();
        });
        draw(cx);
        crate::pdf_view::tests::drain_pages();
        draw(cx);
        // At 13 pt the narrowest form's run takes a line beneath its
        // heading; at 18 pt it wraps to a second, the "more" menu on its own.
        for (text, floor, shown) in [(13, 41., Shown::FloorOneLine), (18, 57., Shown::Floor)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            for side in [0, 1] {
                // The narrowest header a narrow box's side takes, its run on
                // a line beneath its heading, which scrolls at its floor,
                // from the page number (Previous is unavailable on the first
                // page).
                let controls = ["pdf-forward", "pdf-more"]
                    .map(|name| control(name, side))
                    .to_vec();
                let name = if side == 0 {
                    "pdf-side-0"
                } else {
                    "pdf-side-1"
                };
                assert_eq!(
                    crate::pdf_view::DRAWN_FORMS.with_borrow(|drawn| drawn.get(&side).copied()),
                    Some(crate::pdf_view::HeaderForm::Narrowest),
                    "{name} at {text} pt"
                );
                assert_a_header_keeps_its_floor(
                    cx,
                    &format!("{name} at {text} pt"),
                    (view, name),
                    (px(floor), shown),
                    &control("pdf-page-number", side),
                    &controls,
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The rewritten series' review, opened with the app's series keys on a
    /// disposable repository of three commits whose last two a rebase's
    /// start captured, as it captures them, before `rewrite` (Git argument
    /// lists, run in order) rewrote them, its series read.
    async fn rewritten_review<'a>(
        cx: &'a mut gpui_kit::TestAppContext,
        rewrite: &[&[&str]],
    ) -> (
        tempfile::TempDir,
        Entity<SeriesBrowser>,
        &'a mut gpui::VisualTestContext,
    ) {
        use crate::focus_ring::sweep::Opened as _;
        use crate::tags::tests::{draw, git};
        use gitturtle_core::InteractiveRebaseCommand;
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 2))
                .await;
        cx.update(|_, cx| crate::interactive_rebase::init(cx));
        let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
        git(repo.path(), &["reset", "--quiet", "--hard", "main"]);
        let plan = repo.interactive_rebase_plan("main~2").unwrap();
        let steps = plan.steps();
        let command = WriteCommand::InteractiveRebase(Arc::new(InteractiveRebaseCommand::Start {
            plan,
            steps,
            acknowledge_published: false,
        }));
        let mut saved = app
            .update(cx, |app, _| app.prepare_rewrite_capture(&command))
            .expect("a rebase start captures its series");
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        assert!(
            matches!(saved.try_recv(), Ok(Some(Ok(())))),
            "the series is captured"
        );
        for args in rewrite {
            git(repo.path(), args);
        }
        let review =
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_series_review(window, cx)));
        review.clone().finish(cx);
        let review = review.expect("the review opens");
        draw(cx);
        (fixture, review, cx)
    }

    /// A key-down of Enter, not marked held, with no key-up after it: a
    /// fresh press, or under X11 each repeat of a held one; with `down`
    /// false, its key-up.
    fn enter(cx: &mut gpui::VisualTestContext, down: bool) {
        let keystroke = gpui::Keystroke::parse("enter").unwrap();
        if down {
            cx.simulate_event(gpui::KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            });
        } else {
            cx.simulate_event(gpui::KeyUpEvent { keystroke });
        }
        crate::tags::tests::draw(cx);
    }

    /// Return on a commit row of a rewritten series inspects that commit's
    /// files, as Return on a History commit loads its files, and the review
    /// stays open: the review dialog took the key for its default action,
    /// which closed the review before the series list's own Return was
    /// reached. Back to series takes the keys from the list the inspector
    /// replaces, so Escape goes back to the series, keeping the row, and
    /// closes the review from there: left on the list, which is no longer
    /// drawn, focus sent Escape to the window's root, where the review never
    /// saw it. Driven by the route a person takes: a
    /// series captured before its rewrite, the review opened on it, the row
    /// chosen with Down and Return and Escape dispatched as keys.
    #[gpui::test]
    async fn return_on_a_series_row_inspects_its_files_and_escape_closes_the_review(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::draw;
        // The newest commit reworded.
        let (_fixture, review, cx) = rewritten_review(
            cx,
            &[&[
                "commit",
                "--quiet",
                "--amend",
                "-m",
                "fixture notes 2, reworded",
            ]],
        )
        .await;
        let rows = review.read_with(cx, |review, _| {
            review.review.as_ref().map_or(0, |review| review.rows.len())
        });
        assert_eq!(rows, 2, "the rewritten series' two commits");
        assert!(
            cx.update(|window, cx| review.read(cx).focus.is_focused(window)),
            "the loaded series takes the keys"
        );

        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(review.read_with(cx, |review, _| review.selected), 1);
        cx.simulate_keystrokes("enter");
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Return leaves the review open"
        );
        let (inspecting, label, files) = review.read_with(cx, |review, _| {
            (
                review.inspecting,
                review.inspect_label.clone(),
                review
                    .files
                    .iter()
                    .map(|file| file.path().to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            )
        });
        assert!(inspecting, "Return inspects the selected commit");
        assert!(
            cx.update(|window, cx| review.read(cx).back.is_focused(window)),
            "Back to series takes the keys from the series list it replaces"
        );
        assert!(
            label.starts_with("Rewritten ") && label.ends_with("· fixture notes 2, reworded"),
            "the selected row's rewritten commit: {label:?}"
        );
        assert_eq!(files, ["notes.txt"], "its changed files are listed");
        assert!(
            cx.debug_bounds("series-files").is_some(),
            "the inspector draws its file list"
        );

        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape in the inspector goes back to the series"
        );
        assert_eq!(
            review.read_with(cx, |review, _| (review.inspecting, review.selected)),
            (false, 1),
            "the series keeps its row"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape on the series closes the review"
        );
    }

    /// A held Return on a series row inspects once. Under X11 a held key's
    /// repeats arrive as fresh presses, not marked held: the first inspected
    /// and moved focus to Back to series, the next pressed it, which focused
    /// the list again, the next inspected again, so the review toggled and
    /// started and cancelled a changed-files read with every repeat. Back to
    /// series acts on a press of its own, once the held one comes up.
    #[gpui::test]
    async fn a_held_return_on_a_series_row_inspects_once(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::draw;
        let (_fixture, review, cx) = rewritten_review(
            cx,
            &[&[
                "commit",
                "--quiet",
                "--amend",
                "-m",
                "fixture notes 2, reworded",
            ]],
        )
        .await;
        enter(cx, true);
        let generation = review.read_with(cx, |review, _| review.generation);
        assert!(
            review.read_with(cx, |review, _| review.inspecting),
            "the press inspects"
        );
        for repeat in 1..=4 {
            enter(cx, true);
            assert_eq!(
                review.read_with(cx, |review, _| (review.inspecting, review.generation)),
                (true, generation),
                "repeat {repeat} of the held Return leaves the inspection as it is"
            );
        }
        enter(cx, false);
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        assert_eq!(
            review.read_with(cx, |review, _| review.files.len()),
            1,
            "the one read lands"
        );
        enter(cx, true);
        assert!(
            !review.read_with(cx, |review, _| review.inspecting),
            "a press of its own on Back to series goes back to the series"
        );
        enter(cx, true);
        assert!(
            !review.read_with(cx, |review, _| review.inspecting),
            "and its repeats on the series list inspect nothing"
        );
    }

    /// Return on a row whose commit the rewrite dropped, which has only its
    /// original side, inspects that original commit's files: it inspected
    /// only a rewritten side, so Return there did nothing.
    #[gpui::test]
    async fn return_on_a_dropped_commits_row_inspects_its_original(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::draw;
        // The newest commit dropped.
        let (_fixture, review, cx) =
            rewritten_review(cx, &[&["reset", "--quiet", "--hard", "main~1"]]).await;
        let dropped = review.read_with(cx, |review, _| {
            review.review.as_ref().and_then(|review| {
                review
                    .rows
                    .iter()
                    .position(|row| row.rewritten.is_none() && row.original.is_some())
            })
        });
        let dropped = dropped.expect("a row with only its original side");
        for _ in 0..dropped {
            cx.simulate_keystrokes("down");
        }
        draw(cx);
        assert_eq!(review.read_with(cx, |review, _| review.selected), dropped);
        cx.simulate_keystrokes("enter");
        review.read_with(cx, |review, _| review.worker.wait_idle());
        draw(cx);
        let (inspecting, label, files) = review.read_with(cx, |review, _| {
            (
                review.inspecting,
                review.inspect_label.clone(),
                review
                    .files
                    .iter()
                    .map(|file| file.path().to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            )
        });
        assert!(inspecting, "Return inspects the dropped commit");
        assert!(
            label.starts_with("Original ") && label.ends_with("· fixture notes 2"),
            "its original side: {label:?}"
        );
        assert_eq!(files, ["notes.txt"], "its changed files are listed");
    }
}
