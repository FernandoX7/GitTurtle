//! Native tag browser, annotation inspector, and explicit captured actions.
use crate::*;
use focus_reveal::{FocusReveal, Trigger};
use gitturtle_core::{RemoteConfig, Tag, TagCommand, TagDetails, TagList, WriteCommand};
use gpui_kit::{
    component::{
        WindowExt,
        dialog::DialogButtonProps,
        scroll::{Scrollbar, ScrollbarHandle, ScrollbarMode},
    },
    prelude::FluentBuilder,
};

const PREPARING: &str = "Reading tags…";
/// The gap the kit's dialog leaves between its body and its footer, its
/// default 16 px padding. A dialog whose body keeps the focus ring's room
/// below its last control gives the room back by narrowing this gap.
pub(crate) const DIALOG_FOOTER_GAP: Pixels = px(16.);

/// The always-visible scrollbar a scrolling list of tab-stop rows cues the
/// rows beyond its edge with, as the review dialogs cue theirs: the branch
/// chooser, the remote manager, Tags and the tag inspector's Push to… list.
///
/// While the list overflows, its rows keep a gutter at their right, and the
/// scrollbar paints its track there, over the column the rows lie in: inside
/// the list, clear of the room it keeps around its rows for their focus
/// rings, so the track covers no row and no ring, and clear of the dialog's
/// side padding, where the dialog's own scrollbar runs. The gutter is the
/// scrollbar's width plus the room of the ring every palette installs, so a
/// row's ring ends where the track begins, and neither the rows nor the track
/// move with the ring's room, which the list gives back. A list whose rows
/// fit keeps no gutter and draws no scrollbar, so its rows keep their width.
///
/// GPUI lays a frame out before it knows whether the list overflows, so the
/// gutter follows the overflow the list's scroll handle kept from the last
/// frame. A frame whose layout overflows the other way draws once more after
/// it is painted; each frame draws the gutter and the scrollbar together or
/// neither, so a thumb never paints over a row.
pub(crate) struct ScrollCue {
    rows: RowColumn,
    /// Whether this frame draws the gutter and the scrollbar.
    cued: bool,
}

impl ScrollCue {
    /// The cue for the list `scroll` tracks, which keeps `room` around its
    /// rows for their rings, for the frame being rendered.
    pub(crate) fn new(scroll: &ScrollHandle, room: Pixels) -> Self {
        Self {
            rows: RowColumn {
                scroll: scroll.clone(),
                room,
            },
            cued: overflows(scroll),
        }
    }

    /// For the list's `on_children_prepainted`: runs `prepainted`, then draws
    /// once more when this frame's layout overflows otherwise than the cue
    /// assumed.
    pub(crate) fn observe(
        &self,
        prepainted: impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let scroll = self.rows.scroll.clone();
        let cued = self.cued;
        move |bounds, window, cx| {
            prepainted(bounds, window, cx);
            if overflows(&scroll) != cued {
                window.defer(cx, |window, _| window.refresh());
            }
        }
    }

    /// `list`, with the gutter and the scrollbar `id` while it overflows.
    pub(crate) fn list(&self, list: Stateful<Div>, id: &'static str) -> Div {
        let ring = appearance::BUTTON_FOCUS_RING;
        let gutter = Scrollbar::width() + ring.gap + ring.width;
        let room = self.rows.room;
        div()
            .flex()
            .flex_col()
            .child(list.when(self.cued, |list| list.pr(room + gutter)))
            .when(self.cued, |cue| {
                cue.child(
                    Scrollbar::vertical(&self.rows)
                        .id(id)
                        .mode(ScrollbarMode::Always),
                )
            })
    }
}

/// A list's scroll as its scrollbar reads it: over the column its rows lie
/// in, the list's bounds less the room it keeps around them, which scrolls
/// as far as the list does.
#[derive(Clone)]
struct RowColumn {
    scroll: ScrollHandle,
    room: Pixels,
}

