//! Native tag browser, annotation inspector, and explicit captured actions.
use crate::*;
use gitturtle_core::{RemoteConfig, Tag, TagCommand, TagDetails, TagList, WriteCommand};
use gpui_kit::{
    component::{WindowExt, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

const PREPARING: &str = "Reading tags…";

fn static_text(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    draft: Option<Entity<TagForm>>,
}

impl GitTurtle {
    pub(super) fn cancel_tag_action(&mut self) {
        self.tag_actions.generation = self.tag_actions.generation.wrapping_add(1);
        self.tag_actions.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }
    pub(super) fn open_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_tag_action(
            |repo| repo.tags(),
            |result, this, window, cx| match result {
                Ok(list) => {
                    let owner = cx.entity().downgrade();
                    let path = this.path.clone();
                    let browser = cx.new(|cx| TagBrowser::new(owner, path, list, window, cx));
                    window.open_alert_dialog(cx, move |dialog, _, _| {
                        dialog
                            .title(static_text("tags-dialog-title", "Tags"))
                            .width(px(600.))
                            .child(browser.clone())
                            .button_props(DialogButtonProps::default().ok_text("Done"))
                    });
                }
                Err(error) => this.operation_error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn read_tag_action<T: Send + 'static>(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(anyhow::Result<T>, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return false;
        }
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let path = repo.path().to_owned();
        self.cancel_tag_action();
        let generation = self.tag_actions.generation;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self.operations.submit_read(move || prepare(repo));
        self.tag_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Tag preparation was interrupted. Open Tags again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.tag_actions.generation != generation {
                    return;
                }
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                if this.path.as_ref() == Some(&path) && this.page == AppPage::Repository {
                    receive(result, this, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }
    fn inspect_tag(&mut self, tag: Tag, window: &mut Window, cx: &mut Context<Self>) {
        self.read_tag_action(
            move |repo| Ok((repo.tag_details(&tag)?, repo.remote_configs()?)),
            |result, this, window, cx| match result {
                Ok((details, remotes)) => this.show_tag_details(details, remotes, window, cx),
                Err(error) => this.operation_error = Some(format!("{error:#}")),
            },
            window,
            cx,
        );
    }
    fn show_tag_details(
        &mut self,
        details: TagDetails,
        remotes: Vec<RemoteConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let details = Arc::new(details);
        let remotes = Arc::new(remotes);
        let annotation = (details.tag.annotated && details.annotation_unavailable.is_none())
            .then(|| text::editor(&details.message, "text", None, window, cx));
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let p = palette(cx); let tag = &details.tag;
            let deletion = tag.clone(); let delete_owner = owner.clone(); let delete_path = path.clone();
            let oid = tag.oid.clone();
            let content = div().flex().flex_col().gap_3()
                .child(static_text("tag-target-identity", format!("{} · {} {}", if tag.annotated { "Annotated tag" } else { "Lightweight tag" }, tag.target_kind, tag.target_oid)).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .child(static_text("tag-object-identity", format!("Tag object: {}", tag.oid)).text_size(crate::appearance::ui_text(11.)))
                .when(!tag.tagger.is_empty(), |element| element.child(static_text("tag-author", format!("Tagged by {}", tag.tagger)).text_size(crate::appearance::ui_text(12.))))
                .when_some(annotation.as_ref(), |element, annotation| element.child(crate::editor_find::Editor::new(annotation).readonly(true).h(px(160.)).aria_label("Tag annotation and signature, if present")))
                .when_some(details.annotation_unavailable.as_ref(), |element, message| element.child(static_text("tag-annotation-unavailable", message.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
                .child(div().flex().gap_2()
                    .child(button("copy-tag-oid", "Copy object ID", "", false).on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(oid.clone()))))
                    .child(button("delete-local-tag", "Delete local tag…", "", false).on_click(move |_, window, cx| {
                        let _ = delete_owner.update(cx, |this, cx| {
                            if this.path == delete_path && this.operation_busy.is_none() {
                                window.close_dialog(cx);
                                this.confirm_git_write(format!("Delete local tag '{}'", deletion.name), format!("Remove the local name '{}' pointing to {}. Its annotation and signature will no longer be reachable through this name.\n\nRemote tags, branches, the index and working files stay as they are. This action refuses if the tag has moved since you opened it.", deletion.name, deletion.oid), "Delete local tag", WriteCommand::Tag(Arc::new(TagCommand::Delete(deletion.clone()))), window, cx);
                            }
                        });
                    })))
                .child(static_text("tag-push-heading", "Push this tag").text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM))
                .child(static_text("tag-push-consequences", "Choose one remote, then review. No other tags or branches are pushed. Existing remote tags are never replaced.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
                .child(div().id("tag-remote-list").max_h(px(160.)).overflow_y_scroll().flex().flex_col().gap_1().children(remotes.iter().enumerate().map(|(index, remote)| {
                    let remote = remote.clone(); let tag = tag.clone(); let owner = owner.clone(); let path = path.clone();
                    button(("push-tag-remote", index), format!("Push to {}…", remote.name), "", false).on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.path == path && this.operation_busy.is_none() {
                                window.close_dialog(cx);
                                let urls = if remote.push_urls.is_empty() { &remote.urls } else { &remote.push_urls };
                                let destinations = urls.iter().map(|url| crate::workspace::display_remote_url(url)).collect::<Vec<_>>().join("\n");
                                this.confirm_git_write(format!("Push tag '{}'", tag.name), format!("Send only '{}' ({}) to remote '{}':\n{}\n\nThis explicit network action creates the named remote tag. It does not push branches, additional tags, or overwrite an existing remote tag.", tag.name, tag.oid, remote.name, destinations), "Push named tag", WriteCommand::Tag(Arc::new(TagCommand::Push { tag: tag.clone(), remote: Arc::new(remote.clone()) })), window, cx);
                            }
                        });
                    })
                })).when(remotes.is_empty(), |element| element.child(static_text("tag-remotes-empty", "No remotes configured. Add a remote from the branch menu to push a tag.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))));
            dialog.title(static_text("tag-inspector-title", details.tag.name.clone())).width(px(640.)).child(content).button_props(DialogButtonProps::default().ok_text("Done"))
        });
    }
    pub(super) fn finish_tag_write(
        &mut self,
        path: &std::path::Path,
        command: &TagCommand,
        succeeded: bool,
        cx: &mut Context<Self>,
    ) {
        if !succeeded {
            return;
        }
        let TagCommand::Create(plan) = command else {
            return;
        };
        if self.tag_actions.draft.as_ref().is_some_and(|draft| {
            let draft = draft.read(cx);
            draft.path.as_deref() == Some(path)
                && draft.name.read(cx).value().trim() == plan.name
                && draft
                    .annotated
                    .then(|| draft.message.read(cx).value().to_string())
                    == plan.annotation
        }) {
            self.tag_actions.draft = None;
        }
    }
    fn open_create_tag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let target = self
            .selected_commit
            .and_then(|index| self.commits.get(index))
            .map(|commit| commit.oid.clone())
            .unwrap_or_else(|| "HEAD".into());
        let form = self
            .tag_actions
            .draft
            .as_ref()
            .filter(|form| form.read(cx).path == path)
            .cloned()
            .unwrap_or_else(|| cx.new(|cx| TagForm::new(owner, path, target, window, cx)));
        self.tag_actions.draft = Some(form.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(static_text("create-tag-dialog-title", "Create a local tag"))
                .width(px(560.))
                .child(form.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Review tag")
                        .cancel_text("Cancel")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |this, cx| this.submit(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |form, cx| {
                        form.pending = false;
                        let _ = form.owner.update(cx, |this, _| this.cancel_tag_action());
                    });
                    true
                })
        });
    }
}

struct TagBrowser {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    list: TagList,
    query: Entity<InputState>,
    _subscription: Subscription,
}
impl TagBrowser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        list: TagList,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx
            .new(|cx| InputState::new(window, cx).placeholder("Filter tags by name or object ID"));
        let subscription = cx.subscribe_in(&query, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event
                && let Some(tag) = this.matches(cx).first()
            {
                this.activate((*tag).clone(), window, cx);
            }
            cx.notify();
        });
        Self {
            owner,
            path,
            list,
            query,
            _subscription: subscription,
        }
    }
    fn matches(&self, cx: &App) -> Vec<&Tag> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        self.list
            .tags
            .iter()
            .filter(|tag| {
                tag.name.to_lowercase().contains(&query)
                    || tag.oid.contains(&query)
                    || tag.target_oid.contains(&query)
            })
            .take(101)
            .collect()
    }
    fn activate(&self, tag: Tag, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.owner.update(cx, |this, cx| {
            if this.path == self.path && this.operation_busy.is_none() {
                window.close_dialog(cx);
                this.inspect_tag(tag, window, cx);
            }
        });
    }
}
impl Render for TagBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let matches = self.matches(cx);
        div().flex().flex_col().gap_3()
            .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.query).aria_label("Filter local tags").cleanable(true))).child(button("create-tag", "Create tag…", "plus", false).on_click(cx.listener(|this, _, window, cx| {
                let _ = this.owner.update(cx, |owner, cx| { if owner.path == this.path && owner.operation_busy.is_none() { window.close_dialog(cx); owner.open_create_tag(window, cx); } });
            }))))
            .child(static_text("tag-list-summary", format!("{} local tags · Select to inspect, delete locally, or push one named tag.", self.list.tags.len())).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .child(div().id("tags-list").max_h(px(360.)).overflow_y_scroll().flex().flex_col().gap_1().children(matches.iter().take(100).enumerate().map(|(index, tag)| {
                let tag = (*tag).clone(); let label = format!("{} · {} · {}", tag.name, if tag.annotated { "Annotated" } else { "Lightweight" }, short_oid(&tag.target_oid));
                Button::new(("tag-row", index)).ghost().w_full().h(crate::appearance::ui_size(34.)).label(label.clone()).accessibility_label(label).on_click(cx.listener(move |this, _, window, cx| this.activate(tag.clone(), window, cx)))
            })).when(matches.is_empty(), |element| element.child(static_text("tag-list-empty", if self.list.tags.is_empty() { "No local tags yet. Create a tag to name a commit." } else { "No tags match this filter." }).p_3().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))))
            .when(matches.len() > 100, |element| element.child(static_text("tag-list-match-limit", "Showing 100 matches. Narrow the filter to find another tag.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .when(self.list.truncated, |element| element.child(static_text("tag-list-load-limit", "Loaded the first 10,000 local tags by name. Additional tags are outside this bounded browser.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.warning))))
    }
}

