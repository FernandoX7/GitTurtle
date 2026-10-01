//! Native tag browser, annotation inspector, and explicit captured actions.
use crate::*;
use gitturtle_core::{RemoteConfig, Tag, TagCommand, TagDetails, TagList, WriteCommand};
use gpui_kit::{
    component::{WindowExt, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

const PREPARING: &str = "Reading tags…";
/// The gap the kit's dialog leaves between its body and its footer, its
/// default 16 px padding. A dialog whose body keeps the focus ring's room
/// below its last control gives the room back by narrowing this gap.
pub(crate) const DIALOG_FOOTER_GAP: Pixels = px(16.);

fn static_text(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .debug_selector(|| id.into())
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
                    window.open_alert_dialog(cx, move |dialog, _, cx| {
                        // The dialog clips its body to the body's bounds, and
                        // the browser keeps the focus ring's room inside them
                        // above Create tag… and below the last row. The
                        // title's margin and the footer's gap give the room
                        // back, so nothing in the dialog moves.
                        let room = appearance::button_ring_room(cx);
                        dialog
                            .title(static_text("tags-dialog-title", "Tags").mb(-room))
                            .width(px(600.))
                            .gap(DIALOG_FOOTER_GAP - room)
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
            // The Push to… list scrolls, so it clips its buttons to its bounds
            // on both axes: it keeps the focus ring's room around them and
            // gives it back through its margin. The content keeps the room
            // below its last control inside the dialog's clip of its body,
            // and the footer's gap gives that back, so nothing moves.
            let room = appearance::button_ring_room(cx);
            let content = div().flex().flex_col().gap_3().pb(room)
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
                .child(div().id("tag-remote-list").debug_selector(|| "tag-remote-list".into()).max_h(px(160.) + room * 2.).p(room).m(-room).overflow_y_scroll().flex().flex_col().gap_1().children(remotes.iter().enumerate().map(|(index, remote)| {
                    let remote = remote.clone(); let tag = tag.clone(); let owner = owner.clone(); let path = path.clone();
                    button(("push-tag-remote", index), format!("Push to {}…", remote.name), "", false).debug_selector(move || format!("push-tag-remote-{index}")).on_click(move |_, window, cx| {
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
            dialog.title(static_text("tag-inspector-title", details.tag.name.clone())).width(px(640.)).gap(DIALOG_FOOTER_GAP - room).child(content).button_props(DialogButtonProps::default().ok_text("Done"))
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
        // The browser keeps the focus ring's room above Create tag… and below
        // its last control inside the dialog's clip, which the dialog gives
        // back. The list scrolls, so it clips its rows to its bounds on both
        // axes: it keeps the room around them and gives it back through its
        // margin, so no row moves.
        let room = appearance::button_ring_room(cx);
        div().flex().flex_col().gap_3().py(room)
            .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.query).aria_label("Filter local tags").cleanable(true))).child(button("create-tag", "Create tag…", "plus", false).debug_selector(|| "create-tag".into()).on_click(cx.listener(|this, _, window, cx| {
                let _ = this.owner.update(cx, |owner, cx| { if owner.path == this.path && owner.operation_busy.is_none() { window.close_dialog(cx); owner.open_create_tag(window, cx); } });
            }))))
            .child(static_text("tag-list-summary", format!("{} local tags · Select to inspect, delete locally, or push one named tag.", self.list.tags.len())).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .child(div().id("tags-list").max_h(px(360.) + room * 2.).p(room).m(-room).overflow_y_scroll().flex().flex_col().gap_1().children(matches.iter().take(100).enumerate().map(|(index, tag)| {
                let tag = (*tag).clone(); let label = format!("{} · {} · {}", tag.name, if tag.annotated { "Annotated" } else { "Lightweight" }, short_oid(&tag.target_oid));
                Button::new(("tag-row", index)).debug_selector(move || format!("tag-row-{index}")).ghost().w_full().h(crate::appearance::ui_size(34.)).label(label.clone()).accessibility_label(label).on_click(cx.listener(move |this, _, window, cx| this.activate(tag.clone(), window, cx)))
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::component::{FocusRing, Root, Theme};
    use std::{cell::RefCell, rc::Rc};

    /// Runs Git in `path` with no configuration beyond the fixture identity.
    pub(crate) fn git(path: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Three tagged commits: three local tags and three entries in HEAD's
    /// reflog.
    pub(crate) fn tagged_repository(directory: &std::path::Path) -> GitRepository {
        let repo = GitRepository::init(directory.join("repository"), "main").unwrap();
        for index in 0..3 {
            let message = format!("Commit {index}");
            git(
                repo.path(),
                &["commit", "--quiet", "--allow-empty", "-m", &message],
            );
            git(repo.path(), &["tag", &format!("v0.{index}")]);
        }
        repo
    }

    /// The application in a window on `repo`, with its palette and focus ring
    /// installed and dialogs opening at rest.
    pub(crate) fn window<'a>(
        cx: &'a mut TestAppContext,
        repo: &GitRepository,
    ) -> (Entity<GitTurtle>, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
            // A dialog otherwise slides in on a wall-clock animation.
            cx.set_reduce_motion(true);
        });
        let repo = repo.clone();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                // Finish startup's preference reads before GPUI first polls
                // their reply, as `commit_message`'s fixture does.
                futures::executor::block_on(app.preferences_writer.submit(|| Ok(())))
                    .expect("startup preferences worker replied")
                    .expect("startup preferences queue drained");
                app.page = AppPage::Repository;
                app.path = Some(repo.path().to_owned());
                app.repository = Some(repo);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        draw(cx);
        (app, cx)
    }

    pub(crate) fn draw(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.executor().run_until_parked();
        }
    }

    pub(crate) fn rendered(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("rendered {selector}"))
    }

    fn logical(bounds: Bounds<ScaledPixels>, scale: f32) -> Bounds<Pixels> {
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        Bounds::from_corners(
            point(logical(bounds.left()), logical(bounds.top())),
            point(logical(bounds.right()), logical(bounds.bottom())),
        )
    }

    /// Moves the pointer onto the dialog's backdrop, where it hovers nothing.
    fn park_pointer(cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        draw(cx);
    }

    /// The content mask `control` paints in, read from the fill it paints
    /// under the pointer: GPUI keeps a filled quad's mask whole, and that mask
    /// is the intersection of every ancestor's.
    pub(crate) fn content_mask(
        cx: &mut VisualTestContext,
        control: &'static str,
    ) -> Bounds<Pixels> {
        let surface = rendered(cx, control);
        cx.simulate_mouse_move(surface.center(), None, Modifiers::default());
        draw(cx);
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = px(1. / scale);
            let masks = window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    let drawn = logical(quad.bounds, scale);
                    !quad.background.is_transparent()
                        && [
                            (drawn.left(), surface.left()),
                            (drawn.top(), surface.top()),
                            (drawn.right(), surface.right()),
                            (drawn.bottom(), surface.bottom()),
                        ]
                        .into_iter()
                        .all(|(drawn, expected)| (drawn - expected).abs() < device)
                })
                .map(|quad| logical(quad.content_mask.bounds, scale))
                .collect::<Vec<_>>();
            match masks.as_slice() {
                [mask, rest @ ..] if rest.iter().all(|other| other == mask) => *mask,
                _ => panic!("one hovered fill of {control} at {surface:?}, masked by {masks:?}"),
            }
        })
    }

    /// `mask` holds `ring`, within float error of the device pixels both lie
    /// on.
    fn holds(mask: Bounds<Pixels>, ring: Bounds<Pixels>) -> bool {
        let slack = px(0.01);
        mask.left() <= ring.left() + slack
            && mask.top() <= ring.top() + slack
            && mask.right() + slack >= ring.right()
            && mask.bottom() + slack >= ring.bottom()
    }

    /// Where the window painted each quad, then where each of `selectors`
    /// lies.
    fn layout(
        cx: &mut VisualTestContext,
        selectors: &[&'static str],
    ) -> (Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>) {
        let quads = cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .map(|quad| logical(quad.bounds, scale))
                .collect()
        });
        let elements = selectors
            .iter()
            .map(|selector| rendered(cx, selector))
            .collect();
        (quads, elements)
    }

    /// The first painted quad or element of `selectors` that lies elsewhere
    /// in `after` than in `before`.
    fn first_move(
        (before_quads, before_elements): &(Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>),
        (after_quads, after_elements): &(Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>),
        selectors: &[&'static str],
    ) -> Option<String> {
        if before_quads.len() != after_quads.len() {
            return Some(format!(
                "the painted quads: {} instead of {}",
                after_quads.len(),
                before_quads.len()
            ));
        }
        let quad = before_quads
            .iter()
            .zip(after_quads)
            .enumerate()
            .find(|(_, (before, after))| before != after)
            .map(|(index, (before, after))| {
                format!("painted quad {index} from {before:?} to {after:?}")
            });
        quad.or_else(|| {
            selectors
                .iter()
                .zip(before_elements.iter().zip(after_elements))
                .find(|(_, (before, after))| before != after)
                .map(|(selector, (before, after))| {
                    format!("{selector} from {before:?} to {after:?}")
                })
        })
    }

    /// A wheel step down at `position`, which every scrolling container under
    /// it takes as far as it can move.
    fn wheel(cx: &mut VisualTestContext, position: Point<Pixels>) {
        scroll_down(cx, position, ScrollDelta::Lines(point(0., -3.)));
    }

    /// Scrolls every container under `position` down by `delta`, as far as
    /// each can move.
    pub(crate) fn scroll_down(
        cx: &mut VisualTestContext,
        position: Point<Pixels>,
        delta: ScrollDelta,
    ) {
        cx.simulate_mouse_move(position, None, Modifiers::default());
        draw(cx);
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta,
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        draw(cx);
    }

    pub(crate) fn install_ring(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        ring: FocusRing,
    ) {
        cx.update(|window, cx| {
            Theme::global_mut(cx).button_focus_ring = ring;
            app.update(cx, |_, cx| cx.notify());
            window.refresh();
        });
        draw(cx);
    }

    /// Each of `controls` lies in clipping containers. Grown by the installed
    /// Button focus ring's width plus gap, each lies inside the content mask it
    /// paints in. The room that keeps it there moves nothing: the content
    /// fits, and a wheel step over any control scrolls nothing, because the
    /// room's overhang adds nothing a container can scroll to; and laid out
    /// without the room, as a ring taking none lays out, every painted quad,
    /// control and element of `fixed` sits where it does with the room, and
    /// every control's ring is clipped.
    pub(crate) fn assert_room_for_rings(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        controls: &[&'static str],
        fixed: &[&'static str],
    ) {
        assert_rings_whole(cx, controls);
        let selectors = [controls, fixed].concat();
        park_pointer(cx);
        let with_room = layout(cx, &selectors);
        for &control in controls {
            let center = rendered(cx, control).center();
            wheel(cx, center);
            park_pointer(cx);
            if let Some(moved) = first_move(&with_room, &layout(cx, &selectors), &selectors) {
                panic!("a wheel step over {control} scrolls content that fits: it moves {moved}");
            }
        }
        assert_room_moves_nothing(cx, app, controls, fixed);
    }

    /// Grown by the installed Button focus ring's width plus gap, each of
    /// `controls` lies inside the content mask it paints in, the intersection
    /// of every ancestor's.
    pub(crate) fn assert_rings_whole(cx: &mut VisualTestContext, controls: &[&'static str]) {
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(installed, appearance::BUTTON_FOCUS_RING);
        let footprint = installed.gap + installed.width;
        let mut clipped = Vec::new();
        for &control in controls {
            let mask = content_mask(cx, control);
            let ring = rendered(cx, control).dilate(footprint);
            if !holds(mask, ring) {
                clipped.push(format!("{control}: ring {ring:?}, mask {mask:?}"));
            }
        }
        assert!(
            clipped.is_empty(),
            "rings lie outside their content masks:\n{}",
            clipped.join("\n")
        );
    }

    /// Laid out without the ring's room, as a ring taking none lays out, every
    /// painted quad, control and element of `fixed` sits where it does with
    /// the room, and the ring around every control is clipped. The installed
    /// ring is restored afterwards.
    pub(crate) fn assert_room_moves_nothing(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        controls: &[&'static str],
        fixed: &[&'static str],
    ) {
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        let footprint = installed.gap + installed.width;
        let selectors = [controls, fixed].concat();
        park_pointer(cx);
        let with_room = layout(cx, &selectors);
        install_ring(
            cx,
            app,
            FocusRing {
                width: px(0.),
                gap: px(0.),
                ..installed
            },
        );
        park_pointer(cx);
        if let Some(moved) = first_move(&layout(cx, &selectors), &with_room, &selectors) {
            panic!("the room moves {moved}");
        }
        for &control in controls {
            let mask = content_mask(cx, control);
            let ring = rendered(cx, control).dilate(footprint);
            assert!(
                !holds(mask, ring),
                "without room the ring around {control}, {ring:?}, is clipped to {mask:?}"
            );
        }
        install_ring(cx, app, installed);
    }

    /// The gap the kit's dialog leaves between its body and its footer when
    /// nothing overrides it, measured on a dialog of its own.
    pub(crate) fn kit_dialog_footer_gap(cx: &mut TestAppContext) -> Pixels {
        /// A window that shows nothing but its dialogs.
        struct Blank;
        impl Render for Blank {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .children(Root::render_dialog_layer(window, cx))
            }
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Blank), window, cx));
        cx.update(|window, cx| {
            window.open_alert_dialog(cx, |dialog, _, _| {
                // Both are taller than the dialog's minimum height would
                // stretch its body to.
                dialog
                    .child(div().debug_selector(|| "probe-body".into()).h(px(80.)))
                    .footer(div().debug_selector(|| "probe-footer".into()).h(px(80.)))
            })
        });
        draw(cx);
        rendered(cx, "probe-footer").top() - rendered(cx, "probe-body").bottom()
    }

    /// The Tags dialog gives its room back through the footer gap, taking it
    /// to be the kit's default; any other default would move the footer.
    #[gpui::test]
    fn dialog_footer_gap_is_the_kits(cx: &mut TestAppContext) {
        assert_eq!(kit_dialog_footer_gap(cx), DIALOG_FOOTER_GAP);
    }

    /// The dialog clips its body to the body's bounds, and the tag list
    /// scrolls, so GPUI clips it to the list's bounds on both axes. Create
    /// tag…, at the top of the body, and the first and last rows keep their
    /// rings whole, and nothing moves.
    #[gpui::test]
    async fn tag_browser_keeps_room_for_every_focus_ring(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tags are read").await;
        draw(cx);
        assert_room_for_rings(
            cx,
            &app,
            &["create-tag", "tag-row-0", "tag-row-2"],
            &["tags-dialog-title", "tag-list-summary", "tag-row-1"],
        );
    }

    /// The Push to… list scrolls, so GPUI clips it to its bounds on both
    /// axes, and it ends the dialog's body, which the dialog clips. The first
    /// and last Push to… buttons keep their rings whole, and nothing moves.
    #[gpui::test]
    async fn tag_inspector_keeps_room_for_every_push_ring(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        // Local bare remotes: the inspector only lists them, and nothing is
        // fetched or pushed.
        for remote in ["origin", "upstream"] {
            let bare = fixture.path().join(format!("{remote}.git"));
            git(
                fixture.path(),
                &["init", "--quiet", "--bare", bare.to_str().unwrap()],
            );
            git(
                repo.path(),
                &["remote", "add", remote, bare.to_str().unwrap()],
            );
        }
        let tag = repo.tags().unwrap().tags[0].clone();
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tag is read").await;
        draw(cx);
        assert_room_for_rings(
            cx,
            &app,
            &["push-tag-remote-0", "push-tag-remote-1"],
            &[
                "tag-inspector-title",
                "tag-target-identity",
                "tag-push-heading",
                "tag-push-consequences",
            ],
        );
    }
}
