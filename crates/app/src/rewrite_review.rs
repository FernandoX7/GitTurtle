//! A retained native commit-series inspector and a separate reviewed lease push.
use crate::*;
use gitturtle_core::{
    LeasedPublishPlan, OperationControl, PublicationInspection, RewriteReview, WriteCommand,
};
use gpui_kit::component::{WindowExt, dialog::DialogFooter};
use gpui_kit::prelude::FluentBuilder;

impl GitTurtle {
    pub(super) fn open_rewrite_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let remote = self.remote_name.read(cx).value().trim().to_owned();
        let branch = self.remote_branch.read(cx).value().trim().to_owned();
        let form = cx.new(|cx| SeriesBrowser::new(owner, repo, remote, branch, window, cx));
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let close = form.clone();
            let cancel = form.clone();
            dialog
                .title("Review rewritten series")
                .width(px(980.))
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
}

struct SeriesBrowser {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    metadata: SerialExecutor,
    worker: Worker,
    control: OperationControl,
    network_control: Option<OperationControl>,
    generation: u64,
    task: Option<Task<()>>,
    review: Option<Arc<RewriteReview>>,
    selected: usize,
    focus: FocusHandle,
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
            task: None,
            review: None,
            selected: 0,
            focus: cx.focus_handle(),
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
        self.content = None;
        self.editor = None;
        self.patch = None;
        self.selected_file = None;
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
        let editor = text::editor(
            value,
            if self.text_mode == 0 { "diff" } else { "text" },
            (self.text_mode == 0).then_some(presentation.as_ref()),
            window,
            cx,
        );
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
                    owner.operation_busy = None; owner.finish_operation_control(window, cx);
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
    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
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
                        rich_preview::render_comparison(preview, false, self.owner.clone(), cx)
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
                rich_preview::render_comparison(preview, false, self.owner.clone(), cx)
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
        div().id("rewrite-review-body").w_full().min_w_0().max_h(px(height)).when(self.inspecting, |element| element.h(px(height))).overflow_y_scroll().flex().flex_col().gap_2()
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| { this.back(window, cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| { this.back(window, cx); cx.stop_propagation(); }))
            .when(!self.inspecting, |element| element
                .child(div().text_size(appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Original → rewritten · unique patch fingerprints identify likely correspondence. Changed patches may match by unique subject only. Ambiguous, combined and empty commits remain explicit; inspect both sides before publishing."))
                .when_some(self.review.as_ref(), |element, review| element.child(div().text_size(appearance::ui_text(11.)).child(format!("{} · original {} → rewritten {} · base {}", review.branch, short_oid(&review.original_head), short_oid(&review.rewritten_head), short_oid(&review.base)))))
                .child(div().id("series-focus-list").w_full().min_w_0().h(list_height).flex_shrink_0().border_1().border_color(rgb(p.border)).focus_visible(|style| style.border_color(rgb(p.accent))).rounded(px(6.)).overflow_hidden().tab_stop(true).track_focus(&self.focus).role(Role::ListBox).aria_label("Original and rewritten commits").aria_description("Up and Down select a commit pair. Return inspects rewritten files.")
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| { match event.keystroke.key.as_str() { "up" => this.select(this.selected.saturating_sub(1), window, cx), "down" => this.select(this.selected + 1, window, cx), "enter" => this.inspect(false, window, cx), _ => return } cx.stop_propagation(); }))
                    .child(uniform_list("rewrite-series-list", row_count, cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| this.row(index, cx)).collect::<Vec<_>>())).size_full().track_scroll(&self.rows_scroll)))
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
                .child(div().flex().flex_wrap().items_center().gap_2().child(button("back-rewrite-series", "Back to series", "arrow-left", false).on_click(cx.listener(|this, _, window, cx| this.back(window, cx)))).child(div().text_size(appearance::ui_text(12.)).child(self.inspect_label.clone())))
                .when(self.messages.is_none(), |element| element.child(div().h(px(130.)).flex_shrink_0().border_1().border_color(rgb(p.border)).rounded(px(6.)).overflow_hidden().child(uniform_list("series-files", self.files.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| range.map(|index| {
                    let file = &this.files[index]; button(("series-file", index), file.path().to_string_lossy().into_owned(), "", this.selected_file == Some(index)).w_full().h(appearance::ui_size(30.)).on_click(cx.listener(move |this, _, window, cx| this.file(index, window, cx))).into_any_element()
                }).collect::<Vec<_>>())).size_full().track_scroll(&self.files_scroll))))
                .when(matches!(self.content.as_deref(), Some(Content::Text { .. })), |element| element.child(div().flex().gap_2().children(["Patch", "Before source", "After source", "Diagrams"].into_iter().enumerate().filter(|(mode,_)| *mode != 3 || matches!(self.content.as_deref(), Some(Content::Text {diagrams:Some(_),..}))).map(|(mode, label)| button(("series-text-mode", mode), label, "", self.text_mode == mode).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.prepare_editor(window, cx); cx.notify(); }))))))
                .child(div().flex_1().min_h(px(100.)).border_1().border_color(rgb(p.border)).overflow_hidden().child(self.render_content(cx))))
            .when(self.pending, |element| element.child(div().text_color(rgb(p.muted)).child("Reading captured local history…")))
            .when_some(self.error.as_ref(), |element, error| element.child(div().max_h(px(100.)).id("series-review-error").overflow_y_scroll().text_size(appearance::ui_text(12.)).text_color(rgb(p.warning)).child(error.clone())))
    }
}