struct TagForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    name: Entity<InputState>,
    target: Entity<InputState>,
    message: Entity<TextareaState>,
    annotated: bool,
    error: Option<String>,
    pending: bool,
}
impl TagForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            owner,
            path,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("v1.0.0")),
            target: cx.new(|cx| InputState::new(window, cx).default_value(target)),
            message: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("Describe this tag")
                    .rows(4)
            }),
            annotated: true,
            error: None,
            pending: false,
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let name = self.name.read(cx).value().trim().to_owned();
        let target = self.target.read(cx).value().trim().to_owned();
        let message = self
            .annotated
            .then(|| self.message.read(cx).value().to_string());
        let form = cx.entity().downgrade();
        let accepted = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return false; }
            this.read_tag_action(move |repo| repo.create_tag_plan(&name, &target, message), move |result, this, window, cx| {
                let _ = form.update(cx, |form, cx| { form.pending = false; cx.notify(); });
                match result {
                    Ok(plan) => {
                        window.close_dialog(cx);
                        let explanation = format!("Create local tag '{}' at commit {}.\n\n{}{}\n\n{}\n\nNo remote is contacted. Your branches, index, and working files stay as they are.", plan.name, plan.target_oid, if plan.annotation.is_some() { "Annotated tag." } else { "Lightweight tag." }, if plan.signing { " Git's configured signing is enabled." } else { " Git's configured signing behavior is preserved." }, plan.annotation.as_deref().unwrap_or("This tag has no annotation."));
                        this.confirm_git_write("Create local tag".into(), explanation, "Create tag", WriteCommand::Tag(Arc::new(TagCommand::Create(plan))), window, cx);
                    }
                    Err(error) => { let _ = form.update(cx, |form, cx| { form.error = Some(format!("{error:#}")); cx.notify(); }); }
                }
            }, window, cx)
        }).unwrap_or(false);
        self.pending = accepted;
        if !accepted {
            self.error = Some("The repository changed or another operation is running. Reopen Create tag when it finishes.".into());
        }
        cx.notify();
    }
}
impl Render for TagForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div().flex().flex_col().gap_3()
            .child(static_text("tag-name-label", "Tag name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.name).aria_label("Tag name"))
            .child(static_text("tag-target-label", "Target commit (selected commit by default)").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.target).aria_label("Target commit for tag"))
            .child(div().flex().gap_2().children([(false, "Lightweight"), (true, "Annotated")].map(|(annotated, label)| button(label, label, "", self.annotated == annotated).toggled(self.annotated == annotated).disabled(self.pending).on_click(cx.listener(move |this, _, _, cx| { this.annotated = annotated; this.error = None; cx.notify(); })))))
            .when(self.annotated, |element| element.child(Textarea::new(&self.message).aria_label("Tag annotation")))
            .child(static_text("tag-signing-explanation", "Signing follows Git configuration. Signing failures are reported without an unsigned fallback.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when(self.pending, |element| element.child(static_text("tag-preparation-status", "Preparing exact target and signing settings…").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| static_text("tag-preparation-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
    }
}