impl ScrollbarHandle for RowColumn {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.scroll.bounds().dilate(-self.room)
    }

    fn offset(&self) -> Point<Pixels> {
        self.scroll.offset()
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.scroll.set_offset(offset);
    }

    fn content_size(&self) -> Size<Pixels> {
        let viewport = self.viewport_bounds().size;
        let reach = self.scroll.max_offset();
        size(viewport.width + reach.x, viewport.height + reach.y)
    }
}

/// Whether the list `scroll` tracks overflowed when last laid out, as the
/// kit's scrollbar judges it before painting a thumb.
fn overflows(scroll: &ScrollHandle) -> bool {
    scroll.max_offset().y > Pixels::ZERO
}

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
    /// The Tags list or Push to… list opened last, for tests.
    #[cfg(test)]
    list: Option<FocusReveal>,
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
                    #[cfg(test)]
                    {
                        this.tag_actions.list = Some(browser.read(cx).rows.clone());
                    }
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
        // The Push to… list's scroll, which reveals the button Tab moves onto.
        let list = FocusReveal::new(Trigger::Keyboard);
        #[cfg(test)]
        {
            self.tag_actions.list = Some(list.clone());
        }
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            // A button focus has moved onto scrolls into view in this frame.
            list.reveal(window, cx);
            let p = palette(cx); let tag = &details.tag;
            let deletion = tag.clone(); let delete_owner = owner.clone(); let delete_path = path.clone();
            let oid = tag.oid.clone();
            // The Push to… list scrolls, so it clips its buttons to its bounds
            // on both axes: it keeps the focus ring's room around them and
            // gives it back through its margin. The content keeps the room
            // below its last control inside the dialog's clip of its body,
            // and the footer's gap gives that back, so nothing moves. A
            // button Tab moves onto scrolls into view with its ring.
            let room = appearance::button_ring_room(cx);
            let pushes: Vec<_> = (0..remotes.len()).map(|index| list.focus(("push-tag-remote", index), cx)).collect();
            let reveal = list.items(pushes.clone(), room);
            // Past its height the list cues the rest with its scrollbar.
            let cue = ScrollCue::new(list.scroll(), room);
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
                .child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("tag-remote-list").debug_selector(|| "tag-remote-list".into()).max_h(px(160.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(list.scroll()).flex().flex_col().gap_1().children(remotes.iter().zip(pushes).enumerate().map(|(index, (remote, focus))| {
                    let remote = remote.clone(); let tag = tag.clone(); let owner = owner.clone(); let path = path.clone();
                    button(("push-tag-remote", index), format!("Push to {}…", remote.name), "", false).track_focus(&focus).debug_selector(move || format!("push-tag-remote-{index}")).on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.path == path && this.operation_busy.is_none() {
                                window.close_dialog(cx);
                                let urls = if remote.push_urls.is_empty() { &remote.urls } else { &remote.push_urls };
                                let destinations = urls.iter().map(|url| crate::workspace::display_remote_url(url)).collect::<Vec<_>>().join("\n");
                                this.confirm_git_write(format!("Push tag '{}'", tag.name), format!("Send only '{}' ({}) to remote '{}':\n{}\n\nThis explicit network action creates the named remote tag. It does not push branches, additional tags, or overwrite an existing remote tag.", tag.name, tag.oid, remote.name, destinations), "Push named tag", WriteCommand::Tag(Arc::new(TagCommand::Push { tag: tag.clone(), remote: Arc::new(remote.clone()) })), window, cx);
                            }
                        });
                    })
                })).when(remotes.is_empty(), |element| element.child(static_text("tag-remotes-empty", "No remotes configured. Add a remote from the branch menu to push a tag.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))), "tag-remote-scrollbar"));
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
    /// The list's scroll, which reveals the row Tab moves onto.
    rows: FocusReveal,
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
            rows: FocusReveal::new(Trigger::Keyboard),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A row focus has moved onto scrolls into view in this frame.
        self.rows.reveal(window, cx);
        let p = palette(cx);
        let matches = self.matches(cx);
        // The browser keeps the focus ring's room above Create tag… and below
        // its last control inside the dialog's clip, which the dialog gives
        // back. The list scrolls, so it clips its rows to its bounds on both
        // axes: it keeps the room around them and gives it back through its
        // margin, so no row moves. A row Tab moves onto scrolls into view
        // with its ring.
        let room = appearance::button_ring_room(cx);
        let rows: Vec<_> = (0..matches.len().min(100))
            .map(|index| self.rows.focus(("tag-row", index), cx))
            .collect();
        let reveal = self.rows.items(rows.clone(), room);
        // Past its height the list cues the rest with its scrollbar.
        let cue = ScrollCue::new(self.rows.scroll(), room);
        div().flex().flex_col().gap_3().py(room)
            .child(div().flex().gap_2().child(div().flex_1().child(Input::new(&self.query).aria_label("Filter local tags").cleanable(true))).child(button("create-tag", "Create tag…", "plus", false).debug_selector(|| "create-tag".into()).on_click(cx.listener(|this, _, window, cx| {
                let _ = this.owner.update(cx, |owner, cx| { if owner.path == this.path && owner.operation_busy.is_none() { window.close_dialog(cx); owner.open_create_tag(window, cx); } });
            }))))
            .child(static_text("tag-list-summary", format!("{} local tags · Select to inspect, delete locally, or push one named tag.", self.list.tags.len())).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("tags-list").debug_selector(|| "tags-list".into()).max_h(px(360.) + room * 2.).p(room).m(-room).overflow_y_scroll().track_scroll(self.rows.scroll()).flex().flex_col().gap_1().children(matches.iter().zip(rows).enumerate().map(|(index, (tag, focus))| {
                let tag = (*tag).clone(); let label = format!("{} · {} · {}", tag.name, if tag.annotated { "Annotated" } else { "Lightweight" }, short_oid(&tag.target_oid));
                Button::new(("tag-row", index)).track_focus(&focus).debug_selector(move || format!("tag-row-{index}")).ghost().w_full().h(crate::appearance::ui_size(34.)).label(label.clone()).accessibility_label(label).on_click(cx.listener(move |this, _, window, cx| this.activate(tag.clone(), window, cx)))
            })).when(matches.is_empty(), |element| element.child(static_text("tag-list-empty", if self.list.tags.is_empty() { "No local tags yet. Create a tag to name a commit." } else { "No tags match this filter." }).p_3().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))), "tags-scrollbar"))
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
    use crate::focus_reveal::tests::{
        assert_every_frame_reveals, assert_steady, assert_tab_reveals, control, focus_filter,
    };
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
    pub(crate) fn holds(mask: Bounds<Pixels>, ring: Bounds<Pixels>) -> bool {
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

    /// Applies `interface` as the interface text size, as Settings does.
    pub(crate) fn set_interface_text_size(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        interface: u8,
    ) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        draw(cx);
        park_pointer(cx);
    }

    /// Where the list `list`, which keeps `room` around its rows, paints its
    /// scrollbar's track while it overflows: a strip as wide as the scrollbar
    /// at the right of the rows' column, its full height.
    fn scrollbar_track(
        cx: &mut VisualTestContext,
        list: &'static str,
        room: Pixels,
    ) -> Bounds<Pixels> {
        let column = rendered(cx, list).dilate(-room);
        Bounds::from_corners(
            point(column.right() - Scrollbar::width(), column.top()),
            column.bottom_right(),
        )
    }

    /// The scrollbar thumb the last frame painted in `track`: the one filled
    /// quad inside it shorter than the track, which a track's own fill is
    /// not.
    fn painted_thumb(cx: &mut VisualTestContext, track: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
        let thumbs: Vec<_> = cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| logical(quad.bounds, scale))
                .filter(|fill| {
                    holds(track, *fill) && fill.size.height < track.size.height - px(0.5)
                })
                .collect()
        });
        assert!(thumbs.len() <= 1, "one thumb in {track:?}, not {thumbs:?}");
        thumbs.first().copied()
    }

    /// Scrolls the list `scroll` tracks to `fraction` of its reach.
    fn scroll_list_to(cx: &mut VisualTestContext, scroll: &ScrollHandle, fraction: f32) {
        let end = scroll.max_offset().y;
        scroll.set_offset(point(px(0.), -end * fraction));
        cx.update(|window, _| window.refresh());
        draw(cx);
        park_pointer(cx);
    }

    /// At 13 and 18 pt, the list `list`, which keeps `room` around its rows
    /// and whose `rows` overflow it, paints its scrollbar's thumb at the
    /// right of the rows' column, where the thumb follows the list's offset:
    /// it starts with the track at the top, lies in proportion between, and
    /// ends with the track at the end. The track overlaps no row and no row's
    /// ring, its bounds grown by the installed Button focus ring's gap plus
    /// width.
    pub(crate) fn assert_scroll_cue(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        scroll: &ScrollHandle,
        list: &'static str,
        room: Pixels,
        rows: &[&'static str],
    ) {
        let ring = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(ring, appearance::BUTTON_FOCUS_RING);
        let footprint = ring.gap + ring.width;
        for size in [13, 18] {
            set_interface_text_size(cx, app, size);
            scroll_list_to(cx, scroll, 0.);
            assert!(
                scroll.max_offset().y > px(0.),
                "at {size} pt the rows of {list} overflow it"
            );
            let track = scrollbar_track(cx, list, room);
            let covered: Vec<_> = rows
                .iter()
                .map(|row| (row, rendered(cx, row).dilate(footprint)))
                .filter(|(_, ring)| {
                    ring.intersects(&track) && ring.right() > track.left() + px(0.01)
                })
                .collect();
            assert!(
                covered.is_empty(),
                "at {size} pt the track {track:?} of {list} overlaps rings {covered:?}"
            );

            let top = painted_thumb(cx, track)
                .unwrap_or_else(|| panic!("at {size} pt {list} paints a thumb at its top"));
            // The kit insets its thumb from the track's ends.
            let inset = top.top() - track.top();
            assert!(
                inset >= px(0.) && inset < px(8.),
                "at its top the thumb {top:?} starts with the track {track:?} of {list}"
            );
            for fraction in [0.5, 1.] {
                scroll_list_to(cx, scroll, fraction);
                let thumb = painted_thumb(cx, track).unwrap_or_else(|| {
                    panic!("at {size} pt {list} paints a thumb scrolled to {fraction}")
                });
                let travel = track.size.height - thumb.size.height - inset * 2.;
                let along = (thumb.top() - track.top() - inset) / travel;
                assert!(
                    (along - fraction).abs() < 0.02,
                    "at {size} pt, scrolled to {fraction} of its reach, the thumb {thumb:?} of \
                     {list} lies {along} along its track {track:?}"
                );
            }
            let end = painted_thumb(cx, track).expect("a thumb at the end");
            assert!(
                (track.bottom() - end.bottom() - inset).abs() < px(0.5),
                "at its end the thumb {end:?} ends with the track {track:?} of {list}"
            );
            scroll_list_to(cx, scroll, 0.);
        }
        set_interface_text_size(cx, app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// At 13 and 18 pt, the list `list`, which keeps `room` around its rows
    /// and whose `rows` fit it, paints no thumb and keeps no gutter: each row
    /// lies `room` inside the list's left and right edges, as it did before
    /// lists drew a scrollbar.
    pub(crate) fn assert_no_scroll_cue(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        list: &'static str,
        room: Pixels,
        rows: &[&'static str],
    ) {
        let inset = room;
        for size in [13, 18] {
            set_interface_text_size(cx, app, size);
            let track = scrollbar_track(cx, list, room);
            assert_eq!(
                painted_thumb(cx, track),
                None,
                "at {size} pt {list} paints a thumb"
            );
            let bounds = rendered(cx, list);
            for row in rows {
                let row_bounds = rendered(cx, row);
                assert!(
                    (row_bounds.left() - bounds.left() - inset).abs() < px(0.01)
                        && (bounds.right() - row_bounds.right() - inset).abs() < px(0.01),
                    "at {size} pt {row} {row_bounds:?} lies {inset:?} inside {list} {bounds:?}"
                );
            }
        }
        set_interface_text_size(cx, app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
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

    /// The application in a 1000 × 680 window on a repository with three
    /// tagged commits and `tags` more tags on the last.
    fn small_window_with_tags<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
        tags: usize,
    ) -> (GitRepository, Entity<GitTurtle>, &'a mut VisualTestContext) {
        let repo = tagged_repository(fixture);
        for index in 0..tags {
            git(repo.path(), &["tag", &format!("v1.{index:02}")]);
        }
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        (repo, app, cx)
    }

    /// Tab moves through every row of Tags and Shift+Tab back, and the list
    /// scrolls each row into view with its focus ring. Once a row is shown,
    /// redraws and a click on a partly visible row scroll nothing.
    #[gpui::test]
    async fn tab_reveals_every_tag_row(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (_repo, app, cx) = small_window_with_tags(cx, fixture.path(), 27);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tags are read").await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");

        let rows: Vec<_> = (0..30).map(|index| control("tag-row", index)).collect();
        focus_filter(cx, &list, &rows[0]);
        assert_tab_reveals(cx, &list, "tags-list", &rows);
        // A click inspects nothing while an operation runs.
        app.update(cx, |app, _| app.operation_busy = Some("Testing"));
        assert_steady(cx, &list, "tags-list", &rows);
    }

    /// Tab and Shift+Tab through Tags' 30 rows: every frame painted after a
    /// key, the first one that draws the new focus included, shows the
    /// focused row with its whole ring inside the list.
    #[gpui::test]
    async fn tags_reveal_rows_in_the_frame_that_draws_focus(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (_repo, app, cx) = small_window_with_tags(cx, fixture.path(), 27);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tags are read").await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");

        let rows: Vec<_> = (0..30).map(|index| control("tag-row", index)).collect();
        focus_filter(cx, &list, &rows[0]);
        assert_every_frame_reveals(cx, &list, "tags-list", &rows);
    }

    /// Tab moves through every Push to… button of the tag inspector and
    /// Shift+Tab back, and the list scrolls each into view with its ring.
    #[gpui::test]
    async fn tab_reveals_every_push_destination(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (repo, app, cx) = small_window_with_tags(cx, fixture.path(), 0);
        // Local bare remotes: the inspector only lists them.
        let remotes = 10;
        for index in 0..remotes {
            let bare = fixture.path().join(format!("remote-{index:02}.git"));
            git(
                fixture.path(),
                &["init", "--quiet", "--bare", bare.to_str().unwrap()],
            );
            git(
                repo.path(),
                &[
                    "remote",
                    "add",
                    &format!("remote-{index:02}"),
                    bare.to_str().unwrap(),
                ],
            );
        }
        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tag is read").await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Push to… list");

        let pushes: Vec<_> = (0..remotes)
            .map(|index| control("push-tag-remote", index))
            .collect();
        focus_filter(cx, &list, &pushes[0]);
        assert_tab_reveals(cx, &list, "tag-remote-list", &pushes);
        app.update(cx, |app, _| app.operation_busy = Some("Testing"));
        assert_steady(cx, &list, "tag-remote-list", &pushes);
    }

    /// Adds `count` local bare remotes, `remote-00` onwards, to `repo`: the
    /// lists only show them, and nothing is fetched or pushed.
    pub(crate) fn add_remotes(fixture: &std::path::Path, repo: &GitRepository, count: usize) {
        for index in 0..count {
            let bare = fixture.join(format!("remote-{index:02}.git"));
            git(
                fixture,
                &["init", "--quiet", "--bare", bare.to_str().unwrap()],
            );
            git(
                repo.path(),
                &[
                    "remote",
                    "add",
                    &format!("remote-{index:02}"),
                    bare.to_str().unwrap(),
                ],
            );
        }
    }

    /// The selectors `name-0` to `name-{count - 1}`.
    pub(crate) fn selectors(name: &str, count: usize) -> Vec<&'static str> {
        (0..count)
            .map(|index| &*format!("{name}-{index}").leak())
            .collect()
    }

    /// Tags of `tags` rows open at 1000 × 680, and its list.
    async fn open_tags_list<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
        tags: usize,
    ) -> (Entity<GitTurtle>, FocusReveal, &'a mut VisualTestContext) {
        let (_repo, app, cx) = small_window_with_tags(cx, fixture, tags.saturating_sub(3));
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tags are read").await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Tags list");
        (app, list, cx)
    }

    /// The tag inspector of a repository with `remotes` remotes open at
    /// 1000 × 680, and its Push to… list.
    async fn open_push_list<'a>(
        cx: &'a mut TestAppContext,
        fixture: &std::path::Path,
        remotes: usize,
    ) -> (Entity<GitTurtle>, FocusReveal, &'a mut VisualTestContext) {
        let (repo, app, cx) = small_window_with_tags(cx, fixture, 0);
        add_remotes(fixture, &repo, remotes);
        let tag = repo.tags().unwrap().tags[0].clone();
        cx.update(|window, cx| app.update(cx, |app, cx| app.inspect_tag(tag, window, cx)));
        let read = app.update(cx, |app, _| app.tag_actions.task.take());
        read.expect("the tag is read").await;
        draw(cx);
        let list = app
            .read_with(cx, |app, _| app.tag_actions.list.clone())
            .expect("the Push to… list");
        (app, list, cx)
    }

    /// Tags' 30 rows overflow its list at 1000 × 680, and its scrollbar's
    /// thumb follows the list's offset in a gutter clear of every ring.
    #[gpui::test]
    async fn tags_list_cues_rows_beyond_its_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, list, cx) = open_tags_list(cx, fixture.path(), 30).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let rows = selectors("tag-row", 30);
        assert_scroll_cue(cx, &app, list.scroll(), "tags-list", room, &rows);
    }

    /// Tags' three rows fit its list: it paints no thumb, and its rows keep
    /// their width.
    #[gpui::test]
    async fn tags_list_that_fits_paints_no_thumb(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, _list, cx) = open_tags_list(cx, fixture.path(), 3).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        assert_no_scroll_cue(cx, &app, "tags-list", room, &selectors("tag-row", 3));
    }

    /// Ten Push to… buttons overflow the tag inspector's list at
    /// 1000 × 680, and its scrollbar's thumb follows the list's offset in a
    /// gutter clear of every ring.
    #[gpui::test]
    async fn push_list_cues_buttons_beyond_its_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, list, cx) = open_push_list(cx, fixture.path(), 10).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let pushes = selectors("push-tag-remote", 10);
        assert_scroll_cue(cx, &app, list.scroll(), "tag-remote-list", room, &pushes);
    }

    /// Two Push to… buttons fit the tag inspector's list: it paints no
    /// thumb, and the buttons keep their width.
    #[gpui::test]
    async fn push_list_that_fits_paints_no_thumb(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let (app, _list, cx) = open_push_list(cx, fixture.path(), 2).await;
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        assert_no_scroll_cue(
            cx,
            &app,
            "tag-remote-list",
            room,
            &selectors("push-tag-remote", 2),
        );
    }
}
